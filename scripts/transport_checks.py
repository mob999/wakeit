"""Payload isolation and durable event upload on a disposable PostgreSQL/NATS/Runner stack."""
import datetime
import json
import shutil
import sqlite3
import uuid
from integration_support import wait


def verify(context):
    api, sql, task, apply, trigger, final, check = (context[key] for key in
        ['api', 'sql', 'task', 'apply', 'trigger', 'final', 'check'])
    stop, spawn = context['stop'], context['spawn']
    auth_dir, root = context['auth_dir'], context['root']
    runner_id, runner_dir = context['runner_id'], context['runner_dir']
    broker = context['broker']
    nats_config = auth_dir / 'nats.conf'
    initial_config = nats_config.read_text()
    journal = runner_dir / 'data/journal.sqlite'

    def local(query, arguments=()):
        with sqlite3.connect(journal, timeout=10) as connection:
            return connection.execute(query, arguments).fetchall()

    def set_capacity(capacity):
        nonlocal broker
        stop(broker)
        nats_config.write_text(initial_config.replace('max_payload: 2097152', f'max_payload: {capacity}'))
        broker = spawn('nats-transport-' + str(capacity),
                       [shutil.which('nats-server'), '--config', nats_config])

    for maximum in [2 * 1024 * 1024, 1024 * 1024]:
        set_capacity(maximum)
        suffix = str(maximum)
        # A short definition expands an upstream result to <1 MiB raw text,
        # but JSON escaping makes its actual assignment exceed both broker limits.
        producer = dict(id='source', name='Source', pre=[], runner=runner_id, agent='mock-events',
                        cwd=str(context['cwd']), prompt=json.dumps({'text': '\x01' * 100_000}), timeout='20s')
        dependent = dict(producer, id='escaped', name='Escaped assignment', pre=['source'],
                         prompt={'parts': [{'result': 'source'}] * 8})
        normal = dict(producer, id='normal', name='Normal sibling', pre=[], prompt='normal sibling')
        dag = dict(version=1, id='transport-dag-' + suffix, name='Transport fixture', enabled=False,
                   schedule=None, execution=dict(timeout='30s', start_grace='20s', overlap='skip', misfire='latest'),
                   dag={'nodes': [producer, dependent, normal], 'results': ['escaped', 'normal']})
        apply(dag)
        graph_id = trigger(dag['id'])
        graph = wait(lambda: (value if (value := api('/runs/' + graph_id))['state'] in ['warning', 'failed'] else None),
                     'oversized DAG settles', timeout=45)
        nodes = {node['id']: node for node in graph['nodes']}
        assert nodes['source']['state'] == 'succeeded'
        assert nodes['normal']['state'] == 'succeeded'
        assert nodes['escaped']['state'] == 'failed'
        assert 'encoded NATS payload' in nodes['escaped']['reason']
        assert sql("SELECT count(*) FROM outbox WHERE payload->>'run_id'='" +
                   nodes['escaped']['execution_id'] + "' AND quarantined_at IS NOT NULL") == '1'
        # Normal work and its cancellation must still pass after the poisoned row.
        apply(task('transport-control-' + suffix, prompt=json.dumps({'sleep': 10, 'text': 'cancel fixture'})))
        cancel_id = trigger('transport-control-' + suffix)
        wait(lambda: api('/runs/' + cancel_id)['state'] == 'running', 'control fixture running')
        api('/runs/' + cancel_id + '/cancel', 'POST', {})
        final(cancel_id, 'cancelled')
        check(f'{maximum // 1024} KiB broker: escaped Assignment quarantined; sibling work and cancellation continue')

        apply(task('transport-claimed-' + suffix, prompt=json.dumps({'sleep': 3, 'text': 'claimed delivery'})))
        claimed_id = trigger('transport-claimed-' + suffix)
        wait(lambda: api('/runs/' + claimed_id)['state'] == 'running', 'claimed execution running')
        sql("INSERT INTO outbox(subject,message_id,payload,workspace_id) SELECT subject,'bad-redelivery:" + claimed_id +
            "',jsonb_set(payload,'{task,prompt}',to_jsonb(repeat(chr(1),500000))),workspace_id FROM outbox WHERE message_id='start:" + claimed_id + "'")
        wait(lambda: sql("SELECT count(*) FROM outbox WHERE message_id='bad-redelivery:" + claimed_id + "' AND quarantined_at IS NOT NULL") == '1',
             'oversized redelivery quarantined')
        final(claimed_id)
        assert (context['remote'] / 'calls.log').read_text().count('sync ' + claimed_id) == 1
        check(f'{maximum // 1024} KiB broker: quarantining a redelivery preserves an already claimed execution')

        apply(task('transport-event-' + suffix, prompt='event upload fixture'))
        run_id = trigger('transport-event-' + suffix)
        final(run_id)
        wait(lambda: local('SELECT count(*) FROM pending_events WHERE run_id=? AND published=0', (run_id,))[0][0] == 0,
             'initial events uploaded')
        # Simulate pre-existing durable oversized events, followed by a small final
        # state. Exercise exact envelopes, ordered repair, Session identity and originals.
        with sqlite3.connect(journal, timeout=10) as connection:
            claim, sequence = connection.execute('SELECT claim,seq FROM local_runs WHERE id=?', (run_id,)).fetchone()
            timestamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
            session_id = str(uuid.uuid4())
            events = [
                ('log', {'stream': 'stdout', 'text': '\x01' * 500_000}),
                ('session', {'id': session_id, 'runner_id': runner_id, 'agent': 'mock-sync',
                             'cwd': str(context['cwd'].resolve()), 'ready': True,
                             'command': 'x' * (maximum + 1), 'reason': None, 'title': 'Fixture'}),
                ('state', {'state': 'succeeded', 'retryable': False, 'result': '\x01' * 500_000}),
                ('state', {'state': 'succeeded', 'reason': 'following small state'}),
            ]
            for offset, (kind, data) in enumerate(events, 1):
                event = dict(version=1, runner_id=runner_id, run_id=run_id, claim_id=claim,
                             seq=sequence + offset, kind=kind, data=data, timestamp=timestamp)
                connection.execute('INSERT INTO pending_events(run_id,seq,payload) VALUES(?,?,?)',
                                   (run_id, event['seq'], json.dumps(event)))
            connection.execute('UPDATE local_runs SET seq=? WHERE id=?', (sequence + len(events), run_id))
        wait(lambda: local('SELECT count(*) FROM pending_events WHERE run_id=? AND published=0', (run_id,))[0][0] == 0,
             'oversized events repaired and uploaded', timeout=30)
        assert local('SELECT count(*) FROM event_quarantine WHERE run_id=?', (run_id,))[0][0] == 3
        recorded = api('/runs/' + run_id + '/events')
        repaired = {event['seq']: event for event in recorded if event['seq'] > sequence}
        assert len(repaired) == 4
        assert repaired[sequence + 1]['kind'] == 'trace_status'
        assert repaired[sequence + 1]['data']['reason'] == 'event_too_large'
        assert repaired[sequence + 2]['kind'] == 'session'
        assert repaired[sequence + 2]['data']['id'] == session_id
        assert repaired[sequence + 2]['data']['ready'] is False
        assert repaired[sequence + 3]['data']['state'] == 'succeeded'
        assert repaired[sequence + 4]['data']['reason'] == 'following small state'
        assert sorted(repaired) == sorted(event['seq'] for event in repaired.values())
        check(f'{maximum // 1024} KiB broker: oversized event repair preserves order, final states, Session identity and local originals')

    # During a broker outage, native completion releases the sole execution slot
    # while final events stay in the journal. The queued next Run is delivered once.
    apply(task('transport-outage', prompt=json.dumps({'sleep': 4, 'text': 'offline completion'})))
    first_id = trigger('transport-outage')
    wait(lambda: api('/runs/' + first_id)['state'] == 'running', 'outage fixture running')
    stop(broker)
    apply(task('transport-after-outage', prompt='after outage'))
    second_id = trigger('transport-after-outage')
    wait(lambda: local('SELECT phase FROM local_runs WHERE id=?', (first_id,)) == [('done',)],
         'native process ended while broker offline')
    assert local('SELECT count(*) FROM pending_events WHERE run_id=? AND published=0', (first_id,))[0][0] > 0
    wait(lambda: next(row for row in api('/runners') if row['id'] == runner_id)['info']['active'] == 0,
         'completed process releases execution slot before upload', timeout=25)
    set_capacity(1024 * 1024)
    final(first_id, timeout=40)
    final(second_id, timeout=40)
    wait(lambda: local('SELECT count(*) FROM pending_events WHERE published=0')[0][0] == 0,
         'all durable events uploaded after recovery')
    calls = (context['remote'] / 'calls.log').read_text()
    for run_id in [first_id, second_id]:
        assert calls.count('sync ' + run_id) == 1
    check('Broker recovery uploads durable final states, releases native capacity and never repeats Agent execution')
    print('Transport payload acceptance passed.', flush=True)
