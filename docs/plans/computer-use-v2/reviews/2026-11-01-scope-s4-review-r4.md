**MERGE-AFTER-FIXES**

Independent acceptance review of the replacement at `4d33ec94` (`b0758f0c`, `1dfe8bed`, `4d33ec94` after `e6771d90`). Read all three previous reports, including repair records, first. Paths below are relative to `computer-use/crates/`. Static review only; schedules and mutation sensitivity were not executed.

The replacement is **overall simpler**: runtime cancellation, worker startup, joins and detached joiners disappear. The lease-state critical section and original atomic registration remain intact. However, deferred cleanup is not yet a reliable retirement boundary, and synchronous cleanup introduces a new control-path dependency.

**Must-fix, category (a)**

1. **[P1; NEW read-admission regression] An export started after revoke can return stale bytes.** `gcu-mcp/src/files.rs:323`, `:425`, `:431`.

   Principle: deferred physical retirement must not permit a new read of an invalid epoch.

   Schedule: a file admission/stage holds the ledger with a delivered generation-1 file present. Revoke records a ticket, fails `try_lock`, and returns. A subsequent `export()` also fails its preliminary sweep and waits for the ledger. The holder releases its mutex, then is descheduled before `flush_retired()` (`:137` or `:91`). Export wins the mutex and copies every row without checking `visible`. Its eventual unlock may delete the file, but the returned bytes have already escaped. This is not the accepted read-admitted-before-revoke residual. Previously the preliminary sweep waited for the mutex.

   Revalidate/filter under the export lock before reading; unlink after unlocking. Add this exact holder → revoke → new-export handoff test.

2. **[P1; SAME retirement-liveness class as rounds 1–3] The eighth sweep can strand an invalidated row permanently.** `gcu-mcp/src/image_cache.rs:150`, `:164`, `:173`; analogous `gcu-mcp/src/files.rs:342`, `:358`, `:389`.

   Principle: a pending ticket needs a future executor, including when the last mutex holder is itself a sweeper.

   Concrete schedule: A sweeps a still-valid image. For each of passes 1–7, another same-live-lease bind finishes its unlocked callback-registration/request portion while A holds the cache mutex; its request advances the ticket and its `try_lock` fails. The bind's lease assignment occurs between A's passes. On pass 8, A reads the image as valid; then revoke changes the lease and its callback requests another ticket while A still holds the mutex. A marks only its earlier `seen`, unlocks, and exhausts the loop. `extract` has no unlock flush. All other callers have returned; requested > finished and the stale pixels remain forever without another operation. The ledger has the same capped-driver structure.

   Preserve bounded work while providing a guaranteed handoff for leftover tickets, or establish an equivalent progress invariant. Merely leaving them for an unspecified “next unlock” does not satisfy retirement without later get/export/close. Add a deterministic final-pass race.

3. **[P1; NEW synchronous-cleanup/locking class] Cleanup can run under outer output locks and delay input release.** `gcu-core/src/scope/model.rs:727`; `gcu-mcp/src/files.rs:351`; `gcu-cli/src/scope/mod.rs:290`, `:298`, `:478`.

   Principle: artifact cleanup must stay outside output critical sections and cannot obstruct emergency input release.

   Schedule: core observation holds the snapshot mutex (`gcu-core/src/action/observe.rs:352`) when `commit_if_current` calls `expire` (`scope/commit.rs:35`). Expiry invokes the file callback, which can unlink while that snapshot mutex remains held. Likewise, file-admission Drop flushes before the surrounding image admission and screenshot-delivery guards drop (`gcu-mcp/src/result_commit.rs:156`, `:183`). `Hold` tracks only the individual store, not those outer locks.

   Separately, control revoke calls the inline file callback before reaching `release_all_input`; cancellation Drop likewise revokes before scheduling release. A stalled filesystem unlink stalls that path, and a file callback also delays subsequent image callbacks. Eight passes bounds neither the number of unlinks nor syscall duration; there is no cleanup timeout. Defer disk work until all output locks are released and keep input release independent of it. Test parked cleanup against both lock release and input-release issuance.

4. **[P2; SAME ownership/lease-association class as round 3, new interleaving] Concurrent replacement binds can retain the wrong lease's callback.** `gcu-mcp/src/files.rs:298`, `:309`; `gcu-mcp/src/image_cache.rs:109`, `:120`; `gcu-mcp/src/retire.rs:36`.

   Principle: the bound lease and retained callback must change as one serialized association.

   Schedule: A assigns L1, then pauses before `arm`. B binds L2 completely and commits L2 artifacts. A resumes, replaces the strong L2 callback with its L1 callback, then sweeps using the still-bound L2, keeping those valid artifacts. Revoke L2: its weak callback is dead, so the artifacts remain indefinitely. Sequential replacement is correct; concurrent different-lease binds are neither serialized nor refused. This concerns the supported synchronous store surface; current Results use a fixed lease. Serialize/refuse conflicting binds and add a barrier test.

5. **[P2; SAME cross-owner namespace class as round-3 item 2] `Arc<TempDir>` does not preserve directory ownership against the orphan reaper.** `gcu-mcp/src/files.rs:45`, `:375`, `:452`; `gcu-host-protocol/src/owner_unix.rs:233`, `:236` (Windows equivalent: `owner_windows.rs:234`, `:237`).

   Principle: retain the pathname's ownership lock until every operation using that pathname finishes.

   Schedule: an unlink batch retains an aged directory D and pauses before `remove_file`. Concurrent `close()` drops D's `OwnerLock`, retaining only its `Arc<TempDir>`. A later store's normal orphan scan can now lock and rename D after the 60-second grace. If D is subsequently reused, the old unlink—and the old TempDir's eventual pathname-based deletion—can delete replacement content. Name reuse remains conditional, as in r3; an Arc is not an inode anchor. Retain the owner lock with the shared directory/batch. Add parked-unlink → close → reaper → replacement coverage.

