# Imported session titles

## Behaviour added after the handoff (2026-10-22)

- Reader: when the shown events include a compaction with `covered_event_ids`,
  "View the conversation before compaction (N)" opens the last covered event's
  leaf in the same revision; "Back to the latest conversation" returns. Reading
  only; it goes through `openImportedHistory`, so history and continuation
  rules are unchanged.
- Undo: items Undo keeps (continuation reference, later revision kept,
  unproven ownership) form a neutral "Not undone" group. Undo still writes a
  tombstone; a later ordinary import of that session reports `tombstoned`
  (finished, not retryable, explained in the results). Only the default-off
  "Re-import sessions I previously undid" choice on the Projects step passes
  `explicitReimport`, for that commit only; it cannot combine with confirmed
  history links.
- Image reader: links at or below the app-data root are refused; the OS layout
  above it (macOS `/var`, ostree `/home`) is not judged.
- Progress: background job reads pause twice their own duration (250 ms–4 s);
  each read still authenticates the whole signed job.

## Current handoff (2026-10-21)

Read [the Claude handoff](../plans/2026-10-21-session-import-claude-handoff.md)
before the historical checkpoints below. Branch
`handoff/session-import-claude-20261021`, code checkpoint `572ab60a`, saves the
complete cross-layer WIP; no push/PR/release. V814 fixes confirmed-selection
summary and receipt-selected default reader/search/context without rewriting
the source graph or widening grants. Its scoped synthetic Chinese Windows
image/branch/job-reopen and same-path restart checklist passed on 10-16; closed
read-only audit passed on 10-21. No test App remains running. The fixed preview
still points to V789; V814 is a candidate, not whole-feature delivery.
Original53/vendor/live-WAL/actual dispatch/vision/scaling/platform gaps remain.
Old missing-image and pending-restart descriptions below are historical.

## Historical checkpoints

Image reader checkpoint (2026-10-16): ordered preview/reader image blocks and
authenticated identity-only thumbnail/original delivery are implemented. N809
passes7image checks; N808passes1316migration/strict all-target Clippy. F810all7
frontend gates pass899files/10403tests, F805bundle and V806embedded Dev build
pass. Ignored tests are not passes; N800's4318library run predates later fixes.
V806desktop is not accepted: images eventually resolve, but an8.4MiB embedded
source-reference catalog loads slowly. v811adds buffered catalog I/O and a4MiB
read-budget plus duplicate/trailing-data regression; N811passes9image checks.
N813strict all-target Clippy passes. V812embedded Dev build passes119.701s;
desktop completes large embedded thumbnails/originals and Codex/Claude publication.
Selected branchAis correctly persisted; summary and unscoped human reader still
show source-defaultB. Follow task-root plans/v814-published-reader-branch-default.md
for that fix and remaining restart/selection checks. All V806/V812owners quit
normally; V806did not publish, neither run dispatched a model request.
Sole usable preview remains V789. Below missing-image UI notes are historical;
original53/real source/vision/ACP/scaling/platform gaps remain open.

Codex content retention checkpoint (2026-10-16): ordered text/image/text and
pure-image legacy messages, split event images/file_ids/image_order/local_images,
content-aware adjacent projection pairing and replacement-history image assets
are preserved by source occurrence. Unknown content is opaque/partial with no
raw private payload in searchable/model prose. Opaque file IDs never become local
paths; remote URLs are unresolved with no fetch. Strict base64 materialization
rejects malformed bytes; Codex file normalization leaves pending assets for the
existing final/no-root outcome checks. Frozen schema/catalog/normalizer and
plain-text serialized shape remain unchanged; old archives are not rewritten.

N797 full gate passes9focused/1309migration/4313library,0fail, fmt/compile/strict
all-target Clippy;103/107ignored are not passes and sets overlap. N798 explicitly
passes3historical discovery/continuation/grant tests,0fail/0ignored,26seed stable.
Fresh871native/2579frontend/196LF-noBOM texts/40quality and exited owners are
verified; F792 frontend hashes unchanged. Five exact owned temp directories were
retired,10500855logical B, observed free-space delta10919936B measured separately.

This is backend retention/authorization, not finished image UI or vision use.
The reader currently marks every asset missing; previews flatten ordered blocks,
and extensionless digest blobs would get non-image MIME/2MiB206 delivery. Next
task-root plans/v799-imported-media-reader-plan.md covers these concrete gaps.
MCP bytes/base64 success does not prove provider image understanding. Non-string
tool results and replacement-history-plus-rollback remain explicit source-review
questions, not confirmed fixes. Cursor/vendor/live-WAL/ACP/platform/original53
gaps remain. Sole V789 preview is unchanged and does not contain N797 native;
no new package or desktop acceptance was produced by this backend slice.

Current completed-job contract (2026-10-16): read-only paged Import jobs headers
and explicit selection reuse durable results/Undo without changing automatic
recovery/schema/codec. Successful submitted preparation no longer blocks history;
a newer unsubmitted batch still does. N787 full native3history/1299migration/
4303library pass,0fail, strict all-target Clippy/fmt/compile pass;103/107ignored
are not passes and sets overlap. F792 all7frontend gates pass897files/10393tests;
F793 bundle/V789 embedded Dev build bind final inputs. Fresh866native/2579frontend/
26seed/189LF-noBOM texts/40quality checks pass with exited original owners.

Actual Chinese synthetic Windows V789 UI reopens completed tasks after restart.
Copied profile1 actually clicked Confirm Undo but original absolute-publication
path authentication refused; closed audit is unchanged and original V762 batch
is NOT undone. Separate fresh profile2 actually imported3Claude fixtures, edited/
created a prepared unsent continuation, restarted, reopened its new batch and
confirmed authorized Undo:2undone/hidden,1retained due to continuation reference.
Second restart preserves results/sidebar/edited handoff; closed readonly audits
pass4entries/5revisions/5committed receipts,39seed/12Cursor archive files unchanged,
no dispatch. Five lifecycles each quit all8owners normally and consume guards.
Known UX debt: accurate reference reason also shows generic mapping-conflict copy.
V789 is selected by sole start-current-preview.cmd; plain-cmd CheckOnly verifies
the exact hash without launching or opening/copying the stable user data path.
Original53/media/scaling/vendor/live-WAL/ACP/UNKNOWN100/platform/UX gaps remain.
Next is task-root plans/v785; V736/V762 notes below are historical, not current
missing-entry/Undo status. See v786 and V789 desktop observations/audits.

Branch/assets outcome contract (2026-10-16): N783 full native gate passes
19 capacity /1296 migration /4300 library tests,0fail, fmt and strict all-target
Clippy; overlapping sets are not added and5/103/107ignored are not passes.
N784 explicitly passes88 historical compatibility tests,0fail/0ignored, with26
immutable seed files unchanged. Real Claude two-branch/four-image scan/preview/
prepare/offline publication, full human reads and narrowed persisted MCP grants
pass, including denied asset IDs sharing an allowed blob digest. Fix graph-time
recount so pending image materialization does not permanently mark complete data
partial; final/no-root recount stays strict, preserving real truncation/missing/
remote issues. No schema/codec/version bump or old archive rewrite.864native
inputs/HEAD/index/EXE stay bound to N783/N784. This is bounded synthetic evidence.

V762 restart follow-up after explicit human Undo permission: the older isolated
package starts normally and retains4 histories, but its wizard opens Sources
without the completed batch or a way to reopen it. Current engine_recovery.rs
explicitly omits Completed jobs; recovery_tests.rs protects that behavior.
Automatic recovery is not a job-history UI. Confirm Undo was not reachable or
clicked;2undone/1reference conflict remains unverified. Closed readonly restart
audit passes and catalog SHA256 equals the original before-undo audit;4entries/
5revisions/5receipts,39seed/12Cursor archive files and prepared/no-dispatch draft
are unchanged. All8owned processes exit normally. V762 does not prove current
native changes; V736 stays sole preview. Add an explicit read-only completed-job
selection entry before retrying this desktop acceptance; do not restore consumed
plans or mutate the database to manufacture an Undo result. Original53 remain.
Evidence: task-root plans/v780 and plans/v786-completed-job-reopen.md.

Prepared mixed association contract (2026-10-16): N778 compiles and passes13title
tests; N779 explicitly runs all3seed-bound prepared-association tests,0fail/0ignored,
and strict all-target Clippy passes. The new test imports72ordinary sources
(24each Codex/Claude/Cursor) plus one historical Cursor association, with37scoped
selections. Real discovery/prepare/durable plan/cache reset/offline ordinary roots/
public commit/worker publication preserve all218events and exact receipt/member/
selection/draft bindings. The historical import ID, old revision/receipts/job/archive
and source bytes survive; replay publishes0. Only test files/registration changed
from N775 (861native inputs). N777's initial failure was a test comparing a field
selection hash with the full preview/leaf selection-set hash; product hashing did
not change. This closes a bounded mixed correctness gap, not scaling, vendor,
desktop or original53 acceptance. Only Cursor legacy association is supported;
V736 remains sole preview. Detailed evidence: task-root plans/v776.

Tool-field pipeline contract (2026-10-16): N774 capacity17pass/0fail/5ignored;
N775 MCP12 and local continuation58 pass, strict all-target Clippy passes.
Twelve independent Codex/Claude/Cursor JSONL sources use real scan/fixed groups/
async prepare/validated plan/offline worker publication. All60 archived events,
tool JSON/call-result pairing and exact receipt/member/selection bindings survive.
All-off/name-only/input-only/result-only policies constrain actual draft text,
readable/fragment IDs and persisted local-session grants. Real tools/call searches
traverse all pages; excluded tool fields yield no hits, and fully suppressed tools
cannot be read by their IDs. Human paged reads stay complete before and after MCP.
Sources stay byte-identical; same-request replay publishes0; local sessions remain
prepared with empty messages and no dispatch. Only three test files/two registration
lines differ from N767's native inputs (858now/855then). This is a focused synthetic
contract, not current full/Release/platform/vendor/desktop/provider acceptance.
Original53/association capacity/branch/assets and pending Undo obligations remain;
sole preview stays V736. Task-root plans/v771-tool-field-pipeline.md retains the
three initial test-contract failures and final evidence; no production change or PR.

Current original-input range validation (2026-10-16): a full snapshot now reads
job-scoped row identities/types once, then still streams/hashes every original
body against the freshly authenticated complete member set. No schema/codec or
cross-write/transaction SQL authority cache changes. N766 corruption/type/neighbor
regression passes; N767 full six-command gate passes13titles/1293migration/
4297library,0fail/strict Clippy,102/106ignored excluded. N768 exact855input opt3
compile/discovery passes. N76964 and N770uninterrupted1000 each pass every archive/
revision/receipt/reader assertion. Thousand publish537096ms/verify18129ms, wrapper
596.3662102s, max poll61ms, peak test working set50806784B. At64, point lookups
46208→1024 while all47940raw hashes/132809448B remain identical; wall publish
9094→9031ms is essentially unchanged, not a stable/causal speedup. Thousand still
checks11002full snapshots/27019000members: quadratic scaling remains open. This
is synthetic prepared publication, not mixed/scoped/vendor/desktop acceptance.
Original owners/session handles exited/consumed, source/Git stable, own temps
removed. Sole V736/pending V762/original53 gaps stay. See benchmark-baseline and
task-root plans/v766-input-range-streaming.md; do not mechanically start10k.

Bounded mixed-source contract (2026-10-16): N764 capacity tests16pass/0fail/
5ignored; N765 new regression1pass and strict all-target Clippy pass. Twelve
synthetic Codex/Claude/Cursor files (4each), half scoped, use real scan/fixed
group pages/prepare/durable plan/offline publish/receipts/paged reader/drafts.
All36archive messages survive; six narrowed drafts exclude the unselected
reply and retain pins/field switches. Sources stay byte-identical; replay
publishes nothing. Only tests changed from N757. N763's first failure was an
incorrect test requiring Cursor member pins for all providers, corrected to
the actual source contract. This is not current full-suite, vendor, UI or
capacity acceptance; association mix/branches/assets/tool-field content and
large-scale obligations remain. N759 optimized EXE does not contain this test.
See task-root plans/v763-mixed-scoped-contract.md; V736 stays the sole preview.

Current-source desktop follow-up (2026-10-16): embedded Dev V762 builds in117.016s;
fresh3480build hashes match. Chinese Windows synthetic UI imports3Claude fixtures
history-only, reads typed tools/current compaction chain, finds1summary match,
edits/creates a continuation and reopens saved context/completed job. Closed audit
confirms4entries/5revisions/5receipts, exact6/5/4Claude archive events/source hashes,
12unchanged Cursor archive files/39seed files and prepared/empty messages/no
dispatch. All8owned processes exit normally; WebView cleanup13139488logicalB,
observed free+13357056B separately. Actual Undo stops at confirmation pending
action-time user approval;2undone/1referenced-entry conflict is expected, untested.
V736 remains sole preview; V762 is not promoted. Original53/real vendor/ACP/native
platform obligations stay open. See task-root V762 observations/smoke/closed audit.

