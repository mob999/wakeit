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
from changelog import entries, validate

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
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
        self.assertEqual(self.assemble('--tag', f'v{version}').returncode, 0)
        manifest = json.loads((self.output / 'wakeit-release.json').read_text(encoding='utf-8'))
        self.assertEqual(json.loads((ROOT / 'web/package.json').read_text(encoding='utf-8'))['version'], version)
        web_lock = json.loads((ROOT / 'web/package-lock.json').read_text(encoding='utf-8'))
        self.assertEqual(web_lock['version'], version)
        self.assertEqual(web_lock['packages']['']['version'], version)
        rust_lock = tomllib.loads((ROOT / 'Cargo.lock').read_text(encoding='utf-8'))
        self.assertEqual(next(p['version'] for p in rust_lock['package'] if p['name'] == 'wakeit'), version)
        self.assertEqual(manifest['version'], version)
        self.assertEqual(json.loads((ROOT / 'schemas/openapi.v1.json').read_text(encoding='utf-8'))['info']['version'], version)
        notes = entries((ROOT / 'CHANGELOG.md').read_text(encoding='utf-8'))
        self.assertEqual((self.output / 'release-notes.md').read_text(encoding='utf-8'), notes[version] + '\n')
        self.assertEqual(set(manifest['assets']), set(TARGETS))
        installer = self.output / 'install.sh'
        self.assertEqual(installer.read_bytes(), (ROOT / 'install.sh').read_bytes())
        self.assertIn(f"{hashlib.sha256(installer.read_bytes()).hexdigest()}  install.sh",
                      (self.output / 'SHA256SUMS').read_text())
        for entry in manifest['assets'].values():
            self.assertEqual(entry['sha256'], hashlib.sha256((self.output / entry['name']).read_bytes()).hexdigest())
        with tarfile.open(self.output / 'wakeit-skill.tar.gz') as archive:
            names = archive.getnames()
            self.assertIn('wakeit/SKILL.md', names)
            self.assertIn('wakeit/references/deploy-windows.md', names)
            self.assertIn('wakeit/assets/events-adapter.py', names)
            self.assertIn('wakeit/assets/task-dag.yaml', names)
            self.assertIn('wakeit/references/dag.md', names)
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
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
        self.assertNotEqual(self.assemble('--tag', 'v99.0.0').returncode, 0)
        self.assertNotEqual(self.assemble('--tag', f'v{version}-rc.1').returncode, 0)

    def test_changelog_locales_order_duplicates_and_release_extraction(self):
        english = '## 0.1.0\n\n- Current.\n\n## 0.0.9\n\n- Previous.\n'
        chinese = '## 0.1.0\n\n- 当前版本。\n\n## 0.0.9\n\n- 上个版本。\n'
        notes = validate(english, chinese, '0.1.0')
        self.assertEqual(notes['0.1.0'], '- Current.')
        for en, zh, current in [
            (english, '## 0.1.0\n- 当前版本。', '0.1.0'),
            (english, chinese, '0.2.0'),
            (english + '\n## 0.1.0\n- Duplicate.', chinese, '0.1.0'),
            ('## 0.0.9\n- Old.\n## 0.1.0\n- New.', chinese, '0.1.0'),
            ('## 0.1.0\n', chinese, '0.1.0'),
        ]:
            with self.assertRaises(ValueError):
                validate(en, zh, current)


if __name__ == '__main__':
    unittest.main()
