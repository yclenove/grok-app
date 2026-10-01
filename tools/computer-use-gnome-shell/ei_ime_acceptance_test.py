import copy
import unittest

from ei_ime_acceptance import evaluate, evaluate_pair
from ime_acceptance import evaluate as evaluate_physical
from ime_acceptance_test import rows as physical_rows


def rows():
    data = physical_rows()
    for row in data:
        for state in (row["before"], row["after"]):
            state["policy"][2] = 5
    return data


class EiImeAcceptanceTests(unittest.TestCase):
    def test_delivered_virtual_edges_require_no_generation_change(self):
        result = evaluate(rows())
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["cases"]), 6)
        for key in ("nativeEisTransportVerified", "physicalImeObservationVerified",
                    "appGrantRevocationVerified", "fullInputCoverageVerified"):
            self.assertIs(result[key], False)
        self.assertFalse(evaluate_physical(rows())["passed"])

    def test_physical_success_cannot_be_reused_as_virtual_success(self):
        self.assertFalse(evaluate(physical_rows())["passed"])

    def test_each_single_false_positive_fails_the_whole_gate(self):
        for index in range(6):
            data = rows()
            for i, row in enumerate(data):
                if i >= index:
                    row["after"]["policy"][2] += 1
                if i > index:
                    row["before"]["policy"][2] += 1
            with self.subTest(index=index):
                result = evaluate(data)
                self.assertFalse(result["passed"])
                self.assertEqual(sum(c["passed"] for c in result["cases"]), 5)

    def test_no_activity_is_not_proof_of_delivered_virtual_input(self):
        data = rows()
        for row in data:
            for state in (row["before"], row["after"]):
                state["counts"] = dict.fromkeys(state["counts"], 0)
                state["expectedCommit"] = state["preeditActive"] = False
        self.assertFalse(evaluate(data)["passed"])

    def test_missing_reordered_or_duplicate_cases_reject(self):
        data = rows()
        for value in ([], data[:-1], data[::-1], [data[0]] * 6):
            with self.assertRaises(ValueError):
                evaluate(value)

    def test_identity_lock_focus_content_and_counter_faults_reject(self):
        for mutate in (lambda s: s.update(pid=1), lambda s: s.update(owner=":1.99"),
                       lambda s: s.update(entryFocused=False), lambda s: s.update(text="not exported"),
                       lambda s: s["policy"].__setitem__(3, True),
                       lambda s: s["counts"].__setitem__("commit", True)):
            data = copy.deepcopy(rows())
            mutate(data[-1]["after"])
            with self.assertRaises(ValueError):
                evaluate(data)

    def test_between_edge_generation_change_is_not_ignored(self):
        data = rows()
        data[1]["before"]["policy"][2] += 1
        with self.assertRaises(ValueError):
            evaluate(data)

    def test_paired_gate_requires_both_opposite_source_contracts(self):
        result = evaluate_pair(physical_rows(), rows())
        self.assertTrue(result["passed"])
        for key in ("nativeEisTransportVerified", "humanPhysicalInputVerified",
                    "appGrantRevocationVerified", "fullInputCoverageVerified"):
            self.assertIs(result[key], False)
        self.assertFalse(evaluate_pair(rows(), rows())["passed"])
        self.assertFalse(evaluate_pair(physical_rows(), physical_rows())["passed"])
        self.assertFalse(evaluate_pair(rows(), physical_rows())["passed"])

    def test_paired_gate_rejects_each_missing_arm_or_edge(self):
        for physical, virtual in (([], rows()), (physical_rows(), []),
                                  (physical_rows()[:-1], rows()),
                                  (physical_rows(), rows()[:-1])):
            with self.assertRaises(ValueError):
                evaluate_pair(physical, virtual)

    def test_paired_gate_rejects_compositor_restart(self):
        virtual = rows()
        for row in virtual:
            for state in (row["before"], row["after"]):
                state["owner"] = ":1.999"
        with self.assertRaises(ValueError):
            evaluate_pair(physical_rows(), virtual)

    def test_actual_symmetric_ime_failure_cannot_be_accepted_as_pair(self):
        data = physical_rows()
        generation = data[0]["before"]["policy"][2]
        for index, row in enumerate(data):
            row["before"]["policy"][2] = generation
            generation += index % 2
            row["after"]["policy"][2] = generation
        result = evaluate_pair(data, copy.deepcopy(data))
        self.assertFalse(result["passed"])
        self.assertEqual(sum(c["passed"] for c in result["physical"]["cases"]), 3)
        self.assertEqual(sum(c["passed"] for c in result["nativeEi"]["cases"]), 3)
        self.assertFalse(result["physical"]["passed"])
        self.assertFalse(result["nativeEi"]["passed"])


if __name__ == "__main__":
    unittest.main()
