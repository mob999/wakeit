"""Creation-time compilation, atomic instances and frozen execution on real services."""
import concurrent.futures
import copy
import datetime
import json
import os
import subprocess
import uuid
from integration import APIError, command, expect_status, wait


def verify(api, a, b, check, context):
    sql, other, binary, env = (context[k] for k in ['sql', 'other', 'binary', 'cli_env'])
    key = uuid.uuid4().hex
    definition = dict(version=1, id='template-'+key, name='Repository review', description='Synthetic keyed fan-in',
        inputs=dict(type='object', additionalProperties=False, required=['repositories'], properties={
            'repositories':dict(type='array', items=dict(type='object',additionalProperties=False,required=['key','text'],properties={
                'key':dict(type='string'), 'text':dict(type='string'), 'enabled':dict(type='boolean')})),
            'summarize':dict(type='boolean',default=True), 'note':dict(type='string',default='default')}),
        bindings=dict(worker=dict(title='Review environment',defaults=dict(runner=a['id'],agent='mock-events',parameters={}))),
        task=dict(name=dict(template='Review [[ inputs.repositories | length ]]'),schedule=None,
            execution=dict(timeout=None,start_grace='20s',overlap='skip',misfire='latest'),dag=dict(nodes=[
                dict(group='review',for_each=dict(expr='inputs.repositories'),**{'as':'repo'},key='key',when=dict(expr='repo.enabled | default(true)'),node=dict(
                    name=dict(template='Review [[ repo.key ]]'),binding='worker',cwd=a['cwd'],timeout='20s',
                    prompt=dict(parts=[dict(template='{"text":[[ repo.text | tojson ]],"sleep":0.1}')]))),
                dict(id='summary',name='Summary',binding='worker',cwd=a['cwd'],when=dict(expr='inputs.summarize'),
                    pre=[dict(group='review',all=True)],prompt=dict(parts=[dict(text='Reports:\n'),dict(result=dict(group='review',all=True),separator='\n---\n')]))],
                results=[dict(node='summary',when=dict(expr='inputs.summarize')),dict(group='review',all=True,when=dict(expr='not inputs.summarize'))])))
    path='/task-templates/'+definition['id']
    saved=api(path,'PUT',dict(definition=definition,expected_revision=None));assert saved['revision']==1
    request=dict(expected_revision=1,values=dict(repositories=[dict(key='alpha',text='# Alpha'),dict(key='beta',text='# Beta')]),unset=[],bindings={},request_id='draft-1')
    before=int(sql(f"SELECT count(*) FROM tasks WHERE workspace_id='{a['workspace']}'"))
    preview=api(path+'/preview','POST',request)
    assert preview['issues']==[] and preview['request_id']=='draft-1',preview
    assert [n['id'] for n in preview['graph']['nodes']]==['review__alpha','review__beta','summary']
    assert preview['graph']['nodes'][2]['pre']==['review__alpha','review__beta']
    assert int(sql(f"SELECT count(*) FROM tasks WHERE workspace_id='{a['workspace']}'"))==before
    response=api(path+'/instantiate','POST',dict(request,run_now=True),key='create-'+key)
    task_id,run_id=response['task_id'],response['run_id'];assert task_id and run_id
    def finish(id):
        return wait(lambda:(r if (r:=api('/runs/'+id))['state'] in ['succeeded','warning','failed','cancelled','timed_out'] else None),'template Run '+id,timeout=40)
    final=finish(run_id);assert final['state']=='succeeded',final
    assert final['result']['outputs'][0]['result']=='Reports:\n# Alpha\n---\n# Beta',final
    assert all(n['state']=='succeeded' for n in final['nodes'])
    detail=api('/runs/'+run_id+'/nodes/summary')['execution']
    assert detail['execution_snapshot']['input_sources']==['review__alpha','review__beta']
    assert detail['config']['version']==1 and isinstance(detail['config']['prompt'],str)
    task=api('/tasks/'+task_id)
    assert task['template_origin']['template_revision']==1
    assert final['template_origin']==task['template_origin']
    assert task['config'].get('session') is None
    check('templates compile keyed fan-in without writes; atomic create-and-run executes through a v1 Runner with actual input provenance')

    modified=copy.deepcopy(definition);modified['name']='Changed template';modified['task']['dag']['nodes'][0]['node']['prompt']['parts']=[dict(text='Changed')]
    saved=api(path,'PUT',dict(definition=modified,expected_revision=1));assert saved['revision']==2
    api(path+'/archive','POST',dict(expected_revision=2))
    replay=api(path+'/instantiate','POST',dict(request,run_now=True),key='create-'+key,gateway=other)
    assert replay==response
    expect_status(lambda:api(path+'/instantiate','POST',dict(request,run_now=False),key='create-'+key),409)
    expect_status(lambda:api(path+'/instantiate','POST',request),409)
    assert api('/tasks/'+task_id)['config']==task['config']
    assert api('/runs/'+run_id)['config']==final['config']
    restored=api(path+'/restore','POST',dict(expected_revision=3));assert restored['revision']==4
    check('committed operation replays after template edits/archive across Gateways; conflicting keys and stale revisions cannot create duplicates')

    current=dict(request,expected_revision=4,run_now=False,task_id='template-concurrent-'+key)
    def concurrent_create(gateway):return api(path+'/instantiate','POST',current,key='concurrent-'+key,gateway=gateway)
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        results=list(executor.map(concurrent_create,[context['base'],other]))
    assert results[0]==results[1]
    expect_status(lambda:api(path+'/instantiate','POST',current,key='different-'+key),409)
    assert int(sql(f"SELECT count(*) FROM tasks WHERE workspace_id='{a['workspace']}' AND id='{current['task_id']}'"))==1
    for field in ['same-task',current['task_id']]:
        expect_status(lambda:api(path+'/instantiate','POST',dict(current,task_id=field),key=uuid.uuid4().hex),409)
    invalid=dict(current,task_id='invalid-'+key,values=dict(repositories=[dict(key='same',text='one'),dict(key='same',text='two')]))
    expect_status(lambda:api(path+'/instantiate','POST',invalid),422)
    assert sql(f"SELECT count(*) FROM tasks WHERE id='{invalid['task_id']}'")=='0'
    assert sql(f"SELECT count(*) FROM runs WHERE task_id='{invalid['task_id']}'")=='0'
    bad_binding=dict(current,task_id='binding-'+key,bindings=dict(worker=dict(runner=b['id'],agent='mock-events')))
    expect_status(lambda:api(path+'/instantiate','POST',bad_binding),422)
    assert sql(f"SELECT count(*) FROM tasks WHERE id='{bad_binding['task_id']}'")=='0'
    expect_status(lambda:api(path,workspace=b['workspace'],token=b['token']),404)
    expect_status(lambda:api(path+'/instantiate','POST',current,workspace=b['workspace'],token=b['token']),404)
    check('concurrent instances serialize across Gateways, identity conflicts preserve existing Tasks, invalid inputs/bindings roll back and resources stay workspace-scoped')

    # A successful root is not rerun when a generated sibling is manually recovered.
    retry_def=copy.deepcopy(definition);retry_def['id']='template-recovery-'+key
    retry_def['task']['dag']['nodes'][0]['node']['prompt']['parts']=[dict(template='{"text":[[ repo.text | tojson ]],"retry_fixture":"'+key+'-[[ repo.key ]]","fail_first":{% if repo.key == "beta" %}1{% else %}0{% endif %}}')]
    retry_path='/task-templates/'+retry_def['id'];api(retry_path,'PUT',dict(definition=retry_def,expected_revision=None))
    recovered=api(retry_path+'/instantiate','POST',dict(request,run_now=True));failed=finish(recovered['run_id']);assert failed['state']=='failed',failed
    nodes={n['id']:n for n in failed['nodes']};alpha=nodes['review__alpha']['execution_id']
    # Editing both the template and Task must not change same-Run recovery.
    generated=api('/tasks/'+recovered['task_id']);changed=copy.deepcopy(generated['config']);changed['dag']['nodes'][0]['prompt']={'parts':[dict(text='edited future Run')]}
    api('/tasks/'+generated['id'],'PUT',dict(config=changed,expected_revision=generated['revision']))
    api(retry_path+'/archive','POST',dict(expected_revision=1))
    api('/runs/'+recovered['run_id']+'/nodes/review__beta/retry','POST',dict(execution_id=nodes['review__beta']['execution_id'],resume_stopped=False))
    success=finish(recovered['run_id']);assert success['state']=='succeeded',success
    assert next(n for n in success['nodes'] if n['id']=='review__alpha')['execution_id']==alpha
    assert success['result']['outputs'][0]['result']=='Reports:\n# Alpha\n---\n# Beta'
    check('generated graph recovery preserves successful execution IDs and uses the frozen graph even after Task edits and template archival')

    # Permission is rechecked even for an already committed key.
    invite=api('/invites','POST',dict(role='operator'))['token']
    username='template-user-'+key[:8]
    context['global_api']('/invites/accept','POST',dict(token=invite,username=username,password=env['WAKEIT_PASSWORD']),token=None)
    token=context['global_api']('/session','POST',dict(username=username,password=env['WAKEIT_PASSWORD']))['token']
    actor=context['global_api']('/session',token=token)['id']
    viewer_request=dict(current,task_id='permission-'+key)
    api(path+'/instantiate','POST',viewer_request,token=token,key='permission-'+key)
    api('/members/'+actor,'PUT',dict(role='viewer'))
    assert api(path,token=token)['revision']==4
    expect_status(lambda:api(path+'/instantiate','POST',viewer_request,token=token,key='permission-'+key),403)
    expect_status(lambda:api(path,'PUT',dict(definition=modified,expected_revision=4),token=token),403)
    check('Viewer can preview/read but cannot edit or instantiate; revoked write permission also blocks operation replay')

    # CLI typed documents, no session cloning and due/future schedule semantics.
    inputs=context['root']/'template-values.yaml';inputs.write_text('values:\n  repositories:\n    - {key: cli, text: CLI report}\n  summarize: false\nunset: [note]\n')
    definition_file=context['root']/'template-definition.json';definition_file.write_text(json.dumps(definition))
    offline=dict(env,HOME=str(context['root']/'offline-home'),XDG_CONFIG_HOME=str(context['root']/'offline-home'))
    output=json.loads(command([binary,'template','validate','--file',definition_file],offline));assert output['valid']
    cli=json.loads(command([binary,'--workspace',a['workspace'],'--json','task','create','--template',definition['id'],'--inputs-file',inputs,'--expect','4'],env))
    cli_task=api('/tasks/'+cli['task_id']);assert cli_task['config']['dag']['results']==['review__cli']
    assert 'note' not in cli_task['template_origin']['resolved_inputs']
    converted=api('/tasks/'+task_id+'/template');assert converted['task']['dag']['nodes'][2]['prompt']['parts'][1]['result']['node']=='review__alpha'
    now=datetime.datetime.now(datetime.timezone.utc)
    for delta in [-120,3600]:
        scheduled=dict(current,task_id='at-'+str(delta)+'-'+key,run_now=True,schedule=dict(at=(now+datetime.timedelta(seconds=delta)).isoformat(),timezone=''))
        result=api(path+'/instantiate','POST',scheduled);created=api('/tasks/'+result['task_id'])
        assert (created['next_at'] is None)==(delta<0),created
        assert created['config']['schedule']['timezone']=='Asia/Shanghai'
        finish(result['run_id'])
    repeating=dict(current,task_id='repeating-'+key,run_now=True,schedule=dict(every='1h',timezone=''))
    result=api(path+'/instantiate','POST',repeating);created=api('/tasks/'+result['task_id'])
    finish(result['run_id'])
    assert datetime.datetime.fromisoformat(created['next_at'].replace('Z','+00:00'))>datetime.datetime.now(datetime.timezone.utc),created
    assert len(api('/runs?task='+result['task_id']))==1
    check('CLI preserves typed values/unset without login for local validation; task conversion retains references and create-and-run coalesces due one-time schedules while retaining repeat/future occurrences')
    return dict(template_id=definition['id'],task_id=task_id,run_id=run_id)
