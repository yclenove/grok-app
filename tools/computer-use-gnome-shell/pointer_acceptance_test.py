import copy
import unittest

from input_acceptance import COUNTS
from pointer_acceptance import CASES, evaluate


def rows():
    state = {"owner": ":1.25", "pid": 1234, "nativeWayland": True, "windowActive": True,
             "counts": dict.fromkeys(COUNTS, 0),
             "policy": [1, "1e909bcd-a685-42e0-9931-6819e2cf4bb8", 1, False]}
    result = []
    for name, kind, physical in CASES:
        before = copy.deepcopy(state)
        if kind:
            state["counts"][kind] += 1
        state["policy"][2] += int(physical)
        result.append({"name": name, "before": before, "after": copy.deepcopy(state)})
    return result


class PointerAcceptanceTests(unittest.TestCase):
    def test_complete_interleave_and_teardown(self):
        output = evaluate(rows())
        self.assertTrue(output["passed"])
        self.assertEqual(len(output["cases"]), 15)
        for key, value in output.items():
            if key.endswith("Verified"):
                self.assertIs(value, False)

    def test_every_missing_case_rejected(self):
        for i in range(len(CASES)):
            values = rows()
            del values[i]
            with self.subTest(i=i), self.assertRaises(ValueError):
                evaluate(values)

    def test_reordering_rejected(self):
        values = rows()
        values[0], values[1] = values[1], values[0]
        with self.assertRaises(ValueError):
            evaluate(values)

    def test_unaccounted_generation_rejected(self):
        values = rows()
        values[1]["before"]["policy"][2] += 1
        with self.assertRaises(ValueError):
            evaluate(values)

    def test_unaccounted_client_input_rejected(self):
        values = rows()
        values[1]["before"]["counts"]["key_press"] += 1
        with self.assertRaises(ValueError):
            evaluate(values)

    def test_changed_identity_rejected(self):
        for key, value in (("owner", ":1.26"), ("pid", 5678)):
            values = rows()
            values[-1]["after"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                evaluate(values)

    def test_epoch_reset_rejected(self):
        values = rows()
        values[-1]["after"]["policy"][1] = "2e909bcd-a685-42e0-9931-6819e2cf4bb8"
        with self.assertRaises(ValueError):
            evaluate(values)

    def test_blocked_or_non_native_rejected(self):
        for key in ("windowActive", "nativeWayland", "blocked"):
            values = rows()
            if key == "blocked":
                values[-1]["after"]["policy"][3] = True
            else:
                values[-1]["after"][key] = False
            with self.subTest(key=key), self.assertRaises(ValueError):
                evaluate(values)

    def test_saturated_or_boolean_generation_rejected(self):
        for value in (0xffffffff, True):
            values = rows()
            values[-1]["after"]["policy"][2] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                evaluate(values)

    def test_missing_physical_event_is_red(self):
        values = rows()
        values[-1]["after"] = copy.deepcopy(values[-1]["before"])
        self.assertFalse(evaluate(values)["passed"])

    def test_foreign_event_is_red(self):
        values = rows()
        values[-1]["after"]["counts"]["key_release"] += 1
        self.assertFalse(evaluate(values)["passed"])

    def test_duplicate_delivery_is_red(self):
        values = rows()
        values[-1]["after"]["counts"]["button_release"] += 1
        self.assertFalse(evaluate(values)["passed"])

    def test_ei_and_suppression_failures_remain_red(self):
        for index in (0, 3, 6, 11):
            values = rows()
            # Preserve continuity after mutation; only the target edge is wrong.
            if index in (3, 6):
                for i in range(index, len(values)):
                    values[i]["after"]["counts"]["button_press"] += 1
                    if i > index:
                        values[i]["before"]["counts"]["button_press"] += 1
            else:
                for i in range(index, len(values)):
                    values[i]["after"]["policy"][2] += 1
                    if i > index:
                        values[i]["before"]["policy"][2] += 1
            with self.subTest(index=index):
                self.assertFalse(evaluate(values)["passed"])

    def test_missing_source_release_is_red(self):
        values = rows()
        for i in range(11, len(values)):
            values[i]["after"]["counts"]["button_release"] -= 1
            if i > 11:
                values[i]["before"]["counts"]["button_release"] -= 1
        self.assertFalse(evaluate(values)["passed"])

    def test_snapshot_content_injection_rejected(self):
        values = rows()
        values[0]["before"]["coordinates"] = [1, 2]
        with self.assertRaises(ValueError):
            evaluate(values)


if __name__ == "__main__":
    unittest.main()
