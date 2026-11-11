Verdict: **ACCEPT-AFTER-FIXES**

Final round-3, read-only constraint-preservation review of ADR 0019 on `cu2/design-mailbox`, HEAD `581b7ae77fe52089d317ad3191fe3c057ce2a635` ("ADR 0019 — apply round-2 should-fix …"). Baseline: `HEAD~1`, `e267d9f6f50550492f891d45dcc08cbea3e4c95b`, obtained with `git show HEAD~1:computer-use/docs/adr/0019-tart-mailbox-channel.md`. I read both repo-root reports, `docs/plans/computer-use-v2/reviews/2026-11-10-mailbox-design-review.md` and `docs/plans/computer-use-v2/reviews/2026-11-10-mailbox-design-review-r2.md`, including round-2 #13–15.

There is **one missing constraint requiring restoration**: the harmless substitute trees for M7/M9 tests. The five destructive rules and the numerical, identity, protocol, deadline, Stop, owner and admission constraints survive. The former pre-installation requirement is intentionally relaxed under round-2 #13, with the specified replacement safeguards. No additional must-fix, new contradiction or hidden mechanism was found. This is design acceptance conditional on the single restoration below, not native validation or authorization to operate the host. No further review round is requested.

References use ADR 0019 paths relative to `computer-use/`. `B:Lx` denotes a line of the 157-line baseline; `H:Lx` denotes a line of the 346-line HEAD ADR. Line spans below are document references, not proposed implementation changes. Inventory IDs are not findings.

1. **Must-fix — Missing harmless-substitute-tree requirement.** §5, H:L249; §6, H:L335–346. Baseline B:L122 explicitly requires all autonomous-termination/transport/liveness retests in M7/M9 to use harmless substitute trees under block-all; B:L155 repeats that requirement for M7. HEAD retains the transfer to M7/M9, block-all and the required fault cases, but removes “无害替身树” everywhere. The general no-login/model-evaluation gates do not specify the harmless tree that these pre-admission termination tests must actually exercise. This is a lost test-scope constraint, not a request for a new mechanism or a claim that a dangerous test has run.

   **Exact text to restore:** append the following baseline sentence to §5's evidence-boundary paragraph, after H:L249:

   > C7 自主终止及所有 transport/liveness 验收移到 M7/M9，在 block-all 下用无害替身树重测。

   That single restoration also supplies the condition to §6's linked M7/M9 obligations; the card table need not duplicate it. Inventory entries L12 and K08 identify both original occurrences. No other restoration is required.

**Constraint inventory.** “Unchanged” means the same normative force and detail survive, including ordinary rewording/line splitting. “Consolidated” means the obligation survives through the cited authoritative paragraph, table, link or combination of locations. “Weakened—authorized #13” identifies the expressly requested replacement of pre-installation, not an unrequested regression. “Missing” is the finding above. Where a row lists several clauses, its disposition applies to every listed clause; materially different dispositions have separate rows. Repeated baseline statements are mapped to their surviving authoritative rule rather than counted as additional mechanisms.

**Scope, owner decisions and threat boundary**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| A01 | Design only; no implementation or Mini/volume/network operation; argv are untested candidates (B:L5). | Unchanged, H:L5 and §2 heading H:L55. |
| A02 | Do not install a provider, mint admission capability or change S6/acceptance-matrix §8 (B:L5). | Consolidated, §6 H:L323. |
| A03 | Owner has approved only the mailbox exception and reviewed host-volume design (B:L9, L118, L149). | Consolidated, §1 H:L9; M1 points back to that decision at H:L329. |
| A04 | Production has no IP control, no SSH and no gateway allowance (B:L9); no production exec/guest sudo (B:L110). | Consolidated, H:L9–11; the explicit exec/sudo prohibitions moved here from §4. |
| A05 | Softnet block `0.0.0.0/0`, including gateway, with only the approved model endpoint `/32`; no host guard/pf (B:L9). | Unchanged, H:L10 and production argv H:L67. |
| A06 | Only each attempt's fresh capacity-limited volume's `mailbox` is exported; no other host directories, clipboard, drag/drop, device/agent forwarding (B:L9). | Unchanged, H:L10, L43. |
| A07 | DEFAULT mechanics networking is a separately scoped laboratory exception and cannot change production H1 (B:L9, L118). | Consolidated, H:L11, L225–231, L323. |
| A08 | Established visibility facts do not establish quota, trustworthy ownership or lease feasibility; do not adopt `--disk`/`--serial-path` (B:L11). | Unchanged, H:L13–14. Mini/macOS 27/Tart 2.41.1, approximately 1s visibility, >3 MiB/symlink behavior, host-observed UID, guest path, unavailable exec and lack of passwordless sudo remain stated. |
| A09 | All guest bytes, names, metadata and claimed supervisor replies are untrusted (B:L13). | Unchanged, H:L16. |
| A10 | Exact ASCII allowlist, regular files, same-FD nlink/size/dev/FSID and bounded parsing only (B:L13). | Unchanged, H:L16, detailed H:L127–144. |
| A11 | No following links, executing artifacts, accepting paths/shell/URL/argv/credentials/HMAC keys, recursive guest-tree traversal or cleanup (B:L13). | Unchanged, H:L16. The fixed reviewed laboratory program is the expressly scoped #13 exception, H:L240–249, L323. |
| A12 | Cover case/Unicode aliases, cross-volume hardlinks, FIFO/socket/device, sparse files, directory churn and concurrent modification (B:L13). | Unchanged, H:L17; adversarial subcases H:L269–271, L305–307. |
| A13 | Same owner does not identify writer; host `noexec`/other flags do not sandbox the guest (B:L13). | Unchanged, H:L17; C4/C8 reference the same boundary at H:L277, L301. |
| A14 | Model must be completely unable to reach mailbox; S11e must prove both different security principals and an actual OS sandbox (B:L15, L112). | Consolidated, H:L19–20; §4 H:L215 links to §1. The requirement has not become UID-only isolation. |
| A15 | Every descendant barred from listing/reading/writing/renaming mailbox or aliases and inheriting its FDs (B:L15, L112). | Consolidated, H:L19, L215. |
| A16 | Close helper/relay/fixture mediated access and IPC/debug bypasses; no debug/ptrace, memory/handle access, program/startup replacement or acquisition of administrator ability (B:L15, L112). | Consolidated, H:L19–20, L215–218. Although the repeated literal “IPC” in old §4 is removed, the universal mailbox-unreachability rule and explicit mediated-access prohibition remain; this is not permission for a different IPC bypass. |
| A17 | UID/0700/hidden paths/private FDs alone insufficient; any failed negative probe prevents model launch (B:L15). | Unchanged, H:L20. |

