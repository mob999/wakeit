#!/usr/bin/env python3
"""Check service bundles and executable replacement in an isolated directory."""
import argparse
import getpass
import hashlib
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True, encoding="utf-8", check=True)
    return result.stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--probe', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    probe = args.probe.resolve()
    folder = Path(tempfile.mkdtemp(prefix='wakeit-native-'))
    try:
        config = folder / '网关 config.yaml'
        shutil.copyfile(ROOT / 'skills/wakeit/assets/gateway.yaml', config)
        env = folder / 'gateway.env'
        env.write_text("WAKEIT_DATABASE_URL='postgres://fixture:never-log-this@127.0.0.1/wakeit'\n", encoding="utf-8")
        for platform in ('linux', 'macos', 'windows'):
            output = folder / platform
            user = 'LocalService' if platform == 'windows' else getpass.getuser()
            run(binary, 'gateway', 'service', '--config', config, '--env-file', env,
                '--user', user, '--platform', platform, '--output', output)
            for file in output.iterdir():
                assert 'never-log-this' not in file.read_text(encoding='utf-8')
                if file.suffix == '.plist':
                    value = plistlib.loads(file.read_bytes())
                    assert value['UserName'] == user and value['RunAtLoad'] and value['KeepAlive']
                    if sys.platform == 'darwin':
                        run('plutil', '-lint', file)
                if file.suffix == '.xml':
                    root = ET.parse(file).getroot()
                    assert root.findtext('startmode') == 'Automatic'
                if file.suffix == '.sh' and shutil.which('sh'):
                    subprocess.run(['sh', '-n'], input=file.read_text(encoding='utf-8'), text=True, encoding='utf-8', capture_output=True, check=True)
                if file.suffix == '.ps1' and shutil.which('pwsh'):
                    # Parse only; no service is installed or stopped.
                    validator = folder / 'parse-powershell.ps1'
                    validator.write_text("param([string]$Path)\n$t=$null; $e=$null; [System.Management.Automation.Language.Parser]::ParseFile($Path,[ref]$t,[ref]$e) | Out-Null; if ($e.Count) { throw ($e | Out-String) }\n", encoding="utf-8")
                    run('pwsh', '-NoProfile', '-NonInteractive', '-File', validator, '-Path', file)
            if platform == 'linux' and shutil.which('systemd-analyze'):
                run('systemd-analyze', 'verify', output / 'wakeit-gateway.service')
        # The copied probe replaces only itself; live binaries/data are untouched.
        copy = folder / ('probe.exe' if os.name == 'nt' else 'probe')
        shutil.copy2(probe, copy)
        candidate = folder / binary.name
        shutil.copy2(binary, candidate)
        before = hashlib.sha256(copy.read_bytes()).digest()
        try:
            run(copy, folder / 'missing-candidate')
        except subprocess.CalledProcessError:
            pass
        else:
            raise AssertionError('Invalid candidate was accepted')
        assert hashlib.sha256(copy.read_bytes()).digest() == before
        run(copy, candidate)
        assert hashlib.sha256(copy.read_bytes()).digest() == hashlib.sha256(binary.read_bytes()).digest()
        assert run(copy, '--version').strip() == run(binary, '--version').strip()
        print('Three-platform service parsers and isolated replacement/failed-candidate checks passed.')
    finally:
        for attempt in range(40):
            try:
                shutil.rmtree(folder)
                break
            except PermissionError:
                # Windows cleanup helpers wait for the replaced process to exit.
                if attempt == 39:
                    raise
                time.sleep(.25)


if __name__ == '__main__':
    main()
