"""Exercise 0006 -> 0007 against synthetic old records in a disposable schema."""
from pathlib import Path
import uuid


def verify(sql, check):
    schema='retry_upgrade_'+uuid.uuid4().hex[:10]
    migrations=Path(__file__).resolve().parents[1]/'crates/wakeit/migrations'
    old='\n'.join(p.read_text() for p in sorted(migrations.glob('*.sql')) if p.name<'0007')
    user,runner,parent,failed,retry,other,other_failed=[str(uuid.uuid4()) for _ in range(7)]
    setup=f"""
    BEGIN; CREATE SCHEMA {schema}; SET LOCAL search_path TO {schema};
    {old}
    INSERT INTO users(id,username,password_hash,role) VALUES('{user}','upgrade-fixture','synthetic-password-hash','admin');
    INSERT INTO runners(workspace_id,id,name,token_hash,public_key)
      SELECT id,'{runner}','upgrade-device','synthetic-token','synthetic-key' FROM workspaces WHERE is_default;
    INSERT INTO credentials(hash,user_id,kind) VALUES('synthetic-session','{user}','session');
    INSERT INTO tasks(workspace_id,id,config,enabled,created_by,updated_by)
      SELECT id,'upgrade-graph','{{"dag":{{"nodes":[]}},"execution":{{"timeout":"1h"}}}}',true,'{user}','{user}' FROM workspaces WHERE is_default;
    INSERT INTO runs(workspace_id,id,task_id,task_revision,config,state,source,scheduled_at,deadline,started_at,finished_at,cancel_requested)
      SELECT id,'{parent}','upgrade-graph',1,'{{"dag":{{"nodes":[]}},"execution":{{"timeout":"1h"}}}}','cancelled','manual',now(),now(),now()-interval '2h',now(),true FROM workspaces WHERE is_default;
    INSERT INTO runs(workspace_id,id,task_id,task_revision,config,state,source,scheduled_at,deadline,started_at,finished_at,cancel_requested)
      SELECT id,'{other}','upgrade-graph',1,'{{"dag":{{"nodes":[]}}}}','failed','manual',now(),now(),now(),now(),false FROM workspaces WHERE is_default;
    INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,parent_run_id,node_id,finished_at)
      SELECT id,'{failed}','upgrade-graph',1,'{runner}','{{}}','failed','dag_node',now(),now(),'{parent}','root',now() FROM workspaces WHERE is_default;
    INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,parent_run_id,node_id,retry_of,finished_at)
      SELECT id,'{retry}','upgrade-graph',1,'{runner}','{{}}','cancelled','dag_node',now(),now(),'{parent}','root','{failed}',now() FROM workspaces WHERE is_default;
    INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,parent_run_id,node_id,finished_at)
      SELECT id,'{other_failed}','upgrade-graph',1,'{runner}','{{}}','failed','dag_node',now(),now(),'{other}','root',now() FROM workspaces WHERE is_default;
    INSERT INTO dag_run_nodes(workspace_id,parent_run_id,node_id,position,runner_id,config,state,execution_id)
      SELECT id,'{parent}','root',0,'{runner}','{{"pre":[]}}','cancelled','{retry}' FROM workspaces WHERE is_default;
    INSERT INTO dag_run_nodes(workspace_id,parent_run_id,node_id,position,runner_id,config,state)
      SELECT id,'{parent}','after',1,'{runner}','{{"pre":["root"]}}','cancelled' FROM workspaces WHERE is_default;
    INSERT INTO dag_run_nodes(workspace_id,parent_run_id,node_id,position,runner_id,config,state,execution_id)
      SELECT id,'{other}','root',0,'{runner}','{{"pre":[]}}','failed','{other_failed}' FROM workspaces WHERE is_default;
    INSERT INTO dag_run_nodes(workspace_id,parent_run_id,node_id,position,runner_id,config,state)
      SELECT id,'{other}','after',1,'{runner}','{{"pre":["root"]}}','skipped' FROM workspaces WHERE is_default;
    {(migrations/'0007_dag_manual_retry.sql').read_text()}
    DO $verify$ BEGIN
      ASSERT (SELECT attempt_origin='automatic' FROM runs WHERE id='{retry}');
      ASSERT (SELECT stop_cause='graph_cancel' AND window_started_at=started_at AND recovery_generation=0 AND config->'execution'->>'timeout'='1h' FROM runs WHERE id='{parent}');
      ASSERT (SELECT stop_cause='dependency' FROM dag_run_nodes WHERE parent_run_id='{other}' AND node_id='after');
      ASSERT (SELECT stop_cause='graph_cancel' FROM runs WHERE id='{retry}');
      ASSERT (SELECT token_hash='synthetic-token' AND public_key='synthetic-key' FROM runners WHERE id='{runner}');
      ASSERT (SELECT count(*)=1 FROM credentials WHERE hash='synthetic-session');
      ASSERT (SELECT count(*)=5 FROM runs);
    END $verify$;
    ROLLBACK;
    """
    sql(setup)
    check('0006 upgrade preserves IDs, explicit timeouts and credentials; backfills automatic attempts, windows and structured stop causes')