**Resource ownership, identities and phase ordering**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| R01 | Register before effects, prove stopped before unmount, confirm all resources before seal (B:L19). | Unchanged, H:L24. |
| R02 | Use existing caller/provider/external owner; no daemon, observer, new thread, shared lifecycle manager or Drop cleanup (B:L19, L145, L154). | Consolidated, H:L24; caller contract H:L190–198; M6 H:L334 and card-ownership reference H:L325. |
| R03 | Ledger, receipts and persistent Stop remain outside volume and unexported (B:L19). | Unchanged, H:L24. |
| R04 | Candidate 64 MiB HFSX, case-sensitive, non-journaled, single-file SPARSE, layout NONE; no expansion/compression/snapshots (B:L21). | Unchanged, H:L26, exact create argv H:L62. |
| R05 | Both live-image `st_size` and `st_blocks × 512 bytes` ≤80 MiB (B:L21). | Unchanged, H:L27, L306–307. |
| R06 | Record create-scratch paths/peak; scratch total ≤80 MiB (B:L21). | Unchanged, H:L27, L62, L276, L306. |
| R07 | Before starting, ≥256 MiB host headroom and no unreclaimed volume (B:L21). | Unchanged, H:L27. |
| R08 | Capacity bound concerns dedicated filesystem/live image, not CPU/IO or elimination of host APFS COW/snapshot retention; prohibit and separately check backup/snapshot copies; insufficient proof blocks (B:L21). | Unchanged, H:L28, L319. |
| R09 | Host-controlled private root R, exclusive random 128-bit A, I=`A/mailbox.sparseimage`, random empty M inside A, S=`A/seed`, host-generated share name N (B:L23, L25). | Consolidated, H:L30–40. M's underlying location inside A is explicit in the M₀ row. |
| R10 | Component-wise no-follow verification of current UID, directory 0700/file 0600 and ACL; paths only trusted separate argv parameters (B:L23). | Unchanged, H:L30. |
| R11 | S is the fixed synthetic `mailbox/{host-write,guest-write}`, `host-stage`, `.metadata_never_index`, `.fseventsd/no_log` tree; export only `M/mailbox`, never volume root/stage/A/R (B:L23). | Consolidated, H:L38, L43; stage protection H:L112, L121. |
| R12 | Persist R/A parent and child dev/inode/UID/mode/ACL; likewise I, unmounted M₀ and S, and each fixed seed directory/marker (B:L25). | Consolidated, H:L31–39. The common identity-column definition supplies every listed field to every parent/child row. |
| R13 | Persist creation intent before create and identity after create before continuing (B:L25, L30). | Consolidated, H:L24, L31, L48; not replaced by name-based ownership. |
| R14 | Persist V UUID, opened-object `fstat` dev/inode and `fstatfs(fd)` FSID; each mounted mailbox/two children/stage identity; exact I→whole-image-device→volume→M mapping (B:L25). | Consolidated, H:L36, L40–41. |
| R15 | V and covered M₀ are different objects; after detach verify M₀, never compare it to V's inode (B:L25). | Consolidated, H:L39–40, L84. |
| R16 | New owner first recovers unsealed rows, verifies persistent Stop/installation/golden, records exact run/attempt/clone incarnation/config before cloning (B:L29). | Consolidated, H:L47 plus linked rule 2 H:L80–81 and rule 5 H:L87. Persistent Stop remains the object identified at H:L24. |
| R17 | Preparation owner proof begins at reserve, before guest exists (B:L29). | Consolidated, H:L185, prep origin H:L202. |
| R18 | Exclusive creation of A/M/S/seed; create and attach intent before effects; verify I identity/capacity and persist V/whole-device mapping (B:L30). | Consolidated, H:L24, L27, L31–41, L48, L62–65. |
| R19 | Exclude A before create; I/underlying M before attach; verify mounted-root coverage before export (B:L30, L44). | Consolidated, authoritative argv checklist H:L65, referenced by phase H:L48. |
| R20 | Controlled attach and all flags/owner/layout/marker/index/backup readbacks must pass before binding/export of hostile bytes (B:L31). | Unchanged, H:L49. |
| R21 | Before start recheck unique `--dir` and fixed Softnet argv; first inventory binds boot, then guest lease, then individual probes (B:L31). | Unchanged, H:L49, L67. |
| R22 | Revoke phase prohibits new work; revoke/drain, no more model/scope renewal, external owner proof continues through seal (B:L32). | Consolidated, H:L50, L164, L185–186, L192–196. |
| R23 | Stop and independently prove VM/tart/softnet whole-tree exit, close production mailbox FDs, delete VM/prove absence, then detach (B:L32). | Consolidated, H:L50, L80, L82, L164. “证退出” in the short phase row is constrained by the full stop barrier in the linked destructive checklist and the independent host confirmation in §3. |
| R24 | Whole device/mount absent before image unlink; clean only identity-recorded fixed synthetic S entries, then empty M₀/A; VM/volume/channel/directories/receipts all confirmed before seal (B:L33). | Consolidated, H:L24, L38, L51, L83–85. No-recursion/no-unexpected-entry and per-child identity rules still govern S and rmdir. |
| R25 | Otherwise quarantine, attempt `not_run`, stop subsequent attempts (B:L33). | Unchanged, H:L51; uncertain exits/effects H:L137, L207. |

**Fixed argv, permissions and observation gates**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| V01 | Actual eval user, no root/sudo/interactive authorization; no shell/PATH search; umask 077; fixed system paths/verified install manifest (B:L37). | Unchanged, H:L57. |
| V02 | Incrementally cap command output at ≤1 MiB; plist rejects external entities and bounds depth/entries (B:L37). | Unchanged, H:L58. |
| V03 | Unsupported options, GUI/FDA requirements, ignored restrictions or unreadable evidence block; never elevate (B:L37, L44–45). | Consolidated, H:L58, L66, L313. The removed repeated tmutil-specific elevation clause is covered by this common gate. |
| V04 | Exact candidate create argv `/usr/bin/hdiutil create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX -srcfolder S I` (B:L41). | Unchanged, H:L62. |
| V05 | Measure combined option support, non-journaling, suffix/path, root layout/permissions, marker copying, all temporary devices/mounts/scratch/service activity (B:L41). | Consolidated, H:L62, L70–71, C4 H:L275–277. |
| V06 | S synthetic during create; `-srcfolder` may internally mount; pre-populated markers do not prove presence before first mount; reject recipe if necessary restrictions on internal mounts cannot be met (B:L41). | Consolidated, H:L70–71. Explicitly forbids inspecting only later attach. |
| V07 | Exact attach argv `/usr/bin/hdiutil attach -plist -noautoopen -nobrowse -owners on -mountOptions rw,nosuid,noexec,nodev -mountpoint M I` (B:L42). | Unchanged, H:L63. |
| V08 | Verify noninteractive authority, actual flags/owners/UUID/FSID/capacity/ACL/plist/precise device mapping; restrictions effective from this controlled attach; missing flags reject; no remount fallback (B:L42). | Unchanged, H:L63. |
| V09 | Exact inspection tools: `hdiutil imageinfo -plist I`, `hdiutil info -plist`, `diskutil info -plist D`, `mount`, `mdutil -s M`, at their fixed system paths (B:L43). | Unchanged, H:L64. |
| V10 | Eval-readable bounded inspection; D from exact mapping, label not UUID; no authority for mount/diskutil mutations or `mdutil -i off` (B:L43). | Consolidated, H:L57–58, L64; unreadable/invalid evidence blocks, not a successful empty readback. |
| V11 | Exact `tmutil addexclusion X`/`isexcluded X`, X=A/I/M; ordered exclusions and mounted-root coverage (B:L44). | Unchanged, H:L65. |
| V12 | Measure sticky object type, cross-mount survival and need for separate volume exclusion; no opportunistic `-p`/`-v`; success exit is not effective exclusion (B:L44). | Unchanged, H:L65; elevation refusal consolidated at H:L57–58. |
| V13 | Root markers are not access control; prove effect with eval-readable service/file-access observations, index state/canary queries and actual backup readback; finite C10 window, unreadable/incomplete coverage unknown, no global service changes (B:L45). | Consolidated, H:L58, L66, L311–313. |
| V14 | Exact production Tart argv with block-all, approved `/32`, unique mailbox `--dir`; check manifest/actual process/config/sole export; DEFAULT argv laboratory only (B:L46). | Unchanged, H:L67, L49, L225. |
| V15 | Exact normal and force `hdiutil detach E` candidates; E is whole-image device; measure unprivileged authority, selection, time and authoritative absence; force only once under destructive rules (B:L47). | Unchanged, H:L68, L82–83, L95. |

