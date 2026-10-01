# Grok Windows install preflight patch

Base: **tauri-plugin-updater 2.10.1**, copied from the pinned crates.io package,
not an upstream checkout or a mutable Cargo registry edit. `UPSTREAM.json`
records the registry checksum and original hashes for every copied file,
including the build-required `api-iife.js`. Upstream MIT/Apache-2.0 license
files are retained. The upstream JavaScript updater API bundle is unchanged.
Two separately permissioned recovery commands extend the local plugin; the App
grants them only to local main/session windows, not pet/theme-editor windows.

## Why a local patch is required

The upstream Windows implementation calls a non-fallible exit callback, invokes
`ShellExecuteW` without checking its result, and exits the App from `install()`.
A post-install JavaScript cleanup command therefore cannot guard this path.

The patch keeps normal signed download verification, then binds installation
bytes to the exact checked Update with another signature verification. Windows
installation runs in an owned blocking task, including `download_and_install`.
Dropping an IPC waiter does not abort the native install owner.

The transaction order is:

1. Verify signature; select the single root installer without extracting unrelated archive entries.
2. Compile the native launch plan (modes, restart/custom arguments, absolute
   system MSI path, NUL/UTF-16 length checks) before any App cleanup. Persist
   the signed payload and exact installer, verify their readback under
   protected handles, and publish an authenticated per-user recovery journal.
3. Invoke the required App-managed **fallible** `WindowsInstallGuard`.
4. Start a byte-identical private App copy with job breakaway/no-window flags.
   It independently checks the compiled key/app/version/target/mode/arguments,
   signed payload, exact installer member and its protected read lease. Its
   DPAPI Ready receipt binds the full immutable candidate and exact helper
   PID/creation time. Ready is NOT permission to launch.
5. Publish one durable exact-worker delegation after cleanup. That worker alone
   invokes the checked `ShellExecuteExW` plan and retains its original returned
   handle. It publishes immutable Accepted/Rejected/Exited evidence; the App
   reconciles it before normal Tauri exit cleanup. MSI uses system msiexec,
   `SEE_MASK_NOASYNC` completes Shell handoff, not installation.

Preparation failure cannot stop services. Cleanup or known launch failure cannot
exit, replace the candidate, or restage it on retry. Each retry performs fresh
cleanup. Accepted launch is never replayed after an exit callback failure.
Unknown launch outcomes remain quarantined; neither elapsed time nor another
request authorizes replay or exit. A strong process-level owner retains the
staged transaction if the webview resource is closed.

## Process-owned UI recovery

The process catalog retains the original Update/config/guard and protected
prepared file. Its random candidate nonce is independent of Webview resource
ids. `pending_install` reads a short-lock snapshot without waiting for cleanup;
`resume_install(candidateId)` runs the original transaction on a blocking owner.
Resume never downloads, restages, substitutes a version or replays an accepted
launch. Dropping an IPC waiter does not release ownership. Unknown/abandoned
outcomes are blocked, not treated as permission to retry.

Snapshots distinguish running, retryable, blocked and failed-before-preparation.
The last state is an explicit terminal receipt: no cleanup/launch began, so a
new candidate is allowed. Missing or malformed metadata is NOT that receipt.
The original owned attempt records whether its initial staging closure failed.
Only that failure, an unprepared transaction, and a successful empty journal
snapshot together turn a staging `WindowsInstallPending` into a preflight
failure. This covers an incompatible signed completion contract without leaving
the healthy App quarantined. A resume with a missing prepared installer, any
published record, poisoned transaction/journal state, or uncertain publication
does not obtain this permission; launch-intent/accepted fences remain intact.
The UI checks ownership before manifest discovery (also before developer
simulation), recovers after remount, and polls the original candidate read-only.
Query failures disable mutations; stale polls cannot overwrite a newer attempt.
No fulfilled resume IPC is interpreted as installation completion. Both update
surfaces distinguish quarantine from retryable cleanup/launch refusal in all
15 locales. Uncertain plugin availability cannot route around recovery via
the GitHub fallback.

`WindowsInstallPending` alone serializes as a structured error with
`code = update_install_pending`, `phase`, and `message`; other errors retain
upstream string serialization. The frontend keeps the original Update resource,
shows installation **not confirmed**, blocks replacement checks, and retries the
same native transaction rather than downloading or declaring it installed.

