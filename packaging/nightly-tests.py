#!/usr/bin/env python3
"""Run the existing synthetic packaging suites together; no app build or signing."""
import importlib.util
from pathlib import Path
import unittest


def main() -> int:
    here = Path(__file__).resolve().parent
    suite = unittest.TestSuite()
    for name, relative in (
        ("deck_plist_suite", "macos/nightly_plist_tests.py"),
        ("deck_release_dispatch_suite", "nightly-release-tests.py"),
        ("deck_mac_sign_suite", "macos/nightly_sign_tests.py"),
    ):
        spec = importlib.util.spec_from_file_location(name, here / relative)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
    return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