**(b) Residuals and preserved guarantees**

- **Lock principle, SAME preserved lease exclusion:** every `publish` caller releases the lease state guard first: revoke, successful rebind, and deadline expiry. `current`/rebind/renew's second expiry checks also drop their guards before calling `expire`; `remaining` calls it before acquiring its read guard. The callback-list mutex is released before invocation. Read-only callback reentry into the lease therefore does not self-deadlock; nested expiry reaches a terminal state. This does not imply caller-owned output locks are absent (item 3).
- **Deferred-progress principle, NEW conditional contract:** same-thread lease reads under admissions really can publish. Each store's `Hold` branch records a ticket; FileAdmission/ImageAdmission Drop releases its mutex and Hold, then flushes. Ordinary AreaLock also flushes after unlocking. Request-before-`try_lock` plus SeqCst counters handles the ordinary missed-wakeup race: if the holder already completed its unlock/flush before the request, the callback can acquire the free mutex; otherwise an intervening holder owes the flush. Item 2 breaks that obligation for the final capped sweep. Constructed admission guards also flush on unwinding; panic containment alone does not guarantee completion of a panicked sweep. A guard deliberately retained forever cannot promise retirement. While pending, raw file paths remain readable and pixels remain allocated; cache `get` checks visibility, whereas export has item 1. The intended contract is retirement by completion of the holder's next unlock cleanup, not necessarily by revoke return; it is not established universally yet.
- **Arithmetic principle, NEW theoretical limitation:** `Sweep::mark` uses ordinary `<`, drivers use `>=`, and requests wrap (`retire.rs:114`, `:134`). After wrap, a high finished value suppresses new sweeps; store-id wrap can also alias Hold identities. There is no wrap/ABA proof. These require approximately 2^64 operations/allocations and are not practical blockers, but the guarantee assumes no wrap.
- **Expiry principle, NEW weaker replacement contract:** passive stores can retain files/pixels forever after the deadline if nobody reads the lease. Even `remaining()` can straddle the deadline: its initial `expire` sees pre-deadline time, then its second clock read returns zero without publishing. Production Machine's zero-remainder branches revoke; a standalone caller must read again/revoke. This deliberately loses the deleted owner's autonomous expiry progress. It is distinct from explicit revoke/rebind being stranded in items 2/4.
- **Weak ownership principle, SAME lifetime defects removed for ordinary/sequential use:** no runtime or thread owns retirement now. Stores own callback Arcs; callbacks capture weak stores; leases retain weak callbacks, pruning dead entries on push/fire. Store Drop releases resources even with pending tickets. A fire that already upgraded a callback may finish after clear/rebind; it reads the store's current bound lease. Sequential same-lease/new-lease rebinding works. Binding after revoke registers then requests an initial sweep, and valid commits are refused. Registration racing a publish is covered by that initial sweep. Two stores register independently. Callback panic is caught and later callbacks run; an indefinitely blocked callback is not contained by `catch_unwind`.
- **Atomicity/compatibility principle, SAME preserved boundaries:** `scope/commit.rs` and `result_commit.rs` are byte-identical to `e6771d90`; the single stamp/register closure and commit/revoke mutual exclusion remain. Watch publication and waker notification still precede callbacks, with no generation/watch ordering code changed. No new await, process launch, dependency, or `Command::new` was added. Stop constants remain **100 ms / 500 ms / 5 s / 4750 ms**; timing independence nevertheless has item 3. No new Windows/Linux cfg or unused/dead-code defect found by inspection. `files.rs` is **633 lines** (note only).
- **Accepted boundaries, unchanged:** copied response bytes are not recalled; unlink errors remain ignored. A1–A4 and accepted S1/S3 (b) residuals are not reopened. Real helper/TCC/FD/live SecCode/real-model work remains **NOT_RUN by design**.

**Tests and nice-to-have — SAME mutation-coverage class, with NEW replacement gaps**

The revoke/rebind/expiry/runtime-loss/replacement tests check paths or non-residency **before** get/export. Omitting callback registration or `publish`'s fire would fail them. Core's reentrant-current test has an 8-second abort watchdog; the busy-ledger test has 8-second handshakes and a 10-second watchdog. Other synchronous cases do not each have a watchdog. The cache-admission barrier covers another thread owning the cache, not same-thread deferred retirement of previously delivered pixels.

Existing tests do not force the final-pass, export-handoff, conflicting-bind, outer-lock/unlink, or close/reaper schedules above. Nor do they establish every claimed ticket/same-thread mutation: returning from the same-thread branch without recording a ticket needs a held-admission test with already-delivered artifacts; deleting the branch and falling through to `try_lock` may still pass on platforms returning WouldBlock. Add direct coverage. `resident()` correctly observes without sweeping in sequential assertions; it is a test-only raw mutex holder without an unlock flush, so concurrent use can itself strand a callback ticket. Keep that limitation explicit. The dead-weak test proves failed upgrade, not removal of the list entry itself.

Validation: source diff whitespace check passed; the full replacement diff flags only the existing added EOF blank line in the r3 report. No cargo, compilation/clippy, Rust tests/mutations, network/SSH/model calls, personal agent-home access, or source/test edits. Only this report was written, uncommitted.