Optimized follow-up (2026-10-16): N759 compiles/discovers the exact N757 source
under actual release opt3 in817.0007383s; not a full Release suite or App build.
N76064 items pass every archive/receipt/reader assertion, publish9094ms. N761
thousand cancel/offline-resume passes1test: cancel after201commits returns87ms,
worker stops1546ms, retry75ms resumes799items; all1000 entries/revisions/committed
receipts/readers match and replay does not duplicate. Wrapper641.2332073s,
resumed publish466925ms, verify17537ms, resumed poll max320ms. This excludes
uninterrupted-thousand, mixed/scoped,10k,vendor and UI acceptance. Whole-set
scaling remains unresolved; no statistical/causal speedup claim. Original owners
exited/consumed, source/Git stable, own empty temps removed (0logicalB/free delta0).
Fresh853native/2574frontend/26seed/Git/EXE/166LF texts/40quality/owner checks pass.
The old62MiB N746 comparison copy is deferred after automatic approval rejected
the cleanup script; no removal or reclaimed space is claimed. Sole V736 preview
and original53 remaining acceptance scope are unchanged. Evidence: benchmark-
baseline and external plans/v756-member-commitment-review.md.

Current candidate (2026-10-16): exact authenticated member bytes retain their
pure input/allocation commitment. Current SQL range/root/input/allocation and
every post-write authorization remain fresh; no schema or seal changes. N754
acceptance35 and N755 publication96/strict Clippy pass. N756 Debug64 verifies
all entry/revision/committed-receipt/reader results: range3981.928→2810.085ms,
publish24370→23381ms, worker23447.711→22900.566ms. This is one diagnostic,
not Release/UI speedup or a scaling fix. N757 full six-command native gate now
passes: migration1291/library4295,0fail, strict Clippy;102/106ignored are not
passes. N758 explicit historical87 pass/26seed unchanged; fresh853native/
2574frontend/Git/EXE/166LF-noBOM/40quality/owner exit checks pass. N745 below
proves earlier source. See benchmark-baseline
and external plans/v756-member-commitment-review.md. Sole V736 preview and all
original53/native-platform/real-source/provider acceptance gaps remain.

Current native-only writer reuse (2026-10-16): the private authenticated writer
may supply its existing signed acceptance only in the exact unchanged
transaction/connection/job. total_changes invalidates it even after savepoint
rollback. Selected member/input/allocation reads and all post-write checks
remain; no read-only evidence becomes SQL authority. Creation avoids five
duplicate header loads per item; dedup avoids one. N743 publication96pass,
N744 explicit historical87pass/26seed unchanged and N745 six commands pass:
migration1289/0fail/102ignored, library4293/0fail/106ignored, strict all-target
Clippy. Sets overlap; ignored are not passes. Native853/source/Git stable;
F737's2574 recorded frontend inputs still match. N746 compiles/discovers this
exact source under release opt3 in858.8136169s; this is not a full release suite.
N747/N748/N749 each pass one64-item publication test with every durable/reader
result checked: publish10118/10252/10077ms, median10118ms. Headers2052→1732 and
members132288→111808 confirm15.6% less repeated work; full snapshots remain706.
No observed wall-time speedup over historical N739's9041ms. An unmatched old
single run and new three runs cannot establish a causal regression either.
Do not repeat unchanged workloads or jump to1000/10k; first attribute actual
publication stages and input metadata/blob-open costs. Existing hash spans start
after blob_open and total only~0.106s. Inclusive spans overlap, never subtract
them as exclusive time. See benchmark-baseline and external plans/v750. All
original owners exited0/sessions consumed; three own empty runtime dirs removed,
0logicalB/observed0B each. Do not reuse N732 as changed-source proof. V736 remains the sole
preview and does not embed this change. Its current/old reader and version-menu
screenshots were freshly saved as3 JPEG/TXT pairs using the same synthetic
profile; all8 owned processes exited normally and WebView was cleaned. This is
existing-preview evidence, not current-source desktop/full53 acceptance.

Historical optimized baseline (2026-10-16, before writer reuse): N738 compiles/discovers the
same N732 source under actual release opt3; it is not a complete release suite.
N73964/N7401000 ordinary prepared publication each pass1test, verifying every
entry/revision/committed receipt/reader result. Publish9.041s/639.413s; thousand
wrapper696.110s, terminal35ms/catalog2ms, concurrent job poll max772ms. Worker
still performs32004 header/32019000 member checks. Do not launch10k mechanically
or sum inclusive profile spans. Original sessions exited0/consumed; source/Git
stable, own empty runtime cleaned. Current mixed/scoped/cancel/vendor/UI remain
unmeasured by these runs. See benchmark-baseline and external plans/v738/v741.
At that baseline, N732/F737/V736 evidence was unchanged; no preview was replaced.

## Current preview: imported revision navigation (2026-10-16, V736)

`session_import_revisions` lists at most50 metadata rows from a readonly catalog
transaction, with history/generation-bound authenticated cursors. It does not
authorize archive reads. The lazy History versions panel uses the existing
Select and domain navigation, clears branch/anchor on explicit selection and
pins reading/search/export/continuation to the selected revision. Errors keep
the prior page and retry destination; closing drops late replies. An unavailable
latest body does not prevent explicitly opening an older available archive.
All15 locales are translated. Seven hook and four real Host/Select integration
tests pass; the latter cover pinned actions, metadata refresh, keyboard/Escape,
close/reopen races, failed-page retry and recovery from an unreadable latest body.
Frozen-source N732 passes all six native commands: migration1285/0fail/102ignored,
full library4289/0fail/106ignored and strict all-target Clippy. Sets overlap;
ignored are not passes. F737 passes seven commands/895 files/10,376 tests;
F735 bundle and V736 embedded build119.741098s bind exact inputs/Git. F734 was
source-unstable and is superseded. Prior N730 historical87/retention44 evidence
was not rerun. Chinese synthetic Windows desktop selects/reads both exact
revision URLs, searches old text, preserves old query on metadata refresh,
inspects/cancels continuation review, resets on current selection and closes
only Select with Escape. Both bodies are identical: distinct-body/exact-action
scope is Host integration evidence, not a desktop text-change claim. Closed
audit preserves39 seed/12 archive files, two revisions/two receipts and current
revision. All eight owned processes exited normally; WebView cleaned. Fixed
entry is V736; original53/actual Undo/current mixed-scoped and10k/vendor/ACP/platform/
IME-DPI/15-language desktop acceptance remain open. The sections below retain
historical evidence; their older current-entry/missing-selector claims are superseded.

## Historical V727: prepared-batch association review (2026-10-16)

The mapping UI now explicitly reviews/links prepared Cursor members. Host looks
up the actual canonicalized prepared selection by batch/version/candidate/preview,
then retains prepareOrigin and the original finalize/receipt chain. Source panes
are branch previews, never event-list authority. Read both sides before explicit
confirmation; reject duplicate targets and stale/late responses. Recovery shows
actual immutable plan targets and preserves request identity before the first IPC.
The new member list renders at most50 metadata rows per page, keeps off-page
confirmations, and handles empty/busy/error/retry and batch/version reset.
N719 genuine prepared historical tests2 pass; N721 prepare36/strict Clippy and
N722 compile/title pass. F723 seven checks/10,359 tests pass on its stable source;
the later paging change passes67 focused tests/typecheck/affected lint/diff check.
Final F725 passes all7 commands/893 files/10,365 tests; F726 UI build passes,
bound to those exact frontend/bundle inputs and Git. N724 full library has4,281 passes/3 failures/
106 ignored: two old recovery JSON assertions plus the child reusing one of them.
Assertions now require all4 fields and exact summary values, preserving tamper
refusal; N728 fmt/compile/title pass. N729 storage2/recovery1/inherited-home parent1
(and its child) plus strict Clippy pass, source/Git/EXE stable, original owner
exited. N730 passes all12 commands:4,284library/0fail/106ignored, explicit87
historical/44retention and strict Clippy; sets overlap. Fresh850native/2570frontend/
26seed/Git/index/EXE,40quality/154LF-noBOM texts pass, owners exited, own temp
cleaned. V727 embedded build119.24s and scoped Chinese synthetic desktop now pass:
dual-read/cancel/confirm/clear, actual prepared public association import, current
reader and completed-job reopen. Closed audit verifies the same entry with two
revisions/two receipts, six unchanged old archive files and26 unchanged seeds.
Eight owned processes exited normally; WebView and superseded V710 package were
retired. Fixed entry CheckOnly passes and now selects V727. The reader has no
old-revision selector; desktop old/new navigation remains incomplete, even
though old bytes are preserved. Do not claim that step passed. Retain failed
evidence/sandbox and earlier V710 batch/cancel-retry evidence without relabelling
it as this build's test. Do not reuse F723 hashes as paging proof.
Original53/full capacity/vendor/ACP/platform/IME-DPI/locale
acceptance remain open. Details: delivery-current and external v718 report.

## Earlier public confirmed association slice (focused only)

Job counts now say processed in all15 catalogs (75 frontend assertions pass).
Public commit routes explicit confirmed associations through atomic acceptance;
ordinary enqueue still refuses them, and explicit-reimport plus association is
rejected. Accepted-request replay precedes preview reopening. N717 explicitly
executes4 public-commit tests and strict all-target Clippy successfully; fresh847
native/Git hashes match,26 seeds unchanged, owners exited and own empty temp
removed. N715 failure evidence is retained; N716 recompiles its test-helper fix.
At that point no current full gate or desktop claim; the prepared UI was pending.
The implementation above retains prepareOrigin/receipt and actual prepared selections.
Sole preview V710 does not contain these newer source changes. The old status
below (guard/count follow-up/docs-only) is historical. See delivery-current.

## Current public job controls (2026-10-15, not delivery)

Public cancel/retry/undo now require jobId, expectedStateVersion and requestId.
Use the domain job_actions writers; never call a legacy test-only entry from IPC.
The current hook preserves request identity through uncertain replies/storage
errors, refreshes genuine version/identity refusals for explicit user review,
and does not automatically reapply at a newer version. All15 catalogs are wired.
Job reads run on the blocking pool. Undo commits bounded sealed cleanup intents
with content rollback; startup resumes only recorded exact original cleanup
under publication locks/current native creation CAS and reference guards. User
project edits are kept. Notifications follow actual commits/effects, even when
the later reply fails. Old protocol jobs do not acquire new creation authority.

N706 focused92 tests pass (one child helper ignored in the outer set, explicitly
executed by its parent). F705 current frontend10,348 tests and seven quality
commands pass; F709 UI bundle passes. N707 historical81/retention44 pass but
strict Clippy failed on8 now-test-only wrappers and1 useless test conversion;
full library did not run. N712 corrections pass fmt/compile and92 focused tests.
N713 all12 commands pass, including historical81/retention44/strict Clippy and
full library4282/0failed/100ignored. Fresh846native/2564frontend/26seed/Git/index/
EXE/40quality/125UTF8-noBOM-LF bindings pass; original owners exited. Own successful
temp removed380files/5,254,431logicalB; observed H free delta+5,853,184B separately.
See delivery-current.md and task-root plans/v699-public-control-writers.md for
original owners/evidence and remaining original scope. Sole usable preview is
now V710 through start-current-preview.cmd, bound to F705/F709/N713. Chinese
synthetic Windows desktop passed actual2/40/160 imports, read/search/job reopen,
cross-page select-all and running cancel/retry (20 successes retained,160 final).
Closed readonly audit verifies202 unique new entries/revisions/committed receipts,
408 events,6 unchanged old archive files and26 immutable seed files. All8 owned
processes exited normally; WebView cache and superseded V325 executable retired.
Undo confirmation/cancel only, no actual deletion. Cancelled progress labels
terminal items completed although only20 were imported; count copy is a known
follow-up. Real sources/provider/platform/IME-DPI and original full scope remain
open; public association guards remain. No new commit/push/PR/release. Earlier
current-status headings below are history, including their obsolete entry names.

## Current batch performance repair (S240, not delivery)

