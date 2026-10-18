# P9-06 soak report contract (schema_version 1)

No native soak reports are included here. Synthetic integration tests exercise
the harness and validator; they are not platform measurements or L7 evidence.
This contract is independent of `computer-use/bench/`.

From `computer-use/`, on the authorized build machine:

```sh
rcargo test -p gcu-eval --test soak
```

On a matching disposable desktop only, the intended opt-in entry point is:

```sh
cargo run -p gcu-eval -- --suite core --soak-hours 4
cargo run -p gcu-eval -- --suite core --platform linux-x11 --soak-hours 4 \
  --soak-report /path/to/bundle/soak-linux-x11.json
```

Native collection adapters are not part of this skeleton. Without a supplied
bundle the CLI immediately reports `not_run`, exits nonzero, and does not wait
four hours, launch processes or execute the ordinary task runner. The library
`soak::run_soak` accepts injected clock/target/resource/scenario adapters. Its
default duration is zero. Never run a desktop soak as a resident build8 job.
Long runs belong in a one-shot matching desktop with no credentials, real
accounts or user-home mounts; an isolated `GROK_HOME` is not that boundary.
Ordinary CLI invocation without `--soak-hours` keeps ordinary runner behavior.

`--out DIR` explicitly writes `soak-<platform>.json`. There is no implicit
repository output. The file stores raw evidence, not a trusted passing summary.
`--soak-report` reparses/revalidates the bundle and uses the CLI's requested
hours as an independent minimum, so editing the embedded duration cannot
weaken that invocation. Relative artifact references resolve beside the input
report. Copying a report to a different bundle does not copy its artifacts.

All struct fields below use strict serde JSON decoding: unknown fields,
duplicate fields, invalid enums/types, negative counters and unsupported
schema versions fail. Optional evidence/restart fields are nullable. Units are
integer bytes, counts and monotonic milliseconds. Increasing any gate requires
the architecture's approval/ADR process, not a report override.

| Field | Contract |
| --- | --- |
| `schema_version` | Exactly `1`. |
| `config` | `suite: "core"`, `platform: "macos" / "windows" / "linux-x11"`, explicit `hours`, maximum gap `sample_interval_ms` in `1..=60000`. Default hours: `0`. |
| `clock_kind` | `native_monotonic`, `deterministic`, or `unavailable`. Only the first qualifies. |
| `target_kind`, `resource_kind`, `check_kind` | Independently `native`, `fake`, or `unavailable`; all three must be native to qualify. |
| `evidence` | Nullable object: matching `platform`, nonempty `machine`, `os_version`, `engine_revision`, `fixture_revision`, `captured_at_utc`, `disposable_desktop: true`, relative `artifact` path to nonempty real raw desktop logs. |
| `elapsed_ms` | Actual clock difference, never the requested duration substituted as a measurement. |
| `task_ids` | Complete selected matching-platform core schedule, no duplicate/empty IDs. |
| `samples` | Ordered `{sequence, at_ms, quiescent, resources}` including baseline at zero and final sample at elapsed time. Missing/duplicate sequence numbers fail. |
| `tasks` | `{sequence, id, started_ms, finished_ms, observation}` in global round-robin order across all restarts. |
| `checks` | `{kind, at_ms, observation}` for each injected scenario; earlier failures are retained. |
| `errors` | Collection/cancellation errors; no payloads. Any error fails resource validation. |

`resources` contains `rss_bytes`, `handles`, `workers`, `workers_started`,
`workers_exited`, `input_ledger_entries`, and `pressed_keys`. These are aggregate
engine/helper/worker process-tree observations, including retired-but-live
generations. Start/exit counters cover all generations and never reset on
restart. OS key/button observations must be independent of the input ledger.
Native OS sampling must be bounded and run off the executor thread. Handle
counts are retained for inspection; this card invents no handle-count budget.

`observation` contains `status: "pass" / "fail" / "not_run"`, safe `reason`,
`violations` labels, and nullable `restart`. It must come from an external
oracle/check, not the model's completion claim. `not_run` requires a reason.
Restart evidence contains `generation_before`, `generation_after`,
`old_workers_remaining`, `stop_latched_after_restart`, and
`actions_while_stopped`. A successful restart increments generation exactly
once, leaves no old worker and delivers nothing while stopped. Stop restart
also preserves the latch; the harness never issues resume. Task/restart counts
are recomputed from raw records; reports cannot supply replacement totals.

Each scheduler tick runs one task and rotates one of `crash_restart`,
`revocation`, `multi_session`, `stop_restart`, `session_cleanup`, and
`outside_cu_side_effects`. The scheduling cadence is half the configured maximum
gap (minimum 1 ms), leaving headroom for real wall-clock wakeup jitter without
raising the gap ceiling. Resource sampling also continues while a task/check
future is pending. At the horizon, pending work is cancelled without replay,
and a bounded five-second window runs final session cleanup and external scans.
Adapters must clean up on cancellation/drop and retain user Stop. Cleanup
closes remaining sessions and scans all declared original/thumbnail/crop/CLI
temporary-image/payload roots. `scan_session_residue` scans without following
symlinks or reading file contents; read errors/scan limits fail. Empty
directories are allowed; any remaining file or symlink is a violation.
`scan_outside_effects` reuses the runner's oracle over file fingerprints,
process and network observations, and refuses incomplete scan coverage.

`script_limits` and `slow_ffi` explicitly remain `not_run` with reason
`computer_script is not implemented (P6-05)`. The harness never invokes fake
script/FFI implementations, and the validator rejects a claimed pass for
either scenario. These gaps prevent a fully passing L7 soak result in v1.

The validator independently checks:

- Elapsed time is at least the greater of embedded and externally requested
  hours. A positive request is required; the release card uses four hours.
- At least `ceil(required_ms / max(1, sample_interval_ms / 2)) + 1` distinct timestamps;
  baseline/final quiescent samples; no backwards time, uncovered tail or
  interval exceeding the configured maximum gap. Same-time active-to-quiescent
  transitions and the final cleanup resample do not add time coverage.
- Maximum observed aggregate RSS growth from the baseline is **at most
  50,000,000 bytes (50 MB, decimal)**. Exactly the limit passes; one byte more
  fails. A later decrease does not erase an earlier peak. No warmup exclusion.
- Worker starts minus exits equals live workers at every sample; cumulative
  counters never decrease; quiescent workers cannot exceed the baseline.
- Every quiescent boundary has zero input ledger entries and held keys/buttons.
- A complete task cycle, valid task timeline/order, one scenario per task plus
  the two final scans in schedule order, all scenario observations,
  valid restart evidence, final cleanup/external scans and no violations.

Validation returns a separate `resource_gate` for deterministic invariant tests
and an overall native `status`. A fake resource pass is only harness logic
coverage. Fake/unavailable adapters, absent/mismatched metadata, missing raw
artifacts or unavailable scenarios can never produce overall `pass`. Qualified
native evidence with violations fails; missing native evidence stays
`not_run` while exposing all detected violations. File lookup rejects absolute
paths, traversal, symlinks and empty/missing artifacts. Artifact authenticity
comes from the trusted disposable desktop runner and review of its raw bundle;
a filename or manually fabricated JSON is not proof of a native soak.
