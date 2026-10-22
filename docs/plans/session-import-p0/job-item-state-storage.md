# Compact job headers and per-item display state

Implementation candidate, 2026-10-22. Compilation, Rust tests and capacity
measurements belong on the build host; this worktree does not establish a new
capacity result.

## Format and compatibility

New acceptance writes `jobs.json` with `items: []`, `itemStorage: "rows-v1"`,
the existing immutable context, job state, `stateVersion`, and `itemCount`.
`job_item_states(job_id, item_index, candidate_id, json)` stores the exact
`JobItem` display/result JSON. Its primary key supports ordered point reads;
the candidate index supports unique-slot checks. This is an additive table,
initialized by the writable catalog opener. Read-only opens do no DDL.

`job_items.result_json` remains the original durable input, byte-for-byte.
`job_items.phase` remains the phase projection. Display state is separate so a
missing durable input can still be reported failed/cancelled without inventing
replacement input or losing the item's place in the job.

Jobs without the marker keep their embedded items. They are never implicitly
converted: full reads, pages, progress, cancel, retry, recovery and Undo dispatch
to their original layout. A read-only legacy catalog need not have the new
table. Unknown markers, nonempty embedded items alongside a row marker, missing
row tables, missing/noncontiguous slots and mismatched row/JSON candidates fail
closed. Markerless headers with surviving display rows also fail closed, even
on decoded-header cache hits. No archive, source, durable-input schema or
acceptance codec changes.

Older application binaries do not understand the new display format. Backward
reading by this implementation is supported; downgrading the application after
creating new jobs is not a supported writer path.

## Transactions and authority

One item's publication writes its input phase, its one display row and the
compact header's version in the same publication transaction. It validates the
previous slot/candidate, preserves concurrent cancellation, and checks version
overflow. All snapshot witnesses are still consumed before writes. Signed ACK
checks read the exact raw display JSON and retain canonical item, phase,
receipt and JSON/SQL version comparisons.

Complete acceptance/member/input authentication still runs on read snapshots
and at worker start/terminal boundaries. `context_digest` and signed
header/member/receipt/allocation/outcome/tombstone checks are unchanged. Full
membership reads use the selected display layout and compare it with the
original input set and signed member count. Association's existing 16 MiB job
budget includes the header, split display JSON and candidate-column bytes,
checked with byte lengths before item bodies are owned. Candidate columns are
bounded to 16,384 bytes before allocation in point/page readers as well.

The write fence continues to protect other items' `job_items` rows and also
protects `job_item_states`. An apparent own-candidate display change must still
name exactly one row; a trigger cannot disguise a neighbor by duplicating its
candidate. Unclassifiable input/meta deletions still require full snapshot validation.
Display deletions are refused outright. Rows whose post-image belongs to
another job require full membership validation; the entire hook log is still
checked so a replacement insert or later display write cannot hide behind that
fallback. The selected input rowid and display rowid/slot are captured before
writes with indexed lookups; replacements cannot impersonate that candidate,
and repeated writes cannot hide a REPLACE deletion followed by restoration.
The connection disables DELETE's truncate optimization for fenced tables so
whole-table deletion cannot bypass row hooks; normal deletion semantics remain.
Job-header writes are refused except the exact hook event bracketed
by the worker's own progress ACK. Extra header writes before, during or after
that statement are refused, including writes that restore the original bytes.

Whole-job transitions reconstruct their items once and save the compact header,
display rows and phase projections atomically. Explicit action journals expect
`1 + 2*N` normal row writes for split jobs, versus `1 + N` for embedded jobs;
extra trigger writes and missing input projections remain errors. Requesting
cancellation still changes only the header. Batch cancellation updates eligible
display/phase rows and increments the header once, keeping raw receipt checks.
The action journal compares the exact encoded header for its layout; split
jobs additionally compare every ordered slot, candidate and raw display JSON
with the action's observed items, preserving the original embedded-byte seal.

## Reader/writer audit

| Surface | Storage handling |
| --- | --- |
| `job_control::enqueue_inner` | New compact header and display rows in acceptance transaction |
| `jobs::{read_job_body, save_job}` | Reconstruct/save the selected layout; autocommit saves own an atomic transaction |
| `job_progress` | Point update for row jobs; original JSON update for legacy jobs; batch cancel supports both |
| `job_association_header` | Recognizes the versioned marker; ordinary header decode stays bounded independently of N |
| `job_association_members` | Complete display membership from either layout, within the existing job byte cap |
| `job_publication_acceptance_journal::candidate_ids` | Same-snapshot membership from rows or original streaming embedded decoder |
| `engine_commit::job_items_page` | Header, aggregate counts and ordered page in one read transaction; tombstoned remains done |
| `publication_transition::require_ack` | Exact selected item JSON plus current SQL phase/version |
| `publication_dedup::require_unacknowledged` | Same exact selected item reader and original canonical checks |
| `publication_recovery::require_original_terminal` | Indexed unique slot for row jobs; legacy array lookup retained |
| `catalog::latest_plan_job_id` | Header-only projection already handles empty embedded items; no change needed |
| `job_history` | Existing original-input row count is display metadata; no item-body read |
| Undo, retention, request/recovery readers | Use the shared full-job reader; action write budgets include display rows |

The raw-array fault probes backed by legacy primitive fixtures stay legacy.
Real enqueue/Undo fault triggers now target display rows; historical association
probes capture/restore their exact display rows and rowids as well as the header.

## Complexity and verification

`publishing_one_signed_item_reads_and_writes_constant_job_header_bytes` uses
real registered sources and signed acceptance with 1 and 64 items, then runs
one actual publication. SQL triggers count bytes written to `jobs`; codec spans
count decoded header bytes. Both must remain below 1 KiB per header operation,
with the same decode count and only scalar-size differences between workloads.
The test also checks the final receipt-backed ACK/version and untouched peers.

Additional tests cover old catalogs without the table, signed embedded jobs
and split jobs through public cancel/retry/offline recovery/Undo/replay, both layouts through
worker restart, snapshot consistency, tombstoned counts, association byte caps,
malformed/missing rows and markers, overflow and late-write rollback, and
neighbor display corruption, header rewrites, row deletion/movement/replacement
via receipt triggers/the write fence. The negative publication probes compare
every catalog row after refusal and then publish successfully without the
fault trigger.

Full-snapshot authentication and progress aggregates still scan N per-item
rows. Whole-job transitions remain O(N). Legacy progress keeps its previous
cost. This change removes the growing job-row payload from new jobs' per-item
work; it does not claim that polling or all publication costs are O(1).
