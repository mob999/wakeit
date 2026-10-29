"""Run Watch checks against the isolated real service stack."""
import json
import os
import subprocess


def verify(api, other, task, apply, trigger, role_tokens, binary, root, check, workspace):
    home = root / 'watch-home'
    home.mkdir()
    environment = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home),
                       WAKEIT_TOKEN=role_tokens['viewer'], WAKEIT_WORKSPACE=workspace)
    environment.pop('WAKEIT_TOKEN_FILE', None)
    environment.pop('WAKEIT_GATEWAY', None)

    def watch(run_id, structured=True):
        arguments = [str(binary), '--gateway', other]
        if structured:
            arguments.append('--json')
        arguments += ['run', 'watch', run_id, '--timeout', '20s']
        result = subprocess.run(arguments, env=environment, text=True, capture_output=True, timeout=25)
        assert result.returncode == 0, (result.returncode, result.stdout[-2000:], result.stderr[-2000:])
        return result.stdout

    apply(task('watch-live', agent='mock-events',
               prompt=json.dumps({'sleep': 1, 'count': 510, 'text': '# Watch report\n\n完成'})))
    run_id = trigger('watch-live')
    records = [json.loads(line) for line in watch(run_id).splitlines()]
    assert records[-1]['type'] == 'summary'
    assert records[-1]['data'] == {
        'state': 'succeeded', 'terminal': True, 'reason': None,
        'result': '# Watch report\n\n完成', 'exit_code': 0,
    }
    logs = [row for row in records if row['type'] == 'log']
    assert len(logs) == 510
    assert len({row['event_id'] for row in logs}) == 510
    assert any(row['type'] == 'trace' for row in records)
    assert all(row['run_id'] == run_id for row in records)
    assert api('/runs/' + run_id)['state'] == 'succeeded'
    finished = watch(run_id, structured=False)
    assert '# Watch report' in finished and '完成' in finished
    check('Viewer Run Watch across Gateways, live Trace, 500-event pagination and completed report')
