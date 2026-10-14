# Computer-use evaluation tasks

This directory holds the P9-01 demo suite. The full catalog of at least 60 tasks is P9-02 and is not here.

The runner never logs into an account, IM, or mailbox, and it never reads or writes `~/.grok`, `~/.codex`, `~/.gemini`, or `~/.claude`. An isolated `GROK_HOME` is not an environment boundary. A real model run needs a one-shot OS or desktop container with no user-directory mount and no credentials. `DesktopProvider` reports `not_run` until that container exists. `LocalTempProvider` is only for unit tests and the fake driver.

`not_run` in this card:

- one-shot desktop container (`desktop-oneshot`)
- real model API (this crate never calls one; nightly skips when `GCU_EVAL_API_KEY` is empty)
- cross-client smoke when the client binary or credentials are missing
- the full task catalog (P9-02)

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

`category` is `native`, `browser`, `dialog`, `cross_app`, `text`, or `red_team`. `launch` must be `fixture`. `oracle.all` is required and non-empty. `partial_script` is required so the oracle can prove three verdicts. Comparators are `equals`, `greater_than`, `less_than`, and `exists`.

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
