"""Offline regression tests: nix develop .#ci -c python3 -m unittest discover -s tools -p 'test_nix_cache.py'."""
import importlib.util
import json
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('nix_cache', __file__.replace('test_nix_cache.py', 'nix-cache.py'))
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)

SHELL = '/nix/store/' + 'a' * 32 + '-shell.drv'
DRV = '/nix/store/' + 'b' * 32 + '-tool.drv'
OUT = '/nix/store/' + 'c' * 32 + '-tool'
DEV = '/nix/store/' + 'd' * 32 + '-tool-dev'


class ShellInputsTests(unittest.TestCase):
    def check_format(self, modern, validity_status=0):
        def show(path, outputs, inputs):
            if modern:
                return {'version': 4, 'derivations': {path.removeprefix('/nix/store/'): {
                    'outputs': {k: {'path': v.removeprefix('/nix/store/')} for k, v in outputs.items()},
                    'inputs': {'drvs': {k.removeprefix('/nix/store/'): {'outputs': v, 'dynamicOutputs': {}}
                                        for k, v in inputs.items()}}}}}
            return {path: {'outputs': {k: {'path': v} for k, v in outputs.items()}, 'inputDrvs': inputs}}
        responses = [SHELL, json.dumps(show(SHELL, {}, {DRV: ['dev']})),
                     json.dumps(show(DRV, {'out': OUT, 'dev': DEV}, {})), DEV + '\n']
        with patch.object(cache, 'run', side_effect=[subprocess.CompletedProcess([], validity_status if i == 3 else 0, stdout=s) for i, s in enumerate(responses)]) as run:
            self.assertEqual(cache.shell_inputs('.#devShells.aarch64-darwin.ci', {}), [DEV])
            self.assertEqual(run.call_args.args[0], ['nix-store', '--check-validity', '--print-invalid', DEV])

    def test_nix_225_input_drvs_lists(self):
        self.check_format(False)

    def test_nix_235_versioned_derivations(self):
        self.check_format(True)

    def test_missing_input_exit_one_is_a_cache_candidate(self):
        self.check_format(True, validity_status=1)

    def validity(self, status, stdout, destination=None, stderr=''):
        """Last step only: the validity query against the selected store."""
        derivations = [{SHELL: {'inputs': {DRV: {'outputs': ['out']}}}},
                       {DRV: {'outputs': {'out': OUT}}}]
        with patch.object(cache, 'shown_derivations', side_effect=derivations), \
             patch.object(cache, 'run', side_effect=[subprocess.CompletedProcess([], 0, stdout=SHELL),
                                                     subprocess.CompletedProcess([], status, stdout, stderr)]) as run:
            return cache.shell_inputs('.#devShells.aarch64-darwin.default', {}, destination), run.call_args.args[0]

    def test_isolated_destination_store_is_queried(self):
        paths, command = self.validity(1, OUT + '\n', '/private/tmp/collab-store')
        self.assertEqual(paths, [OUT])
        self.assertEqual(command[1:3], ['--store', '/private/tmp/collab-store'])

    def test_valid_paths_are_skipped(self):
        paths, command = self.validity(0, '')
        self.assertEqual(paths, [])
        self.assertNotIn('--store', command)

    def test_store_error_is_not_an_empty_success(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.validity(1, '', stderr='store error')

    def test_store_failure_is_not_silently_ignored(self):
        with patch.object(cache, 'run', side_effect=[
            subprocess.CompletedProcess([], 0, stdout=SHELL),
            subprocess.CompletedProcess([], 0, stdout=json.dumps({SHELL: {'outputs': {}, 'inputDrvs': {DRV: ['out']}}})),
            subprocess.CompletedProcess([], 0, stdout=json.dumps({DRV: {'outputs': {'out': {'path': OUT}}, 'inputDrvs': {}}})),
            subprocess.CompletedProcess([], 1, stdout='', stderr='store error'),
        ]):
            with self.assertRaises(subprocess.CalledProcessError):
                cache.shell_inputs('.#shell', {})

    def test_empty_shell_inputs(self):
        with patch.object(cache, 'run', side_effect=[
            subprocess.CompletedProcess([], 0, stdout=SHELL),
            subprocess.CompletedProcess([], 0, stdout=json.dumps({SHELL: {'outputs': {}, 'inputDrvs': {}}}))
        ]) as run:
            self.assertEqual(cache.shell_inputs('.#shell', {}), [])
            self.assertEqual(run.call_count, 2)

    def test_already_valid_input_is_not_requested(self):
        with patch.object(cache, 'run', side_effect=[
            subprocess.CompletedProcess([], 0, stdout=SHELL),
            subprocess.CompletedProcess([], 0, stdout=json.dumps({SHELL: {'outputs': {}, 'inputDrvs': {DRV: ['out']}}})),
            subprocess.CompletedProcess([], 0, stdout=json.dumps({DRV: {'outputs': {'out': {'path': OUT}}, 'inputDrvs': {}}})),
            subprocess.CompletedProcess([], 0, stdout=''),
        ]):
            self.assertEqual(cache.shell_inputs('.#shell', {}), [])


if __name__ == '__main__':
    unittest.main()