The App registers the guard during setup before loading webviews. It shares the
existing cancellation-independent shutdown gate used by returning installers.
Call synchronous Rust `Update::install` on a blocking worker: the App's guard
bridges into its async shutdown gate and must not be nested on an async worker.
Both supported JavaScript install commands already enforce this offloading.

## Tests and maintenance

Run the vendor unit tests and the App library tests from the workspace manifest.
On Windows, embed the App's `windows-test-manifest.xml` into the exact test
artifact before running it. The owned launch fixture is intentionally ignored
as a standalone test: its parent test copies the harness into a private temp
directory, launches that exact child, verifies its process handle/PID, marker,
and terminal exit code, and reaps only that owned handle on timeout. It never
runs an actual installer. Signature vectors use a generated, public-only fixture;
the generator never saves its ephemeral private key or reads product keys.
The new inert MZ-prefix staging fixture is not a runnable PE or real installer:
its cleanup always refuses. A real Tauri ResourceTable close drops the original
resource; the production staging/recovery path retains the exact read-protected
file and re-invokes the original guard, while rejecting a replacement candidate.
Staging regressions exercise four publisher-signed unsupported completion
contracts through the production install entry, explicit failed snapshots, a
corrected fresh nonce and same-nonce cleanup retries. Native sharing denial on
the private active journal proves an uncertain publication stays blocked even
after the denying handle closes; a missing recovery payload also stays blocked.

## Cross-process prepared-candidate recovery

The App-scoped owner now holds a journal in the trusted App local-data directory.
The on-disk record is user-scoped DPAPI protected, bound to its store path, and
contains no executable path to be followed. Artifact names are derived from a
canonical random UUID and fixed suffixes. An exclusive OS file handle fences
other App processes; pinned directory handles reject reparse points and prevent
ancestor rename while owned. Artifact reads reject hardlinks/reparse points.
The initialization sentinel prevents a missing former journal from meaning empty.

After process death, a `Prepared` record restores the same nonce and exact file,
not a new download or re-extraction. The current App binary hash/path, source
version, publisher key, install mode and installer arguments must match; the
original package is signature-verified again and the retained installer is
compared to its signed member. Callbacks/App handles always come from the new
trusted App context, never disk. Original restart arguments are encrypted at rest.
Cleanup must run again before resume. Native discovery also honors the owner,
not just the React UI. Recovery IPC disk work runs on a blocking worker.

`LaunchIntent` is flushed before OS dispatch. An explicit OS refusal may return
it to `Prepared`; a panic, uncertain journal write or accepted handoff cannot.
The atomic publication uses flushed files and Windows write-through rename,
followed by readback. This is not a proof against every filesystem/power-loss
failure or malicious code already running as the same Windows user/admin.

Recovered intent/accepted records remain visibly blocked unless the explicit
NSIS completion contract below is proved; they are never automatically replayed.
PID/creation-time alone is **not** a completion/rollback receipt. Legacy/MSI,
unsuccessful and ambiguous outcomes still require authoritative reconciliation;
a successful real update must not ship with an unresolved quarantine path.

Recovery polling now opens only query/synchronize rights for the recorded PID,
matches creation time, and holds that exact process handle. It distinguishes
running, signaled exit, missing/reused PID and inaccessible observation. Exit
code 259 is terminal when the handle is signaled, not incorrectly STILL_ACTIVE.
A matched exit's identity/time/code is atomically appended to the DPAPI record
without changing `Accepted`; repeat observations do not rewrite it. Publication
failure remains blocked. Even exit 0 cannot grant resume, retirement or success.
New transactions use the independent dispatcher, not the older post-launch
handle-transfer observer. The copied helper starts before durable authorization;
its compiled policy is supplied lazily by the App's reserved pre-Tauri entry.
A local/debug or updater-disabled App refuses the dispatch role. No production
key, launch path or host policy is accepted through arguments, environment,
IPC or a journal-supplied key. The journal contains no arbitrary path to launch.

The helper validates its UUID-derived image path/hash and exact process creation
identity. It polls complete authenticated journal snapshots with READ|DELETE
sharing (never WRITE); this permits the owner's atomic replacement without
weakening protected installer/payload leases. No matching grant means no launch.
After the grant, the helper launches once and owns the original OS handle from
creation. The App can die before launch or after OS acceptance but before the
acknowledgement without losing custody to a parent-to-helper transfer gap.
Tests exercise both actual parent deaths with owned native processes.

