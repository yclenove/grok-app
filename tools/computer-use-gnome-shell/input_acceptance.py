"""Strict, content-free acceptance of input routed to a native Wayland client.

This is a test oracle, not an input monitor. A passing hardware-VM run does not
prove human takeover, IME/touch coverage, EI exclusion, or App grant revocation.
"""
import re

CASES = (
    ("pointer-move-1", "motion"),
    ("pointer-move-2", "motion"),
    ("button-down", "button_press"),
    ("button-up", "button_release"),
    ("scroll", "scroll"),
    ("key-down", "key_press"),
    ("key-up", "key_release"),
)
COUNTS = frozenset(kind for _, kind in CASES)


def _counter(value):
    return type(value) is int and 0 <= value <= 0xffffffff


def _snapshot(value):
    if not isinstance(value, dict) or set(value) != {
        "owner", "policy", "counts", "nativeWayland", "windowActive", "pid"
    }:
        raise ValueError("unexpected or content-bearing snapshot fields")
    policy = value["policy"]
    if (not isinstance(policy, list) or len(policy) != 4 or type(policy[0]) is not int or policy[0] != 1
            or not isinstance(policy[1], str)
            or not re.fullmatch(r"[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}", policy[1])
            or not _counter(policy[2]) or policy[3] is not False):
        raise ValueError("invalid, blocked, or failed helper policy")
    if (not isinstance(value["owner"], str) or len(value["owner"]) > 255
            or not re.fullmatch(r":[0-9]+\.[0-9]+", value["owner"])
            or value["nativeWayland"] is not True or value["windowActive"] is not True
            or type(value["pid"]) is not int or value["pid"] <= 1):
        raise ValueError("native foreground client or Shell identity unproven")
    counts = value["counts"]
    if (not isinstance(counts, dict) or set(counts) != COUNTS
            or not all(_counter(count) for count in counts.values())):
        raise ValueError("invalid event counters")


def evaluate(rows):
    if not isinstance(rows, list) or len(rows) != len(CASES):
        raise ValueError("all seven ordered cases are required; no filtered pass")
    results = []
    previous = None
    for row, (name, kind) in zip(rows, CASES):
        if not isinstance(row, dict) or set(row) != {"name", "before", "after"} or row["name"] != name:
            raise ValueError("missing, reordered, or duplicate case")
        before, after = row["before"], row["after"]
        _snapshot(before)
        _snapshot(after)
        for key in ("owner", "pid"):
            if before[key] != after[key] or (previous and previous[key] != before[key]):
                raise ValueError("owner changed during input delivery")
        if before["policy"][1] != after["policy"][1] or (
                previous and previous["policy"][1] != before["policy"][1]):
            raise ValueError("helper restarted; generation comparison is invalid")
        if after["policy"][2] < before["policy"][2] or (
                previous and previous["policy"][2] > before["policy"][2]):
            raise ValueError("helper generation moved backwards")
        if any(after["counts"][k] < before["counts"][k] or (
                previous and previous["counts"][k] > before["counts"][k]) for k in COUNTS):
            raise ValueError("client event counters moved backwards")
        delivered = after["counts"][kind] > before["counts"][kind]
        observed = after["policy"][2] > before["policy"][2]
        results.append({"name": name, "clientReceived": delivered,
                        "helperAdvanced": observed, "passed": delivered and observed})
        previous = after
    return {"passed": all(row["passed"] for row in results), "cases": results,
            "humanPhysicalInputVerified": False, "appGrantRevocationVerified": False,
            "fullInputCoverageVerified": False}
