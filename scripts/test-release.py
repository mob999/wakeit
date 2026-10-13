#!/usr/bin/env python3
"""Exercise packaging with fixtures, without creating a tag or release."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/package-release.py'
TARGETS = ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
           'x86_64-apple-darwin', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc')


class Packaging(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.directory = Path(self.temp.name)
        self.source = self.directory / 'native'
        self.source.mkdir()
        self.output = self.directory / 'release'
        for target in TARGETS:
            name = f'wakeit-{target}' + ('.exe' if 'windows' in target else '')
            folder = self.source / name
            folder.mkdir()
            (folder / name).write_bytes(f'fixture: {target}'.encode())

    def tearDown(self):
        self.temp.cleanup()

    def assemble(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), 'assemble', '--input', str(self.source),
                               '--output', str(self.output), *args], capture_output=True, text=True)

    def test_manifest_matches_all_artifacts_and_skill_is_independent(self):
        self.assertEqual(self.assemble().returncode, 0)
        manifest = json.loads((self.output / 'wakeit-release.json').read_text(encoding='utf-8'))
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
        self.assertEqual(manifest['version'], version)
        self.assertEqual(set(manifest['assets']), set(TARGETS))
        for entry in manifest['assets'].values():
            self.assertEqual(entry['sha256'], hashlib.sha256((self.output / entry['name']).read_bytes()).hexdigest())
        with tarfile.open(self.output / 'wakeit-skill.tar.gz') as archive:
            names = archive.getnames()
            self.assertIn('wakeit/SKILL.md', names)
            self.assertIn('wakeit/references/deploy-windows.md', names)
            self.assertIn('wakeit/assets/events-adapter.py', names)
            self.assertTrue(all(name.startswith('wakeit') and '..' not in Path(name).parts for name in names))

    def test_missing_and_duplicate_targets_fail(self):
        (self.source / f'wakeit-{TARGETS[0]}' / f'wakeit-{TARGETS[0]}').unlink()
        self.assertNotEqual(self.assemble().returncode, 0)
        (self.source / f'wakeit-{TARGETS[0]}' / f'wakeit-{TARGETS[0]}').write_bytes(b'fixture')
        duplicate = self.source / 'duplicate'
        duplicate.mkdir()
        (duplicate / f'wakeit-{TARGETS[0]}').write_bytes(b'other fixture')
        self.assertNotEqual(self.assemble().returncode, 0)

    def test_wrong_or_preview_tag_cannot_publish(self):
        self.assertNotEqual(self.assemble('--tag', 'v99.0.0').returncode, 0)
        self.assertNotEqual(self.assemble('--tag', 'v0.1.0-rc.1').returncode, 0)


if __name__ == '__main__':
    unittest.main()
