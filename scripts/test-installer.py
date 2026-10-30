#!/usr/bin/env python3
"""Run the real POSIX installer against isolated release/command fixtures."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Installer(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='wakeit-installer-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "user's home"
        self.home.mkdir()
        self.bin = self.home / '.local/bin'
        self.fixture = self.root / 'release'
        self.fixture.mkdir()
        self.commands = self.root / 'commands'
        self.commands.mkdir()
        self.env = {**os.environ, 'HOME': str(self.home), 'SHELL': '/bin/zsh',
                    'ZDOTDIR': str(self.home), 'XDG_CONFIG_HOME': str(self.home / '.config'),
                    'TMPDIR': str(self.root), 'FIXTURE_RELEASE': str(self.fixture),
                    'FIXTURE_LOG': str(self.root / 'requests'),
                    'PATH': str(self.commands) + os.pathsep + os.environ['PATH']}
        self.command('uname', '#!/bin/sh\ncase "$1" in -m) echo "${FIXTURE_ARCH:-arm64}";; -s) echo "${FIXTURE_OS:-Darwin}";; esac\n')
        self.command('curl', '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args=sys.argv[1:]
url=args[-1]
with open(os.environ['FIXTURE_LOG'],'a') as log: log.write(url+'\\n')
if os.environ.get('FAIL_DOWNLOAD') == url.rsplit('/',1)[-1]: sys.exit(22)
out=pathlib.Path(args[args.index('--output')+1])
shutil.copyfile(pathlib.Path(os.environ['FIXTURE_RELEASE'])/url.rsplit('/',1)[-1],out)
if os.environ.get('CORRUPT_BINARY') and out.name.startswith('wakeit-') and out.name != 'wakeit-skill.tar.gz': out.write_bytes(b'corrupt')
''')
        assets = {}
        lines = []
        for target in ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
                       'aarch64-apple-darwin'):
            name = f'wakeit-{target}.tar.gz'
            binary = self.fixture / name
            with tarfile.open(binary, 'w:gz') as archive:
                program = b'#!/usr/bin/env python3\nimport json,pathlib,sys\nif sys.argv[1:]==[\'--version\']:\n print(\'wakeit 0.2.0\')\nelif sys.argv[1:4]==[\'--json\',\'plugin\',\'validate\']:\n root=pathlib.Path(sys.argv[sys.argv.index(\'--dir\')+1])\n manifest=json.loads((root/\'wakeit-plugin.json\').read_text())[\'wakeit\']\n if not all((root/p).is_file() for p in [*manifest[\'agents\'].values(),*manifest[\'commands\'].values()]):\n  print(\'missing declared resource\',file=sys.stderr);sys.exit(1)\n print(\'{"valid":true}\')\nelse:\n sys.exit(2)\n'
                manifest = {'manifest_version':1,'name':'fixture','version':'0.2.0','wakeit':{'protocol':1,'agents':{'fixture':'agent.json'},'commands':{'adapter':'bin/fixture-runtime'}}}
                files = {
                    'wakeit': program,
                    'wakeit-plugins/0.2.0/wakeit-plugin.json': json.dumps(manifest).encode(),
                    'wakeit-plugins/0.2.0/agent.json': b'{"version":1,"name":"Fixture","mode":"sync","submit":{"command":"./bin/fixture-runtime"}}',
                    'wakeit-plugins/0.2.0/bin/fixture-runtime': b'#!/bin/sh\nexit 0\n',
                }
                for path, data in files.items():
                    item = tarfile.TarInfo(path)
                    item.size = len(data)
                    item.mode = 0o755 if path.endswith(('wakeit', 'fixture-runtime')) else 0o644
                    archive.addfile(item, io.BytesIO(data))
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            assets[target] = {'name': name, 'sha256': digest}
            lines.append(f'{digest}  {name}')
        self.manifest = {'product': 'cli', 'version': '0.2.0', 'assets': assets}
        self.write_manifest()
        with tarfile.open(self.fixture / 'wakeit-skill.tar.gz', 'w:gz') as archive:
            for name, data in [('wakeit/SKILL.md', b'# Fixture skill'), ('wakeit/references/connect.md', b'Connect')]:
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        digest = hashlib.sha256((self.fixture / 'wakeit-skill.tar.gz').read_bytes()).hexdigest()
        lines.append(f'{digest}  wakeit-skill.tar.gz')
        (self.fixture / 'SHA256SUMS').write_text('\n'.join(lines) + '\n')

    def test_missing_declared_resource_keeps_the_existing_installation(self):
        self.bin.mkdir(parents=True)
        previous=self.bin/'wakeit'
        previous.write_bytes(b'previous installation')
        archive=self.fixture/'wakeit-aarch64-apple-darwin.tar.gz'
        with tarfile.open(archive) as source:
            files=[(item,source.extractfile(item).read()) for item in source if not item.name.endswith('/agent.json')]
        with tarfile.open(archive,'w:gz') as output:
            for item,data in files:output.addfile(item,io.BytesIO(data))
        digest=hashlib.sha256(archive.read_bytes()).hexdigest()
        self.manifest['assets']['aarch64-apple-darwin']['sha256']=digest
        self.write_manifest()
        checksums=self.fixture/'SHA256SUMS'
        lines=checksums.read_text().splitlines()
        checksums.write_text('\n'.join(f'{digest}  {archive.name}' if line.endswith('  '+archive.name) else line for line in lines)+'\n')
        result=self.install()
        self.assertNotEqual(result.returncode,0)
        self.assertIn('incomplete plugin bundle',result.stderr)
        self.assertEqual(previous.read_bytes(),b'previous installation')

    def command(self, name, source):
        file = self.commands / name
        file.write_text(source)
        file.chmod(0o755)

    def write_manifest(self):
        (self.fixture / 'wakeit-cli-release.json').write_text(json.dumps(self.manifest, indent=2) + '\n')

    def install(self, *args, **environment):
        return subprocess.run(['sh', str(ROOT / 'install.sh'), *args], env={**self.env, **environment},
                              capture_output=True, text=True)

    def test_install_progress_pinned_downloads_and_idempotent_shell_access(self):
        # Spaces and an apostrophe exercise actual startup-file quoting.
        for _ in range(2):
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(os.access(self.bin / 'wakeit', os.X_OK))
        self.assertTrue(os.access(self.bin / 'wakeit-plugins/0.2.0/bin/fixture-runtime', os.X_OK))
        for stage in range(1, 6):
            self.assertIn(f'[{stage}/5]', result.stderr)
        requests = (self.root / 'requests').read_text().splitlines()
        self.assertIn('/download/cli-v0.2.0/wakeit-cli-release.json', requests[0])
        self.assertIn('/download/cli-v0.2.0/SHA256SUMS', requests[1])
        startup = self.home / '.zshrc'
        self.assertEqual(startup.read_text().count('# Wakeit PATH:'), 1)
        fresh = subprocess.run(['sh', '-c', '. "$1"; wakeit --version', 'sh', str(startup)],
                               env=self.env, capture_output=True, text=True)
        self.assertEqual(fresh.returncode, 0, fresh.stderr)
        self.assertEqual(fresh.stdout.strip(), 'wakeit 0.2.0')
        self.assertEqual(list(self.root.glob('wakeit-install.*')), [])

    def test_platforms_version_and_custom_directory_without_shell_changes(self):
        for system, arch, target in [('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
                                     ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu'),
                                     ('Darwin', 'arm64', 'aarch64-apple-darwin')]:
            with self.subTest(system=system, arch=arch):
                result = self.install('--version', 'v0.2.0', '--bin-dir', str(self.root / target),
                                      '--no-modify-path', FIXTURE_OS=system, FIXTURE_ARCH=arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue((self.root / target / 'wakeit').exists())
        self.assertFalse((self.home / '.zshrc').exists())

    def test_intel_macos_stops_before_download_or_replacement(self):
        self.bin.mkdir(parents=True)
        for script, binary in [('install.sh', 'wakeit'), ('deploy-gateway.sh', 'wakeit-gateway')]:
            with self.subTest(script=script):
                existing = self.bin / binary
                existing.write_bytes(b'existing installation')
                result = subprocess.run(['sh', str(ROOT / script)],
                                        env={**self.env, 'FIXTURE_OS': 'Darwin', 'FIXTURE_ARCH': 'x86_64'},
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Unsupported macOS architecture: x86_64', result.stderr)
                self.assertFalse((self.root / 'requests').exists())
                self.assertEqual(existing.read_bytes(), b'existing installation')

    def test_failures_keep_existing_executable(self):
        self.bin.mkdir(parents=True)
        existing = self.bin / 'wakeit'
        existing.write_bytes(b'existing installation')
        for environment, message in [({'CORRUPT_BINARY': '1'}, 'SHA-256 mismatch'),
                                     ({'FAIL_DOWNLOAD': 'SHA256SUMS'}, 'Could not download'),
                                     ({'FIXTURE_OS': 'Windows'}, 'supports macOS and Linux'),
                                     ({'FIXTURE_ARCH': 'riscv64'}, 'Unsupported architecture')]:
            with self.subTest(environment=environment):
                result = self.install(**environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)
                self.assertEqual(existing.read_bytes(), b'existing installation')
        self.manifest['assets']['aarch64-apple-darwin']['sha256'] = '0' * 64
        self.write_manifest()
        result = self.install()
        self.assertIn('manifest and checksum list disagree', result.stderr)
        self.assertEqual(existing.read_bytes(), b'existing installation')

    def test_complete_skill_install_reuse_and_preservation_of_customizations(self):
        skill = self.home / 'agent skills/wakeit'
        args = ('--skill-dir', str(skill), '--no-modify-path')
        for _ in range(2):
            result = self.install(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((skill / 'references/connect.md').exists())
        (skill / 'SKILL.md').write_text('my customization')
        (self.bin / 'wakeit').write_bytes(b'keep existing CLI')
        result = self.install(*args)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('local customizations were kept', result.stderr)
        self.assertEqual((skill / 'SKILL.md').read_text(), 'my customization')
        self.assertEqual((self.bin / 'wakeit').read_bytes(), b'keep existing CLI')

    def test_skill_only_keeps_cli_and_shell_untouched(self):
        skill = self.home / 'agent skills/wakeit'
        result = self.install('--skill-only')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires --skill-dir', result.stderr)
        result = self.install('--skill-only', '--skill-dir', str(skill))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((skill / 'references/connect.md').exists())
        self.assertFalse(self.bin.exists())
        self.assertFalse((self.home / '.zshrc').exists())
        self.assertNotIn('/wakeit-aarch64-apple-darwin.tar.gz', (self.root / 'requests').read_text())
        self.bin.mkdir(parents=True)
        existing = self.bin / 'wakeit'
        existing.write_bytes(b'existing CLI')
        result = self.install('--skill-only', '--skill-dir', str(skill))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(existing.read_bytes(), b'existing CLI')

    def test_gateway_installer_has_separate_binary_version_and_operations(self):
        name = 'wakeit-gateway-aarch64-apple-darwin'
        binary = self.fixture / name
        binary.write_text('#!/bin/sh\nif [ "$1" = --version ]; then echo "wakeit-gateway 0.4.2"; else printf "%s\\n" "$*" >> "$FIXTURE_INVOCATIONS"; fi\n')
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        manifest = {'product': 'gateway', 'version': '0.4.2', 'assets': {
            'aarch64-apple-darwin': {'name': name, 'sha256': digest},
        }}
        (self.fixture / 'wakeit-gateway-release.json').write_text(json.dumps(manifest, indent=2))
        (self.fixture / 'SHA256SUMS').write_text(f'{digest}  {name}\n')
        self.bin.mkdir(parents=True)
        (self.bin / 'wakeit').write_bytes(b'existing CLI')
        invocations = self.root / 'gateway-commands'
        result = subprocess.run(['sh', str(ROOT / 'deploy-gateway.sh'), '--config', 'gateway.yaml',
                                 '--env-file', 'gateway.env', '--start'],
                                env={**self.env, 'FIXTURE_INVOCATIONS': str(invocations)},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.bin / 'wakeit').read_bytes(), b'existing CLI')
        self.assertTrue((self.bin / 'wakeit-gateway').exists())
        self.assertEqual(invocations.read_text().splitlines(), [
            'migrate --config gateway.yaml --env-file gateway.env',
            'start --config gateway.yaml --env-file gateway.env',
        ])
        self.assertIn('/gateway-v0.4.2/', (self.root / 'requests').read_text())


    def test_bash_and_fish_configuration_preserves_existing_content(self):
        (self.home / '.bash_profile').write_text('# User login config\n')
        result = self.install(SHELL='/bin/bash')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('# User login config', (self.home / '.bash_profile').read_text())
        self.assertTrue((self.home / '.bashrc').exists())
        fish = self.home / '.config/fish/conf.d/wakeit.fish'
        fish.parent.mkdir(parents=True)
        fish.write_text('# User fish config\n')
        for _ in range(2):
            result = self.install(SHELL='/usr/bin/fish')
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(fish.read_text().startswith('# User fish config'))
        self.assertEqual(fish.read_text().count('# Wakeit PATH:'), 1)


if __name__ == '__main__':
    unittest.main()
