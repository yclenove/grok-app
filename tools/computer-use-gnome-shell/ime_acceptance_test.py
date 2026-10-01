import copy
import unittest

from ime_acceptance import CASES, COUNTS, evaluate


def rows():
    state = {"owner": ":1.31", "policy": [1, "f0194527-4330-4029-91e1-7911127739ab", 5, False],
             "pid": 2000, "counts": dict.fromkeys(COUNTS, 0), "nativeWayland": True,
             "windowActive": True, "entryFocused": True, "imeModule": "wayland",
             "engine": "libpinyin", "preeditActive": False, "expectedCommit": False}
    result = []
    for index, name in enumerate(CASES):
        before = copy.deepcopy(state)
        state["policy"][2] += 1
        if name.endswith("-up"):
            state["counts"]["key_release"] += 1
        else:
            state["counts"]["preedit"] += 1
            state["preeditActive"] = index < 4
        if index == 4:
            state["counts"]["commit"] = 1
            state["expectedCommit"] = True
        result.append({"name": name, "before": before, "after": copy.deepcopy(state)})
    return result


class ImeAcceptanceTests(unittest.TestCase):
    def test_full_composition_requires_helper_and_does_not_certify_whole_scope(self):
        result = evaluate(rows())
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["cases"]), 6)
        for key in ("everyKeyEdgeVerified", "humanPhysicalInputVerified",
                    "appGrantRevocationVerified", "fullInputCoverageVerified"):
            self.assertIs(result[key], False)

    def test_composition_and_commit_with_no_helper_activity_remain_red(self):
        data = rows()
        for row in data:
            row["before"]["policy"][2] = row["after"]["policy"][2] = 5
        result = evaluate(data)
        self.assertFalse(result["passed"])
        self.assertTrue(all(c["imeDelivered"] and not c["helperAdvanced"] for c in result["cases"]))

    def test_helper_activity_without_composition_is_not_pass(self):
        data = rows()
        for row in data:
            for point in (row["before"], row["after"]):
                point["counts"] = dict.fromkeys(COUNTS, 0)
                point["preeditActive"] = point["expectedCommit"] = False
        self.assertFalse(evaluate(data)["passed"])

    def test_client_ibus_or_simple_context_does_not_certify_compositor_consumption(self):
        for key, value in (("imeModule", "ibus"), ("imeModule", "simple"), ("engine", "xkb:us::eng")):
            data = rows()
            data[0]["before"][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                evaluate(data)

    def test_raw_key_delivery_is_not_early_compositor_consumption(self):
        data = rows()
        data[-1]["after"]["counts"]["key_press"] = 1
        result = evaluate(data)
        self.assertFalse(result["passed"])
        self.assertEqual(result["cases"][-1]["rawClientPresses"], 1)

    def test_missing_reordered_or_duplicate_stages_reject(self):
        data = rows()
        for bad in ([], data[:-1], data[::-1], [data[0]] * 6):
            with self.assertRaises(ValueError):
                evaluate(bad)

    def test_focus_lock_backend_and_identity_loss_reject(self):
        for key, value in (("owner", ":1.99"), ("pid", 3000), ("pid", True),
                           ("nativeWayland", False), ("windowActive", False), ("entryFocused", False)):
            data = rows()
            data[-1]["after"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                evaluate(data)
        for index, value in ((0, True), (1, "a" * 36), (2, 0), (3, True)):
            data = rows()
            data[-1]["after"]["policy"][index] = value
            with self.subTest(index=index), self.assertRaises(ValueError):
                evaluate(data)

    def test_extra_content_and_malformed_flags_or_counters_reject(self):
        for mutate in (lambda s: s.update(text="never retain this"),
                       lambda s: s.update(expectedCommit=1),
                       lambda s: s["counts"].update(preedit=True),
                       lambda s: s["counts"].update(commit=-1),
                       lambda s: s["counts"].update(keyCode=1)):
            data = rows()
            mutate(data[-1]["after"])
            with self.assertRaises(ValueError):
                evaluate(data)

    def test_unaccounted_input_between_stages_rejects(self):
        data = rows()
        data[1]["before"]["policy"][2] += 1
        with self.assertRaises(ValueError):
            evaluate(data)

    def test_non_pristine_fixture_rejects(self):
        data = rows()
        data[0]["before"]["counts"]["preedit"] = 1
        with self.assertRaises(ValueError):
            evaluate(data)

    def test_incorrect_commit_or_still_active_preedit_is_not_success(self):
        for key, value in (("expectedCommit", False), ("preeditActive", True)):
            data = rows()
            data[-1]["after"][key] = value
            self.assertFalse(evaluate(data)["passed"])
        data = rows()
        data[-1]["after"]["counts"]["commit"] = 2
        self.assertFalse(evaluate(data)["passed"])

    def test_release_without_forwarded_client_event_is_not_success(self):
        data = rows()
        data[-1]["after"]["counts"]["key_release"] -= 1
        self.assertFalse(evaluate(data)["passed"])

    def test_unconsumed_key_down_does_not_prove_early_ime_observation(self):
        data = rows()
        data[-2]["after"]["counts"]["key_press"] = 1
        data[-1]["before"]["counts"]["key_press"] = 1
        data[-1]["after"]["counts"]["key_press"] = 1
        result = evaluate(data)
        self.assertFalse(result["passed"])
        self.assertFalse(result["cases"][-2]["imeDelivered"])


if __name__ == "__main__":
    unittest.main()
