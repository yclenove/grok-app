# Session import fixtures

Synthetic, no real user chats. Structure follows public Codex / Claude Code / Cursor shapes as of 2026-09-08 research.

| Path | Purpose |
| --- | --- |
| `codex/g1-main.jsonl` | G1: 3 messages + 1 call + 1 result |
| `codex/g4-dup.jsonl` | G4: stream+final collapse, real repeat remains |
| `claude/g1-tools.jsonl` | F03: tool result is not a user bubble |
| `claude/sidechain.jsonl` | F05: sidechain marked `isSidechain` |
| `claude/compaction.jsonl` | P3: typed compact summary and separate old/live chains |
| `cursor/transcript.jsonl` | Cursor agent transcript |
| `temporal-matrix.json` | Ten synthetic four-message production-pipeline cases: roles, direct times, missing/backwards/offset times |
| `store-wire.json` | Synthetic DB hex metadata, ordered Merkle references, plain/protobuf JSON leaves, unreachable marker |
| `generic/fenced.md` | F10: fence must not split roles |
| `generic/illegal.json` | F10: illegal object is not markdown |
| `generic/messages.json` | Compatible JSON array |

SQLite Composer/Store WAL fixtures are built at test runtime so WAL files stay live.

The compaction fixture is synthetic. `isCompactSummary` user-shaped records and
the `compact_boundary`/`logicalParentUuid` branch distinction follow Anthropic's
SDK commit `23bb0157f51f83c21a7c47fbed1e5741ad581082`,
`src/claude_agent_sdk/_internal/sessions.py` (lines 259-285, 935-941, 1029-1036)
and `tests/test_sessions.py` (lines 801-828). UUIDs, timestamps and all text are
invented; this is not an installed Claude Code transcript or real-source test.

The Cursor temporal and Store wire shapes follow the community reference
S2thend/cursor-history commit `aa0d4698289bd3801d2bac07895fe7b6adbc5ce0`,
`src/core/parser.ts`, `src/core/storage.ts`, `src/core/store-stack/store-db.ts`
and `tests/unit/store-stack-store-db.test.ts`. They were freshly fetched and
compared with the research cache. All text, IDs and source dates are invented;
these are not an official Cursor format contract or installed-source smoke.
Numeric Composer/legacy timestamps are epoch milliseconds, while valid source
RFC3339 strings keep their original offset/literal form. Store session metadata
does not supply message timestamps. Fixtures retain a named abandoned marker
so production publication/search/export/handoff can assert its absence.

Cursor identity production fixtures are generated under IsolatedAppHome by
`engine_cursor_identity_tests.rs`. Same database key suffixes across Composer
and legacy Chat, same raw transcript IDs in different declaration families, and
arbitrary same-named Store directories must not share an entry without proof.
An explicit composerId plus matching Composer key in one installation may share
identity, but different source bodies remain independently readable revisions.
These sources and markers are synthetic; no private history or credential is
read. Tests verify original bytes, publication/restart/retry and fixed-reference
behavior, not only discovery metadata. Store directory names alone are not
official native-identity evidence; supported layout/group association remains
separate D04 work.