**All five destructive rules, including recovery**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| D01 | Rule 1: querying inode/UUID does not bind later `/dev/diskN`; verified parent FD does not lock child name; eval cleanup lock excludes only participants (B:L51). | Unchanged, H:L77. |
| D02 | Rule 1: trusted exclusive host-mutation window from mapping check through effect completion; no other manual/automatic attach/detach, hotplug or target replacement (B:L51). | Unchanged, H:L78. |
| D03 | Rule 1: owner reserves window, pauses jobs/prohibits concurrent tools and records it in receipt (B:L51). | Unchanged, H:L78. |
| D04 | Rule 1: M2 measures reuse/mapping/semantics; M5 injects post-check replacement; repeated queries do not close race (B:L51). | Unchanged, H:L79. |
| D05 | Rule 1: without exclusion, require measured effect-time attachment binding, otherwise issue no destructive argv and quarantine; no daemon/host guard workaround (B:L51). | Unchanged, H:L79. |
| D06 | Rule 2: select only exact attempt in unsealed ledger; append `RecoveryNoted`; check clone incarnation/config, recorded I parent/dev/inode, UUID/FSID, whole device/all mounts (B:L52). | Unchanged, H:L80. |
| D07 | Rule 2: recheck lease/identity/stop barrier before every effect (B:L52). | Unchanged, H:L80. |
| D08 | Rule 2: unknown is not absence; no prefix/name/label claiming, remount of guest-containing image, touching replacement or other disk (B:L52). | Unchanged, H:L81. |
| D09 | Rule 3: valid VM/tart/softnet exit proof before normal detach; only sufficient remaining budget plus still-valid identity/exclusion permits one force; no killing system services (B:L53). | Unchanged, H:L82. |
| D10 | Rule 3: image must be unmounted everywhere and have no attachment before unlink; even a recorded image never known attached requires no-device proof; possibly attached never permits unlink (B:L53). | Unchanged, H:L83. |
| D11 | Rule 4: every unlink/rmdir uses verified parent FD, no-follow and recorded child identity; after detach recheck M₀, no unexpected entries before rmdir (B:L54). | Unchanged, H:L84. |
| D12 | Rule 4: lost mkdir reply, intent without child inode, replaced directory/image, unresolvable lost create/attach response, lost detach response without independent absence all quarantine (B:L54). | Unchanged, H:L85. |
| D13 | Rule 4: no recursive or same-name fallback; intent-only orphan may remain indefinitely; skip independently confirmed absent steps idempotently (B:L54). | Unchanged, H:L85. |
| D14 | Rule 5: expired old owner permanently `Suspended`, no delete/detach/unlink/seal, cleanup=unknown (B:L55). | Unchanged, H:L86. |
| D15 | Rule 5: only new recovery owner with new bounded ends may take over; old attempt permanently `not_run` (B:L55). | Unchanged, H:L86. |
| D16 | Rule 5: abnormal unsealed state preserves/latches Stop outside VM before considering allocation; no automatic resume (B:L55). | Unchanged, H:L87. |

**Budgets, deadlines and receipt/log limits**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| B01 | Single budget table; host-monotonic absolute ends include kill/reap (B:L57). | Unchanged, H:L89, L200–207. |
| B02 | reserve→ready preparation ≤10min; installation and each query ≤5s; clone ≤60s (B:L61). | Unchanged, H:L93. |
| B03 | start ≤120s includes volume preparation ≤40s, create ≤20s, attach ≤10s; only remaining start budget available for boot (B:L61). | Unchanged, H:L93. |
| B04 | Each probe ≤2s, probe group ≤60s; bootstrap/final ACK ≤5s; login ≤5min (B:L62). | Unchanged, H:L94. |
| B05 | run ≤min(task.max_seconds,15min) (B:L62). | Unchanged, H:L94. |
| B06 | Cleanup total ≤60s: revoke/drain 10s, stop/confirmation 20s, delete/volume 20s, seal 10s (B:L63). | Unchanged, H:L95. |
| B07 | Within deletion's shared 20s: VM/confirmation 5s, normal detach 5s, single force 5s, unlink/absence 5s (B:L63). | Unchanged, H:L95. |
| B08 | Total interval between Stop/liveness servicing ≤100ms, not just each individual IO (B:L64, L102). | Consolidated, H:L96, L190–198; #14 explicitly distinguishes issuance. |
| B09 | Host publication and external owner proof each ≤1s; guest grant fixed `g+2s`; ≤1s revoke/tree termination reserve, total ≤3s; independent S3 scope 3s unchanged (B:L64). | Unchanged, H:L96, L180–187. |
| B10 | Sub-budgets are caps, not a promise all maxima fit; insufficient enclosing time stops/quarantines, never extends total (B:L66). | Unchanged, H:L98. |
| B11 | Receipt records complete resource binding, intents/identity/readbacks, versions/argv digests, permissions/flags/index/backup, capacity peaks, every effective deadline/time/exit, force count, independent absence, recovery/unknown (B:L66). | Unchanged, H:L99; resource table H:L31–41 supplies the full binding. |
| B12 | Locators only in protected off-volume ledger; ordinary logs only logical IDs/enums/lengths/digests, no raw guest content or personal paths (B:L66). | Unchanged, H:L100. |

