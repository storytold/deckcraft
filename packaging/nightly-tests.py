#!/usr/bin/env python3
"""Run the existing synthetic packaging suites together; no app build or signing."""
import importlib.util
from pathlib import Path
import subprocess
import unittest

HERE = Path(__file__).resolve().parent


class LinuxLauncherContractTests(unittest.TestCase):
    """The launchers' argv contract, proven by a recorder binary instead of a build."""

    def test_every_linux_launcher_passes_the_profile(self):
        for script in ("linux/nightly-launch-test.sh", "linux/nightly-arch-tar-test.sh"):
            result = subprocess.run(["bash", str(HERE / script)], capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, f"{script}\n{result.stdout}\n{result.stderr}")


def main() -> int:
    suite = unittest.TestSuite()
    for name, relative in (
        ("deck_plist_suite", "macos/nightly_plist_tests.py"),
        ("deck_release_dispatch_suite", "nightly-release-tests.py"),
        ("deck_mac_sign_suite", "macos/nightly_sign_tests.py"),
    ):
        spec = importlib.util.spec_from_file_location(name, HERE / relative)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
    suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(LinuxLauncherContractTests))
    return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