Approved checkpoint/upstream merge7eb1b7b9 is complete. N681/N682 prove their
original sources, not subsequent edits. N686 compiles bounded pure member-codec
memoization; N687 acceptance34/authenticator4 controls pass (sets overlap).
N688 same64-candidate full publish/receipt/reader assertions pass: publish30.582s
vs48.014s (single-run36.3% less), max concurrent poll29ms. Cache keys bind the
loaded-key context and exact current bytes; SQL existence/root/input/owner/ACK
checks and writer transaction/total_changes witnesses stay fresh. Own N688 empty
temp inventoried/removed:0logicalB/observed free0B; useful logs/seeds remain.
N689 compiles one current SQL member-range scan and synchronous read-only fact
reuse, with exact-key/root/count/type/UTF-8/8KiB checks. Candidate construction is
once per header; AcceptedJob drops the read members and retains its original
writer checks. Budget/BOM refusal precedes cache hashing. New warm negatives,
copied/renamed member, neighbor and8192/8193B controls pass in N690 along with
strict Clippy. N691 same64-candidate full workload passes: publish25.379s, max poll
21ms; this is DEBUG timing, not release/UI. Fresh837native/2562unchanged F621
frontend/26seed/Git/index/EXE/40quality/84LF-noBOM verified. N69281seed/44retention/
Clippy passed, but full library was deliberately interrupted201.65s to add an
early metadata key-byte bound. Original owners exited; keep its partial logs and
runtime. SQL now projects only exact prefix+64BYTE TEXT keys;1MiB-key refusal
added. N693fmt/compile/77outcome, N69435acceptance/4authenticator and strict Clippy
passed. N695same64full workload passes after repair: publish24.389s/test27.63s/
max poll30ms, all64entries/revisions/committed receipts/readers verified. Single
DEBUG publish49.2% shorter vs N685, not a UI/release/other-capacity claim. Fresh
837native/2562unchanged F621frontend/26seed/Git/index/EXE/40quality/84LF-noBOM and
exited owners verified. N696current full12-command gate on N693 passes:4267library/
0fail/99ignored,1088.90s; explicit81seed/0ignored,228.70s;44retention/strict Clippy
pass. Counts are not additive. Original session70639 exited0/consumed, native/
Git/index/EXE stable. Fresh837native/2562unchanged F621frontend/26seed/40quality/
85LF-noBOM modified texts and exited owners verified. Own temp inventory→Execute
removed380files/5,254,431logicalB; observed free+5,853,184B includes concurrent
activity, not exclusive physical recovery. N692interrupted10files/295,627logicalB
and N683 evidence remain. Never reuse N692 as green or restart
consumed sessions. No1000-cancel/desktop/platform acceptance or promotion.
Original N683 interrupted catalog retained; sole user preview staysV325. Original
remaining delivery scope stays open. See S240 and task-root v683 report. Below is history.

Scope:64-candidate fixtures begin with a synthetic completed prepare batch and
registered installation. Production plan/acceptance/worker/files/readers execute,
but discovery/vendor parsing do not. Never call these end-to-end source-import
or UI timings; N682 is a separate real-source measurement on its original code.

## Current complete native and large-source checks (S239, not delivery)

Approved local merge7eb1b7b9 remains unchanged. N681full12-command native gate
passes:4264library tests/0fail/98ignored, plus explicit81historical-seed tests
with0ignored,44retention tests and strict all-target Clippy. Counts overlap.
Fresh834native/2562unchanged F621frontend/26seed/Git/index/EXE,40quality and
78UTF8-noBOM/LF texts are verified; original owners exited. Preserve prior REDs.
N682100MiB real source-to-worker three independent publishes and three warm
reads/searches pass1test in115.92s. Each cold API path~37.6–37.9s, subsequent
page11–14ms and search445–455ms; peak sampled working set~40.92MiB. Imports
storage~2.029xsource, below3x. Cold means isolated App home, not flushed OS
cache. Neither UI first-visible nor encoded durable-input size is measured.
Successful own temps are removed with owner/path checks, evidence retained;
N681380files/5,254,431logicalB, observed H free+5,853,184B; N682final empty
runtime0logicalB/observed0B. Immutable large-source hashes rechecked successfully.
N6831000-cancel was aborted at528.57s for slowdown: last worker log481.975s/
58commits, before target200. Native-1/wrapper1 is interruption, not a test
assertion failure/pass. Cancel/stop/offline-resume remain unmeasured. Source/Git
stable, owners exited; retain logs/runtime. Repeated full-member authentication
needs profiling before a repair; no relaxed checks or extrapolated completion.
See task-root v683-signed-batch-performance-triage.md. Original capacity/cancel/public/UNKNOWN/
UI15locales/vendor-live-WAL/ACP/platform/IME-DPI and current embedded-preview
acceptance remain open. No App, real data or publication changes; soleV325 stays.
See S239 delivery notes and task-root v664 report. The sections below are history.

## Current startup authentication regression (S238, not delivery)

Signed worker startup validates all original bodies once in its same Immediate
writer, before any state change. N670fmt/compile/77outcome passed; N671 proves
missing/malformed/oversized-title zero-write refusal, healthy queue drainage and
exact-original recovery. Undo fault tests still mixed old parser/conflict
semantics with signed results: first six filters pass, fault group1pass/2fail;
ownership/Clippy not executed. Retain genuine historical v2 assertions and
signed atomic refusal separately. N672compile failed a missing helper import;
corrected N673fmt/compile/77outcome passed. N674five association Undo faults
passed, ordinary signed missing-receipt failed its historical conflict assertion.
N675fmt/compile/77outcome pass. Complete seed N676runs81:80pass/1fail, all Undo/
ownership pass; discovery's expected plan2 conflicts with actual3. N677passed
fmt/compile/77outcome. N678later unaccepted publish still expected old PlanStale;
separate signed missing-crypto and historical no-context refusals remain.
N679fmt/compile/77outcome and N680discovery1test pass. Explicit v3/descriptor and
independent historical parser probes preserve both protocols. N681full native
with all81seed tests and strict Clippy is next, not yet a passing claim.
No complete native/desktop/platform claim, no App
or preview promotion. Approved local merge7eb1b7b9 exists; sole previewV325.
See S238 delivery notes and task-root v664 triage. Following sections are history.

## Unreleased explicit old-seed compatibility (S237, RED)

N664first explicit --ignored group ran57:48pass/9fail; five groups did not run.
No full-native claim. Current test repairs preserve genuine independent Legacy
v1/v2 capacity/refusal/recovery while signed publication uses actual worker ACK
slots. Added signed input corruption/queue isolation, exact-original retry,
atomic ACK and mixed cancellation controls were added. N665failed
two helper-scope compilation checks; N666fmt/compile/77outcome pass. N667first
group59pass/1fail of60: previous nine pass, but new signed input corruption
queue control gets unexpectedOk(2). Both mixed cancellation and one-ACK tests
pass; six later filters/Clippy did not run. N668fmt/compile/77outcome pass. N669
confirms malformed signed input changes the header before refusal, while missing
input is zero-write. Startup now validates all original bodies in its same writer
before the existing state update, once at start rather than per progress item.
N670is compiling this production repair; no authentication relaxation. Source/
seed evidence retained; soleV325 unchanged.
See S237 current delivery notes; original delivery/platform scope stays open.

## Unreleased normal signed producer (S236, regression repair in progress)

Normal plans and jobs now use SignedOutcomeV1 in the local WIP; earlier notes
about normal V3/V8 remaining fenced are historical. The public association guard
still applies. Genuine source-to-worker create/match/fallback/dedup/restart/Undo
and original N558 ownership controls have run, not just codec primitives.
Proof1 adoption captures the original import/revision before its canonical move
and rechecks it in acceptance. Ordinary recovery authenticates the original
acceptance-transaction plan snapshot, never a mutable draft or a repaired copy.
Signed input, owner/member and allocation checks and Legacy capacity stay intact.

N647 is a real RED:1199pass/50fail/88ignored,1249 actually executed; Clippy did
not run. N64866pass/5fail/75ignored; N64932acceptance tests pass. N650 all seven
focused groups pass98tests, but strict Clippy fails on four unused-code/stale
expect diagnostics. N651 confirms an Undo→explicit-reimport prior-ACK failure.
N652fmt/compile/71outcome and N65346qualified legacy tests plus strict Clippy
pass; explicit signed restoration publishes fresh allocation IDs while retaining
the original import/alias and old archive, never relaxing prior-ACK checks.
N653fresh binding/40quality passed. N65473pass/1fail: real late dedup rollback/
offline retry and workspace-reference protection passed; modified-ACK refusal
was correct, but the test's stale derived-progress DTO comparison failed. Fix
the fixture to compare persisted bytes, not product refusal. N655engine finished
212pass/2fail/79ignored; both failures mixed Legacy jobs with signed plan IDs.
The helper now creates independent genuine v1 plans/hashes through real commit,
never overwriting signed plans. N656fmt/compile/77outcome pass, including actual
mixed Undo/late-trigger rollback/initial-failed/repeated Undo and offline retry.
N657engine214pass/79ignored and job-control50pass/1ignored, but mapping5pass/
1fail; retention/protocol/Clippy did not run. N658fmt/compile/77outcome pass;
N659single diagnostic0pass/1fail. Debugger N660/N661 locates the failure in a
synthetic completed-ACK rewrite on retry, NOT initial publication (earlier
diagnosis withdrawn). Debugger exit0 is not child101 success. Historical replay
now uses genuine v1 while signed normal and actual receipt-fault/offline-retry
controls preserve original mappings/IDs. N662fmt/compile/77outcome pass. N663
mapping8/retention-crypto4/protocol15 and strict Clippy55.6843089s pass. Fresh
833native/2562unchanged F621frontend/26seed/Git/index/EXE/40quality/66LF-noBOM
texts are stable, owners exited; own empty N662/N663 temps inventoried/removed,
0logicalB and observed H free-4096/0B, not reclaimed-space claims.
Counts overlap. See S236 current delivery notes and task-root
triage report. No full native/desktop/platform acceptance or preview promotion;
sole user entry is still V325. No additional commit/push/PR/release or real data.

## Unreleased concrete Undo writer boundary (S235, slice regression passed)

UndoSnapshot validates all initially committed signed results including dedup
before target writes and supplies fixed receipts, never mutable latest results.
The same writer rechecks original input/owner/acceptance/member/fence/allocation,
terminal/revisions and the NEW ACK phase/error/version after save_job, together
with ordered shared-entry/tombstone effects. Dedup cannot own a created project.
No preview/archive/native I/O or result re-signing occurs in this boundary.
Legacy behavior, reference-lock order and native full Project CAS are unchanged.

N63866outcome, N639408regressions/2existing ignored plus strict Clippy and N640
65actually executed old-seed tests passed, with overlapping counts. Two fresh
bindings cover829native/2562unchanged F621frontend/26seed/Git/index/EXE and
40quality, owners exited; own disposable temps were inventoried and removed.
New tests are result/effect primitives, NOT accepted signed source/grants or
normal producer/Undo/mixed rollback proof. Noncommitted/repeated Undo and original
N558 remain OPEN; normal V3/V8 stays fenced. Next actual source-to-worker protocol
and legacy project-preservation acceptance, not another primitive-only loop.
No App launch/promotion/publication; sole preview remains V325. See S235 delivery
notes and task-root next-protocol§21 / B5§27; original delivery scope is unchanged.

## Unreleased prior publisher and Reuse boundary (S234, slice regression passed)

Actual receipt-owner crypto is prepared outside SQL. Current snapshots reselect
original durable input/accepted owner/member/allocation, complete terminal and
unique ACK slot/version, receipt and revision. A dedup job is not the original
publisher and cannot own its created project. Reuse validates real archive files
outside snapshots, then rechecks SQL facts; old semantic fallback does metadata
comparison first. Final writers recheck the publisher before and after writes.
The normal require_publisher fence remains; no new cross-transaction SQL grant.

N63563outcome, N636405regressions/2existing ignored plus strict Clippy and N637
65actually executed old-seed tests passed, with overlapping counts. Two fresh
bindings cover827native/2562unchanged F621frontend/26seed/Git/index/EXE and
40quality, owners exited. Own disposable temps were inventoried and removed.
Four new tests include real Legacy archive/autocommit controls, but terminal
primitives are NOT accepted signed source/grants or normal producer acceptance.
Undo/mixed/original N558 and genuine signed prior-owner acceptance remain OPEN;
V3/V8 fenced. No App launch/promotion/publication; sole preview remains V325.
See S234 current delivery notes and task-root next-protocol§20 / B5§26.

## Unreleased independent outcome recovery (S233, slice regression passed)

Recovery borrows original durable metadata without reopening preview or building
a CommitItem. Signed snapshots authenticate owner/acceptance/member/input/fence/
allocation, full result/receipt prefix, revision and exact ACK. Files/projection
are verified outside snapshots before fresh fact comparison. Recorded progress
retains its verified version and does not call the Pending setter again. Legacy
recovery/title behavior is preserved; signed ACK uses original input title, not
the user's editable catalog title. Dedup cannot inherit old created ownership.