**Fixed layout and every publish/read procedure step**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| P01 | Export root contains only host-write/guest-write (B:L70). | Unchanged, H:L104. |
| P02 | host-write: binding plus inventory/probe/bootstrap/login/run/lease/revoke `.msg`, ≤8 entries; guest-write: seven `.msg` and seven corresponding `.msg.tmp`, ≤14 entries (B:L70). | Unchanged, H:L105. |
| P03 | Each entire file including framing ≤4096 bytes (B:L70). | Unchanged, H:L105, read size H:L130. |
| P04 | Binding immutable after first publication; only version, pinned run/attempt/VM/install/policy digests and protocol constants; no secrets/scope token (B:L70). | Unchanged, H:L106. |
| P05 | Unexported host-stage only fixed host `.msg.tmp`, ≤8 entries, not an additional shared directory (B:L70). | Consolidated, H:L43, L106, L112, L121. |
| P06 | Single publisher per direction (B:L72). | Unchanged, H:L110. |
| P07 | Publish: same-volume, unexported, guest-inaccessible verified stage-parent FD; exclusive `openat(O_CREAT\|O_EXCL\|O_NOFOLLOW\|O_CLOEXEC)` temporary creation (B:L72). | Unchanged, H:L112. |
| P08 | Keep created source FD through publication; same-FD regular/nlink=1/size/dev and `fstatfs(fd)` FSID checks (B:L72). | Unchanged, H:L112–113, L119, L136. |
| P09 | Write complete bounded frame, fsync, reverify source before rename (B:L72). | Consolidated, H:L105, L113–115. |
| P10 | Source name remains in protected parent; exported replaceable tmp is not object authority; measure export/`..`/hardlink aliases cannot reach stage, otherwise block (B:L72). | Unchanged, H:L121. |
| P11 | Destination directory matches persisted identity, verified before/after; fixed target only absent or prior published slot, replacement found on precheck violates protocol (B:L72). | Unchanged, H:L116, L118. |
| P12 | `renameat(stage_fd,tmp,host_write_fd,slot)` publishes protected source, atomically replaces target entry without following target link; target's post-check substitution cannot replace source identity (B:L72). | Unchanged, H:L117, L122. |
| P13 | Sync both parent directories, postcheck directory/publication, fail on anomaly, then close source FD (B:L72). | Unchanged, H:L117–119. |
| P14 | Guest may still rewrite/remove exported object; publication not provenance; guest reply tmp→msg in own directory remains untrusted (B:L72). | Unchanged, H:L122–123. |
| P15 | Existing tmp or any fsync/rename failure stops, no in-place fallback (B:L72). | Unchanged, H:L123. |
| P16 | Read step 1: trusted M parent FD verifies current mount, then component-wise `openat(O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` to guest-write (B:L74). | Unchanged, H:L127. |
| P17 | Read step 2: byte allowlist and no-follow fstatat precheck; all directory identities checked against persisted ones before/after (B:L74). | Unchanged, H:L127–128. |
| P18 | Read step 3: fixed file `openat(O_RDONLY\|O_NOFOLLOW\|O_NONBLOCK\|O_CLOEXEC)` (B:L74). | Unchanged, H:L129. |
| P19 | Read step 4: same-FD regular, 0<size≤4096, nlink=1, expected dev, opened-object `fstatfs(fd)` FSID (B:L74). | Unchanged, H:L130. |
| P20 | Read step 5: fixed cap+1 buffer through EOF, total poll ≤128 KiB (B:L74). | Unchanged, H:L131. |
| P21 | Read step 6: fstat compares dev/inode/nlink/size/mtime/ctime; fstatat confirms name still same object; decode/binding check then close FD (B:L74). | Unchanged, H:L132. |
| P22 | Each directory at most whitelist limit+1 entries; no sorting, recursion or scan-until-stable (B:L74). | Unchanged, H:L128. |
| P23 | FIFO swap relies on nonblocking open; device swap additionally on proven nodev; pathname queries cannot replace same-FD checks (B:L74). | Unchanged, H:L134. |
| P24 | Metadata rechecks are not atomic snapshots, cannot eliminate hardlink/same-file mutation; reads never authenticate origin (B:L76, L141). | Consolidated, H:L135; omitted repetition in §6 does not remove it. |
| P25 | FDs scoped to one bounded IO, except source FD through that publication; none across poll/attempt/unmount (B:L76). | Unchanged, H:L136. |
| P26 | Same-volume rename/fsync do not prove virtiofs timely/atomic view or power-loss durability; measure both (B:L76). | Unchanged, H:L136, C1/C2 H:L257–265. |
| P27 | O_NONBLOCK does not bound disk syscalls; external owner's provider deadline/kill-reap; uncertain exit/effect unknown and no new attempt (B:L76). | Unchanged, H:L137. |

**Codec, identity binding and all seven operation contracts**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| O01 | Four-byte big-endian length equals remaining bytes, strict UTF-8 JSON, fixed buffer before parse (B:L78). | Unchanged, H:L143. |
| O02 | Reject unknown/duplicate fields, trailing data, nesting>3, arrays>16, overflow/noninteger sequences (B:L78). | Unchanged, H:L143. |
| O03 | Strings only closed enums or ≤64-byte ASCII identities; digests fixed 32-byte encoding (B:L78). | Unchanged, H:L144. |
| O04 | Common fields `v=1,run_id,attempt_nonce,vm_config_digest,boot_id,op,seq,request_seq,status,payload`; request seqs equal, replies echo; public 128-bit nonce is binding, not key (B:L78). | Unchanged, H:L144. |
| O05 | Startup inventory boot=null; terminal supplies boot; all subsequent frames precisely boot-bound, boot change revokes (B:L78). | Unchanged, H:L145. |
| O06 | Request status=request, wire replies only accepted/terminal; pending is local incomplete state (B:L78). | Unchanged, H:L145. |
| O07 | Existing `GuestChannel` seven methods and version 1 unchanged; M3 normalizes bounded pending/accepted/terminal op/request/result body (B:L80). | Unchanged, H:L147. It does not add a method or capability. |
| O08 | Missing file/valid unfinished operation returns pending, not CallError; genuine IO/protocol errors fail closed (B:L80). | Unchanged, H:L148. The following explicit protocol-error rule prevents treating malformed input as benign pending. |
| O09 | Adapter privately holds request/seq; repeated poll of same logical operation never reissues it (B:L80). | Unchanged, H:L148, L190, L206. |
| O10 | Closed payload fields only; sequences/milliseconds u64, checked arithmetic, UID u32, typed booleans/enums (B:L80). | Unchanged, H:L149. |
| O11 | Four asynchronous operations' terminal has outcome ok/failed/unknown/cancelled; values only for ok, null otherwise; no free error text; interruption/failure not completion proof (B:L80). | Unchanged, H:L149, explicitly names bootstrap/login/run/revoke. Their accepted body is `{}`, as in the baseline operation rows. |
| O12 | inventory request `{consume_op,consume_request_seq,consume_revoke_seq}`, null/0 for none; only terminal snapshot with uid/desktop/config/sample_ms/active_op/active_request_seq/active_status/active_result/revoke_request_seq/revoke_status/revoke_result (B:L84). | Unchanged, H:L153. |
| O13 | inventory active=one ordinary non-probe side effect, priority revoke separate; none/pending/terminal, none=null/0, pending result=null, terminal original schema; snapshot seq immutable, fresh sample requires fresh request (B:L84). | Consolidated, H:L161. |
| O14 | probe request `{probe:closed Probe}`, no accepted; pending then terminal `{check:passed/failed/unknown}`; consume only same Probe/seq (B:L85). | Consolidated, H:L154, L162. |
| O15 | bootstrap `{}`; accepted `{}` only started; completion through inventory active_result `{native_ack_generation:u64}`; independent S3/S4 final native ACK/S12/real FD, disk field not ACK (B:L86). | Consolidated, H:L149, L155, L163, L218. |
| O16 | login `{}`, accepted `{}`, inventory terminal `{login:ready/missing}`; metadata only, no device code/link/credential (B:L87). | Consolidated, H:L149, L156, L163. |
| O17 | run `{}`, accepted `{}`, inventory terminal `{score:pass/fail/partial/timeout/crash}`; fixed preinstalled task only, provisional, no score before cleanup/seal (B:L88). | Consolidated, H:L149, L157, L163. |
| O18 | lease request `{inventory_request_seq,sample_ms}`; pending then terminal `{inventory_request_seq,sample_ms,until_ms}` exactly matching issued grant; no owner/scope/admission/cleanup authority (B:L89). | Consolidated, H:L158, L162, L173–174. |
| O19 | revoke `{}`, accepted `{}` not drain; inventory revoke_result `{scope_revoked:true,input_released:true,tree_exited:true}` completes; host independently stop/confirms exit; irreversible revocation (B:L90). | Consolidated, H:L149, L159, L164. |

