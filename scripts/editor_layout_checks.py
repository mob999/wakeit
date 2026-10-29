"""Shared canvas coordinates: atomic Task writes, scope and CLI compatibility."""
import concurrent.futures
import copy
import json
import subprocess
import uuid
from integration_support import APIError, expect_status, wait


def verify(api, a, b, check, context):
    task_id = 'canvas-' + uuid.uuid4().hex[:10]
    config = dict(version=1, id=task_id, name='Canvas fixture', schedule=None, enabled=False,
                  dag=dict(nodes=[dict(id=id, name=id, runner=a['id'], agent='mock-events',
                       cwd=a['cwd'], prompt=json.dumps({'text':'Synthetic layout report'}), pre=pre) for id, pre in [('source', []), ('report', ['source'])]]))
    layout = dict(version=1, positions=dict(source=dict(x=-120.5, y=0), report=dict(x=320, y=180)))
    def save(c, revision, value='omitted', gateway=None, **kwargs):
        body = dict(config=c, expected_revision=revision)
        if value != 'omitted': body['editor_layout'] = value
        return api('/tasks/' + task_id, 'PUT', body, **(dict(gateway=gateway) if gateway else {}), **kwargs)
    first = save(config, None, layout)
    assert first['editor_layout'] == layout
    assert 'editor_layout' not in first['config']
    assert api('/tasks/' + task_id, gateway=context['other'])['editor_layout'] == layout
    for invalid in [dict(version=2, positions={}), dict(version=1, positions={'unknown': dict(x=0, y=0)}),
                    dict(version=1, positions={'source': dict(x=None, y=0)}),
                    dict(version=1, positions={}, extra=True)]:
        expect_status(lambda: save(config, first['revision'], invalid), 422 if 'extra' in invalid or invalid.get('positions', {}).get('source', {}).get('x', 0) is None else 400)
        unchanged = api('/tasks/' + task_id)
        assert unchanged['revision'] == first['revision'] and unchanged['editor_layout'] == layout
    expect_status(lambda: save(config, first['revision'], layout, workspace=b['workspace'], token=a['token']), 403)
    expect_status(lambda: api('/tasks/' + task_id, workspace=b['workspace'], token=b['token']), 404)
    # A stale form cannot alter either the definition or the coordinates.
    changed = copy.deepcopy(config); changed['name'] = 'Stale draft'
    expect_status(lambda: save(changed, 0, dict(version=1, positions={})), 409)
    assert api('/tasks/' + task_id)['config']['name'] == config['name']
    assert api('/tasks/' + task_id)['editor_layout'] == layout
    check('canvas layout validates version, finite coordinates and node ownership; revision and workspace boundaries are atomic')

    # Concurrent replicas must choose a complete config/layout pair, never a mixture.
    def race(marker, gateway):
        c = copy.deepcopy(config); c['name'] = 'Canvas ' + str(marker)
        l = copy.deepcopy(layout); l['positions']['source']['x'] = marker
        try: return save(c, first['revision'], l, gateway=gateway)
        except APIError as e:
            assert e.status == 409
            return None
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        rows = list(pool.map(lambda args: race(*args), [(11, None), (22, context['other'])]))
    assert sum(row is not None for row in rows) == 1
    latest = api('/tasks/' + task_id)
    assert latest['config']['name'] == 'Canvas ' + str(int(latest['editor_layout']['positions']['source']['x']))
    preserved = latest['editor_layout']
    for value in ['omitted', None]:
        latest = save(latest['config'], latest['revision'], value)
        assert latest['editor_layout'] == preserved
    check('two-Gateway concurrent saves retain one complete definition/layout pair; omitted and null layout preserve positions')

    if context.get('global_api'):
        global_api = context['global_api']
        invite = api('/invites', 'POST', {'role': 'viewer'})['token']
        username = 'canvas-reader-' + uuid.uuid4().hex[:8]
        global_api('/invites/accept', 'POST', dict(token=invite, username=username, password='workspace-fixture-password'), token=None)
        token = global_api('/session', 'POST', dict(username=username, password='workspace-fixture-password'), token=None)['token']
        user_id = global_api('/session', token=token)['id']
        assert api('/tasks/' + task_id, token=token)['editor_layout'] == preserved
        expect_status(lambda: save(latest['config'], latest['revision'], preserved, token=token), 403)
        api('/members/' + user_id, 'PUT', dict(role='operator'))
        latest = save(latest['config'], latest['revision'], preserved, token=token)
        check('Viewer can read shared layout; Read/write can save it without instance administration')

    # CLI apply remains execution-only and does not clear shared editor state.
    file = context['root'] / (task_id + '.json')
    file.write_text(json.dumps(latest['config']))
    env = dict(context['cli_env'], WAKEIT_TOKEN=a['token'])
    subprocess.run([str(context['binary']), '--gateway', context['base'], '--workspace', a['workspace'],
                    '--json', 'task', 'apply', '--file', str(file)], env=env, check=True, capture_output=True)
    latest = api('/tasks/' + task_id)
    assert latest['editor_layout'] == preserved
    changed = copy.deepcopy(latest['config']); changed['dag']['nodes'].pop()
    pruned = save(changed, latest['revision'])
    assert list(pruned['editor_layout']['positions']) == ['source']
    # Layouts remain presentation-only even when a graph is actually executed.
    run = api('/tasks/' + task_id + '/runs', 'POST', {})
    finished = wait(lambda: (r if (r := api('/runs/' + run['id']))['state'] == 'succeeded' else None), 'layout fixture execution')
    assert 'editor_layout' not in finished['config']
    assert finished['result']['outputs'][0]['result'] == 'Synthetic layout report'
    check('CLI apply preserves shared coordinates, removed-node coordinates are pruned and Run snapshots exclude layout')
