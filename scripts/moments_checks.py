"""Completion ordering, local days, bounded previews and cursor pagination on real PostgreSQL."""
from integration_support import expect_status
import datetime as dt
import json
import uuid
from urllib.parse import urlencode

def verify(api, base, other, sql, task, apply, role_tokens, runner_id, check, ws):
    assert api('/moments/days')['total_runs'] == 0
    assert api('/moments/days')['days'] == []
    config = task('moments-fixture')
    apply(config)
    settings = api('/settings')
    ids = []
    def insert(at, state='succeeded', result='report', source='manual', finished=True):
        id = str(uuid.uuid4()); ids.append(id)
        literal = json.dumps(result, ensure_ascii=False).replace("'", "''")
        stamp = "'" + at + "'::timestamptz" if finished else 'NULL'
        sql(f"INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,state,reason,source,scheduled_at,deadline,finished_at,result,started_at) SELECT workspace_id,'{id}',id,revision,runner_id,config,'{state}','actual failure reason','{source}',now(),now()+interval '1 hour',{stamp},'{literal}'::jsonb,'{at}'::timestamptz-interval '10 hours' FROM tasks WHERE id='moments-fixture'")
        return id
    try:
        before = api('/moments/overview')
        stamp = dt.datetime.now(dt.timezone.utc).isoformat()
        parents = [insert(stamp, state) for state in ['succeeded','failed','timed_out','interrupted','warning','cancelled','skipped']]
        child = insert(stamp)
        sql(f"UPDATE runs SET parent_run_id='{parents[0]}',node_id='summary' WHERE id='{child}'")
        overview = api('/moments/overview')
        totals = lambda value: tuple(sum(day[key] for day in value['daily']) for key in ['succeeded','failed','warning'])
        assert tuple(a-b for a,b in zip(totals(overview),totals(before))) == (1,3,1)
        assert len(overview['daily']) == 7 and overview['timezone'] == settings['timezone']
        assert len(api('/moments/overview?days=1')['daily']) == 1
        directory = api('/moments/days')
        recent = api('/moments?date='+directory['days'][0]['date'])['items']
        assert len(recent) == 7 and all(row['creator']['username'] == 'admin' for row in recent)
        for role,token in role_tokens.items():
            assert totals(ws.call(base,'/moments/overview',token=token)) == totals(overview), role
        expect_status(lambda:ws.call(base,'/moments/overview'),401)
        for bound in [0,32]: expect_status(lambda:api('/moments/overview?days='+str(bound)),400)
        sql("DELETE FROM runs WHERE task_id='moments-fixture'")
        ids.clear()
        check('Moments overview counts parent Runs, excludes cancellations/skips, uses creator identity and supports Viewer access')
        api('/settings','PUT',dict(settings, timezone='America/New_York',run_retention_days=3650))
        insert('2026-11-01T05:30:00Z', result='你好🙂' * 900)
        days = api('/moments/days')
        assert days['timezone'] == 'America/New_York' and days['total_runs'] == 1 and days['days'] == [{'date':'2026-11-01','count':1}]
        one = api('/moments?date=2026-11-01')['items'][0]
        assert len(one['preview']) == 2048 and one['has_more'] and one['format']=='markdown'
        assert 'config' not in one and 'prompt' not in one
        # Repeated DST hour: same local time, two distinct completion instants.
        insert('2026-11-01T06:30:00Z', result={'summary':'JSON','truncated':True})
        for i,state in enumerate(['failed','cancelled','timed_out','skipped','interrupted']):
            insert(f'2026-11-01T07:00:0{i}Z',state,{'truncated':True,'preview':'retained'} if i==0 else None,source='retry')
        for i in range(21):insert('2026-11-01T08:00:00Z',source='schedule' if i==0 else 'manual')
        insert('2026-11-02T04:30:00Z',result='local previous day')
        insert('2026-11-02T05:30:00Z',result='overnight completion')
        for state in ['unknown','running','starting','queued']:insert('2026-11-01T09:00:00Z',state)
        insert('2026-11-01T09:00:00Z',finished=False)
        for i in range(16):insert((dt.datetime(2026,10,31,12,tzinfo=dt.timezone.utc)-dt.timedelta(days=i)).isoformat())
        directory = api('/moments/days?limit=14')
        assert directory['total_runs']==46 and len(directory['days'])==14
        assert directory['days'][0]=={'date':'2026-11-02','count':1}
        assert directory['days'][1]=={'date':'2026-11-01','count':29}
        more = api('/moments/days?'+urlencode({'before':directory['next_before'],'limit':14}))
        assert more['total_runs']==46 and len(more['days'])==4 and more['next_before'] is None
        assert not set(d['date'] for d in directory['days']) & set(d['date'] for d in more['days'])
        first = api('/moments?date=2026-11-01')
        second = api('/moments?'+urlencode({'date':'2026-11-01','cursor':first['next_cursor']}))
        all_rows = first['items']+second['items']
        assert len(first['items'])==20 and len(second['items'])==9 and second['next_cursor'] is None
        assert len({r['id'] for r in all_rows})==29
        tied=[r['id'] for r in all_rows if r['finished_at'].startswith('2026-11-01T08:00:00')]
        assert len(tied)==21 and tied==sorted(tied,reverse=True)
        assert any(r['format']=='json' and 'summary' in (r['preview'] or '') for r in all_rows)
        assert any(r['truncated'] and r['preview']=='retained' for r in all_rows)
        assert all(r['runner']=='test-device' and r['agent']=='mock-sync' and r['logo'] for r in all_rows)
        sql("UPDATE tasks SET archived_at=now(),enabled=false WHERE id='moments-fixture'")
        assert api('/moments/days')['total_runs']==46
        for role,token in role_tokens.items():
            assert ws.call(other,'/moments/days',token=token)['total_runs']==46, role
            assert len(ws.call(base,'/moments?date=2026-11-01',token=token)['items'])==20
        expect_status(lambda:ws.call(base,'/moments/days'),401)
        for path in ['/moments/days?limit=0','/moments/days?before=wrong','/moments?date=2026-02-30','/moments?date=2026-11-01&cursor=wrong','/moments?date=2026-11-01&limit=101']:
            expect_status(lambda:api(path),400)
        expect_status(lambda:api('/moments?date=2026-11-01&timezone=UTC'),409)
        api('/settings','PUT',dict(settings,timezone='UTC',run_retention_days=3650))
        assert api('/moments/days')['days'][0]=={'date':'2026-11-02','count':2}
        assert api('/moments?date=2026-11-02')['timezone']=='UTC'
        check('Moments completion days, DST, Unicode previews, tie cursors, both pagination levels, archives and Viewer access across Gateways')
    finally:
        sql("DELETE FROM runs WHERE task_id='moments-fixture'; DELETE FROM tasks WHERE id='moments-fixture'")
        api('/settings','PUT',settings)
