"""Strict native pointer/EI interleave oracle, never a product grant source.

One virtual pointer stays connected across physical input. The final EI source
is disconnected while holding right; delivery must recover without a stuck grab.
Transport identity and original process/session retirement are checked separately.
"""
from input_acceptance import COUNTS, _snapshot

# name, delivered category (None means seat-wide suppression), physical ingress
CASES = (
    ("ei-motion", "motion", False),
    ("physical-motion", "motion", True),
    ("ei-left-down", "button_press", False),
    ("physical-overlap-down", None, True),
    ("physical-drag", "motion", True),
    ("ei-drag", "motion", False),
    ("physical-overlap-up", None, True),
    ("ei-wheel", "scroll", False),
    ("physical-wheel", "scroll", True),
    ("ei-left-up", "button_release", False),
    ("ei-right-down", "button_press", False),
    ("ei-source-stop", "button_release", False),
    ("physical-motion-after-stop", "motion", True),
    ("physical-right-down", "button_press", True),
    ("physical-right-up", "button_release", True),
)


def evaluate(rows):
    if not isinstance(rows, list) or len(rows) != len(CASES):
        raise ValueError("all fifteen ordered pointer cases required")
    previous = None
    results = []
    for row, (name, kind, physical) in zip(rows, CASES):
        if not isinstance(row, dict) or set(row) != {"name", "before", "after"} or row["name"] != name:
            raise ValueError("missing, duplicate, or reordered pointer case")
        before, after = row["before"], row["after"]
        _snapshot(before)
        _snapshot(after)
        if previous is not None and before != previous:
            raise ValueError("unaccounted input or state between pointer cases")
        if (before["owner"] != after["owner"] or before["pid"] != after["pid"]
                or before["policy"][1] != after["policy"][1]):
            raise ValueError("original owner or helper epoch changed")
        if any(v["policy"][2] >= 0xffffffff for v in (before, after)):
            raise ValueError("saturated helper policy cannot establish continuity")
        delta = {k: after["counts"][k] - before["counts"][k] for k in COUNTS}
        generation = after["policy"][2] - before["policy"][2]
        if generation < 0 or any(n < 0 for n in delta.values()):
            raise ValueError("counter regressed")
        # libei discrete scroll may be delivered as smooth + legacy GDK events;
        # every delivered event must nevertheless be scroll, not arbitrary input.
        delivered = (not any(delta.values()) if kind is None else
                     (delta[kind] >= 1 if kind == "scroll" else delta[kind] == 1)
                     and all(delta[k] == 0 for k in COUNTS - {kind}))
        observed = generation > 0 if physical else generation == 0
        results.append({"name": name, "clientDeltas": delta,
                        "helperGenerationDelta": generation,
                        "deliveryOrSuppressionVerified": delivered,
                        "passed": delivered and observed})
        previous = after
    return {"passed": all(row["passed"] for row in results), "cases": results,
            "nativeTransportProvenanceVerified": False,
            "originalSourceRetirementVerified": False,
            "humanPhysicalInputVerified": False, "appGrantRevocationVerified": False,
            "fullInputCoverageVerified": False}