Only an explicit OS refusal can restore the original Prepared nonce. Receipt
fingerprints include immutable signed-candidate fields, not just a UUID. An
outcome-bearing ticket cannot be reused. A late refusal can repair the blocked
in-memory owner only after signature/file/policy reverification and always
requires fresh App cleanup. Conflicting outcome/exit evidence fails closed.
Exit-only evidence can prove acceptance if acceptance publication failed, but
never successful installation. Accepted/unknown outcomes cannot be replayed.

A live authorized helper has no elapsed-time ACK expiry: a slow Shell/UAC
handoff remains an owned wait, and cancellation of a UI waiter does not stop
it. Pre-authorization readiness is bounded and grants nothing on timeout.
Helper death/identity loss, unavailable evidence, corrupt receipts and uncertain
publication stay quarantined. There is no restart-on-timeout path.

Legacy witness records remain readable. Their reserved entry still receives
an exact query/synchronize-only handle over its private bounded stdin pipe and
publishes immutable exits. Legacy observer launch/transfer setup remains only
in regression tests; the observer itself still cannot launch an installer.

This closes the specifically tested parent-death custody windows, NOT all
machine/worker failure cases or installation lifecycle recovery. Worker death,
power loss, installer delegation and authoritative rollback remain open. NSIS
terminal reconciliation is implemented below but needs native acceptance. The
real debug App entry is only a fail-closed/legacy-observer smoke test; unit
harness private copies are not signed installed-App or UAC acceptance.

The detailed local evidence is indexed in the Computer Use Windows update
preflight checkpoint. Library tests and compiling the complete App are **not**
signed installed-product install/update/rollback acceptance.

For an upstream upgrade, review/rebase these files explicitly:
`src/lib.rs`, `src/commands.rs`, `src/error.rs`, `src/updater.rs`, the new
`src/install_transaction.rs`, `src/install_recovery.rs`, and
`src/updater/windows_install.rs`, `src/updater/windows_install/durable.rs`,
`src/updater/windows_install/process_witness.rs`,
`src/updater/windows_install/launch_plan.rs`,
`src/updater/windows_install/verified_artifact.rs`,
`src/updater/windows_install/detached_witness.rs`,
`src/updater/windows_install/detached_dispatch.rs` and its tests,
`src/updater/windows_install/inventory.rs` and its tests,
`src/updater/windows_journal.rs`, `src/updater/windows_journal/witness.rs`,
`src/updater/windows_journal/dispatch.rs`,
plus `Cargo.toml`, `build.rs` and generated
permission references/schema. Refresh the upstream hash manifest and API bundle from the same
package; compare the full lockfile against the previous pin, run tests with and
without `zip` in a dependency-isolated validation workspace, and repeat real signed installed-product acceptance on all
supported Windows installer types. Do not remove this gate merely because
upstream installation or a download-only test is green.

## Remaining acceptance and lifecycle gaps

- Windows candidates may carry `windows_install_inventory` containing an exact
  UTF-8 JSON document and a separate minisign signature. Before journal creation
  or App cleanup, the compiled updater key authenticates those exact bytes; the
  document binds application/version/target/architecture/installer kind and the
  complete signed payload SHA-256. Recovery and independent dispatch reverify it.
  Strict bounded paths reject ADS, devices, traversal, case aliases and file-as-
  directory conflicts. The immutable dispatch fingerprint also binds this envelope.
- Once a durable installer exit exists, the candidate-version App can compare
  its current bound installation against that signed file list. Native directory
  and read-only file leases reject reparse points, hardlinks and existing writers;
  handles stay held through the whole observation. A match is only point-in-time
  file evidence. It remains `installer_exited`/Blocked, never retires the journal,
  releases another candidate, retries launch, or grants completion/rollback.
  Legacy candidates without inventory remain readable but cannot claim verified
  installed files. Invalid or explicit-null inventory is not silently ignored.
- Build scripts hash an extracted image without executing the installer and
  attach exact signed bytes to the selected platform metadata. Explicit NSIS
  mode excludes only root bootstrap plugins and the generated uninstaller;
  registry/uninstaller state is outside this application-file inventory. Output
  and payload cannot live inside the image. Local tests sign with the PUBLIC
  test-only key, not a release key. Release CI wiring is not CI-run or signed
  installed-product acceptance; new installer layouts require real verification.