**Pending, consumption, sequence and bounded obsolete-ACK state**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| Q01 | At most one ordinary side effect in flight; inventory independent; lease/revoke priority (B:L92). | Unchanged, H:L166, L161. |
| Q02 | Guest same-sequence reply immutable; ordinary slot retained until next request implicitly consumes it (B:L92). | Consolidated, H:L166, L172 (same-sequence changed contents violate protocol). |
| Q03 | bootstrap/login/run terminal, including cancelled by revoke, retained until host consumes and later inventory confirms; active+revoke retention ≤2 (B:L92). | Unchanged, H:L167. Revoke terminal retention is now explicit too. |
| Q04 | Enqueue confirmation only after terminal delivered successfully to machine and machine records consumption in same step; accepted/pending do not consume (B:L92). | Unchanged, H:L168. The H:L170 example follows this order and creates no alternative schema. |
| Q05 | Confirmation exactly matches retained terminal; already-consumed confirmations idempotent, wrong confirmation protocol violation (B:L92). | Unchanged, H:L168. |
| Q06 | Lost/expired result unknown; no side-effect replay (B:L92). | Unchanged, H:L168. |
| Q07 | M4 must update bootstrap/probe/login/run/lease/revoke and startup inventory parsing/transitions, not just login/run (B:L92). | Consolidated, M4 H:L332, L341–344. |
| Q08 | No accepted status creates score, native ACK, cleanup proof or admission (B:L92). | Unchanged, H:L164. |
| Q09 | Every op sequence starts at 1, continuous, no wrap; consumed old same-seq/same-byte slot only means still waiting; changed same-seq, wrong op/request/boot/attempt, future/unissued seq violate protocol (B:L94). | Unchanged, H:L172. |
| Q10 | Lease overwritable exception: host increments continuously, guest may skip observations (B:L94). | Unchanged, H:L173. |
| Q11 | Per attempt inventory and lease each ≤2048 requests; record all issued lease `(seq,inventory_request_seq,sample_ms,until_ms)` and seen reply digests; exhaustion revokes (B:L94). | Unchanged, H:L173; compatible-issuance proof added at H:L197–198. |
| Q12 | Old lease ACK actually issued in this boot/attempt and content-consistent is discarded even if first seen; never extends any deadline; missing-table/inconsistent ACK fails (B:L94). | Unchanged, H:L174. |
| Q13 | M3 owns codec/overwritten slots/delayed-frame decoding; M4 owns adoption and deadlines (B:L94). | Consolidated, H:L147, L172–174, M3/M4 H:L331–332 and scheduling link. No scheduler is assigned to codec. |

**Lease, caller scheduling, saved ends and Stop**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| T01 | `sample_ms=g` is this guest boot's continuous monotonic milliseconds; measure VM pause/host sleep advancement; if resume can revive an actually expired grant, block (B:L98). | Unchanged, H:L180. |
| T02 | Guest retains bounded table of truly generated boot-local inventory `(request_seq,g)`; only real referenced, nonfuture samples with nonoverflowing `g+2000`; repeated sample cannot extend deadline (B:L98). | Unchanged, H:L181. |
| T03 | Host echoes actual sample; guest deadline fixed at `g+2s`, never receipt+2s; renewal arrives before old end; initial grant before its own end (B:L98). | Unchanged, H:L182. |
| T04 | Expiry irreversible; last second reserved for revoke/release/kill entire tree, total ≤3s (B:L98). | Unchanged, H:L183, budget H:L96. |
| T05 | Measure sample-to-guest-acceptance age, effective grant gap and under-load termination completion; ~1s visibility cannot waive gate; never extend lease/Stop (B:L98). | Consolidated, H:L13–14, L183, L257–259, L346. |
| T06 | External owner proof over existing private control connection every ≤1s, attempt-bound and increasing sequence, reserve/preparation through seal; no local-now self-renewal (B:L100). | Unchanged, H:L185. |
| T07 | Guest renewal only after first boot-bound inventory until beginning revoke; no mailbox writes after VM exit; guest/owner/S3 leases independent (B:L100). | Unchanged, H:L186. |
| T08 | Before sending, accepting results or destructive dispatch, check old owner deadline; expiry permanently suspends; post-revoke lease/late terminal cannot restore scope (B:L100). | Unchanged, H:L187, L86. |
| T09 | Provider death external owner reclaims; both die only guest expiry stops model tree, no claim VM deleted; restart first recovery/persistent Stop (B:L100). | Unchanged, H:L188. |
| T10 | Same synchronous caller in existing owner keeps command/provider process handles and pending records; begin once, each step one bounded poll slice then return (B:L102). | Unchanged, H:L190. |
| T11 | Each iteration first Stop/owner proofs/all old ends, then revoke/expiry/applicable guest renewal, then one IO/state slice (B:L102). | Unchanged, H:L192–194. |
| T12 | Queuing/IPC/parsing/IO all included in ≤100ms total service gap; never 16 serial probes or several 100ms slices before yielding (B:L102). | Unchanged, H:L194. |
| T13 | Same loop for reserve readback, volume preparation, start/inventory/probe/bootstrap/login/run/revoke/stop/delete/detach/queries/seal (B:L102). | Unchanged, H:L196. |
| T14 | Runner must not wait for slow child exit; no thread, observer or runner-callback secret renewal (B:L102). | Unchanged, H:L196; lifecycle ban H:L24. |
| T15 | On first begin save `op_end=min(initial caller, start+cap, phase_end, aggregate_end, attempt_end)` (B:L104). | Unchanged, H:L200, same formula. |
| T16 | Origins: prep at reserve, start at volume preparation, Task at first run, cleanup at first revoke/cleanup (B:L104). | Unchanged, H:L202–203. |
| T17 | Single `machine/io.rs` per-slice dispatch clips to `min(current caller, saved op_end/phase_end, hard attempt end, owning aggregate)` including kill/reap margin (B:L104). | Unchanged, H:L205. |
| T18 | Query may have shorter cap but cannot exceed original bootstrap/login/Task aggregate/probe group/cleanup end; pending/accepted/new inventory never reset original ends or restart commands (B:L104). | Unchanged, H:L206. |
| T19 | If expiry within slice, discard late success first on return, stop new dispatch, kill/reap; uncertain effect unknown/quarantine; dispatched destruction cannot be claimed undone (B:L104). | Unchanged, H:L207. |
| T20 | Offline traces: slow query, sequential probes, 4s stop, owner proof during cleanup, within-slice expiry/kill-reap/no revival (B:L104). | Consolidated, M4 H:L332, L344. Long-login/max-task count/age/gap trace additionally required by #14. |
| T21 | Existing fake/IO lacks Task aggregate; immediate ACK/score, arming only Running, arbitrary successful revoke body cannot count as real wiring (B:L104). | Consolidated, exact M4 code obligations H:L341–342; H:L344 forbids login/run-only update. |
| T22 | Stop ≤100ms latch, ≤500ms release, ≤5s native, 4750ms driver, existing platform retry allowances; no auto resume; mailbox/VM budgets never enlarge constants (B:L106). | Unchanged, H:L209. |
| T23 | If filesystem blocking cannot meet scheduler contract, no admission of real channel (B:L106). | Unchanged, H:L209. |