N63259 outcome tests,N633401 regression tests/2existing ignored plus strict
Clippy,N63465 actually executed old-seed tests pass; counts overlap. Two fresh
bindings cover826native/2562unchanged F621 frontend/26seed/Git/index/EXE and
40quality with exited owners. Own successful temp cleanup is complete. New
primitive tests are NOT accepted signed source/grant or normal-producer proof.
Signed prior-owner, Undo/mixed rollback, original N558 and normal producer
acceptance remain open; normal V3/V8 stays fenced. No new desktop/full-platform
acceptance, real data migration, preview promotion or publication. See S233
current delivery notes. The local upstream merge is7eb1b7b9; sole preview is V325.

## Unreleased upstream v0.2.41 integration (S232, not delivery)

The local-only upstream integration preserves import resume grants, captured
official/custom routing, durable external queue claims and UNKNOWN/no-resend.
PR-monitor MCP binds the actual App chat and cwd, without rereading foreign
global MCP settings. Host message origin travels with one send to its actual
journal write; it is not a session-wide arm for the next user message. Durable
queued Host messages are marked only when drained, without changing old codecs.
The composer domain hook owns repo PR rows and session-scoped watches.

Full frontend regression and production frontend build pass. Native integration,
publication regression and actually executed old-seed compatibility pass with
fresh exact-source/seed/EXE bindings; see S232 delivery notes for nonadditive
counts, failures, cleanup and audit scope. This is not full native, desktop or
platform acceptance. Durable outcome reader/recovery, signed prior-owner,
Undo/mixed rollback and original N558 are still open; V3/V8 remains fenced.
The sole V325 preview is unchanged. No push/PR/release or real data migration.

## Unreleased committed ACK replay (S231, slice regression passed)

Signed publisher re-entry must propagate the already verified ACK as Recorded
progress, not acknowledge it again through the normal Pending setter. The ACK
validator returns its current consistent version from the same snapshot; dedup
replay uses that return value too. Ordinary legacy Pending/recovery semantics
remain unchanged. No caller-selectable authentication bypass is introduced.

Restart/zero-write dispatch and Pending positives are synthetic primitives with
normal legacy jobs, not accepted signed source/producer evidence. See the
current delivery document for validation: N616fmt/compile/55pass,N617395pass/
2existing ignored plus strict Clippy,N61865actual --ignored old-seed tests all
passed. Counts overlap; these are not full native/desktop/delivery acceptance.
Each subset fresh-binds815native/2556unchanged frontend/26seed/Git/index/EXE,
136LF/noBOM texts and40quality, with exited owners. Own successful temp was
inventoried then removed; failed N615 evidence retained. The independent durable reader,
signed prior-owner, Undo/mixed rollback and original N558 are still open. V3/V8
remains fenced; sole V325 preview is unchanged and no publication is authorized.

## Unreleased deduplicated outcome wiring (S230, slice regression passed)

The fenced worker now has a distinct terminal Deduplicated record, chained to
its acceptance-born allocation. It binds the actual old import/revision, exact
verified receipt/revision/selection, entry identity and final ACK item. It must
not consume the reserved new revision, produce a receipt/native project or
claim created ownership. Published and deduplicated terminal branches conflict.

Archive preflight is outside the writer. The short final writer reauthenticates
acceptance/source/proof/member/input/fence, compares the original SQL facts,
and atomically saves ACK, explicit reimport restoration, fallback-title repair
and the prebuilt result. Replay reads the original envelope without writes;
missing result plus a completed ACK cannot become another first publication.
User titles/aliases and a newer current revision retain their existing behavior.

Normal V3/V8 stays fenced. A signed prior owner still requires the independent
durable outcome reader; it explicitly refuses instead of using legacy receipt
authorization. Full registered-producer/recovery/Undo/N558 acceptance remains
open. N608 passed51; N609 retained53pass/1fixture-setup failure; corrected N610
passed54. Intermediate N611 passed392/2existing ignored and strict Clippy. The
final accepted-current CAS refuses a target advanced since acceptance without
conflating that current with the older reused revision. Current N612 passed55
(13new synthetic primitives), N613 passed112/1existing ignored plus strict
Clippy, N614 actually executed65 old-seed tests with --ignored, all passing.
Counts overlap; no full-native/desktop/delivery claim. Fresh814native/2556frontend/
26seed/Git/index/EXE,135text encodings and40quality are stable; successful own
temp was inventoried then removed with exited owners, failure evidence retained.
See the current delivery document; V325 was not promoted or launched.
Historical input2/3, receipt2 and proof1/2 bytes/capacity remain unchanged.

## Unreleased worker outcome integration (S229, slice regression passed)

The worker now consumes acceptance-born IDs and uses deterministic phase receipt
addresses for the fenced V3 path. Linked receipt/store-result are atomic; the
prospective final envelope is built before the first ACK write, then receipt,
catalog/revision, mapping, outcome and the actual worker ACK share one commit.
Each writer reauthenticates current facts. Missing receipt/result pairs are not
repaired or re-signed, and exact saved ACK item/slot/phase/version is checked.

Native target failures are distinct from corrupt provenance and storage errors;
only an explicitly allowed target failure can become unassigned-reported. A
MatchedAlias result binds actual and requested raw paths after the real native
resolver proves equivalence; it cannot promote a reused project to created.
Result fields must agree with the receipt they bind. Historical receipt2/input2/3/
proof1/2 codecs and ordinary receipt capacity remain unchanged. Legacy ordering
and duplicate-revision behavior are preserved.

Normal V3/V8 remains fenced. Signed dedup, genuine producer positives, durable
recovery/Undo/mixed rollback and original N558 are still open. Synthetic native/
receipt/ACK primitive tests are not source/proof authorization or delivery.
See the current delivery document for run-bound validation; older green runs do
not validate later source changes. The sole user preview remains V325.

N604 found a genuine normal-lib export/strict-Clippy failure after its366 tests
passed; it is retained. Corrected N605 fmt/compile/42 outcome tests,N606 current
366 regression tests (2 existing ignored) plus strict all-target Clippy, and
N607 actual65 old-seed tests with --ignored all pass. Counts overlap; these are
not a full native/desktop/platform/delivery gate. Fresh812native/2556frontend/
26seed/Git/index/EXE,132text encodings and40quality are stable; owners exited,
own successful temp cleanup completed. No App/preview promotion or publication.

## Unreleased signed acceptance allocation

The fenced V3 acceptance producer now prepares allocation metadata outside the
catalog snapshot and stores it in the same transaction as acceptance. Each signed
member requires an allocationDigest; the complete membership root binds both
the original input and canonical allocation data. This private, unopened codec
does not change historical input2/3, receipt2 or proof1/2 bytes.

Allocation captures receipt/import/revision IDs, previous revision and explicit
Create/Match/Unassigned intent. It is metadata, not source/proof/native/SQL
authorization. Existing catalog identity/revision is rechecked before and after
inserts. Native project reads and key loading are outside the acceptance writer;
the writer uses only SQL and preloaded crypto. Explicit create plus an existing
match ID is rejected, not silently converted into newly created ownership.

An existing acceptance requires its original allocation. A worker/recovery reader
must use AcceptedJob::allocated in its concrete current transaction; missing,
corrupt, foreign or independently re-signed allocation data is an error, never
permission to generate another UUID set or repair a row. The identical detached
producer can retry without writes; recapturing a different UUID set cannot replace
an accepted result. Detached verified records remain codec evidence, not grants.

Normal V3/V8 producer, native store-result/final/recovery/Undo integration and the
original N558 ownership regression are still open. Do not remove the rollout
fence based on synthetic acceptance/allocation primitive tests. Current evidence
and the next slice are in docs/plans/2026-10-08-session-import-delivery-current.md.

S197 diagnostic reds,not recovery delivery: only two existing test modules and
one new four-test module changed since N383; product implementation unchanged.
N384 original44093 exit1:5passed/1genuine failed. N385 original66358 exit1:
6passed/3genuine failed/0ignored,719stable inputs/Git,fmt/compile/manifest pass.
Cleanup wrongly deletes an unaccepted unexpired preview after pre-COMMIT process
exit,a different completed preview with the same candidate,or failed rollback I/O.
The real post-COMMIT lost-ACK owner survives and its worker completes; five old
controls pass. No accepted rows were manufactured; no association positive.
Fresh719native/2529frontend/26seed drift0,quality40/40,App13294/1k80,owners0.
Two failed empty temps0B deferred pending full replacement; failure logs kept.
Current EXE was relinked and is not N383's binary; no current full/Clippy/seed
pass claimed. No App/package/provider/security/publication or upstream integration.
Checkpoint permission remains unanswered. v386-retention-crash-reconciliation
and N385 original terminal/closeout govern the fix; original full scope stays open.

S196 bounded-header prerequisite: stored_job uses row/type metadata,read-only BLOB bounds and
same-snapshot streaming strict header. Original16MiB job cap/ordinary input2/v1
rules stay unchanged. Shared serde guard limits scratch/depth; duplicate/missing/
unknown/type/SQL-column drift refuses,including createdAt. Mutable items/progress/
cursor are ignored projections,not immutable authority or whole-body validation.
N38063passed/5genuine failures/1ignored retained; N381 original29471 exit0,
718inputs71focused passed/0failed/1ignored,including9actual new tests. N382 original
21194 exit0,same718inputs one actual immutable-seed test;28/22/54/91subchecks are
not195tests. Quality40/40/App13294/1k80 unchanged,five native paths LF/noBOM.
Own successful empty temps removed0logicalB,observed H deltas0/-4096B separately;
N383 original31064 exit0: full six gates pass,71focused/1058domain/3987library
passed,0failed;1/14/16ignored are not passes,sets overlap. Strict Clippy passes;
full library864.8905384s. Fresh718native/2529F344frontend/26seed drift0,seed/full
inputs equal,HEAD/index stable,original/task owners0,quality40/40,App13294/1k80.
Guarded cleanup removed765own-temp files/10483322logicalB and N380empty temp0B;
observed H deltas+11440128B/+4096B separately. Source/WIP/deps/seed/current EXE/
symbols/soleV325/data and failure logs retained; N363nonempty evidence deferred.
Fixed preview CheckOnly passes/no launch. Both main refs fetched again,unchanged,
not integrated; local WIP checkpoint permission remains unanswered. No App/package/
provider/security/publication/commit/push/PR. See N383 original terminal,fresh,
cleanup and closeout records; full synthetic regression is not product acceptance.
Accepted CAS/reader,publisher/recovery,Undo/ref/grant/UI and original full scope53
remain open; externalv375/v380 plans and original N380–383 records govern next work.

S195 normal enqueue prerequisite, not accepted association delivery: a process/
OS acceptance owner spans SQL rollback and exact new retention-bit repair;
immutable lease identity is checked, latest expiry/batch and existing accepted
retention are preserved. In-memory retention follows successful COMMIT only.
N376 real COMMIT-contention red is retained; N377 five actual tests pass, including
Windows lease rename failure and independent process busy/retry. N378 one actual
immutable-seed test binds the same716inputs;28/22/54/91subchecks are not195tests.
N379 original95857 exit0:5focused/1049domain/3978library passed,0failed;
0/14/16ignored are not passes, sets overlap. Fmt/compile/manifest/Clippy and fresh
quality40/40 pass;716native/2529F344frontend/26seed drift0,seed/full equal,Git
stable,owners0,App13294/1k80 unchanged. Guarded cleanup removed770own-temp files/
10486224logicalB and old empty0Btemp; observed H deltas+11431936B/0B separately.
Logs/source/WIP/deps/current EXE/soleV325/data retained; no App/package/provider/
security/publication. Crash/orphan and rollback-I/O repair recovery are not proved.
Association lanes stay refused; reuse the owner for genuine acceptance later.
Both main refs fetched, not merged; no WIP-checkpoint approval. Next externalv375
plan first bounds exact stored-job headers before real accepted CAS/reader,
publisher/recovery,Undo/ref/grant and UI. Original full scope/53 remain open.

S194 strict proof protocol prerequisite, not accepted association delivery:
registration_input keeps its original16MiB cap with shared pre-body bounds;
ordinary worker input2 capacity/bytes remain unchanged. Explicit proof1/2 dispatch
preserves real accepted v1 flat JSON/digest and store marker1; receipt2/input3 are
separate protocols. The shared inspection witness holds OperationGuard, not a
catalog transaction, serializable grant or idle cache. Public review uses the
same authentication/payload. Test-only drafts bind actual job/plan/input bytes
and confirmation, never accepted authority; proof2 cannot bypass authorization
even with matching current entry/receipt identities. N368/N370 genuine reds were
fixed; N369 premise and N371 compile wiring failures are retained honestly.
N373 executes1 immutable-seed test with28 inspection/22 plan/54 input/91 draft
subchecks, not195 tests. N374 original68751 exit0:714stable inputs,62focused/
1044domain/3973library passed,0failed;1/14/16ignored are not passes, sets overlap.
Fmt/compile/manifest/Clippy and fresh40 quality pass;714native/2529F344frontend/
26seed drift0,seed/full inputs equal,Git unchanged,owners0,App13294/1k80 unchanged.
Guarded cleanup removed763own-temp files/10484006logicalB (observed H delta
+11460608B separately) and empty own/old temps; logs/seed/source/WIP/deps/current
EXE and soleV325/data retained. No new App/package/provider/security/publication.
Both main refs fetched successfully; upstream is28 commits ahead with79 WIP
overlapping paths, not merged/stashed. Next external v375 plan closes real
acceptance/reader/publisher/recovery, then Undo/refs/grants and integrated UI.
The current local:-only late-producer guard is not proof for old UUID producers;
retention rollback needs exact ownership, not clearing another job's boolean.
Original53/vendor-WAL/ACP four combinations/100round/platform/IME-DPI/capacity/
15locale/small-PR scope and broader SQL allocation audit remain open. Goal active.

