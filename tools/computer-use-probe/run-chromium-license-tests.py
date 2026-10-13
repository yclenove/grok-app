"""Explicit local license-tool suite; unittest discovery skips hyphen names.

Never load collected upstream source-data. Each trusted local test module has
an independently retained minimum count; an empty or skipped suite is failure.
"""
import json
from pathlib import Path
import runpy
import sys
from types import SimpleNamespace
import unittest


TEST_FILES = {
    "test-chromium-license-tree.py": 13,
    "test-chromium-deps-pins.py": 13,
    "test-chromium-external-licenses.py": 38,
    "test-chromium-license-input-gaps.py": 15,
    "test-chromium-license-policy.py": 20,
    "test-chromium-license-gap-classification.py": 15,
    "test-chromium-recursive-deps.py": 13,
    "test-chromium-gclient-sources.py": 5,
    "test-chromium-gclient-inheritance.py": 8,
    "test-chromium-public-builds.py": 10,
    "test-chromium-archive-builds.py": 15,
    "test-chromium-archive-logs.py": 13,
    "test-chromium-archive-inputs.py": 6,
    "test-chromium-snapshot-objects.py": 11,
    "test-chromium-archive-checkouts.py": 7,
    "test-chromium-checked-recursive-sources.py": 8,
    "test-chromium-checked-recursive-licenses.py": 6,
    "test-chromium-recipe-instance.py": 7,
    "test-chromium-recipe-package.py": 9,
    "test-chromium-recipe-provenance.py": 8,
    "test-chromium-builder-config.py": 6,
    "test-chromium-platform-coverage.py": 18,
    "test-chromium-generated-input-sources.py": 7,
    "test-bounded-tar-xz.py": 8,
    "test-bounded-tar-gzip.py": 6,
    "test-chromium-node-generated-licenses.py": 15,
    "test-chromium-node-generated-coverage.py": 9,
    "test-chromium-crashpad-lss-sources.py": 13,
    "test-chromium-crashpad-coverage.py": 10,
    "test-chromium-integration-sources.py": 41,
    "test-chromium-perfetto-licenses.py": 8,
    "test-chromium-integration-coverage.py": 18,
    "test-chromium-consumer-coverage.py": 10,
    "test-khronos-notice-texts.py": 11,
    "test-chromium-notice-candidates.py": 8,
    "test-chromium-notice-supplements.py": 11,
    "test-llvm-notice-texts.py": 7,
    "test-chromium-swiftshader-notices.py": 8,
    "test-chromium-swiftshader-copied.py": 8,
    "test-chromium-skia-coverage.py": 8,
    "test-chromium-dawn-coverage.py": 8,
    "test-chromium-notice-assembly.py": 24,
    "test-chromium-condition-context.py": 20,
    "test-chromium-dawn-support.py": 8,
    "test-chromium-angle-generation.py": 12,
    "test-chromium-angle-registry-sources.py": 7,
    "test-chromium-angle-registry-coverage.py": 9,
    "test-chromium-angle-library-notices.py": 7,
}


def main():
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    counts = {}
    for name, minimum in TEST_FILES.items():
        module = SimpleNamespace(**runpy.run_path(str(Path(__file__).with_name(name))))
        tests = loader.loadTestsFromModule(module)
        count = tests.countTestCases()
        if count < minimum:
            raise ValueError("license module test count dropped: " + name)
        counts[name] = count
        suite.addTests(tests)
    if loader.errors:
        raise ValueError("local license test loading errors")
    print(json.dumps({"scope": "local source-inspection tools only, not product/runtime acceptance",
                      "testModules": counts, "tests": suite.countTestCases()}), flush=True)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
