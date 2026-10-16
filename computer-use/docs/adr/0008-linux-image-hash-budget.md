# Linux cold image hashing budget and cache
Status: accepted (P7-04 CI repair assignment)
Date: 2026-10-15

## Context
PR #59 Linux job 116800977204 timed out hashing its own debug test image.
P7-04 already streamed 64 KiB chunks on spawn_blocking, but had no cache,
a 256 MiB size cap, and a five-second deadline around the entire inspection.
P3's synchronous process inspector also rehashed on every call and opened the
canonical path separately from the running image. Raising a test timeout would
leave the product failure and repeated CPU cost intact.

## Decision
Share the opened-image streaming implementation between P3 and Wayland.
Use spawn_blocking for async inspection and a bounded dedicated worker for the
legacy synchronous APIs. Keep fixed 64 KiB buffers and cap images at 4 GiB.
Allow 180s for a cold digest: 4 GiB / 32 MiB/s = 128s, plus 52s scheduling/I/O
margin. Inspection allows another 5s for metadata and worker queueing. This is
a read-only identity budget, independent of DRIVER_DEADLINE and the unchanged
five-second bounds on compositor IPC and input delivery. Callers with shorter
budgets may still cancel and receive a definitely-unexecuted timeout.

Cache at most 128 images by dev/inode/size/mtime (nanoseconds) plus ctime, with
single-flight hashing for concurrent uses. Reopen and check metadata on hits;
recheck the open image, canonical path, proc link and process start time on
every process inspection. ctime also invalidates content edits that restore
mtime. Never cache incomplete, unstable, cancelled or timed-out digests.
Hashing checks an injected monotonic clock and cancellation between chunks;
dropping an async caller signals cancellation even while its worker is queued.
An OS read already in progress cannot be interrupted; the async wait remains
bounded and the worker checks cancellation as soon as the read returns.
Timeout returns Timeout(maybe_executed=false); unstable/oversized images fail
closed to handoff. No failed inspection produces an authorized identity.

## Alternatives
Keep five seconds / increase only the test timeout: rejected, repeated cold
hashing still fails in production. Read the whole file: rejected, unbounded
memory cost. Cache by path/PID: rejected, updates could inherit old grants.

## Impact and exit condition
No Driver/protocol/authorization-key changes. Metadata changes force rehashing;
changed image identity still requires authorization. Revisit the size/throughput
budget if supported images exceed 4 GiB or measured cold throughput is lower.
