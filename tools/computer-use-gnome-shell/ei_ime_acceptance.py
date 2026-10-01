"""Additional exclusion gate; never replaces the physical six-edge IME gate.

The real transport owner must separately prove native EIS and joined retirement.
These content-free rows alone cannot certify provenance, consent or all inputs.
"""
from ime_acceptance import evaluate as evaluate_physical


def evaluate(rows):
    physical = evaluate_physical(rows)  # Keep exactly the same delivery/identity checks.
    cases = [{**case, "passed": case["imeDelivered"] and not case["helperAdvanced"]}
             for case in physical["cases"]]
    return {"passed": all(case["passed"] for case in cases), "cases": cases,
            "nativeEisTransportVerified": False, "physicalImeObservationVerified": False,
            "appGrantRevocationVerified": False, "fullInputCoverageVerified": False}


def evaluate_pair(physical_rows, ei_rows):
    """Both source contracts must hold; neither arm may stand in for the other.

    The caller still owns binary/source hash, actual transport and retirement
    evidence. Equal snapshots or a passing mock cannot establish that evidence.
    Separate fixtures/epochs are expected, but the compositor must stay the same.
    """
    physical = evaluate_physical(physical_rows)
    virtual = evaluate(ei_rows)
    if physical_rows[0]["before"]["owner"] != ei_rows[0]["before"]["owner"]:
        raise ValueError("paired IME sources require the same Shell owner")
    return {"passed": physical["passed"] and virtual["passed"],
            "physical": physical, "nativeEi": virtual,
            "nativeEisTransportVerified": False, "humanPhysicalInputVerified": False,
            "appGrantRevocationVerified": False, "fullInputCoverageVerified": False}