- Independent dispatch now reuses the checked native launch plan and signed
  retained-artifact verifier. Dedicated test-only Rust/Node fixtures use an
  explicitly PUBLIC deterministic test key, compile an owned non-installer
  PE, sign its actual bytes, and exercise the production verifier/dispatch.
  That test key and test barriers are not compiled into production binaries.
  The earlier public-only ephemeral signature vectors remain regression tests.
  Neither test kind uses release keys or changes an installed product.
- Prepared recovery supplies the running binary's trusted key. ZIP physical
  central-header coverage still rejects duplicate names hidden by its map.
  Invalid launch data before staging is Failed/staging without App cleanup.

- No actual signed NSIS/MSI App update, UAC cancellation, installation completion,
  repair/rollback or reinstall acceptance is proven by these tests.
- Offline validation now covers both the product default `zip` build and a
  separate no-zip consumer workspace (`default-features = false`, `rustls-tls`).
  The real vendor library tests run in both feature graphs; Cargo artifact
  metadata confirms `zip` is absent in the second graph. The App workspace and
  its lockfile are not edited to manufacture this configuration. This is not
  a claim of native-TLS or every possible feature combination acceptance.
- Resource-close, UI remount and prepared-candidate process-restart recovery are
  implemented. The declared NSIS success callback can retire one proved outcome;
  unknown outcomes, MSI completion, rollback and real installed-WebView
  acceptance remain required.
  A different prepared candidate remains blocked.
- Cleanup may partially stop services before a later cleanup/launch failure.
  The retained candidate is not a claim that every service is still running.
- Ordinary Quit, clipboard/native input retention, and whole-product update
  recovery remain separate required lifecycle work.
- The production durable path does not extract unrelated archive entries.
  Orphans from interrupted preparation/publication and older upstream temp
  directories still require a safe, identity-aware garbage-collection policy.
- macOS/Linux installed-product regression, native UI matrices, real Grok E4,
  and a 12-hour active soak of one final frozen candidate remain required.

## Publisher-declared NSIS completion (current-user installations only)

The optional signed inventory `completion` object declares exactly
`grok-nsis-install-complete-v1`, the bundle identifier and `currentUser` scope.
Unknown/null contracts and a contract attached to MSI are refused. Older signed
inventories remain readable but never gain completion permission retroactively.
The launch plan derives the receipt path from the pinned journal directory and
original nonce, rejects existing receipt paths and reserved installer overrides,
and places native Unicode completion arguments before NSIS `/ARGS`.

The production `.onInstSuccess` callback reads back the expected installed
registration and main/uninstaller paths, then writes a CREATE_NEW UTF-16LE
receipt with candidate nonce, product/version/path and its native PID/creation
time. It does not modify registration or execute the App. Callback bytes are
not a separate publisher signature or protection against same-user/admin code:
trust depends on the authenticated payload/contract and original native process
custody, within the journal's existing local-user threat boundary.

The candidate App at the original path must verify exact installer exit 0,
the complete callback record, and all publisher-signed installed-file hashes.
Receipt, tree and terminal archive read leases extend through publication.
Only then is a bound DPAPI terminal archive flushed/read back and the active
journal atomically cleared. The process catalog releases the old nonce only
after this durable CAS; the next candidate can then stage normally. A failed
archive/active publication cannot release it. Reopening after an archive-only
write re-verifies the callback/files and finishes idempotently, not by timeout.
A previous process-witness cache never substitutes for the next journal identity.

The native status publishes an explicit same-nonce/version `completed` terminal
receipt, not an ambiguous missing-owner/null. React accepts only its exact
original receipt and returns to ordinary discovery without reinstall/relaunch.
A slow window may query its original nonce after another window begins the next
candidate: the bound DPAPI archive and callback are reauthenticated read-only,
without claiming that the historical image is still the current live tree or
releasing the newer owner. The historical DTO cannot construct the journal's
write-authorizing completion capability. Missing/corrupt history stays blocked.

