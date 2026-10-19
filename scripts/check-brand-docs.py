#!/usr/bin/env python3
"""Check documentation assets, standalone skill links and schema examples."""
from pathlib import Path
import json
import re
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    files = sorted(ROOT.glob('README*.md')) + [ROOT / 'CONTRIBUTING.md']
    files += list((ROOT / 'docs').glob('*.md'))
    files += list((ROOT / 'skills/wakeit').rglob('*.md'))
    for path in files:
        for target in re.findall(r'\]\(([^)]+)\)', path.read_text(encoding='utf-8')):
            target = target.split('#')[0]
            if target and not re.match(r'[a-z][a-z0-9+.-]*:', target):
                assert (path.parent / target).exists(), f'Broken link in {path}: {target}'
                if '/skills/wakeit/' in str(path):
                    assert (path.parent / target).resolve().is_relative_to(ROOT / 'skills/wakeit'), f'Skill is not standalone: {target}'
    import yaml
    import jsonschema
    for filename, kind in [('task.yaml','task'), ('task-dag.yaml','task'), ('sync-agent.yaml','agent'), ('async-agent.yaml','agent'), ('events-agent.yaml','agent')]:
        value = yaml.safe_load((ROOT / 'skills/wakeit/assets' / filename).read_text(encoding='utf-8'))
        schema = json.loads((ROOT / 'schemas' / f'{kind}.v1.json').read_text(encoding='utf-8'))
        jsonschema.validate(value, schema)
    jsonschema.validate(yaml.safe_load((ROOT / 'examples/task-dag.yaml').read_text(encoding='utf-8')),
                        json.loads((ROOT / 'schemas/task.v1.json').read_text(encoding='utf-8')))
    adapter = ROOT / 'skills/wakeit/references/adapters.md'
    stream_schema = json.loads((ROOT / 'schemas/stream.v1.json').read_text(encoding='utf-8'))
    for line in adapter.read_text(encoding='utf-8').splitlines():
        if line.startswith('{"version":1,"type":'):
            jsonschema.validate(json.loads(line), stream_schema)
    entry=(ROOT / 'skills/wakeit/SKILL.md').read_text(encoding='utf-8')
    assert '[TODO' not in entry
    with tempfile.TemporaryDirectory() as tmp:
        out=Path(tmp) / 'unpacked'
        # Prove references survive an independent skill install, outside repo.
        import shutil
        shutil.copytree(ROOT / 'skills/wakeit',out)
        for path in out.rglob('*.md'):
            for target in re.findall(r'\]\(([^)]+)\)',path.read_text(encoding='utf-8')):
                if not re.match(r'[a-z][a-z0-9+.-]*:',target):
                    assert (path.parent/target.split('#')[0]).exists()
    print('Documentation links, independent skill install, YAML and event examples passed.')


if __name__ == '__main__':
    main()
