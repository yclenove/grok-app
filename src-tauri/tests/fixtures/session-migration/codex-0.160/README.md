# codex-cli 0.160.0 rollouts (owner export, scrubbed)

Source: codex-cli 0.160.0 on Windows (seven), Administrator test profile,
exported by the owner on 2026-10-21 to C:\grok-drop\native\codex. The
conversation is the synthetic demo in
`H:\aicoding\grok-session-import-validation\native-demo\演示 工作区`
(notes.md, calc.py, two generated PNG images); prompts A1–A8 are listed in
that folder's PROMPTS.md.

| File | What it shows |
| --- | --- |
| `…-01a15f42-…cc141.jsonl` | Main thread: image input that Codex itself failed to read, three exec calls (one exit 1), view_image returning an image item, apply_patch, then /compact. Tool outputs are content-item arrays. |
| `…-01a15f42-…cc141_01a15f4a-….jsonl` | Rewind branch written by resume (no rollback record): paginated rollout whose session_meta carries `history_base {thread_id, end_ordinal_exclusive: 75, end_byte_offset}` pointing before A6 and before the compaction. |
| `…-01a15f43-….jsonl` | Separate `codex exec` session. |

Scrubbing: `python -I scrub.py <export-dir> <out-dir>`. It is an allowlist
over records: developer/AGENTS.md/environment/permissions text, base
instructions, turn context, world state, thread settings, token usage,
encrypted reasoning and item_completed details are replaced; every record is
kept so ordinals still match, and `end_byte_offset` is recomputed for the
scrubbed parent (same record boundary, 75 records). The script refuses to
write output containing the exporting profile's paths or credential words.

Original export SHA-256 (not committed):
    7813aa53c6f9e5994fab02a1071333b522b353072f34229784ab557646896dec  rollout-2026-10-22T00-19-04-01a15f42-520d-7f22-8aa3-b764cfccc141.jsonl
    e942b1843479e491c6965454832768a04d41577ae40e59b04f539a82b5d850a4  rollout-2026-10-22T00-20-29-01a15f43-a066-7351-b5de-316fdc562f9f.jsonl
    20dfc9022147e5b4cbbac90e1469a6c2c9c1cf5037c1c12e233f19f7404b879a  rollout-2026-10-22T00-28-11-01a15f42-520d-7f22-8aa3-b764cfccc141_01a15f4a-aa3e-7781-8d92-dee133706e94.jsonl

Scrubbed fixture SHA-256:
    b71dc60229107a8726f6bd2e144c3a7bfec033813c5397fbe2e6cf3eda8afaf3  rollout-2026-10-22T00-19-04-01a15f42-520d-7f22-8aa3-b764cfccc141.jsonl
    b975498554be4690fdea0ff47eee1f1075a7488913e60007dd88fd9c8279025b  rollout-2026-10-22T00-20-29-01a15f43-a066-7351-b5de-316fdc562f9f.jsonl
    8a12f02c9440a7409d49d373ab60f96b6e0c0b5de9dc637362ec16a137353844  rollout-2026-10-22T00-28-11-01a15f42-520d-7f22-8aa3-b764cfccc141_01a15f4a-aa3e-7781-8d92-dee133706e94.jsonl
