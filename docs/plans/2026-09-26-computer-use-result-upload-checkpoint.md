# Computer Use — bounded result upload rejection

Workstation-local September 26, 2026 (+08:00). Branch remains
`feat/computer-use-implementation`, HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
This checkpoint records Windows execution; the existing evidence directory name
`tools/computer-use-probe/.run/linux-native-20260925/` does not imply Linux runs.

## Reproduction and cause

`prepare-phase-full-core.log` finished with 509 passed / 1 failed. The failure was
the real-HTTP observation transport test. Sanitized route/size/error diagnostics
and repetition reproduced Windows error 10054 (`ConnectionReset`) on round five
in `transport-rejection-repeat-5.log`.

The rejected request was a deliberately oversized **1,058,720-byte** JSON upload
to `/cu/extension-result`. It was expected to receive HTTP 413. The body-limit
extractor dropped the unread request while the client was still writing, so the
client sometimes observed a connection reset instead of the typed rejection.
This is not evidence that an ordinary within-limit screenshot completion failed.
Diagnostics do not print request bodies, credentials, headers or document text.

## Change

`ipc/result_body.rs` separates bounded buffering from bounded rejection cleanup.
Accepted JSON remains limited to **768 KiB**; that limit has not been increased.
After overflow, buffered bytes are discarded and remaining bytes are consumed
without parsing, up to twice the accepted limit in total wire bytes. Upload work
has an absolute two-second budget and a 4,096-frame cap. Stalled or unbounded
senders cause bounded rejection and connection closure; reliable delivery to an
arbitrarily large/endless sender is not claimed.

The existing single result-admission slot is acquired before upload. It remains
held through result decoding. Ordinary traffic, heartbeat and Stop retain their
independent admission paths. Accepted content goes through the standard Axum
JSON media/syntax/schema extraction and the existing exact Broker authority
checks. No action retries, authority relaxation or credential export was added.

## Verified evidence

- Five collector tests cover the exact limit, finite overflow/EOF, hard drain
  cap, empty-frame cap and a stalled-body deadline.
- Two real-HTTP tests cover unknown-length/chunked rejection with CORS, subsequent
  schema/syntax/media rejection on the same client, single upload admission,
  concurrent heartbeat/Stop, timeout and permit release.
- `transport-drain-fixed-1.log` through `transport-drain-fixed-30.log`: **30/30**
  consecutive terminal successes for the original intermittently failing test.
- `transport-drain-full-core-r2.log`: **517/517**, no ignored tests, 107.34s.
- `transport-drain-prod-clippy.log`: strict production core-library Clippy passes.
- The subsequent rebuilt App harness also passes **104 tests / 1 existing ignored**
  in `setup-cancel-app-tests.log`. Its scope includes the later WebView setup work;
  it is not a second independent count of the core tests.

The first new HTTP test attempts are retained. One assertion incorrectly compared
an entire malformed input (`{`) with JSON error punctuation; another test appended
`text/plain` after its helper's existing JSON content type. Both were test-authoring
errors, corrected without changing the required HTTP statuses. Their intermediate
58/59 and 516/517 runs must not be represented as all-green production acceptance.

## Scope still open

This does not resolve the separate managed-browser `/cancel-run` / shutdown timeout
history, arbitrary upload delivery, all-platform installed transport, real-model E4,
or the final frozen 12-hour active soak. No commit, push, PR or release was made.
The full Computer Use goal remains active.