S193 record-reader prerequisite only: rowid + read-only BLOB length replaces SQL
CAST classification for worker input and proof bounds. Oversized input3 uses the
same strict streaming header before body fetch; ordinary input2 keeps capacity.
Preview retention projects only its path, refuses invalid references and is not
authorization. Present unreadable proof rows refuse explicitly, not None; store
schema1/v1 unchanged. N359/N360 real reds retained, N359 missing terminal is an
interrupted log closeout, N363 diagnostic interruption is not full green, N364's
58pass/1fail is a wrong EXPLAIN P2 test operand. Both query-plan tests now include
body-read controls. N36559focused passed; N366 one actual immutable-seed test with
28B1/22plan/54input subchecks passed (not104 tests). N367 original3862 exit0:
711stable inputs,59focused/1041domain/3970library passed,0failed,fmt/compile/
manifest/Clippy green;1/14/16ignored are not passes, sets overlap. Fresh711native/
2529F344frontend-owned/26seed drift0; N366/N367 snapshots equal,Git stable,owners0,
quality40/40,1k80/80,App13294 unchanged. Guarded cleanup removed759own-temp files/
10482846logicalB, two empty temps0B and three superseded8a EXE/PDB/depfiles/
839238192logicalB; observed H deltas+10924032/+8192/+839237632B separately, not
exclusive reclamation. N363205files/3175537B remain for evidence classification;
failure logs/seed/deps/source/WIP/current EXE and soleV325/data retained, other
chats untouched. Plain cmd CheckOnly exit0/no launch; no App/package,private
source,provider,security change or publication. Not Proofv2/accepted association/
UI delivery or a whole-SQL allocation audit. Next external v368 plan hardens
registration_input with its existing16MiB cap, then strict proof/shared witness/
reader; v359 B4acceptance/B5Undo-ref/B6UI and original V5/contracts/53/platform/
provider/WAL/IME-DPI/capacity/15locale/small-PR scope remain open. Goal active.

S192 input protocol slice: real plan→CommitItem→input3 retains confirmation/raw
selection and exact job/request/plan bindings. Ordinary input2 bytes and receipt2
remain unchanged. Strict presence/version parsing rejects downgrade/null/unknown/
duplicate behavior; metadata verification survives completed-preview disposal.
Intent is not a grant: enqueue/direct publisher/worker/committed recovery refuse
association until accepted authority exists. No UI or accepted association yet.
N355 genuine seed red retained; N354/N356 failed before assertions. N357 executes
one actual immutable-seed test with28 B1+22 plan+52 input subchecks, not102 tests.
N358 original95649 exited0:709 stable inputs,51 focused/1033 domain/3962 library
passed,0failed,fmt/compile/manifest/Clippy green.1/14/16 ignored are not passes;
sets overlap. Full797.637s. Fresh709native/2529F344frontend-owned/26seed drift0,
N357/N358 snapshots exactly equal,Git stable,task owners0. Quality40/40,1k80/80,
App13294 unchanged. Inventory→Execute removed own755files/10483161logicalB and
four old empty temps0B; observed H deltas+11436032/+4096B separately,not exclusive
physical reclamation. Logs/seed/current EXE/deps/WIP retained,other chats untouched.
SoleV325/data unchanged,plain cmd CheckOnly exit0/hash valid/no launch; no App/package,
security change,private source,real provider,agents or publication. Next finish
pre-materialization v3 bounds and explicit oversized-proof refusal,then strict
Proofv2/reader→accepted CAS/publisher/recovery→Undo/ref/grant→UI. External
plans/v359-proof-v2-reader.md. Original V5/contracts/53/vendor-WAL/ACP/platform/
IME-DPI/capacity/15locale/small-PR scope remains open; not whole-product delivery.

S191 prerequisite: options cannot carry hidden identityAssociation(s), even null.
N351 genuinely failed one loader regression (46passed/1failed/1ignored). Shared
guard now covers plan loading, enqueue, durable shape and direct publisher before
ordinary writes/retention/project effects; worker replay refuses independent
publication. N352706inputs fmt/compile/50focused passed; N353same706inputs pinned
seed1actual test with28B1+22plan subchecks passed and extra Clippy51.126s passed.
No current full-domain/library rerun is claimed. Quality40/40,1k budget80/80,
App13294 unchanged; fresh706native/2529frontend/26seed drift0,Git stable,owners0.
Own empty temps removed0logicalB, observed H deltas0/-1495040B separately; failed
N351empty temp deferred under full-replacement policy, logs/deps/seed/WIP retained.
Input/receipt stay2 and v1 authority unchanged. Not complete B2b/input3,Proofv2,
accepted publication or UI. SoleV325/data unchanged/no launch; original scope and
53 remain open. Next external plans/v354-durable-association-input.md.

S190 in progress: B2a strict Confirmation and durable plan v2 are connected to
an async Host command, without UI/identity publication. Ordinary v1 payload is
unchanged; v1 association-field presence (even null/empty/options) is refused.
v2 retains raw selection and binds confirmation/planId/created/expiry. Invalid
reviews refuse before lease/plan writes. Associated plans refuse ordinary commit;
existing request→same plan/job retry order remains. N345two genuine reds retained;
N34646passed/0failed/1ignored, N347one actual pinned-seed test with28 B1 and22
plan subchecks. N348 original50252 exited0:704inputs fmt/compile/46focused/
1028domain/Clippy/3957library passed,1/14/16ignored are not passes. Afterwards
only two test paths changed: four complete test bodies moved verbatim into a
child module,1158→941parent/222child. N349 current705inputs fmt/compile/46focused
and extra Clippy passed; quality40/40,1k-file budget80/80,limit unchanged. N350
same705inputs pinned-seed test passed with28+22subchecks. Product implementation
hashes equal N348; do not claim705-input full library rerun or delivery. Original
53/vendor-WAL/ACP/platform/IME-DPI/capacity/15locale/small-PR stay open,V325/data
unchanged. Next design section9: B2b versioned input; commit refusal stays.

S189 evidence correction: the same batch also added the frontend read-only
inspect wrapper, nine API tests and public export. N343 is native-only. F344
original37117 exited0: types/whole lint/UI build passed, 10176 tests/0failed/
0pending (including all9 wrapper assertions),3232 stable inputs/Git. After the
owner exited, own tmp1312files/11216160logicalB removed; observed H free delta
+14475264B separately, not exclusive reclamation. Bundle/logs/deps/V325/data
remain. B2a is still in progress; this frontend gate is not desktop acceptance.

Latest S189 (2026-10-13): explicit identity-association inspection implemented in
the read-only domain and async Tauri command,not UI/plan-job/Proofv2/commit. N342
executed the pinned V243 synthetic archive and28 fault/digest subchecks:1actual
test/0failed/0ignored. Catalog schema/row digests and preview lease hashes stay
unchanged; old history/ordinary independent publication/same-request retry remain.
Source-target use one snapshot; registered aliases bind the digest. Existing
proof rows,including oversized corruption,block another association; v1 unchanged.
N337 wrong alias and N340 wrong default-WAL premises remain failed evidence; only
isolated fixtures changed,not product journal mode. N34138focused/1020domain
passed,but Clippy failed on uncalled domain dead-code; no full-library run. N342
registered spawn_blocking command passes the same seed and exact domain-error
checks,no allow(dead_code). N343 original54361exit0/terminal.passed=true:
39focused/1021domain/3950library passed,0failed;fmt/compile/manifest/Clippy pass.
1/14/16ignored are not passes. Fresh701native/26seed drift0, N342/N343input sets
exactly equal,Git unchanged,task owners0. V325CheckOnly exit0/hash valid/no launch;
no App/package/preview-data change. This is synthetic backend evidence only.
Four own successful empty temps removed:0logicalB; observed H deltas each-4096B
separately,not exclusive reclaimed storage. Next B2confirmation/plan-input→B3
Proofv2→B4accepted publication→B5Undo-ref→B6UI; details in historical-identity-
association design. Original53/vendor-WAL/ACP/platform/IME-DPI/capacity/15locale/
small-PR scope remains open. No publication or agents; inspection is not delivery.
After replacement and owner exit, retired own755files/10483173logicalB and7old
temps370files/5234072logicalB; observed H deltas+11436032/+5701632B separately,
not exclusively attributed storage. Failure logs/seeds/current executable/V325/
source/WIP/dependencies remain; other chats or uncertain owners untouched.
Evidence: external N343closeout/terminal/two cleanup JSONs and N342seed log.

Previously S188: N332 executed existing hash-pinned V243 synthetic
normalizer1.0.0 archive through strict current reader, explicit registered UUID
directory scan, preview, durable plan, runtime reset, enqueue, worker publish,
reload and same-request retry:1passed/0failed/0ignored. Old entry/job/5receipts/
immutable archive remain unchanged, two messages per revision; current1.0.6
scoped fallback publishes an independent entry. No explicit old/new target was
authorized. Association remains unimplemented; see the bounded design in
docs/plans/2026-10-13-session-import-historical-identity-association.md.
N33332compatibility tests passed/0failed/1ignored; the external-seed test was
actually executed separately in N332, not counted passed merely by ignoring it.
Clippy all-targets -D warnings exit0.697native fresh drift0/Git stable; compared
with N328 only test-module linkage/new test changed, no product implementation
change or full-domain/library rerun. Original26file seed drift0, task App/build/
test owners0; no new desktop/package/private data/provider/security/publication.
V325 sole preview/data unchanged, CheckOnly exit0/hash valid/no launch. Strict
cleanup removed two own empty successful temps:0logicalB, observed H deltas
-4096B/0B separately; three failed empty0B temps deferred under full-replacement
policy, failure logs retained. Original scope/53/platform/provider/WAL/IME-DPI/
capacity/15locale/PR acceptance remains open. Evidence: external N332closeout
and N333test/Clippy logs. Synthetic discovery is not identity-association delivery.

Previously S187: sole start-current-preview.cmd now selects V3256f79ce5d/SHA45d9677f;
plain cmd CheckOnly exit0/hash valid/no launch, original data path untouched.
Build48089 exit0/112.4755s,3275source/162assets stable; exact2527unchanged F312
frontend inputs reused. N32832focused/1014domain/3943library plus Clippy passed,
696native fresh drift0; sets overlap and13/15ignored are not acceptance. Current
binary includes publication-owner repair. Chinese1282x802 synthetic desktop
passed old2-message read, A/B missing-tail/93byte full field/emoji, adoption without
auto-selection, one manual B prepare1/0/0, completed-prepare close/reopen/Back/
Refresh selection retention, dark/light and Escape with visible Compare focus
ring. UIA reports RootWebArea, not independently verified native focus identity.
No product commit/Undo/send. App26120/all8capturedfamily normally Quit, lifecycle
65241/closeout78474 exit0. Immutable seed26/archive6/source4 drift0, entries1/
jobs1/receipts5. Strict exited WebView cleanup262files/13916246logicalB, observed
H delta14147584B; verified superseded V313 EXE1file/94038528logicalB retired,
observed delta94035968B separately. Keep current package/source/WIP/deps/seed/
failure logs; no other chat process touched. Old registered fixture metadata
retains UUID but new scoped transcript-locator differs from old1.0.0 bare stem;
not prepared/adopted, genuine historical identity replay remains unverified.
Continue isolated-domain old seed replay and explicit-association design. Original
V5/contracts/53/vendor-WAL/realACP/platform/IME-DPI/capacity/15locale/PR scope
remains open; no agents/publication/private sources/providers. See external V325
desktop-observations/prepare-inspection/desktop-smoke/exit/cleanup evidence.

