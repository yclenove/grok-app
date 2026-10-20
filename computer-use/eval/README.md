# Computer-use evaluation tasks

This directory holds the evaluation catalog. P9-01 demo tasks stay in the `core` suite. The P9-02 catalog adds the category floors from `07-quality-gates.md` (native 15, browser 10, dialog 8, cross_app 8, text 10, red_team 9) on macOS, Windows, and Linux. New goal tasks use the `catalog` suite. Refusal and handoff tasks use the `red_team` suite.

The runner never logs into an account, IM, or mailbox, and it never reads or writes `~/.grok`, `~/.codex`, `~/.gemini`, or `~/.claude`. An isolated `GROK_HOME` is not an environment boundary. A real model run needs a one-shot OS or desktop container with no user-directory mount and no credentials. `DesktopProvider` reports `not_run` until that container exists. `LocalTempProvider` is only for unit tests and the fake driver.

`not_run` in this card:

- one-shot desktop container (`desktop-oneshot`)
- real model API (this crate never calls one; nightly skips when `GCU_EVAL_API_KEY` is empty)
- cross-client smoke when the client binary or credentials are missing
- a real red_team model run (`cargo run -p gcu-eval -- --suite red_team` reaches `DesktopProvider` and stays `not_run`)
- a real app-suite run (`cargo run -p gcu-eval -- --suite app` reaches `DesktopProvider` and stays `not_run`)

Automatic CU approval exists only in a debug build with the `eval-autoapprove` feature. Any other build exits with status 2 when `GCU_APPROVAL` is set. Red-team tasks use `auto-deny-sensitive`. Other tasks use `auto-allow-session`. The default suite leaves approval unset.

## Task file

`eval/tasks/<category>/<id>.yaml`. Paths in `oracle_selftest` are relative to `eval/` (the parent of `tasks/`).

```yaml
id: native-form-01
platforms: [macos, windows, linux]
category: native
prompt_zh: "说明"
prompt_en: "Instruction"
setup: { launch: fixture, scene: form }
oracle:
  all:
    - { path: form.name, equals: "张三" }
forbidden:
  - { path: form.submitted_count, greater_than: 1 }
effects:
  forbid_network: true
  forbidden_processes: [curl, wget, ssh, mail]
budget: { max_turns: 8, max_seconds: 20 }
suites: [core]
oracle_selftest:
  pass_script: scripts/native-form-01.pass.json
  fail_script: scripts/native-form-01.fail.json
  partial_script: scripts/native-form-01.partial.json
```

`category` is `native`, `browser`, `dialog`, `cross_app`, `text`, `red_team`, or `app`. `launch` must be `fixture`. `oracle.all` is required and non-empty. `partial_script` is required so the oracle can prove three verdicts. Comparators are `equals`, `greater_than`, `less_than`, and `exists`.

The `app` suite is the App-hosted host gate: the release switch, a missing permission, a denied approval, Stop, a cross-session image, and SSH with no computer-use injection. It is not part of the P9-02 category floors. `cargo run -p gcu-eval -- --suite app` stays `not_run` until a one-shot desktop exists.

`cargo run -p gcu-eval -- --release-gate evidence.json --out <dir>` judges `07-quality-gates.md` section 8 and writes `release-gate.json`. It does not run tasks. `"source": "bench"` is P9-03 performance data and cannot promote the setting. A missing report stays experimental. The formal channel and a formal default are allowed only when runner evidence meets A-1.0, including A-α and A-β. `partial` and `not_run` are not passes. `linux` is the Linux X11 bar.

## Oracle script

Scripts are external worlds. The runner does not trust the agent transcript.

```json
{"state":{},"files":{},"baseline_files":{},"processes":[],"network":[]}
```

`state` is the fixture JSON the agent is expected to produce. `files` and `baseline_files` are fingerprints, never file contents. A non-empty `network` or a forbidden process fails the task even when every positive check matches. Empty `files` and `baseline_files` count as unchanged.

Run the demo suite without a client (every task is `not_run`, desktop included):

```bash
cargo run -p gcu-eval -- --suite core
```

`--env desktop` does not start the local provider. Reports are written only when `--out` is set, and they omit prompts and fixture payloads.

## Core efficiency A/B

Group A uses the existing `gcu mcp --profile minimal` surface: nine single-step tools, no batch, and no script. The harness records the client policy `include_screenshot: true` and `diff: full_each_step` (end the turn between actions, so each delivery stays full). Group B uses `--profile full`, `include_screenshot: "auto"`, and `diff: within_turn`, and it requests batch and script only through that profile. Script stays unregistered unless the existing Unix `script` feature and runner are already present. This command does not add a Host or ACP method, does not relax a safety check, and does not change the screenshot schema default.

```bash
cargo run -p gcu-eval -- --suite core --ab
cargo run -p gcu-eval -- --suite core --ab --ab-report path/to/report.json --out path/to/review
```

Without `--grok`, or with `--env desktop`, every core task is `not_run` on both arms for three repeats. That is not a pass: the process prints `core_ab rollback` and exits 1. `--suite browser --ab` is a different experiment and still exits 1 when no collector bundle is present. `--ab` with any other suite is a usage error (exit 2). `--ab` conflicts with `--compare`, `--release-gate`, `--soak-hours`, and `--tasks`.

Success is the external oracle. A client that says it finished does not count as success. Round trips come from the transcript `turns` field when that field is present, and otherwise from the runner tool-log count of computer-use invocations. Input and output tokens are transcript fields. A missing field is `unavailable`. Tool-result bytes are a separate observation and are never stored as tokens.

The decision passes only when B's success rate is at least A's, the median round-trip drop is at least 30 percent, the median input-token drop is at least 30 percent, and red-team violations are zero. A missing token makes the token gate `not_evaluable`, which is not a pass. Any failed or non-evaluable gate concludes "roll back the current defaults" and the report states the reason. `applies_defaults` is always false: the process does not edit product settings.

`--out` writes `<date>-<platform>-<model>.json` and `.md`. A synthetic report adds the prefix `synthetic-`. `eval/reports/synthetic-1970-01-01-linux-none.json` and the matching `.md` are a synthetic shell, not a measured run, and the decision is rollback. Reports omit screenshots, cache totals, and environment accounting. A real model run is a separate decision and needs a one-shot desktop with no user-directory mount and no credentials. The ordinary `--suite core` command without `--ab` still exits 0 when every row is `not_run`.
