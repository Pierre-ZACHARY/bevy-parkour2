"""Regression checks for selecting missing shell inputs without any network access."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("nix_cache", Path(__file__).with_name("nix-cache.py"))
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class ShellInputs(unittest.TestCase):
    def select(self, result, destination=None):
        derivations = [{"shell": {"inputs": {"dep": {"outputs": ["out"]}}}},
                       {"dep": {"outputs": {"out": "/nix/store/example"}}}]
        with patch.object(cache, "run", return_value=subprocess.CompletedProcess([], 0, "shell")), \
             patch.object(cache, "shown_derivations", side_effect=derivations), \
             patch.object(cache.subprocess, "run", return_value=result) as check:
            paths = cache.shell_inputs("shell", {}, destination)
            return paths, check.call_args.args[0]

    def test_invalid_is_expected_and_destination_is_respected(self):
        paths, command = self.select(subprocess.CompletedProcess([], 1, "/nix/store/example\n"), "/private/tmp/test-store")
        self.assertEqual(paths, ["/nix/store/example"])
        self.assertEqual(command[1:3], ["--store", "/private/tmp/test-store"])

    def test_valid_paths_are_skipped(self):
        paths, command = self.select(subprocess.CompletedProcess([], 0, ""))
        self.assertEqual(paths, [])
        self.assertNotIn("--store", command)

    def test_store_error_is_not_an_empty_success(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.select(subprocess.CompletedProcess([], 1, "", "store error"))


if __name__ == "__main__":
    unittest.main()