Earlier S186: N324 original43710 exited0:27focused/1009domain/3938library,
zero failed, Clippy passed;13/15ignored are not acceptance.695native/Git stable,
fresh drift0 and owner45944/task owners0. Strict own-temp inventory/Execute
removed752files/10482367logicalB, observed H delta10915840B separately; logs/
EXE/WIP/deps/seeds/failure evidence retained. No desktop App/new package.
N3265f50369f original6491 exited1,28passed/2genuine failures of30focused:
obsolete publication creates an unrelated project or overwrites saved mapping
version2 with old project/version1 before its final refusal; normal control passes.
696native/Git stable, owners exited, empty0Btemp/failure logs retained. Repair
only actual adoption: nonblocking existing process/OS publication owners before
SQLite writer/store locks; SourceBusy leaves plan/request/identity/job/leases
unaccepted and retryable. Ordinary UUID enqueue/publish/Undo remain unchanged.
N3275697383e original63851 exited0:32focused/0failed/0ignored plusfmt/compile/
manifest,696stable native/Git, owner74048 exited and own empty temp cleaned0B.
N328a7ea2927 original69945 exited0:32focused/1014domain/3943library zero failed,
Clippy exit0; domain502.342s/full794.652s.13/15ignored are not acceptance and
sets overlap.696native/Git stable and fresh drift0, original31804/test68904/task
owners0, EXE835dc05f hash verified. Strict inventory/Execute removed only own
compiler-temp1559files/13147717logicalB; observed H delta14217216B separately.
N324 full green applies only to earlier inputs. Six strict desktop/entry guards
allow only exactV325; duplicate alternatives removed, parse0errors/LF-noBOM,
actual AST boundary allows325 and rejects326/path escape. F312 fresh2527frontend
inputs drift0 and162bundle assets equal V313's embedded manifest; only this
frontend subset is reusable. One embedded candidate
windows-preview-v325-6f79ce5d7d944f97996be2ca34de47a0 is building under original
48089/owner29104; no second package or desktop App/promotion. V313 remains current.
SoleV313 and original V5/contracts/53/platform/provider
scope preserved; no private/provider/product-App commit-Undo/security/publication.
Continue plans/v326-legacy-publication-project-side-effects.md before replacement.

Earlier S183: N317 full green binds its694inputs, not later changes. Corrected
N319 original94520 exited1:19passed/4failed,695stable native/Git. Three actual
reds: family-prefixed ordinary/verbatim locator conflict wrongly accepted;
genuine queued legacy producer duplicates history after adoption and after Undo.
Lease byte failure was captured before plan renewed expiry, a fixture error;
N318 raw-path/error-code preconditions are also corrected. All failed logs remain.
sources now preserves known adapter family prefixes during exact path spelling;
publisher checks persisted legacy ownership before receipt allocation and in its
final transaction, refusing obsolete producers including old explicit reimport.
Registered UUID publication and committed receipt recovery keep their contracts.
No archived bytes, durable IDs or receipt bytes are rewritten; no new completion
framework. Rebuild/schema, real lease replacement and supported legacy unassigned
workspace proofs passed the earlier focused slice, not desktop/vendor acceptance.
N320 test compile failed on String.code; use existing parse_command_err instead.
N321 caa4ac40 original91109 exited0: fmt/compile/manifest and26focused passed,
23.057s,695stable native/Git and owner22184 exited; own empty temp cleaned0B.
N322 added a27th test but valid same-options dedup bypassed its second probe:
26passed/1fixture failed. Use real changed rawRetention with a digest assertion.
N323 original85039 was diagnostically interrupted before the new test executed:
static inspection found test-only Undo reentry under publication's lock. Move
Undo after publisher returns; no observed product deadlock. Exact test65852
only was interrupted; finally/source-Git stable, owners exited,4files/249980B
interruption evidence retained. N32485869361 original43710 subsequently exited0,
27focused/1009domain/3938library zero failed plusClippy;695stable inputs/fresh
drift0 and exited owners. This result binds those inputs, not later tests.
F3122527frontend-owned
inputs freshly drift0; sole previewV313 unchanged, no new App/package. Original
V5/contracts/53/platform/provider scope preserved. No private/provider/product
App commit/Undo/security/publication. Continue external plans/v319-legacy-producer-ownership.md.

Earlier S182 keeps S180/S181 atomic registered enqueue and exact provenance:
immutable archives/receipts are not rewritten; shared reader/recovery authority
is strict. Final N316 original12734 exited1:16legacy focused passed, domain
994passed/4failed/13ignored; Clippy/full were not run. Four capacity fixtures
used an unregistered placeholder source. The common factory now calls actual
sources::register once and shares the canonical root/UUID, preserving all paging,
cancel/offline/crash/receipt/hash/reader assertions, not bypassing authorization.
Final N317 e8fa7707 original82163 exited0, all six gates passed: fmt/compile/
manifest, capacity12passed/0failed/4ignored, domain998passed/0failed/13ignored
in487.216s, Clippy53.174s, full3927passed/0failed/15ignored in783.551s.
694native fresh drift0 after exit; owner28792 and task App/build/test owners0.
Sets overlap; ignored is not passed acceptance. F312 has11changed native paths
and lacks3new native files; only2527frontend-owned inputs are freshly unchanged.
V313 is the sole preview, CheckOnly exit0/hash valid/no launch, without this fix.
No desktop App/package/private source/provider/product commit/Undo/security/
publication. N316 owners exited; failed temp840entries/370files/5234072logicalB
is retained for individual evidence classification. Strict inventory then Execute
removed own compiler-temp758files/10483047logicalB; observed H free delta
11472896B separately, not exclusively attributed physical recovery. The cleanup
record binds exact files/hashes/regeneration; EXE/logs/WIP/deps/seeds remain.
HEAD/index/App13294 unchanged. Human confirms no security prompt; no settings change.
Next external plans/v318-capacity-registration-repair.md closes cross-locator/foreign preflight/
retention/workspace/rebuild/real queued-legacy-producer proofs before one replacement
desktop cycle. Current normalizer1.0.6 synthetic layout is not vendor/old-normalizer/
WAL/real ACP/original-platform/IME-DPI acceptance; original V5/contracts/53 stay open.

Earlier S180 implemented atomic registered enqueue. Final N315 d51ecac4 original
32854 exited1: fmt/compile/manifest passed;4passed/1failed/0ignored of5focused.
Later snapshot rejection and injected job_items failure now roll back identity;
normal retry/dedup and collision controls pass. Old-revision reader is still red.
692native inputs/Git stable/fresh drift0;owner8960 exited/task owners0, empty
compiler-temp0logicalB retained with failure logs/current executable. No desktop
package/material deletion/current full-domain-Clippy-library proof is claimed.
V313 stays the sole preview. Continue external plans/v316-legacy-receipt-provenance.md
with exact provenance and shared strict reader/recovery checks; never rewrite old
archives or bypass receipt authorization. Original V5/contracts/53 remain open.

Earlier S179 compatibility audit is red, not a production fix. Final V314
56f4b1f9 original41441 exited1: fmt/compile/manifest passed, two normal controls
passed and two actual invariants failed. The valid current-schema historical
local-registration seed becomes unreadable after accepted identity migration
because catalog/receipt identities diverge. A later changed prepared snapshot
rejects enqueue with no new job/changed counts, but earlier migration persists
after restart. Archive hashes remain unchanged. Earlier fixture/precondition
failures are retained and are not additional product bugs. This is normalizer1.0.6
registration-layout evidence, not a genuine old-normalizer/vendor replay.
692inputs/source-Git stable, fresh drift0; all three original owners exited and
task App/build/test count0. Compiler-temp directories are empty/0logical bytes;
logs/current test executable retained, no new desktop package/material cleanup.
Only cfg(test) wiring and gate focus were added, no product behavior changed.
V313 remains the only preview; F312/N309/V313 evidence applies to their pinned
inputs, not a new full gate. Continue external
plans/v315-registration-compatibility-repair.md with exact retained identity
provenance and atomic short enqueue acceptance. Do not bypass receipt/hash/member
authorization or rewrite immutable archives. V311 old-normalizer association and
all original V5/contracts/53/platform/provider requirements remain open.

Current S178 preview: the sole start-current-preview.cmd entry is V31324fa3a33,
after F31210167passed/0failed/0pending plus types/whole lint/UI build and a
successful embedded build. N309 applies only to unchanged691native inputs.
Chinese synthetic desktop verified A/B adoption, dirty cancel/confirm, exact B
manual preparation and the prior completed-prepare close/reopen/Back regression:
same-scan two rows/B selection recover, independent remains unselected, explicit
Refresh and another reopen work. Dark/light and comparison Escape/focus verified.
Normal Quit of App68988/all8family members, guard20281/helper45489 exit0; no App
left running, immutable6archive/26seed/3source hashes unchanged. No product
commit/Undo/private-source/real-provider/security action. Plain cmd -CheckOnly
passes hash/argument forwarding without launch, data path unchanged. Six replaced
successful EXEs retired; failure evidence and uncertain owners retained.1675
temporary files plus6EXEs removed603091751logical bytes; observed H free delta
606339072bytes separately, not exclusively attributed physical recovery.
No verified narrow dimension/true IME/system DPI/vendor/live-WAL/real ACP/other
original-platform/all53 acceptance. The V310 FALSE proof remains FALSE. Continue
plans/v311-legacy-session-identity-discovery.md before any new identity behavior;
existing commit_identity registration migration/workspace adoption are not absent.
All original V5/contracts/53 delivery obligations remain open, no publication.

Imported-history titles are source display metadata. Renaming in Grok App writes
a local alias; it must not rename a source file or overwrite its transcript.

- Codex: first readable user prompt, falling back to the native session id.
- Cursor JSONL: first readable `user`/`human` text, including structured text
  blocks, falling back to the transcript identity. Assistant text, tool output,
  metadata and sidechain records do not supply the main title. Composer/Store
  names retain their existing adapter behavior.
- Claude Code: last nonempty `customTitle`, then `aiTitle`, `lastPrompt`,
  `summary`, then first readable main user prompt, then native session id.
  Explicit source names are retained in full after trimming outer whitespace.
  Prompt-derived names are whitespace-normalized and limited to 80 Unicode
  characters. Tool results, `isMeta`, `isSidechain` and `isCompactSummary`
  records are not prompts.
- Claude's `summary`, `custom-title`, `ai-title` and `last-prompt` records remain
  metadata, not user bubbles or unsupported content. Listing uses the existing
  bounded head/tail samples because Claude appends renames at the end. Full
  preview resolves metadata from the fixed complete transcript.
- On unchanged reimport, an automatically generated native-id title may improve
  without changing the archive. Preserve aliases and non-fallback catalog titles.
- Normalizer `1.0.1` recognizes Claude title metadata and durable transcript
  names. Earlier normalization stays immutable; reimport may create a new
  revision of the same entry. Do not relabel an old projection as a new one.
- Normalizer `1.0.2` additionally preserves Claude `isCompactSummary` content
  as typed compaction events. `compact_boundary` stays metadata, and its
  `logicalParentUuid` is a historical compaction relation, not a live Parent
  link. Earlier events remain readable on their original branch. Neither a
  logical parent nor a summary proves which event IDs were covered; coverage
  remains explicitly unknown. `isMeta` content keeps its system-message
  quarantine and sidechain authorization remains unchanged. Unknown summary
  blocks are opaque loss markers, not tool calls, and make the import partial.

Regression coverage lives in
`src-tauri/src/session_migration/engine_title_tests.rs`: production scan,
preview, plan/commit, catalog reload, Unicode/fallback, appended renames and
reimport/alias preservation. These are synthetic integration checks, not proof
of an installed source application's compatibility or real provider continuation.

