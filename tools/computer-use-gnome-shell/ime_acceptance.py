"""Content-free native compositor IME acceptance, not a product input monitor.

Six owned edges prove a specific composition and commit, not all IME behavior.
Both delivered composition and generation advancement are mandatory. An early
IME-consumed key must not be mistaken for a client-side IM module success.
"""
import re

CASES = ("compose-start-down", "compose-start-up", "compose-continue-down",
         "compose-continue-up", "chinese-commit-down", "chinese-commit-up")
COUNTS = frozenset(("preedit", "commit", "key_press", "key_release"))
FIELDS = frozenset(("owner", "policy", "counts", "nativeWayland", "windowActive", "pid",
                    "entryFocused", "imeModule", "engine", "preeditActive", "expectedCommit"))


def _counter(value):
    return type(value) is int and 0 <= value <= 0xffffffff


def _snapshot(value):
    if not isinstance(value, dict) or set(value) != FIELDS:
        raise ValueError("unexpected or content-bearing snapshot fields")
    policy = value["policy"]
    if (not isinstance(policy, list) or len(policy) != 4 or type(policy[0]) is not int or policy[0] != 1
            or not isinstance(policy[1], str)
            or not re.fullmatch(r"[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}", policy[1])
            or not _counter(policy[2]) or policy[3] is not False):
        raise ValueError("invalid, blocked, or failed helper policy")
    if (not isinstance(value["owner"], str) or len(value["owner"]) > 255
            or not re.fullmatch(r":[0-9]+\.[0-9]+", value["owner"])
            or type(value["pid"]) is not int or value["pid"] <= 1
            or any(value[k] is not True for k in ("nativeWayland", "windowActive", "entryFocused"))
            or value["imeModule"] != "wayland" or value["engine"] != "libpinyin"
            or any(type(value[k]) is not bool for k in ("preeditActive", "expectedCommit"))):
        raise ValueError("native focused IME or process identity unproven")
    if (not isinstance(value["counts"], dict) or set(value["counts"]) != COUNTS
            or not all(_counter(n) for n in value["counts"].values())):
        raise ValueError("invalid IME counters")


def evaluate(rows):
    if not isinstance(rows, list) or len(rows) != len(CASES):
        raise ValueError("all six ordered IME edges required; no filtered pass")
    previous = None
    results = []
    for row, name in zip(rows, CASES):
        if not isinstance(row, dict) or set(row) != {"name", "before", "after"} or row["name"] != name:
            raise ValueError("missing, duplicate, or reordered IME stage")
        before, after = row["before"], row["after"]
        _snapshot(before)
        _snapshot(after)
        pairs = [(before, after)]
        if previous is not None:
            pairs.append((previous, before))
        for first, last in pairs:
            if any(first[k] != last[k] for k in ("owner", "pid")) or first["policy"][1] != last["policy"][1]:
                raise ValueError("Shell, helper epoch, or fixture changed")
            if first["policy"][2] > last["policy"][2] or any(
                    first["counts"][k] > last["counts"][k] for k in COUNTS):
                raise ValueError("IME or policy counter rolled backwards")
        if previous is not None and before != previous:
            raise ValueError("unaccounted input or state change between stages")
        if name == CASES[0] and (any(before["counts"].values()) or before["preeditActive"] or before["expectedCommit"]):
            raise ValueError("fixture was not pristine before composition")
        raw_presses = after["counts"]["key_press"] - before["counts"]["key_press"]
        raw_releases = after["counts"]["key_release"] - before["counts"]["key_release"]
        if name.endswith("-up"):
            delivered = (raw_releases == 1 and raw_presses == 0
                         and all(before[k] == after[k] for k in ("preeditActive", "expectedCommit"))
                         and all(before["counts"][k] == after["counts"][k] for k in ("preedit", "commit")))
        elif name != "chinese-commit-down":
            delivered = (after["counts"]["preedit"] > before["counts"]["preedit"]
                         and after["preeditActive"] and not after["expectedCommit"]
                         and raw_presses == raw_releases == 0
                         and after["counts"]["commit"] == before["counts"]["commit"] == 0)
        else:
            delivered = (before["preeditActive"] and not before["expectedCommit"]
                         and not after["preeditActive"] and after["expectedCommit"]
                         and raw_presses == raw_releases == 0
                         and after["counts"]["commit"] - before["counts"]["commit"] == 1)
        observed = after["policy"][2] > before["policy"][2]
        results.append({"name": name, "imeDelivered": delivered,
                        "rawClientPresses": raw_presses, "rawClientReleases": raw_releases,
                        "helperAdvanced": observed, "passed": delivered and observed})
        previous = after
    return {"passed": all(case["passed"] for case in results), "cases": results,
            "everyKeyEdgeVerified": False, "humanPhysicalInputVerified": False,
            "appGrantRevocationVerified": False, "fullInputCoverageVerified": False}
