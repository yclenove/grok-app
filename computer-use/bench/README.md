# Settle benchmark contracts

## Native performance budget gate (P9-03)

From `computer-use/`, on the authorized build machine:

```sh
rcargo test -p gcu-eval --test budget
rcargo test -p xtask --test commands
rcargo run -q -p xtask -- bench-check
rcargo run -q -p xtask -- bench-check --reports <native-artifact-directory>
rcargo run -q -p xtask -- budget
```

`bench-check` reads `macos.json`, `windows.json` and `linux-x11.json` from this
directory by default, independent of the caller's working directory. All three
platforms and all four operations must pass; missing files/samples, `not_run`,
invalid schema/setup, duplicate entries, inconsistent p50/p95 or over-budget
percentiles produce a nonzero exit. Successful output includes p50/p95 and raw
sample counts. The existing 14000-byte tool schema ceiling also applies; current
profiles are subsets of that checked schema. There are no threshold or platform
subset overrides.

See [report-v1.md](report-v1.md) for the versioned report shape, fixed fixture
workload, window/DPI and collection semantics. Budget constants live only in
`budgets.rs`; changes require an approved ADR. `gcu-eval::budget` exports the
same validation sources and the native collection interface from `collector.rs`.
The existing fake settle executable below never produces native report files.

Matching desktop runners must supply real artifacts before using this command
as a required measurement job. An absent platform stays `not_run` and fails;
the command never generates replacement samples. Native CI integration is
configured separately. The existing CI test suite can run the synthetic gate
tests, which verify rejection rules and make no native performance claim.

## Offline settle benchmark (P6-01)

P6-01 delivers deterministic logic and an offline executable skeleton. Native
latency, P1/P2/P3 baseline comparisons and the native false-negative rate are
**not_run**. Fake logical time is never evidence of native p50/p95 or support.

From `computer-use/`, on the authorized build machine:

```sh
rcargo test -p gcu-core --test settle
rcargo test -p gcu-core --test router
rcargo bench -p gcu-core --bench settle
rcargo clippy -p gcu-core --all-targets -- -D warnings
```

The executable at `settle.rs` uses the real engine, an in-memory driver and a
paused Tokio clock. It samples each scenario 50 times: notification silence,
polling, delayed notification, and silent delayed polling. The fixture oracle
must report exactly one submission; the driver must receive exactly one action.
Output explicitly identifies fake logical settle milliseconds. It neither
creates native report files nor supplies fabricated native measurements.

## Defaults awaiting measurement

These are the existing architecture §6 defaults, not measured platform tuning.
The router can inject `SettleEngine::new(SettleConfig, clock)`; tool call budget
overrides still cap at 1500ms. Zero intervals clamp to 1ms. Stop checks remain
10ms, native reads cap at the remaining horizon and `DRIVER_DEADLINE` (4750ms),
and the product's native-call ceiling remains 5s.

| Platform | Event quiet | Tree polling | Action / type_text budget | Native subscription facts | Native baseline p50/p95 | Tuned parameters |
| --- | --- | --- | --- | --- | --- | --- |
| macOS | 120ms | 150ms | 800 / 1500ms | AXObserver; registration can start asynchronously; AccessKit lazy activation observed in STATUS | not_run | 待实测 |
| Windows | 120ms | 150ms | 800 / 1500ms | In-process UIA structure/property/focus, bounded coalescing; deferred registration; worker transport has no events (ADR 0005) | not_run | 待实测 |
| Linux X11 | 120ms | 150ms | 800 / 1500ms | Current live AT-SPI subscriber returns None; polling is required | not_run | 待实测 |

Native acquisition/registration changes are outside this fake-clock card. A
quiet stream alone cannot prove registration readiness. The engine also needs
two complete non-busy raw observations; incomplete reads reset stability. A
closed stream drops immediately and falls back to polling. Each event batch is
bounded to 64 and subsequent draining waits at least 1ms on the injected clock.

## Effect and timeout rules

The existing wire shape is unchanged: `settle: {ms, reason}`. Consumers derive
`timed_out = (settle.reason == "budget")`; `SettleReport::timed_out()` implements
that same rule. `event_quiet` and `tree_stable` mean confirmed evidence;
`budget` means the hard horizon ended without confirmation. No action is replayed
after a settle error, cancellation or unknown delivery.

Full raw state is compared in memory, including fields omitted by text limits,
offscreen pruning and rendered node budgets. Any complete sample different from
the pre-action state is retained as change evidence, even if the final tree
reverts. Equal early samples retain the entire configured horizon, so a delayed
change is not labeled unchanged merely because an event source is silent.
At the horizon, unchanged evidence is confirmed only with two complete non-busy
matching samples, a sample no older than one poll interval and no pending native
read. Event silence or tree stability can confirm it; queued notifications
invalidate that confirmation.

`no_visible_change` additionally requires complete non-busy pre/post trees and
comparable redacted pre/post images with identical geometry and pixels. Changed
raw state or pixels produce `changed`, including on budget expiry. Unconfirmed
stability, missing images, partial trees and BUSY yield `not_checked` when no
change is proven. No claim of business success follows from any of these effects.

## Native fixture construction and acceptance

Use an isolated fixture desktop, never an account or real IM/mail/payment. Fix
the window at 1280×860 logical pixels and use macOS 2x / Windows 150% / Linux 1x
scaling, as required by quality gates §4. Record machine, OS, driver/fixture
version, subscription route, window geometry and DPI alongside each run. Warm
discovery/accessibility activation separately; include labeled cold lazy-tree
cases. Compare the same machine/fixture/setup with P1/P2/P3 baselines, with 50
samples per latency case and nearest-rank p50/p95. Missing platform/baseline is
not_run; do not substitute fake time or raise a budget to make a comparison pass.

Construct controlled visible-change cases with an independent fixture oracle
(state file plus painted marker/region), not the tree/diff being tested:

- Immediate count/toggle/value/focus/modal/geometry changes.
- Delayed effects before and after the 120ms/150ms boundaries and near the
  800ms/1500ms horizons; include silent, coalesced, delayed and dropped events.
- Initial partial/lazy AccessKit/UIA trees, later complete activation, provider
  BUSY, intermittent incomplete trees, event floods and native reads that stall.
- Pixel-only canvas changes, long values differing after character 500,
  offscreen/pruned changes, changes outside the node budget and transient
  changes that revert. Include transient durations on either side of a poll.
- No-op controls, missing/closed subscriptions and unavailable safe captures.
  These controls must not inflate the changed-case denominator.

For each native platform, gather at least 200 oracle-proven visible-change cases
with fixed published seeds and repetitions distributed across the above classes.
Self-test the oracle using positive and no-op controls before collecting a run.
Count a false negative only when the oracle proves a visible change in the
observation horizon and the action returns `effect=no_visible_change`.
`rate = false_negatives / oracle_proven_changed_cases` must be **strictly <2%**
per platform and overall (exactly 2% fails). Separately report `not_checked`,
timeouts, errors and missing images; they must not be represented as successful
change detection. Report late effects outside the horizon separately, including
whether uncertainty was returned, and never hide them by extending the cap.

Persist raw counts, case IDs/seeds, timing classes, oracle verdicts, all effects,
settle reasons/durations and latency samples in the eventual native reports.
The fake tests exercise boundary rules and evidence preservation; they cannot
prove the empirical <2% target or capture arbitrary sub-poll silent transients.
All native results in this card remain not_run.