The Claude precedence and append-only `custom-title` shape were checked against
[Anthropic's session listing implementation](https://github.com/anthropics/claude-agent-sdk-python/blob/23bb0157f51f83c21a7c47fbed1e5741ad581082/src/claude_agent_sdk/_internal/sessions.py#L441)
and [session rename implementation](https://github.com/anthropics/claude-agent-sdk-python/blob/23bb0157f51f83c21a7c47fbed1e5741ad581082/src/claude_agent_sdk/_internal/session_mutations.py#L97).
The source JSONL format remains version-dependent.

## Import preview request ownership

The content-state hook owns each async preview, page and branch request with a
hook-lifetime monotonic request sequence, in addition to the immutable candidate,
revision, preview and leaf checks. Removing a slot must not allow its replacement
to reuse an old request owner. Close/reopen, scan reset and an explicit member
change invalidate obsolete replies, including errors and busy cleanup.

`invalidateCandidate` removes only that candidate's pending/preview state and
retained membership. Other candidates keep their content selections, cursors and
errors. Ordinary A -> B -> A navigation still restores retained content; it is
not an invalidation. Only the actively opened candidate may adopt the active
preview key. Unmount clears request ownership without setting React state.

Synthetic hook regressions are in
`src/hooks/useSessionImportContentStates.lifecycle.test.tsx`. They mock preview
and event IPC, not the ownership guard, and do not substitute for desktop or
provider acceptance.

Compact-summary shapes and the live-chain distinction were checked against the
same pinned [Anthropic source](https://github.com/anthropics/claude-agent-sdk-python/blob/23bb0157f51f83c21a7c47fbed1e5741ad581082/src/claude_agent_sdk/_internal/sessions.py#L935)
and its [public synthetic example](https://github.com/anthropics/claude-agent-sdk-python/blob/23bb0157f51f83c21a7c47fbed1e5741ad581082/tests/test_sessions.py#L801).
Additional regression coverage is in
`src-tauri/src/session_migration/adapters/claude_compaction_tests.rs` and the
synthetic fixture `claude/compaction.jsonl`.

## Fixed groups and lazy member metadata

`useSessionImportGroups` owns the `fixed-groups` selector separately from the
legacy candidate scan selector. Never adopt its descriptor into `ScanState`.
Scope includes scan ID, epoch and UI generation; requests and mutations have
hook-lifetime owners. Reserve CAS mutation before IPC, use the accepted version,
and reconcile descriptor/membership at most three times. Do not show new counts
with old checkboxes after an acknowledged mutation. A subsequent read failure is
not a rollback. A callback can synchronously change scope; recheck ownership
before the mutation starts another page read.

Metadata growth does not select new groups or replace existing member preference.
For an unchanged group-ID page, metadata-only refresh does not reread signed
choices. Snapshot caller filters before awaiting and reject foreign/duplicate
IDs, counts, identity, filters and selection versions. Typed native identity is
group evidence, never proof of equal bodies or a recommendation of the best one.

`useSessionImportGroupMembers` is lazy: opening one group reads one page of at
most 50 metadata rows. Next/previous replaces rows, retry retains its attempted
cursor, and close/reopen, group/scope changes and unmount expire late success,
error and finally. Empty live discovery tails refresh in place rather than add
identical cursor-stack entries. No source body or preview is loaded by this hook.

`useSessionImportSelection` now connects these domains to the wizard. New scans
use fixed groups; stable group row IDs are distinct from chosen candidate preview
IDs. The candidate descriptor stays in `ScanState.selection`. The deliberate
independent-candidate method must not become an automatic error fallback. The
wizard exposes a deliberate domain selector; switching either way releases only
actually opened preview owners (bounded to 200), including independent previews.
It never rewrites an existing batch or source history.

`SessionImportGroupMemberPicker` opens one solid portaled member menu at a time.
Only its mounted lazy owner reads metadata (50 per page), never source bodies.
Labels use the API's format/root/locator; first-discovered is not a best-version
recommendation. CAS failures retain content; an ACK followed by a read failure
leaves a review/refresh path rather than claiming rollback. Menu close expires
late page/action replies; a completed old action cannot close a reopened menu.
Escape restores its group trigger, outside focus is retained, and cancelled Tab
explicitly advances from that trigger in the parent dialog's logical tab order.
IME confirmation does not choose a version. These remain synthetic interaction
results until the replacement desktop gate.

Preparation verifies only retained preview overrides, at most 200, with bounded
membership reads of 50. Require the exact accepted selector version; do not
silently adopt another writer's newer membership or enumerate 10k chosen members.
Expire the owner before and after each await. Reserve preparation synchronously
and reject concurrent choice reads/edits. A mismatched prepare acknowledgement
requests cancellation and is not adopted; this is not proof cancellation succeeded.
Existing/recovered batches remain locked until explicit abandonment.

Choosing, clearing and accepted refresh/external preference changes invalidate
only obsolete candidate previews. CAS failure keeps prior content. Keep the last
coherent display snapshot during the ACK-to-membership gap so rows retain their
DOM identity; show its matching counts, disable stale controls/Next, and keep the
active descriptor unready until membership agrees. A read failure is not rollback.
Reject zero-version nondefault choices, nonempty clear acknowledgements and
same-version changes to previously accepted chosen IDs/revisions/selection bits.

A changed transcript or workspace source intent clears the old scan synchronously
as well as group ownership, including workspace-preview failure. Ordinary close
can retain a completed scan for reopening, but an old discovery acknowledgement
cannot recreate a closed selection. Source/close changes also expire navigation
waiting for prepare abandonment. Initialization failures remain visible.

Synthetic domain/owner regressions live beside the two hooks and in
`src/lib/sessionImport/groupSelectionState.test.ts`; they mock only IPC and do
not replace native desktop, installed-source or provider acceptance.
Real-hook synthetic wizard integration is in `useSessionImport.selection.test.tsx`
and `SessionImportHost.groups.interact.test.tsx`. Existing focused wizard unit
tests retain their explicit selection facade mock; they are not bridge evidence.

## Explicit local Cursor history locations

The source step exposes location management only for the existing Cursor/local
contract. A readable default installation is registered only after an explicit
user action, without sourceId or relocation; listing never registers or merges
the two default roots. A picked folder is associated only on a second explicit
confirmation, with sourceId/provider/host/path/expectedPrimaryRoot fixed.

Closing or losing the association acknowledgement is not rollback. Retain the
original idempotent request and review that same request on reopening. An accepted
association stays accepted if the subsequent list fails; it is not automatically
selected for import. Read one page of at most 50 metadata roots and replace it on
paging; retry keeps the attempted cursor. Users may explicitly select up to 200
roots. SourceChoice retains full locations, while ScanSource sends the existing
top-level provider/path/sourceId/locationId/host wire fields, not a nested DTO.

Scope includes committed source intent, installation and primary root. Layout
effects invalidate obsolete owners on committed input changes and unmount;
abandoned renders must not mutate ownership. Expire old success/error/finally
and synchronously guard selection/association flights. The default-registration
trigger stays mounted for close-focus restoration. No source history or old
batch is rewritten, and none of this moves a workspace.

Synthetic regressions are in `useSessionImportLocations.test.tsx`,
`SessionImportSourceStep.locations.test.tsx`, `sourceChoice.test.ts`, and the
real-hook Host group interaction suite. They cover lost acknowledgements,
late default registration, bounded paging and the actual multi-root scan wire;
only IPC is mocked. F283 passed its focused gates, then two ownership effects
were moved to layout timing. F284's 9745 executed assertions passed, but one old
gallery suite did not collect because its picker mock was incomplete. The
mock-only fix passed all 12 gallery tests. F285 subsequently passed full frontend
typecheck/lint, 9757 tests and the UI build with 3170 stable inputs and unchanged
native inputs. These checks still do not substitute for replacement desktop,
installed-source or real provider acceptance.

V284's embedded native build subsequently passed. Its closed synthetic Windows
cycle exercised explicit association without auto-selection, both-root scan,
5 candidates / 3 groups / A-B-C member menu, deliberate B selection, domain
switch preview invalidation, history-only publish, reader/search/clipboard export
and a local continuation draft without sending. The read-only r4 audit binds B's
body/sourceMember, the unique receipt/revision, six unchanged old archive files
and the published creation intent; catalog/scan and companions remain unchanged.
The structured HandoffDraft hash is not the text-file SHA: validate the intent
digest and draftHash-to-sidecar/reference binding, and compare intent text to
handoff_draft bytes separately. Earlier validator failures remain recorded.

All eight owned App/child processes exited normally and the disposable WebView
cache was removed. The source row cropped under a temporary-source banner,
light-theme import surfaces, actual narrow-window/IME/DPI and actual undo remain
open (undo needs its action-time confirmation). This partial evidence does not
promote V241 or complete replacement desktop/vendor/ACP/platform acceptance.

The reuse-profile restart checked the light source/location/reader surfaces and
actually resized to 931x802. Choosing a registered source reproduced the clipped
third row; a wheel over its root pane did not reveal the directory action. S162
therefore restricts the fix to source-wizard sizing/scrolling: its measurement
child keeps natural height and bounded root scrollports may chain to the outer
body. Content workbench sizing, sibling footer and unrelated root containment
stay unchanged. CSS contracts are not pixel or native acceptance; a new full
frontend bundle and embedded desktop retest are still needed. The older metadata
test now explicitly defers search: continuation stays disabled while busy, then
uses the pinned revision/leaf when settled. Product continuation gating is not
weakened. Both owned desktop families are closed and their caches cleaned.

F287 subsequently passed the current full frontend gate: 9761 tests, zero
failed/pending and no failed suite, plus typecheck, repository lint and UI build;
3170 inputs and Git stable. Fresh native 685 inputs remain unchanged from N279.
Its exited-owner disposable tmp was removed while the bundle and logs remain.
V288 embedded that bundle successfully (129.832s). Its closed cycle/restart
verified source-summary wheel reachability and final-directory keyboard/focus
at actual 1282x712 and 931x712. Five enlargement actions were observed; numerical
200% was not independently verified. Minimized/user-input recovery interrupted
the later Settings check; light current-source surfaces are not passed. Both
eight-process families exited via normal Quit and their disposable caches were
removed. All 3220 build-input hashes and six seeded archive hashes remained
unchanged before documentation closeout. No new import/send/actual undo occurred.
This limited desktop evidence does not promote V241 or close body comparison,
explicit legacy adoption, remaining desktop or the original 53 clauses.

## Explicit fixed-preview comparison (D1 and D2 page owner)

`session_import_source_comparison` is an actual registered async command. It
reads only two explicitly opened fixed previews in the same signed source
group/scan/epoch with different members. Disk hashes, RAM pins, the shared
prepared-selection/member/lease guard and post-read integrity all apply. Its
signed cursor binds both ordered sides, snapshot digests, leaves and version.
It does not reread live sources, change selectors or create import plans/jobs.

Rows compare full typed body fields; IDs/source time are separate metadata.
Each side retains event IDs/spans, a bounded excerpt and full fields digest.
Different branch lengths have explicit missing-side rows. Opaque/unknown or
partial content remains uncertain. Always retain position-only/unverified:
equal excerpts or an empty differences list do not prove equivalent histories.
`complete` means paging reached the tail, not that either source is complete.
Page uncertainty is local to that page and source outcome, not all earlier rows.

`sessionMigrationComparison.ts` copies both scopes before awaiting transport,
preserving cursor/order and propagating native failure without rescan or fallback.
`useSessionImportSourceComparison` independently owns bounded 50-row pages.
Committed scan/epoch/generation changes expire replies at layout timing; branch
open, close/reopen and unmount also invalidate success/error/finally. Reserve
busy synchronously, keep the accepted page on failure and retry its attempted
cursor. Validate both fixed summary identities/digests and contiguous positions,
response bytes, missing tail and uncertainty before adopting a response.

F293 passed 11 focused/134 related tests and F294 passed 59 focused/113 related
tests (sets overlap), types/targeted lint/quality, with input/Git stability.
These synthetic API/owner checks mock IPC, not request ownership. N292's 6
focused and 969 domain tests passed but Clippy failed on a test len-zero
assertion; its failure is retained. The equivalent is_empty correction passed
N295's full six gates: 6 focused, 969 domain / 13 ignored and 3898 full-library /
15 ignored, zero failed, 688 stable native inputs. This is not desktop or vendor
acceptance.

`useSessionImportComparisonPreviews` now owns independent lazy A/B previews.
Mounting reads no body; explicit start opens A and explicit member B selection
opens B. Same-side reservations are synchronous; retry keeps the fixed revision.
Committed scope/group/primary changes, close/reopen and unmount expire obsolete
success/error/finally. Changing a leaf does not change import content selection.
Closing drops UI references only; native snapshots remain under existing verified
lease expiry/cleanup, not a new arbitrary-path deletion command.

`SessionImportSourceComparisonDialog` is connected to the real ContentStep/Host.
It uses GlassModal/Select and shared tokens, with labelled A/B regions that stack
in narrow windows, bounded replacement pages, cursor retry, copy and IME/Escape
paths. F296 passed 38 focused/142 related and final F297 passed 18 focused/175
related tests plus types/targeted lint/quality and stable inputs/Git (overlap is
not cumulative). All 15 catalogs include the new copy. Real-hook synthetic Host
tests verify no choose/select/prepare/plan/commit/provider mutation and focus
restoration on close. This is not a full frontend bundle or pixel/native test.

The dialog is still read-only: deliberate A/B fixed-preview adoption into import
selection is not implemented. A's format/root
metadata was shown only when the lazy member page contained A; the new exact
metadata slice below addresses that separately. Do not claim fixed adoption,
legacy adoption, D03/D04 or the original 53 clauses are closed.

## Full typed-field inspection from the same fixed pair

`session_import_source_comparison_field` is a registered async command sharing
FixedPair/Snapshot/body-field authorization with row comparison, including both
previews' integrity, lease/member guards, lifecycle operation and post-read check.
It accepts no paths or arbitrary JSON pointers and does not reread live sources,
fetch referenced asset payloads, change source choices or create plans/jobs.

Read one top-level typed body field with that row's expectedFieldsDigest. Return
all typed body field names, eventId, full body/field digests, available, exact
canonical JSON-text segment, UTF-8 byte offsets/total and continuation. Absent
fields are unavailable with zero text/bytes and null digest, not the JSON value
null. An existing null serializes to the available text `null`. Envelope ID/time/
spans remain separate metadata. The cursor binds ordered snapshots/leaves,
side/position/field/full-body/full-field/version and authenticates before byte
offset validation. Row cursors are not field cursors. Native segments are at most
64 KiB, UI requests 4096 bytes and the response remains at most 256 KiB. Splits
preserve UTF-8 boundaries and make progress; complete is only the field tail.

`useSessionImportComparisonField` owns a single bounded segment independently of
row paging. Retry keeps the failed cursor; previous/next replaces text. Layout-
committed context, field changes, close/reopen and unmount expire obsolete success,
error and finally. The functional inspector is opened only by an explicit row-
side click, can view unavailable changed fields, keeps its field picker/focus
while another field is loading, and restores the clicked trigger on close.
Changing row pages unmounts it. Labels, JSON segment copy/error/retry and narrow
surfaces use shared components/tokens and all 15 catalogs.

N298 retained 11 passed / 2 fixture setup failures: only Cursor currently has
typed-native grouping, and an oversized first record exceeded listing sampling.
Fixtures were corrected without loosening grouping. N299 passed 14 comparison/
field regressions, fmt/compile/manifest; BaselineOnly is not a full native gate.
N301's original handle exited successfully: 14 focused/977 domain / 13 ignored,
3906 full-library / 15 ignored, fmt/compile/manifest/Clippy passed with 690 stable
native inputs/Git. Its original owners exited and compiler temporary files were
cleaned. This full result does not cover the later exact-member slice.
Compaction coverage here is typed serialization unit evidence, not a forged
Claude/Codex same-group command. Final F300 run `39d7a3bddd5546f3812ad3483faf7ec1`
passed 49 focused/96 related tests,
types/targeted lint/quality and 2508 stable frontend inputs/Git. Earlier F300
mock cleanup/export and readonly-test type failures remain recorded. These
checks are not a full frontend bundle, native desktop or final acceptance.
The public wrapper copies only the four Side identity fields; extended frozen
caller objects cannot leak UI/summary metadata into IPC. That has a regression.

## Exact primary source metadata, independent of the lazy B page

`session_import_scan_group_member` is an async read-only scan/epoch/group/candidate
point command. It returns one indexed member, verifies group schema/identity and
attested Cursor provenance, bounds metadata bytes and accepts no paths. It never
initializes selectors, scans live sources, lists every member or falls back to
firstMember. Old/incompatible schemas fail without read-time DDL.

`useSessionImportComparisonPrimaryMember` is a separate lazy metadata owner with
synchronous reservation and scope/group/primary/native/revision/lifetime guards.
The real dialog reads A's metadata through that owner, not the current B page.
Metadata errors have independent copy/retry and do not reread A's fixed body or
silently borrow metadata. Existing menus, tokens and translated labels are reused.
F302's formal test-first reproduced the missing-primary page. Final F302 run
`02e48836e6ba4b3c95fe0ecc361bfa4e` passed 56 focused/61 related tests, types,
targeted lint and quality with 2510 stable inputs/Git. N303 run
`d4a689d5569d4c08afe287f4f5c4e489` passed fmt/compile/manifest and 120 focused
tests with 691 stable native inputs/Git. Its five new command regressions cover
offline nonfirst chosen metadata, unchanged choice/schema/refs/source bytes,
foreign/missing identities, tampered group/provenance, legacy schema and byte
bounds. BaselineOnly excludes a new domain/Clippy/full-library result. Original
owners exited and disposable caches were cleaned; no fixed-preview adoption,
desktop, source-vendor or final acceptance is claimed here.

## Fixed-content transfer boundary (not yet the complete adoption transaction)

`useSessionImportContentStates.adoptFixedPreview` captures a detached validated
comparison request and discard consent under a full scan/epoch/UI-generation
scope. It reserves one flight synchronously and reads only that preview's exact
leaf first page (20 events), never ordinary preview(candidate,revision). An
independent authority must return true while its content owner remains current
before fresh default content replaces a slot. The digest is the comparison's
snapshotDigest, not a listing sourceRevision. Retained dirty primary/target
content requires explicit consent; pending branch choice blocks transfer.

Cancellation or failure keeps old content/edits, permanently expires the
target's old page request and cannot restore obsolete success/error/finally.
Close/reopen, all scope dimensions, primary/target invalidation and unmount
expire ownership. Direct focus and ordinary content mutations are blocked during
the flight. Active rollback ancestors remain valid; non-default leaves use only
their page flags, not active-branch IDs. Old pinned/assets/subagent/reasoning
grants cannot leak into a fresh selection. Native repeated asset references are
valid display metadata, not grants. DTO and record/page budgets fail closed.

F304 `4a4a75a9986d48fdae8c07607065dc73` passed 143 focused (including the existing
46) and 218 related regressions plus types, targeted lint and 40 quality checks
with 2516 stable frontend inputs/Git. Original owners exited; disposable caches
were removed with logs retained. This is not a full frontend/native/desktop
result. The selection reservation,
bounded binding registration, Host transaction and deliberate Adopt A/B UI are
still pending; do not claim the original adoption or 53-clause acceptance closed.

`useSessionImportGroups.chooseAcknowledged` now returns `acknowledgement` and
`reconciled` separately; ordinary `choose` retains its existing boolean contract.
Only an identity/descriptor/selected-bit checked response from the current
mutation owner produces an ACK. After that, failed page/membership read, callback
failure or reset keeps the returned detached ACK but does not roll it back or
repeat choose. An expired owner before receipt does not expose the old response
as a current ACK. Received wire, callbacks and result are detached from live state.

`reconciled=true` says a coherent current page was read, not that fixed adoption
is complete. Another writer can advance its descriptor/member while the original
ACK retains its old version. The future selection authority must check the exact
ACK descriptor and accepted candidate/revision before preview binding/content
commit. ACK-plus-read-failure needs explicit reconciliation, not another CAS.
F305 `c4991d92c3654366bdecbb305c0ac596` passed 96 focused (existing 78/new 18) and
218 related plus types/targeted lint/40 quality checks, 2517 stable inputs/Git and
fresh drift0. Original owners exited and caches were cleaned. This remains
synthetic IPC evidence, not a Host adoption transaction, UI or native acceptance.

## Fixed adoption transaction (V306 prerequisite; V307 UI below)

The preceding V304/V305 paragraphs describe their earlier boundaries. Selection
now owns `adoptFixedPreview(request, transfer)` above child comparison lifetime.
It synchronously reserves before fixed-events preflight, captures exact scope,
group/primary/revision/descriptor/selected bit, and grants one exact-request CAS
authority. Full ACK identity/version/counts and the accepted member's revision
must still match after coherent reads. `reconciled=true` alone is not adoption.

Owned ACK retains prior edited content for review but revokes prepare eligibility.
Review-only and eligible preview owners share the 200-slot budget. Prepare checks
all override identities/duplicates before a private eligibility projection; the
original `selectedOverrides` membership validation is unchanged. Public preview
registration stays blocked; only the exact adoption authority can register the
new binding through the same bounded invariants. Failed completion revokes that
binding by object identity, never a later registration with the same candidate ID.
Explicit clear/source/wizard close reclaim review-only state.

`transfer` is `{ run, isCurrent, getError }`, not a commit callback. The actual
content hook's `isCurrent` checks its just-committed slot object/lifetime/scope.
Do not use a verifier that merely returns true or matches preview IDs. Editing,
closing or invalidating content between its commit and transaction completion
must leave no import eligibility. Later deliberate edits after completed adoption
remain ordinary content overrides and are validated at prepare.

ACK plus read failure holds one bounded request/accepted ACK for explicit
`reconcileFixedPreview`: exact fixed events and authoritative reads, never another
choose or ordinary preview request. Existing prepare-batch locks remain enforced.
Lost ACK is uncertain, not permission to resend. Superseded ACK or invalid/expired
fixed snapshot is stale and needs explicit recompare. Cancel does not reverse an
accepted native preference; an exact already-dispatched tombstone lasts only until
that command settles to prevent destructive late callbacks. Full source/wizard
close resets both selection and lower groups owners; late success/error/finally
cannot update a new flight.

F306 `7dbbce17719146ec8801e0b8901f1c77` original12528 exit0 passed179 focused
(143 existing/36 new),326 related, types/targeted lint/quality,2521 stable inputs/
Git and fresh drift0. Real hooks with only synthetic IPC are exercised, including
prepare eligibility and the total200 bound; Host adoption UI/complete native
prepare chain/embedded desktop/vendor/real provider/original53 acceptance remain
unfinished. Current preview stays V241. Continue with external
`plans/v307-fixed-adoption-ui.md`; do not mark delivery complete from these tests.

## Host adoption UI and exact recovery (V307)

The Host now owns comparison lifetime above ContentStep with one context, dirty
consent intent and retained read-retry request. Only ownsComparison() for the exact
captured request exempts an ACK/read gap from ordinary stale-context dismissal.
Other scope/group/member changes remain invalidating. Source/wizard/unmount and
all explicit close paths cancel both owners without undoing accepted preference.

Deliberate Use version A/B takes A's independent primaryMetadata.member or B's
actual slot.member, original previewId/adapter and the visible native summary.
Never manufacture adapters from display summaries or infer full A metadata from
the lazy B member page. Shared GlassModal consent is exact-request scoped; cancel
dispatches no events/CAS. Synchronous refs/getStatus()/isLocked() defend the gap
before React rerender and after awaits. Accepted read failure offers read-only
Retry; lost ACK/stale/expired requires explicit Compare again, not CAS replay.
comparisonContextKey intentionally ignores metadata-only memberCount/title growth;
the adoption transaction separately validates the captured group. Do not change
that read-only key to repair an adoption ownership issue.

Adoption never selects a group, prepares/imports automatically or inherits grants.
Success reveals the content preview on narrow screens, and focus restore avoids a
filtered row that was removed. New attempted transfer diagnostics do not reuse an
earlier transfer error after dirty refusal; retained slot errors/edits are preserved.

SessionImportHost.adoption.test.tsx uses real Host/selection/groups/content and
controller, mocking IPC/native-cover only. Import the adoption fixture first.
F307 0c8c92bdface4db19b386a241ad4a2fc passed196 focused/784 related (overlapping),
types/targeted lint/quality with2524 stable inputs/Git and exited original owners.
Thirty close permutations, prepare overrides and read recovery are synthetic IPC
proof, not native desktop or real provider acceptance. Fifteen locales have parity.
V308 frontend/V309 native full gates have now exited0 at S175: 10158 frontend,
14 native focused/982 domain (13 ignored)/3911 full (15 ignored), zero failed.
Types/whole lint/UI build and fmt/compile/manifest/Clippy all passed. Source/Git
are stable with fresh3218 frontend/691 native drift0; original owners exited and
only disposable own caches were cleaned. These overlapping sets are not summed.
V241 is the sole usable preview until one V310 embedded replacement passes scoped desktop and normal
Quit/family-zero validation. Continue plans/v310-current-source-delivery.md; the
original V5/contracts/53 and real vendor/provider/platform obligations remain open.
At S176 V310 2e0b24a774824726a53483e3b9af5659 embedded build exited0 (122.850s,
3268 source/2525 frontend stable inputs and Git). Its Chinese synthetic native
desktop passed deliberate A/B adoption, dirty cancel/confirm and manual exact-B
prepare, but completed-prepare close/reopen/Back persistently empties grouped
rows and Refresh cannot recover. desktop-smoke.json is FALSE, so V241 stays
current. Theme causality is unproven. App44960 and all eight owned processes
exited normally; guard78869 and helper6609 exited0; cache cleaned and no real
source/provider/import commit/Undo. Old6 archive/26 immutable seed/3 source hashes
remain unchanged. First follow plans/v310-prepare-return-recovery.md with an
actual Host/controller red regression; do not remove the prepare lock or revive
stale grants. Keep V310 as failure evidence, not a promoted user package.
S177 formal red afdcc2fa and two late-read reds are fixed: actual Host/controller
final222f751e passed9 focused/751 related (overlap), types/lint/quality,2526 stable
inputs/Git and fresh drift0. Discard before same-scan reopen; failed cancellation
keeps batch lock. Closing/new source/unmount guards old navigation. A visible
content lifecycle reinitializes after lock release; no live scan returns Source
without default discovery. Missing scope Refresh reinitializes read-only, scoped
failure requires explicit retry. No selection, prepare, commit or grants revive.
Native N309691-input manifest remains identical; no new native claims. Actual
full frontend F3127b31b450/original22660 is still in flight; inputs frozen until
terminal, then one V313 embedded desktop replay and exact normal family exit
before any promotion. The original V5/contracts/53 scope remains unchanged. Then
plans/v311-legacy-session-identity-discovery.md audits existing legacy registration
and workspace adoption before adding any new old-normalizer identity association.