**S11e supervisor and owner-only preparation**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| S01 | Trusted supervisor installed/sealed on no-credential preparation copy; independent protected principal; fixed program/startup-config digest (B:L110). | Unchanged, H:L215. |
| S02 | Restricted startup item starts only that program and waits for mailbox; never loads code from mailbox, no production SSH/exec/guest sudo (B:L110). | Consolidated, H:L215 and linked §1 H:L9. The #13 upload is a separate mechanics-only scope. |
| S03 | M2 deterministic mechanics probe independent of supervisor implementation; cannot substitute for its security evidence (B:L110). | Consolidated, H:L227, L249, L330, L335. |
| S04 | S11e must close OS-sandbox/mailbox/alias/FD/IPC/debug bypasses (B:L112). | Consolidated, A14–A17, H:L19–20 and §4 link H:L215. |
| S05 | Supervisor single event loop owns fixture/helper/private scope FD/entire model tree including descendants outside process group (B:L112). | Unchanged, H:L217. |
| S06 | Lost control/parent death/expiry/full disk first revoke, release, kill tree; no automatic restart (B:L112). | Unchanged, H:L217. |
| S07 | S3 install/renew/revoke via guest-inherited FD; require S4 final ACK/S12 live-code identity/real TCC, reject if any absent; no ordinary helper/shared UID/inherited admin shortcut (B:L112). | Unchanged, H:L218; native evidence H:L163–164. |
| S08 | Preparation installation/fixed-helper permissions only necessary owner GUI; per-attempt cross-UID/desktop/TCC/clone preservation verification; changes re-sealed/signed off; no logged-in state (B:L112). | Unchanged, H:L219. §2 no-elevation measurements and §5 upload do not replace this explicit later owner action. |

**Laboratory restrictions and common probe rules**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| L01 | Only pure C1–C10 mechanics may use disposable DEFAULT-Softnet/key-only SSH; exact `TART run CLONE_ID --net-softnet --dir <N>:<M>/mailbox`, no block/allow (B:L116). | Unchanged, H:L225. |
| L02 | Former entry is a pre-installed fixed-digest deterministic probe on no-personal-data/no-login prepared copy, launched by restricted fixed SSH command, independent of M7 (B:L116). | Weakened—authorized #13: pre-installation replaced by per-clone fixed-digest/size-bounded upload, H:L226–227, L237–249. Fixed code/no personal data/no login/restricted execution/M7 independence survive. This is the requested replacement, not text to restore. |
| L03 | Two non-admin test principals act only on their own objects; unauthorized cross-UID/chown recorded permission-denied; missing identity/privilege subcase unknown/defer; no invented guest sudo (B:L116). | Consolidated, H:L228, L246, L301. Explicitly no account creation/elevation authority from upload. |
| L04 | Before run require owner's one-line notice naming gateway exposure in this DEFAULT experiment (B:L118). | Unchanged, H:L230, M1 H:L329. |
| L05 | DEFAULT exposes measured host gateway ports 22/53/5900/445/7000; key authentication does not restrict egress; clone disposal cannot undo external effects (B:L118). | Unchanged, H:L231. |
| L06 | Trust reviewed image/startup scripts/deterministic code; no host-network/firewall change or reopening production H1; DEFAULT only experiment (B:L118). | Consolidated, H:L11, L225, L231, L323. |
| L07 | Pinned host key/exact clone-IP-boot mapping, no automatic key acceptance, dedicated restricted identity/disposable key, private key only host (B:L120). | Unchanged, H:L237; explicitly covers transfer and launch. |
| L08 | `ssh -F /dev/null -i <dedicated key>`, independent known_hosts, `GlobalKnownHostsFile=/dev/null`, `StrictHostKeyChecking=yes`, `IdentitiesOnly=yes`, `IdentityAgent=none`, `BatchMode=yes` (B:L120). | Unchanged, H:L238. |
| L09 | Publickey only; no passwords/keyboard-interactive, agent/X11/all port forwarding (`ClearAllForwardings=yes`), TTY, inherited config/ProxyCommand/ProxyJump, general remote shell or interactive authentication; fixed reviewed commands only (B:L120). | Consolidated, H:L239–240. The only added command scope is #13's bounded fixed-purpose transfer/verification/launch. |
| L10 | Bounded output; each command explicit ≤60s deadline, connection ≤5s, external timeout/kill-reap; probe local workload deadline; SSH/mapping uncertainty ends/quarantines (B:L120). | Unchanged, H:L242, L251–252; upload stages now expressly included. |
| L11 | No model/eval credentials, personal mounts, external downloads or intentional guest network requests, except host→guest SSH driver (B:L122). | Consolidated, H:L241, L246; fixed-command restrictions H:L240. |
| L12 | Autonomous C7 termination/all transport-liveness acceptance belongs to M7/M9, under block-all **using harmless substitute trees** (B:L122). | **Missing in part:** transfer/retest H:L249 and block-all H:L337, L346 survive; harmless substitute trees are absent. Restore finding 1. |
| L13 | SSH must not supply measured lease, expected file bytes or authoritative mount/cleanup results; patterns generated independently at both ends; guest/host local clocks (B:L122). | Consolidated, H:L247–248. More explicit separation of deployment from measured traffic/answers preserves the restriction. |
| L14 | Guest summary read via same-volume fixed ASCII ≤4096-byte test-result slot, separate test allowlist/nonproduction frames; host reads files/devices/mounts/ledger directly (B:L122). | Unchanged, H:L248. |
| L15 | SSH stdout diagnostic only; all results DEFAULT-network/mechanics-only, no production egress/S11e/control-liveness/real-eval proof (B:L122). | Unchanged, H:L249. SSH deployment digest also explicitly non-evidentiary for tested publication/mount/cleanup. |
| L16 | Every subcase separate attempt with exact ledger and synthetic canaries inside volume/across export/outside volume, no personal contents (B:L124). | Unchanged, H:L251. |
| L17 | Workload ≤60s, teardown ≤60s, obey shorter §2 command/phase caps; stop on first work/time limit; incomplete unknown, watchdog not filesystem failure (B:L124). | Unchanged, H:L251–252. |
| L18 | Report sample count, observation points, maxima/errors/completeness; no credible p99 from small samples; diagnostics/read-only output bounded (B:L124). | Unchanged, H:L252. |
| L19 | All resources absent before next subcase, otherwise quarantine; only C7 intentionally detaches while running instead of production stop-first (B:L124). | Unchanged, H:L253; strengthened to independent clone/all-resource absence and removal of copied probe. |

