# P6-05 script sandbox evidence

Checklist row from `docs/plans/computer-use-v2/07-quality-gates.md` §7:

> 脚本沙箱：无 `eval` 外逃、无 IO/网络/进程/环境；受限 worker 经窄 RPC，JS/宿主/并发/队列/输出限额生效，独立 watchdog 与取消生效。

This file records test names. It is not the release signature. The orchestrator signs the row.

Other §7 rows (image denylist, approval bypass, gateway binding, clipboard, hotkeys, prompt injection, private API, supply chain) are not this card.

## Engine

- rquickjs 0.14, `default-features = false`, `features = ["std"]`.
- Forbidden features `rust-alloc`, `allocator`, `loader`, `dyn-load`, `full`, and `parallel` are absent from that dependency.
- Test: `manifest_disables_forbidden_rquickjs_features`.
- Context intrinsics are JSON, Promise, and Eval. Eval is on only so the host can compile user source with the async-function constructor. QuickJS rejects that constructor when Eval is absent (`eval is not supported`). Lockdown then deletes `eval` and `Function` and replaces both constructors with a throwing trap before the user function runs.
- User source is a string value passed to `AsyncFunction`, never concatenated into host source. Regular expressions are not an intrinsic.
- `globalThis` receives `cu` and `emit`. `eval`, `Function`, and host names are deleted and replaced with a non-configurable undefined. `Function.prototype.constructor` and the async-function constructor are replaced with a throwing trap.
- Tests: `unexposed_globals_are_absent_in_user_script`, `prototype_constructor_escape_is_rejected`, `wrapper_breakout_does_not_run_as_host_code`, `fresh_runtime_drops_previous_globals`.

## Limits

- JS heap `set_memory_limit` 32MB. Host reply text above 32MB is rejected and not stored. Output 2MB. Emits 5. Driver calls 200. Native concurrency 1. Queue 32. Code 20KB. Wall clock 30s. Stack 256KB.
- Tests: `memory_bomb_stays_inside_process_and_rejects`, `host_buffer_over_32mb_is_not_retained`, `oversized_emit_is_rejected_at_2mb`, `emit_over_five_is_rejected`, `deep_recursion_is_rejected_by_stack_limit`, `driver_call_over_200_does_not_reach_host`, `queue_over_32_is_rejected_without_blocking`, `native_concurrency_stays_at_one`, `infinite_loop_hits_wall_clock_before_cap`.
- `find` is local. Test: `local_find_does_not_call_host`.

## Narrow RPC and the same router entry

- Worker frames are length-prefixed. No serde and no native handle, path, or token.
- Every `cu.*` driver call uses `ActionRouter::observe_enqueued` or `ActionRouter::execute_enqueued` while the script holds `Session::enqueue`. There is no second channel.
- Hard stops and unknown outcomes latch. A later `cu.*` does not call the host again.
- Tests: `success_path_returns_script_value`, `stale_error_is_not_replayed`, `approval_denied_is_not_replayed`, `timeout_error_is_not_replayed`, `unknown_outcome_is_not_replayed`.
- Real router: `script_set_value_reaches_fake_desktop_through_router`, `stop_before_script_does_not_change_the_desktop`.
- Codec fuzz target `script_rpc` (not run on this host).

## Watchdog and cancel

- A thread that does not enter QuickJS fires within 500ms of the deadline.
- Production parent calls `Child::kill` (SIGKILL on Unix). It does not signal pid 0.
- Unix launcher sets `RLIMIT_AS` to 128MB in the forked child. Windows job objects are not this card.
- Tests: `watchdog_kill_runs_within_500ms_of_deadline`, `watchdog_kills_sleep_within_500ms_of_deadline`, `cancel_flag_stops_infinite_loop`, `cancel_during_host_call_unblocks_without_replay`.
- Process start under the address cap: `script_worker_process_returns_under_address_limit`.

## Exposure

- Feature off: `computer_script` is not in `IMPLEMENTED_TOOLS` and no runner is installed.
- Feature on: the tool is listed only for profile Full when a runner is installed. Default stays 15 tools. The protocol schema bytes are unchanged.
- Test: `full_profile_with_runner_reaches_router_for_computer_script`.
- `cu.d.ts` lives at `computer-use/crates/gcu-script/cu.d.ts`.

## Not run here

- `cargo +nightly fuzz run script_rpc -- -max_total_time=60`
- Real-device apps, Windows job object, and the gcu-eval soak row for `computer_script` (left `not_run`).
