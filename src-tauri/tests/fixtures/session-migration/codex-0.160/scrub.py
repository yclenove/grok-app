"""Allowlist scrubber for codex-cli 0.160.0 rollouts exported on 2026-10-21.

Usage: python -I scrub.py <input-dir> <output-dir>

The demo conversation itself (synthetic prompts about notes.md/calc.py and two
generated images) is kept. Everything that came from the exporting machine's
configuration is replaced, record by record, so ordinals stay aligned:
developer/AGENTS.md/environment messages, base instructions, turn context,
world state, thread settings, token usage, encrypted reasoning and any path
under the exporting user's profile. history_base.end_byte_offset is then
recomputed against the scrubbed parent so it still ends at the same record.
"""
import json
import os
import re
import sys

REDACTED = "[redacted by scrub.py]"
FORBIDDEN = re.compile(r"aihub|administrator|api[_-]?key|sandbox-secret|sk-[a-z0-9]|bearer |\.codex[\\/](scripts|skills|plugins)", re.I)
KEEP_META = ("id", "session_id", "timestamp", "cwd", "originator", "cli_version", "source",
             "history_mode", "history_base", "forked_from_id", "model_provider")


def scrub_message(payload):
    role = payload.get("role")
    content = payload.get("content")
    if role == "developer":
        payload["content"] = [{"type": "input_text", "text": REDACTED}]
        return payload
    if isinstance(content, list):
        kept = []
        for part in content:
            text = part.get("text")
            if isinstance(text, str):
                # Keep the vendor's structural markers (adapters recognise
                # injected context by them); replace only what is inside.
                if text.startswith("# AGENTS.md instructions"):
                    part = dict(part, text="# AGENTS.md instructions for the demo workspace\n\n<INSTRUCTIONS>\n"
                                + REDACTED + "\n</INSTRUCTIONS>")
                elif text.startswith("<environment_context>"):
                    part = dict(part, text="<environment_context>\n" + REDACTED + "\n</environment_context>")
                elif text.startswith("<permissions") or FORBIDDEN.search(text):
                    part = dict(part, text=REDACTED)
            kept.append(part)
        payload["content"] = kept
    return payload


def scrub_record(record):
    kind = record.get("type")
    payload = record.get("payload")
    if not isinstance(payload, dict):
        return record
    ptype = payload.get("type")
    if kind == "session_meta":
        record["payload"] = {k: payload[k] for k in KEEP_META if k in payload}
        record["payload"]["base_instructions"] = {"text": REDACTED}
    elif kind in ("turn_context", "world_state", "token_usage_record"):
        record["payload"] = {"redacted": True}
    elif kind == "event_msg" and ptype in ("thread_settings_applied", "token_count"):
        record["payload"] = {"type": ptype, "redacted": True}
    elif kind == "event_msg" and ptype == "item_completed":
        item = payload.get("item") or {}
        record["payload"] = {"type": ptype, "item": {"type": item.get("type"), "id": item.get("id"), "redacted": True}}
    elif kind == "response_item" and ptype == "message":
        record["payload"] = scrub_message(payload)
    elif kind == "response_item" and ptype == "reasoning":
        if "encrypted_content" in payload:
            payload["encrypted_content"] = REDACTED
        payload.pop("internal_chat_message_metadata_passthrough", None)
    elif kind == "compacted":
        payload["message"] = "Synthetic compaction summary of the notes.md/calc.py demo turns."
        payload["replacement_history"] = [scrub_message(dict(item)) if item.get("type") == "message" else item
                                          for item in payload.get("replacement_history", [])]
        for key in ("latest_token_usage_record", "resume_metadata", "retained_context"):
            if key in payload:
                payload[key] = {"redacted": True}
    if kind == "response_item":
        payload.pop("internal_chat_message_metadata_passthrough", None)
    return record


def dump(records):
    return b"".join(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode("utf-8") + b"\n" for r in records)


def main(src, dst):
    os.makedirs(dst, exist_ok=False)
    files = {}
    for name in sorted(os.listdir(src)):
        if name.endswith(".jsonl"):
            with open(os.path.join(src, name), encoding="utf-8") as handle:
                files[name] = [scrub_record(json.loads(line)) for line in handle if line.strip()]
    for name, records in files.items():
        base = records[0]["payload"].get("history_base") if records and records[0].get("type") == "session_meta" else None
        if base:
            parent = [n for n in files if base["thread_id"] in n and "_" not in n.rsplit("-", 5)[-1] and n != name]
            assert len(parent) == 1, parent
            prefix = files[parent[0]][: base["end_ordinal_exclusive"]]
            base["end_byte_offset"] = len(dump(prefix))
    for name, records in files.items():
        data = dump(records)
        hit = FORBIDDEN.search(data.decode("utf-8"))
        assert hit is None, (name, hit.group(0))
        with open(os.path.join(dst, name), "wb") as handle:
            handle.write(data)
        print(name, len(records), len(data))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