**Every finite C1–C10 workload, observation and outcome**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| C01 | C1: each direction 8 publications of 4096-byte seq/pattern; each visibility wait ≤3s (B:L128). | Unchanged, H:L257. |
| C02 | C1: both local write/fsync/rename/first-complete-read times, lost updates/sample round trip, open/read/publish-slice max and 100ms threshold (B:L128). | Unchanged, H:L258. |
| C03 | C1: only complete old/new frame, mixed/torn fails; slow mechanics may be reported but round trip ≥2s or insufficient fixed-grant margin blocks real channel, no publish-frequency rescue; finite success still needs block-all/loaded accept-age/gap/termination gates (B:L128). | Consolidated, H:L249, L252, L259, L346. Mechanics report is not lease admission; harmless-tree part separately missing at L12/K08. |
| C04 | C2: file-sync/directory-sync/rename boundary each independent exit-fault subcase, 3 publications each; record support/errors/cache/complete old or new bytes; incomplete never ACK; no power-loss durability conclusion (B:L129). | Unchanged, H:L263–265. |
| C05 | C3: each attack type independent subcase, ≤32 controlled replacements/≤64 reads; preplanted write tmp link, tmp rename/substitute after open, regular→FIFO/device after precheck, directory replacement, rename-into-place, cross-export/outside-volume hardlink, repeated-read mutation/replacement, case/Unicode aliases/special files (B:L130). | Unchanged, H:L269. The row remains in the host publish/read attack context, H:L267–271. |
| C06 | C3: synchronized fault windows; check source inode/dev, refusal/nonblocking/bounded memory/all canaries unchanged; stage unreachable/export substitute never published; refused hardlink/device syscall only observation, never relax checks (B:L130). | Unchanged, H:L270–271; missing coverage now expressly unknown. |
| C07 | C4: create and controlled attach separate subcases, ≤12 fixed option/permission probes; local internal-device/mount/scratch, root marker/layout/service observations; read flags/owners/ACL/capacity; synthetic execute/setuid/device/cross-UID tests; unauthorized unknown, host flags not guest sandbox (B:L131). | Consolidated, H:L275–277 and local-evidence rule H:L248–249; §2 recipe/internal-mount gates explicitly apply. |
| C08 | C5: after intent and after successful effect/before reply fault points for each effect; one fault/attempt, ≤12 queries; lost mkdir/seed/create/attach reply, detach before ACK, directory/image-inode replacement, device reuse/post-check retarget (B:L132). | Unchanged, H:L281–282. |
| C09 | C5: M5 controlled injection rejects in advance; real tests only registered synthetic resources; no destruction of replacements, missing identity quarantines (B:L132). | Consolidated, H:L283 explicitly invokes all five rules H:L77–87. |
| C10 | C6: file-FD and directory-FD separate cases; guest one synthetic FD until stop and measure impact; stop/prove VM entire tree exited before one limited host probe holds busy FD; normal/force at most once each (B:L133). | Consolidated, H:L287, L80–82. Exit proof is still mandatory, not simply elapsed stop time. |
| C11 | C6: record max/exit/independent absence; probe closes FD/kill-reap, timeout never unlink; no such busy FD retained in production (B:L133). | Unchanged, H:L288–289. |
| C12 | C7: live clone, detach only dedicated volume, guest 8 bounded local reads and 8 writes; observe errors/cache/Tart liveness, verify underlying M₀ identity/content no fall-through writes; anomaly blocks, mechanics only, autonomous stop/control-loss bound deferred M7/M9 (B:L134). | Consolidated, H:L293–295, L249. The missing harmless-tree restriction is finding 1, not permission for M2 to test autonomous termination. |
| C13 | C8: two prepared principals, each ≤12 create/chmod/chown-attempt/rename; both-side UID/GID/ACL/nlink/dev/inode/refusal observations; no chown elevation, no host-owner writer inference (B:L135). | Consolidated, H:L228, L299–301, linked H:L17 and H:L57–58/L246. “Existing” principals is the authorized #13 clarification, not a new setup privilege. |
| C14 | C9: separate ordinary-write ≤80×1MiB; sparse/truncate ≤16 and declared ≤1GiB; small files ≤8192×1KiB; churn ≤4096; cleanup held-FD reuses C6 single FD (B:L136). | Unchanged, H:L305. |
| C15 | C9: each batch and at end measure I st_size/blocks×512 and create scratch, obey §2 bounds; only predeclared scratch/backup/index storage observed, bounded ledger/receipts separate (B:L136). | Unchanged, H:L306–307. |
| C16 | C9: fill without ENOSPC unknown; other cases actual logical/allocated change; sampled peaks not universal bound or all-host-no-growth; full-disk liveness deferred M7/M9 (B:L136). | Unchanged, H:L307. |
| C17 | C10: index/FSEvents/backup separate 30s subcases, ≤16 1KiB synthetic canaries each; marker/exclusion readback from create-internal mount through after export (B:L137). | Unchanged, H:L311–312. |
| C18 | C10: eval-readable service logs/file access/index queries/actual backup coverage readback; first establish interface covers volume (B:L137). | Unchanged, H:L312. |
| C19 | C10: invisible service, uncovered phase, no actual backup job/idle-only all unknown, cannot infer no read/backup; privilege/global change requirement blocks recipe (B:L137). | Unchanged, H:L313. |

**Residual outcomes, delivery ownership and required owner actions**

| ID | Baseline constraint and source | HEAD disposition and location |
| --- | --- | --- |
| K01 | TCB still includes host kernel/filesystem/virtiofs/Tart/Softnet/provider/supervisor/helper; malicious IO can DoS, checks cannot eliminate concurrent writes; OS refusal to terminate/unmount only quarantine (B:L141). | Consolidated, H:L319 and the retained non-atomicity rule H:L135. |
| K02 | Image unlink is logical deletion, not SSD erasure; insufficient backup/index/capacity/identity proof means no run (B:L141). | Unchanged, H:L319. |
| K03 | No-IP control does not prove egress; S11c still must prove ADR 0018 §5 LAN/metadata/DNS/IPv6/UDP/QUIC, endpoint service boundary/counters; IPv4 `/32` insufficient (B:L143). | Unchanged, H:L321. |
| K04 | Endpoint leakage, short-lived/no-refresh credentials, residual process/windows and S3/S4/S12/TCC inherited from ADR 0017/0018 (B:L143). | Unchanged, H:L322. |
| K05 | All new volume/permissions/protocol/timing/recovery/real wiring/isolation/Stop/login/model-evaluation work remains not_run; only prior bidirectional visibility measured (B:L143). | Unchanged, H:L322. |
| K06 | Document-only/default-closed admission, no IP/serial/block-device fallback, general file exchange/execution or automatic recovery (B:L143). | Consolidated, H:L5, L323. Sole #13 fixed-program laboratory exception is expressly bounded, not a general transport fallback or admission. |
| K07 | Each card one reviewable session, ≤3 independent review rounds, one owning card per obligation; no observer/thread gap-filling (B:L145). | Consolidated, H:L325 and linked §2 H:L24. |
| K08 | M7 tests use harmless substitute trees under block-all (B:L155, repeated B:L122). | **Missing in part**, H:L335, L346 retain tests/block-all but no harmless-tree condition. Same missing constraint as L12/finding 1; one restoration fixes both occurrences. |
| K09 | M1 independent re-review; no host operation before passing; approved mailbox/volume decision plus pre-experiment notice (B:L149). | Consolidated, H:L329 linked H:L9/L230. |
| K10 | M2 owns S11-0c pure mechanics/no-elevation/results labelled DEFAULT-network/mechanics-only; no S11e/M7 dependency; failure shrinks design; no model login (B:L150). | Consolidated, H:L330 linked §5 H:L225–253. |
| K11 | M2 former owner-prepared probe installation/two identities action (B:L150). | Weakened—authorized #13: copied probe needs no owner install; existing two identities or unknown/defer, no account creation/elevation, H:L227–228, L330. Later M7/M8 preparation remains required at H:L323, L335–336. |
| K12 | M3 owns codec/open/publication, offline fixed slots/source replacement/same-FD races/bounded status/obsolete ACK decoding; independent safe-open/publish fake review; no owner action (B:L151). | Consolidated, H:L331 links full publish/read contract H:L112–137 and operation contract H:L143–174; same-FD race coverage also in C3 H:L269–271. “离线竞态” is not a waiver of those linked cases. |
| K13 | M4 owns all seven completion/pending/consumption/retention/overwrite-skips/late-ACK/first-lease/owner-through-seal-vs-guest/scheduling/deadline semantics; review before real wiring; no owner action (B:L152). | Consolidated, H:L332 links H:L178–207 and code/traces H:L339–344; protocol/consumption contract H:L145–174 still applies in full. |
| K14 | M5 owns persistent volume/directory identities/full destruction barrier fake: lost mkdir, replaced directory, M₀ versus V, lost replies, pre-effect retarget, recovery; cleanup/recover must not seal on VM-only success; no owner action (B:L153). | Consolidated, H:L333 links five rules/C5; H:L345 explicitly requires full resource barrier in `cleanup.rs:202`/`recover.rs:108`. |
| K15 | M6 private host-volume/Tart adapter only M2-measured fixed argv, connected to M5, no new lifecycle manager, admission still refused, only reviewed host scope (B:L154). | Consolidated, H:L334 links §2 lifecycle and §6 admission; H:L24/L323 retain prohibitions. |
| K16 | M7 owns S11e supervisor/golden/sandbox, autonomous lease/delayed grant/parent death/full disk/early detach/escaped descendants/mailbox-unreachability/Stop preservation; owner necessary preparation installation/sealing signoff (B:L155). | Consolidated, H:L335, L346, linked §4 H:L215–219. Block-all retained H:L346; harmless-tree omission separately K08. |
| K17 | M8 owns S11e admission and S3/S4/S12 real inherited FD/native ACK/TCC wiring/negative evidence; owner fixed-helper TCC clicks/change signoff (B:L156). | Unchanged, H:L336, with explicit M7/§4 prerequisite. |
| K18 | M9 combined S11c/d/f acceptance under block-all for transport/liveness/loaded timing/isolation/Stop/tree/volume cleanup; SSH results excluded, S11g separately scheduled (B:L157). | Consolidated, H:L337, L346; actual no-model gate remains, and finding 1 restores harmless trees. |
| K19 | M9 owner receipt review/necessary user recovery; only after every matrix §8 gate may login/model evaluation be separately scheduled (B:L157). | Unchanged, H:L337; Stop/user-only recovery H:L87, L209. |

