import copy
import unittest

from input_acceptance import CASES, COUNTS, evaluate


def passing_rows():
    state = {"owner": ":1.24", "policy": [1, "f0194527-4330-4029-91e1-7911127739ab", 1, False],
             "counts": dict.fromkeys(COUNTS, 0), "nativeWayland": True,
             "windowActive": True, "pid": 1234}
    rows = []
    for name, kind in CASES:
        before = copy.deepcopy(state)
        state["counts"][kind] += 1
        state["policy"][2] += 1
        rows.append({"name": name, "before": before, "after": copy.deepcopy(state)})
    return rows


class AcceptanceTests(unittest.TestCase):
    def test_matching_client_receipts_and_generations_only_prove_selected_cases(self):
        result = evaluate(passing_rows())
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["cases"]), 7)
        for key in ("humanPhysicalInputVerified", "appGrantRevocationVerified", "fullInputCoverageVerified"):
            self.assertFalse(result[key])

    def test_late_filter_client_delivery_without_generation_is_red(self):
        rows = passing_rows()
        for row in rows:
            row["before"]["policy"][2] = row["after"]["policy"][2] = 3
        result = evaluate(rows)
        self.assertFalse(result["passed"])
        self.assertTrue(all(r["clientReceived"] and not r["helperAdvanced"] for r in result["cases"]))

    def test_generation_without_actual_client_receipt_is_not_pass(self):
        rows = passing_rows()
        for row in rows:
            row["before"]["counts"] = dict.fromkeys(COUNTS, 0)
            row["after"]["counts"] = dict.fromkeys(COUNTS, 0)
        self.assertFalse(evaluate(rows)["passed"])

    def test_missing_reordered_or_duplicate_cases_reject(self):
        rows = passing_rows()
        for bad in ([], rows[:-1], rows[::-1], [rows[0]] * 7):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                evaluate(bad)

    def test_identity_focus_lock_and_backend_changes_reject(self):
        for key, value in [("owner", ":1.99"), ("pid", 4321),
                           ("nativeWayland", False), ("windowActive", False)]:
            rows = passing_rows()
            rows[2]["after"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                evaluate(rows)
        for index, value in ((1, "a" * 36), (2, 0), (3, True)):
            rows = passing_rows()
            rows[2]["after"]["policy"][index] = value
            with self.subTest(index=index), self.assertRaises(ValueError):
                evaluate(rows)

    def test_counter_rollback_boolean_and_content_fields_reject(self):
        for mutate in (lambda s: s["counts"].update(motion=-1),
                       lambda s: s["counts"].update(motion=True),
                       lambda s: s.update(keyText="must never be accepted")):
            rows = passing_rows()
            mutate(rows[3]["after"])
            with self.assertRaises(ValueError):
                evaluate(rows)

    def test_protocol_types_and_identity_format_reject(self):
        for mutate in (lambda s: s["policy"].__setitem__(0, True),
                       lambda s: s["policy"].__setitem__(1, "-" * 36),
                       lambda s: s.update(owner=":not-a-unique-owner")):
            rows = passing_rows()
            mutate(rows[0]["before"])
            with self.assertRaises(ValueError):
                evaluate(rows)

    def test_unrelated_event_kind_does_not_prove_keyboard_delivery(self):
        rows = passing_rows()
        rows[-1]["after"]["counts"]["key_release"] = 0
        rows[-1]["after"]["counts"]["motion"] += 1
        result = evaluate(rows)
        self.assertFalse(result["passed"])
        self.assertFalse(result["cases"][-1]["clientReceived"])


if __name__ == "__main__":
    unittest.main()
