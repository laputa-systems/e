#!/usr/bin/env python3
"""Cheap selection and preflight checks; never run a benchmark."""
import importlib.util
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('bench_driver', ROOT / 'scripts/bench.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class DriverTest(unittest.TestCase):
    def invoke(self, *args, env=None):
        return subprocess.run([sys.executable, str(ROOT / 'scripts/bench.py'), *args], cwd=ROOT,
                              text=True, capture_output=True, env=env)

    def test_registry_matches_benchmark_functions(self):
        source = (ROOT / 'benches/bench.rs').read_text()
        actual = set(re.findall(r'#\[rustybench::bench\]\s*fn (\w+)', source))
        registered = {name for name, tier, _ in driver.WORK if tier in ('Change', 'Scenario')}
        self.assertEqual(actual, registered)

    def test_selection_and_exclusions(self):
        result = self.invoke('run', '--tier', 'Change', '--area', 'render', '--skip', 'render_full_frame_plain', '--dry-run')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('excluded render_full_frame_plain: skipped', result.stdout)
        self.assertIn('bench::render_full_frame_rust_syntax --exact', result.stdout)
        self.assertNotIn('cargo bench --locked --bench bench -- bench::open_and_render_python_10mb', result.stdout)
        for args in (('--name', 'bad'), ('--area', 'bad'), ('--skip', 'bad'),
                     ('--tier', 'Edit', '--area', 'render'), ('--tier', 'Change', '--name', 'render')):
            self.assertNotEqual(self.invoke('run', *args, '--dry-run').returncode, 0)

    def test_preflight_failure_and_dry_run_make_no_run_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            env = dict(os.environ, PATH=temp)
            with mock.patch.object(driver, 'ROOT', Path(temp)):
                selected = [('file_read_real_rust_source_160kb', 'Change', 'io')]
                with mock.patch.dict(os.environ, env, clear=True):
                    self.assertTrue(driver.preflight(selected))
                self.assertFalse((Path(temp) / '.work').exists())
            result = self.invoke('run', '--tier', 'Change', '--name', 'file_read_real_rust_source_160kb', env=env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('cargo missing', result.stderr)
            self.assertNotIn('summary:', result.stdout)
        before = set((ROOT / '.work/bench').glob('*')) if (ROOT / '.work/bench').exists() else set()
        result = self.invoke('run', '--dry-run')
        self.assertEqual(result.returncode, 0, result.stderr)
        after = set((ROOT / '.work/bench').glob('*')) if (ROOT / '.work/bench').exists() else set()
        self.assertEqual(before, after)

    def test_failed_workload_propagates(self):
        with tempfile.TemporaryDirectory() as temp, mock.patch.object(driver, 'ROOT', Path(temp)), \
             mock.patch.object(driver, 'preflight', return_value=[]), \
             mock.patch.object(driver, 'command', return_value=[sys.executable, '-c', 'import sys; sys.exit(7)']), \
             mock.patch.object(sys, 'argv', ['bench.py', 'run']):
            self.assertEqual(driver.main(), 1)
            summary = next((Path(temp) / '.work/bench').glob('*/summary.json'))
            self.assertIn('"exit": 7', summary.read_text())


if __name__ == '__main__':
    unittest.main()