**Round-2 #13 — every condition checked**

The requested per-clone fixed-artifact upload is implemented as specified, subject to the existing host-operation/design-review/owner-notice gates. It does not require another owner installation, modified golden, new service or GUI/TCC step for copying the probe; it does not waive M7/M8 preparation or the missing-second-identity outcome.

| #13 condition | HEAD evidence / result |
| --- | --- |
| Reviewed fixed-digest deterministic program, fixed upper size, each disposable no-personal-data/no-login DEFAULT clone, same restricted key-only SSH | H:L225–227: present. |
| No owner installation/new golden/service; unsafe copy/execute unknown, no privilege expansion | H:L227, L240; later preparation/sealing preserved H:L323: present. |
| Existing two non-admin identities only; missing identity/permissions denied/unknown/defer; no account creation/elevation | H:L228, L330: present. |
| Host verifies pinned source digest and size; guest verifies complete-copy digest before execute | H:L241: present. |
| Fixed ASCII path in fresh private directory of limited identity, outside mailbox | H:L241: present. |
| Reject links/special files/unexpected owner or mode/replacement; protect directory/file from other principals through execution | H:L241: present. |
| Pin interpreter/dependencies; no package install/download/fetched script/guest-selected command | H:L241: present. |
| Preserve all original SSH restrictions, host-key/clone-IP-boot binding for both transfer and launch | H:L237–240: present; exact options checked in L07–L09. |
| Only fixed bounded upload plus reviewed fixed verification/launch commands; no general shell/interactive auth | H:L240: present. |
| Verify SFTP/forced-command compatibility; unavailable if restricted route incompatible; no general SFTP, legacy shell fallback, privileged config or new service | H:L240: present. |
| Bound program bytes/output and connection/transfer/verification/execution/kill-reap within declared setup/attempt ends | H:L226, L242: present; ≤60s per command/≤5s connect and shorter enclosing caps retained. No fabricated numeric artifact cap is implied. |
| Lost result/digest mismatch ends subcase; retries never reset aggregate; probe local finite workload end | H:L242: present. |
| Remove artifact with clone; independently prove clone/all resources absent before next case | H:L253: present. |
| SSH deployment/driver only; no measured mailbox traffic, sample answers, lease or authoritative results | H:L247: present. |
| Each end independently generates pattern from reviewed fixed-digest program/specification and uses local clocks | H:L248: present. |
| Deployment digest only validates trusted lab deployment; stdout diagnostic, neither proves mailbox publication/mount/cleanup | H:L249: present. |
| Keep DEFAULT-network/mechanics-only/no production-egress/S11e/liveness/real-eval claims and block-all retests M7/M9 | H:L249, L337, L346: present. The separate baseline requirement for harmless substitute trees is finding 1. |
| Preserve pre-run owner notice/review scope and later M7/M8 install/sealing rather than treating upload as new admission | H:L230, L329, L323, L335–337: present. |

**Round-2 #14 — servicing versus issuance and cap-compatible M4 schedule**

Implemented as the report specified. H:L96/H:L194 preserve the ≤100ms *total servicing gap*, including queued work, IPC, parsing and IO. H:L197 expressly says this is **not** an instruction to issue a fresh inventory/lease each tick. It requires M4 to demonstrate a bounded fresh-sample/grant schedule across the longest supported preparation/task lifetime, explicitly including longest login/cleanup and first/completion/consumption samples, within **each 2048-request cap** and measured grant-age/gap limits. H:L344 requires long-login/max-task count/age/gap traces.

H:L173/H:L198 retain fail-closed exhaustion and gate failure if no compatible schedule exists; H:L93–96/H:L180–209 retain all budgets, saved ends, expiry and Stop constants. No numerical schedule or successful measurement is claimed to exist yet: round-2 #14 required M4's implementation/trace obligation, and HEAD now states it. Demanding native feasibility evidence in this document-only round would add a new review condition. The ADR does not enlarge a cap, extend a grant, or add a renewal worker to make the schedule fit.

**Restructuring checks and limits of this verdict**

Protected staging remains on the same volume outside the sole export, with source FD held through publication; it is not an additional shared directory. The source/destination checks and effect-time exclusion alternatives are retained, including their explicit non-atomicity limits. Same-sequence/consumption rules, delayed issued lease ACKs, boot binding, independent native evidence, old-owner suspension and indefinite quarantine remain requirements, not evidence already obtained. The new illustrative login sequence follows the authoritative table and consumption order and adds no launch/admission capability.

The existing caller/owner still owns temporary subprocess handles and performs the entire bounded loop. No thread, observer, daemon, shared lifecycle manager, runner-callback renewal, Drop cleanup, automatic resume, production SSH or second control transport is introduced. §4's links consolidate previously repeated production/no-elevation boundaries; its explicit owner-only preparation/TCC actions remain distinct from the unprivileged volume/probe measurements. Joint M7/M9 acceptance references do not reassign codec work to M4 or scheduling work to M3. The only lost test restriction found is harmless substitute trees, addressed by the exact restoration in finding 1. No additional new contradiction or ambiguity rises to must-fix; no cosmetic nits are requested in this final round.

Historical round-1 acceptance narration/its repeated link, redundant principle labels, and repeated residual-risk statements were shortened or removed. They are not discarded implementation constraints: unchanged admission/not_run/non-atomicity rules are mapped above, and both original reports remain unchanged. The one deliberate substantive relaxation is #13's pre-installation replacement, exhaustively checked above.

Verification was local document inspection, `git show`/diff/status and report validation only. No network, Cargo/build/test, VM/volume/SSH commands, host changes or git commit were performed. All native measurements remain **not_run**. Only this repo-root round-3 report file was written; the ADR and earlier reports were not edited.
