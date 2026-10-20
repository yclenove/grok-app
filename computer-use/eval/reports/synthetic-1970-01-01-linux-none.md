# Core efficiency A/B

This report is synthetic. It is not a measured run.

schema_version: 1
suite: core
synthetic: true
provenance: synthetic
date: 1970-01-01
platform: linux
model: none
effort: unspecified
machine: unspecified
seed: 1818341383
conclusion: rollback
applies_defaults: false

Round trips use the client transcript turns field when it is present. Otherwise the count is the runner tool-log tally of computer-use invocations. Missing token fields stay unavailable and are never filled from tool-result bytes.

## Arms

| arm | profile | include_screenshot | diff | batch | script_requested |
| --- | --- | --- | --- | --- | --- |
| A | minimal | true | full_each_step | false | false |
| B | full | auto | within_turn | true | true |

## Gates

| gate | result |
| --- | --- |
| success | not_evaluable |
| rounds | not_evaluable |
| tokens_in | not_evaluable |
| red_team | not_evaluable |

Input tokens omitted from the transcript are unavailable. Tool-result bytes are not tokens.

## Arm totals

Scored rows only. `not_run` rows are omitted. Tool-result bytes are not tokens.

| arm | scored | passed | images | image_bytes | unknown_calls | false_success | extra_approvals | tool_result_bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| B | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Paired tests

rounds: not_evaluable
tokens_in: unavailable

## Reasons

- Roll back defaults.
- success rate is not evaluable
- round-trip median drop is not evaluable
- input tokens are unavailable
- red-team violations are not evaluable
- synthetic evidence cannot change product defaults
- product defaults were not changed

## Notes

- one-shot OS/desktop container or VM is not available; isolated GROK_HOME is not an environment boundary

## Trials

| task | arm | repeat | status | rounds | rounds_source | tokens_in | tokens_out | images | image_bytes | unknown_calls | false_success | extra_approvals | tool_result_bytes | violations |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
