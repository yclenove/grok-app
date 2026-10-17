# Native latency report v1

The authoritative Rust contract is `report.rs`, shared by `gcu-eval::budget`
and `xtask bench-check`. `budgets.rs` is the single budget source, transcribed
from architecture §14. Budget changes require an approved ADR (变更须 ADR).
This contract is separate from the engine's tool and Host wire protocols.

The checker requires `macos.json`, `windows.json` and `linux-x11.json` in the
report directory. It binds each filename to its `platform` field. Wayland and
fake-driver results cannot substitute for a required platform. These files
are native artifacts; there are deliberately no checked-in passing examples.
Synthetic data lives only in gate tests and is never written here.

Every JSON object rejects unknown fields. Missing required fields, duplicate
object fields, unknown enum values, wrong types and unsupported versions fail.

| Report field | v1 value/type |
| --- | --- |
| `schema_version` | Integer `1` |
| `platform` | `macos`, `windows`, or `linux-x11` |
| `kind` | `native_wall_clock` |
| `status` | `measured` or `not_run`; `not_run` always fails |
| `environment` | Object with the nonempty strings below |
| `setup` | Fixed workload object below |
| `measurements` | Array with each required operation exactly once |

`environment` contains `machine` (model/CPU/memory, no personal hostname),
`os_version`, `engine_revision`, `driver_version`, `fixture_revision`,
`captured_at_utc` (RFC3339 UTC by convention), and `subscription_route`
(actual native subscription or polling route). Engine and fixture revisions
must agree across platform reports. The checker verifies presence and revision
agreement, not the authenticity of this metadata; native artifact provenance
belongs to the matching desktop runner. It never turns a label into evidence of
a run.

| Setup field | Required value |
| --- | --- |
| `fixture` | `gcu-fixture/performance-v1` |
| `window_width_logical` / `window_height_logical` | `1280` / `860` |
| `scale_percent` | macOS `200`, Windows `150`, Linux X11 `100` |
| `max_tree_nodes` | `1200` |
| `include_screenshot` | `true` |
| `text_characters` | `50` |
| `list_apps_warm` | `true` |

Each measurement has `operation` (`get_app_state`, `click`, `type_text` or
`list_apps`), `samples_ms` (at least 50 finite, positive JSON numbers in
collection order), and `percentiles` (`p50_ms`, `p95_ms`, JSON numbers).
Fractional milliseconds are retained. There is no trusted sample-count field.
The checker sorts a copy, uses nearest rank `ceil(n * percentile / 100)`
(1-based), verifies the published summary against those raw samples, and
compares both recomputed percentiles to the inclusive limits. For 50 samples,
p50 is the 25th and p95 is the 48th sorted value. It never rounds a value down
to fit a limit or removes an outlier. Extra samples are allowed; all are used.

## Fixed fixture workload

Use the cross-platform fixture's default controls, same fixture and engine
revision across platforms, on an isolated desktop without accounts. Observe
the actual client geometry and OS scaling; do not infer them from requested
settings or resize captured images to conceal a mismatch. Keep the normal
router policy, safety gates, redaction and settle configuration enabled.

Reset state and resolve fresh handles/indices before each sample, outside the
timer. Warm discovery and accessibility activation outside the timer; cold
lazy-tree runs belong in separate artifacts, not this warmed budget report.

- `get_app_state`: default controls, node cap 1200, screenshot explicitly true;
  include tree acquisition, transforms, safe capture/encoding and rendering.
- `click`: semantic element click on `counter_button`, counter initially zero;
  independent fixture state must show exactly one increment.
- `type_text`: focus empty `name_field`, then send the 50-character payload
  `01234567890123456789012345678901234567890123456789`; independent state must
  match it exactly. Focus/reset is outside the timed call.
- `list_apps`: warm discovery first; the complete response must include the
  running fixture. In particular, Windows's discovery cache must be warm.

Time dispatch through the complete tool response with a monotonic wall clock.
Action timing includes delivery, the existing P6-01 settle engine, observation
and rendering; neither `settle.ms` nor a driver's delivery duration is a
substitute. Check the oracle outside the timer. Failed/unknown/cancelled calls
abort collection; never drop and replace attempts, retry an uncertain action,
or fabricate durations. Missing permissions, geometry/DPI mismatch or an
unavailable native desktop remain `not_run` and cannot pass.

`collector.rs` defines the bounded, cancellable native adapter interface.
This module registers no native adapter and produces no measurement files.
P6-01 baseline and false-negative evidence remains a separate acceptance
requirement; this latency report cannot prove it.