Tests combine actual owned process exits, the actual test-image hash and
explicitly synthetic callback bytes. Native file sharing exercises a failed
active-slot clear after terminal publication. A separate NSIS fixture still
compiles the production callback without executing that compile-only installer.
An owned non-installing fixture now executes the exact shared native writer,
checking Unicode bytes and independently held native process creation identity.
Another inert fixture invokes the complete production `.onInstSuccess` callback
against real registration in a process-private application hive. It requires
successful RegLoadAppKey, RegOverridePredefKey and a private fence read before
any registration write; failure paths abort, and host 32/64-bit HKCU views are
checked for absence of the fixture's unique keys. The callback and independent
private-hive readback use the 64-bit view, matching production x64 SetContext.
It never executes an installed
App/uninstaller or writes the real Grok registration. This exposed NSIS's
case-insensitive nonce lookup: an explicit StrCmpS now enforces canonical
lowercase UUIDs before callback publication. The native receipt can also be
fed to the explicitly ignored Rust parser test via
`GROK_NSIS_NATIVE_CALLBACK_FIXTURE`; CI runs it explicitly, never counts an
absent fixture or a zero-test filter as success. Parsing this evidence does not
grant journal retirement or substitute for signed inventory/live-file checks.
None of these proves a signed Grok installation, UAC, repair/rollback, MSI
completion, installed UI behavior or final release readiness. The protocol and
archive need explicit review when rebasing the NSIS template or vendor.

## Publisher-declared failure/cancellation evidence

The signed NSIS completion contract may explicitly include
`failure_protocol: "grok-nsis-install-failed-v1"`. Missing means legacy (no
failure inference); null, a non-string or an unsupported version is rejected,
not silently downgraded. The release inventory producer opts in explicitly.
The original candidate's checked launch plan adds a reserved, quoted,
CREATE_NEW-only `/GROKUPDATEFAILURE` path before any forwarded application args.

Production `.onInstFailed` and Modern UI's `MUI_CUSTOMFUNCTION_ABORT` publish
separate `failed` / `cancelled` outcomes. Modern UI retains ownership of
`.onUserAbort`, including any configured confirmation before the callback.
Both writers share the strict canonical nonce gate. Failure does not read or
assert restored registration: installation may have partially changed files.
These are not independently signed receipts; the existing local-user trust
boundary still requires the authenticated payload/contract, original candidate,
App/layout, exact native PID/creation identity and persisted nonzero exit.

Recovery rejects conflicting success/failure files, wrong fields, missing exit,
zero exit, truncated/oversized encodings and multiply-linked/non-plain files.
Verified outcomes refine the **Blocked** diagnostic to `installer_failed` or
`installer_cancelled`, including after restart; they cannot construct a
completion capability, retire the journal, resume/replay or replace its owner.
Legacy success contracts and their authenticated archives remain compatible.

Registry-free inert NSIS fixtures execute a real failed Section and a real MUI
abort callback. The cancellation fixture sends Cancel only to its own window
after its page timer starts; its parent retains the exact process handle before
releasing the fixture permit. Rust's explicitly invoked native test consumes
both actual callback files and independently captured process identities. These
tests do not execute a Grok installer/App, prove repair/rollback, or validate an
installed application's visual UI. Full signed install/update/repair/rollback/
uninstall and cross-platform release acceptance remain separate requirements.

## Atomic journal publication with concurrent helper snapshots

An owned native regression reproduced `Access denied (5)` from
MoveFileEx(REPLACE_EXISTING) while immutable helper snapshots were open with
READ | DELETE sharing. A ReplaceFile experiment removed that refusal but
exposed a missing-path interval to concurrent readers; it is not the product
solution. The original sporadic authorization log did not record its API stage,
so reproducing this defect does not retrospectively prove every old error's cause.

Existing active records now use SetFileInformationByHandle(FileRenameInfoEx),
REPLACE_IF_EXISTS | POSIX_SEMANTICS, with a checked/aligned native UTF-16 buffer.
The fresh file is write-through and sync_all-flushed before publication; its
replacement handle has DELETE/attributes access but no WRITE-sharing relaxation.
Initialization still uses no-overwrite MoveFileEx with WRITE_THROUGH. Publication
errors retain stage + native error diagnostics, poison the in-memory journal and
preserve evidence. There is no retry, delete-first, older-API fallback, readonly
override, successful-install inference, or second OS installer dispatch.

Only read-only helper snapshots may validate a zero-link already-open old file
retired by POSIX replacement; they still reject reparse points, directories and
multiple links, and must pass DPAPI/schema/candidate checks. Installer/image and
owner readback leases remain strict single-link, no-delete-share reads. Native
tests retain old bytes across a new publication, exercise four concurrent
readers, and prove no-delete-share/readonly failures still fence further writes.

This path requires Windows/filesystem FileRenameInfoEx POSIX support (SDK gate:
Windows 10 RS1); unsupported native operations remain blocked, not silently
downgraded. This phase tests the current Windows/NTFS host, not older-Windows,
other filesystems, sudden-power-loss durability, signed installed Grok, or the
complete cross-platform acceptance matrix.
