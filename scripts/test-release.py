#!/usr/bin/env python3
"""Exercise each product's release packaging with isolated native fixtures."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from changelog import entries, validate, product_version, changelog_paths

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/package-release.py'
TARGETS = ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
           'aarch64-apple-darwin', 'x86_64-pc-windows-msvc')


class Packaging(unittest.TestCase):
    def setUp(self):
        temporary = ROOT / '.local' / 'packaging-checks'
        temporary.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=temporary)
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.source = self.directory / 'native'
        self.source.mkdir()
        for product in ('cli', 'gateway'):
            prefix = 'wakeit' if product == 'cli' else 'wakeit-gateway'
            for target in TARGETS:
                name = f'{prefix}-{target}' + ('.tar.gz' if product == 'cli' else ('.exe' if 'windows' in target else ''))
                (self.source / name).write_bytes(f'fixture: {product} {target}'.encode())
                if product == 'cli':
                    (self.source / f'wakeit-agents-{target}.tar.gz').write_bytes(b'plugin fixture')

    def assemble(self, product, *args):
        return subprocess.run([sys.executable, str(SCRIPT), 'assemble', '--product', product,
                               '--input', str(self.source), '--output', str(self.directory / product),
                               *args], capture_output=True, text=True)

    def test_product_artifacts_versions_and_notes_are_independent(self):
        for product in ('cli', 'gateway'):
            with self.subTest(product=product):
                version = product_version(product)
                result = self.assemble(product, '--tag', f'{product}-v{version}')
                self.assertEqual(result.returncode, 0, result.stderr)
                output = self.directory / product
                manifest = json.loads((output / f'wakeit-{product}-release.json').read_text())
                self.assertEqual((manifest['product'], manifest['version']), (product, version))
                self.assertEqual(set(manifest['assets']), set(TARGETS))
                prefix = 'wakeit' if product == 'cli' else 'wakeit-gateway'
                for target, entry in manifest['assets'].items():
                    self.assertEqual(entry['name'], f'{prefix}-{target}' + ('.tar.gz' if product == 'cli' else ('.exe' if 'windows' in target else '')))
                    self.assertEqual(entry['sha256'], hashlib.sha256((output / entry['name']).read_bytes()).hexdigest())
                installer = 'install.sh' if product == 'cli' else 'deploy-gateway.sh'
                self.assertEqual((output / installer).read_bytes(), (ROOT / installer).read_bytes())
                english, _ = changelog_paths(product)
                self.assertEqual((output / 'release-notes.md').read_text(), entries(english.read_text())[version] + '\n')
                self.assertEqual((output / 'wakeit-skill.tar.gz').exists(), product == 'cli')
        with tarfile.open(self.directory / 'cli/wakeit-skill.tar.gz') as archive:
            names = archive.getnames()
            self.assertIn('wakeit/SKILL.md', names)
            self.assertIn('wakeit/references/connect.md', names)
            self.assertIn('wakeit/assets/events-adapter.py', names)
            self.assertTrue(all(name.startswith('wakeit') and '..' not in Path(name).parts for name in names))

    def test_sdk_bundle_keeps_manifest_and_packages_shared_program_once(self):
        manifest = json.loads((ROOT / 'wakeit-plugin.json').read_text(encoding='utf-8'))
        resources = manifest['wakeit']
        self.assertFalse(resources.get('agents'), 'Official package must not require Agent YAML')
        declared = set(resources.get('commands', {}).values())
        declared.update(entry['command'].removeprefix('./') for entry in resources['plugins'].values())
        for target in (TARGETS[0], TARGETS[-1]):
            with self.subTest(target=target):
                suffix = '.exe' if 'windows' in target else ''
                binary = self.source / ('wakeit' + suffix)
                binary.write_bytes(b'CLI fixture')
                for resource in declared:
                    (self.source / (Path(resource).name + suffix)).write_bytes(b'Runtime fixture')
                output = self.directory / target
                result = subprocess.run([
                    sys.executable, str(SCRIPT), 'binary', '--product', 'cli', '--target', target,
                    '--input', str(binary), '--output', str(output),
                ], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = {resource + suffix for resource in declared}
                with tarfile.open(output / f'wakeit-agents-{target}.tar.gz') as archive:
                    self.assertCountEqual(archive.getnames(), expected)
                    for member in expected:
                        self.assertEqual(archive.extractfile(member).read(), b'Runtime fixture')
                with tarfile.open(output / f'wakeit-{target}.tar.gz') as archive:
                    prefix = f'wakeit-plugins/{product_version("cli")}/'
                    self.assertCountEqual(archive.getnames(), [
                        'wakeit' + suffix, prefix + 'wakeit-plugin.json',
                        *(prefix + member for member in expected),
                    ])
                    self.assertEqual(archive.extractfile(prefix + 'wakeit-plugin.json').read(),
                                     (ROOT / 'wakeit-plugin.json').read_bytes())
                    for member in expected:
                        self.assertEqual(archive.extractfile(prefix + member).read(), b'Runtime fixture')

    def test_staging_sdk_entry_without_a_command_alias_requires_its_program(self):
        spec = importlib.util.spec_from_file_location('package_release', SCRIPT)
        package = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(package)
        manifest = json.loads((ROOT / 'wakeit-plugin.json').read_text(encoding='utf-8'))
        manifest['wakeit'].pop('commands', None)
        source = self.directory / 'sdk-only'
        source.mkdir()
        (source / 'wakeit-plugin.json').write_text(json.dumps(manifest))
        binary = self.source / 'wakeit'
        binary.write_bytes(b'CLI fixture')
        output = self.directory / 'staged-sdk'
        with patch.object(package, 'ROOT', source):
            with self.assertRaisesRegex(SystemExit, 'Build the native helper'):
                package.stage(binary, output)
            resources = {entry['command'].removeprefix('./')
                         for entry in manifest['wakeit']['plugins'].values()}
            for resource in resources:
                (self.source / Path(resource).name).write_bytes(b'SDK-only fixture')
            package.stage(binary, output)
        self.assertEqual(json.loads((output / 'wakeit-plugin.json').read_text(encoding='utf-8')), manifest)
        for resource in resources:
            self.assertEqual((output / resource).read_bytes(), b'SDK-only fixture')
        self.assertFalse(list(output.rglob('*.yaml')))

    def test_missing_and_duplicate_targets_fail(self):
        name = f'wakeit-{TARGETS[0]}.tar.gz'
        (self.source / name).unlink()
        self.assertNotEqual(self.assemble('cli').returncode, 0)
        (self.source / name).write_bytes(b'fixture')
        duplicate = self.source / 'duplicate'
        duplicate.mkdir()
        (duplicate / name).write_bytes(b'other fixture')
        self.assertNotEqual(self.assemble('cli').returncode, 0)

    def test_wrong_product_version_and_preview_tags_cannot_publish(self):
        for product in ('cli', 'gateway'):
            version = product_version(product)
            other = 'gateway' if product == 'cli' else 'cli'
            for tag in (f'v{version}', f'{other}-v{version}', f'{product}-v99.0.0', f'{product}-v{version}-rc.1'):
                with self.subTest(product=product, tag=tag):
                    self.assertNotEqual(self.assemble(product, '--tag', tag).returncode, 0)

    def test_changelog_locales_order_duplicates_and_release_extraction(self):
        english = '## 0.1.0\n\n- Current.\n\n## 0.0.9\n\n- Previous.\n'
        chinese = '## 0.1.0\n\n- 当前版本。\n\n## 0.0.9\n\n- 上个版本。\n'
        self.assertEqual(validate(english, chinese, '0.1.0')['0.1.0'], '- Current.')
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
