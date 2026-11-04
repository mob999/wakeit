-- Device extension instances, durable business events, hooks and management operations.
CREATE TABLE plugin_catalogs (
 runner_id uuid PRIMARY KEY REFERENCES runners(id) ON DELETE CASCADE,
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 catalog jsonb NOT NULL DEFAULT '{}', updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE plugin_instances (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 runner_id uuid NOT NULL REFERENCES runners(id), source text NOT NULL,
 extension text NOT NULL, name text NOT NULL, version text NOT NULL,
 enabled boolean NOT NULL DEFAULT false, state text NOT NULL DEFAULT 'disabled',
 revision bigint NOT NULL DEFAULT 1, grants jsonb NOT NULL DEFAULT '[]',
 declaration jsonb NOT NULL, managed_by uuid NOT NULL REFERENCES users(id),
 updated_at timestamptz NOT NULL DEFAULT now(), UNIQUE(workspace_id,runner_id,id)
);
CREATE TABLE plugin_bindings (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 instance_id uuid NOT NULL REFERENCES plugin_instances(id), runner_id uuid NOT NULL REFERENCES runners(id),
 kind text NOT NULL CHECK(kind IN ('event','hook')), event text NOT NULL, handler text NOT NULL,
 enabled boolean NOT NULL DEFAULT true, filter jsonb NOT NULL DEFAULT '{}',
 position integer NOT NULL DEFAULT 0, failure_policy text NOT NULL DEFAULT 'reject' CHECK(failure_policy IN ('reject','wait','continue')),
 timeout_seconds integer NOT NULL DEFAULT 30 CHECK(timeout_seconds BETWEEN 1 AND 3600),
 max_attempts integer NOT NULL DEFAULT 5 CHECK(max_attempts BETWEEN 1 AND 100),
 expires_seconds integer NOT NULL DEFAULT 604800 CHECK(expires_seconds BETWEEN 1 AND 7776000),
 revision bigint NOT NULL DEFAULT 1, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE plugin_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), workspace_id uuid NOT NULL REFERENCES workspaces(id),
 event_type text NOT NULL, schema_version integer NOT NULL DEFAULT 1,
 aggregate_type text NOT NULL, aggregate_id text NOT NULL, aggregate_revision bigint NOT NULL,
 source_runner_id uuid, operation_id uuid, correlation_id uuid, causation_id uuid,
 origin jsonb NOT NULL DEFAULT '{"kind":"system","id":null,"roles":[]}',
 occurred_at timestamptz NOT NULL DEFAULT now(), committed_at timestamptz NOT NULL DEFAULT now(),
 data jsonb NOT NULL, routed boolean NOT NULL DEFAULT false,
 expires_at timestamptz NOT NULL DEFAULT now()+interval '90 days'
);
CREATE INDEX plugin_events_route ON plugin_events(committed_at,id) WHERE NOT routed;
CREATE INDEX plugin_events_workspace ON plugin_events(workspace_id,committed_at DESC,id);
CREATE TABLE plugin_aggregate_revisions (
 workspace_id uuid NOT NULL REFERENCES workspaces(id), aggregate_type text NOT NULL,aggregate_id text NOT NULL,
 revision bigint NOT NULL DEFAULT 0, PRIMARY KEY(workspace_id,aggregate_type,aggregate_id)
);
CREATE TABLE plugin_operations (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 runner_id uuid REFERENCES runners(id), instance_id uuid REFERENCES plugin_instances(id),
 kind text NOT NULL, handler text, request_key text NOT NULL,
 actor_id uuid REFERENCES users(id), state text NOT NULL DEFAULT 'pending', revision bigint NOT NULL DEFAULT 1,
 request jsonb NOT NULL, result jsonb, error text, attempt integer NOT NULL DEFAULT 0,
 locked_until timestamptz, available_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '1 day',
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(workspace_id,request_key)
);
CREATE INDEX plugin_operations_pending ON plugin_operations(runner_id,available_at) WHERE state IN ('pending','retry_wait','running');
CREATE TABLE plugin_deliveries (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), workspace_id uuid NOT NULL REFERENCES workspaces(id),
 event_id uuid NOT NULL REFERENCES plugin_events(id) ON DELETE CASCADE,
 binding_id uuid NOT NULL REFERENCES plugin_bindings(id), instance_id uuid NOT NULL REFERENCES plugin_instances(id),
 runner_id uuid NOT NULL REFERENCES runners(id), replay_id uuid,
 invocation_id uuid NOT NULL DEFAULT gen_random_uuid(), state text NOT NULL DEFAULT 'pending',
 attempt integer NOT NULL DEFAULT 0, available_at timestamptz NOT NULL DEFAULT now(),
 locked_until timestamptz, expires_at timestamptz NOT NULL,
 result jsonb,error text,context jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX plugin_delivery_identity ON plugin_deliveries(binding_id,event_id) WHERE replay_id IS NULL;
CREATE INDEX plugin_delivery_ready ON plugin_deliveries(runner_id,available_at) WHERE state IN ('pending','retry_wait','running');
CREATE TABLE plugin_hook_calls (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id), operation_id uuid NOT NULL,
 binding_id uuid NOT NULL REFERENCES plugin_bindings(id), input_digest text NOT NULL,
 binding_revision bigint NOT NULL, instance_revision bigint NOT NULL,
 proposal jsonb NOT NULL, decision jsonb, state text NOT NULL DEFAULT 'pending', error text,
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(operation_id,binding_id,input_digest)
);
CREATE TABLE plugin_api_tokens (
 hash text PRIMARY KEY, instance_id uuid NOT NULL REFERENCES plugin_instances(id),
 actor_id uuid NOT NULL REFERENCES users(id), workspace_id uuid NOT NULL REFERENCES workspaces(id),
 invocation_id uuid,causation_id uuid,causation_depth integer NOT NULL DEFAULT 0,
 expires_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE plugin_artifacts (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 instance_id uuid NOT NULL REFERENCES plugin_instances(id), runner_id uuid NOT NULL REFERENCES runners(id),
 run_id uuid REFERENCES runs(id) ON DELETE SET NULL, name text NOT NULL, format text NOT NULL,
 reference jsonb NOT NULL, metadata jsonb NOT NULL DEFAULT '{}',
 created_at timestamptz NOT NULL DEFAULT now(), expires_at timestamptz
);

CREATE FUNCTION plugin_append_event(ws uuid, typ text, aggregate_kind text, aggregate_key text, payload jsonb, device uuid DEFAULT NULL) RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE eid uuid; rev bigint;
BEGIN
 INSERT INTO plugin_aggregate_revisions(workspace_id,aggregate_type,aggregate_id,revision)
 VALUES(ws,aggregate_kind,aggregate_key,1)
 ON CONFLICT(workspace_id,aggregate_type,aggregate_id) DO UPDATE SET revision=plugin_aggregate_revisions.revision+1 RETURNING revision INTO rev;
 INSERT INTO plugin_events(workspace_id,event_type,aggregate_type,aggregate_id,aggregate_revision,data,source_runner_id,origin,causation_id,expires_at)
 VALUES(ws,typ,aggregate_kind,aggregate_key,rev,payload||jsonb_build_object('_causation_depth',COALESCE(NULLIF(current_setting('wakeit.causation_depth',true),'')::integer,0)),device,COALESCE(NULLIF(current_setting('wakeit.actor',true),'')::jsonb,'{"kind":"system","id":null,"roles":[]}'::jsonb),NULLIF(current_setting('wakeit.causation_id',true),'')::uuid,now()+make_interval(days=>COALESCE((SELECT (data->>'run_retention_days')::integer FROM settings WHERE workspace_id=ws),90))) RETURNING id INTO eid;
 RETURN eid;
END $$;

ALTER TABLE runs ADD COLUMN plugin_origin jsonb,ADD COLUMN plugin_causation_id uuid,ADD COLUMN plugin_causation_depth integer NOT NULL DEFAULT 0;
CREATE FUNCTION plugin_run_context() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 NEW.plugin_origin:=NULLIF(current_setting('wakeit.actor',true),'')::jsonb;
 NEW.plugin_causation_id:=NULLIF(current_setting('wakeit.causation_id',true),'')::uuid;
 NEW.plugin_causation_depth:=COALESCE(NULLIF(current_setting('wakeit.causation_depth',true),'')::integer,0);
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_run_context BEFORE INSERT ON runs FOR EACH ROW EXECUTE FUNCTION plugin_run_context();

-- Every state writer participates, including scheduler skips, quarantine, cancellation and recovery.
CREATE FUNCTION plugin_run_changes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE typ text; child boolean; payload jsonb;
BEGIN
 child:=NEW.parent_run_id IS NOT NULL;
 IF NEW.plugin_causation_id IS NOT NULL THEN
  PERFORM set_config('wakeit.causation_id',NEW.plugin_causation_id::text,true);
  PERFORM set_config('wakeit.causation_depth',NEW.plugin_causation_depth::text,true);
 END IF;

 payload:=jsonb_build_object('run_id',NEW.id,'task_id',NEW.task_id,'task_revision',NEW.task_revision,
 'parent_run_id',NEW.parent_run_id,'node_id',NEW.node_id,'state',NEW.state,'reason',NEW.reason,
 'source',NEW.source,'runner_id',NEW.runner_id,'name',NEW.config->>'name',
 'started_at',NEW.started_at,'finished_at',NEW.finished_at,'retry_of',NEW.retry_of,
 'recovery_generation',NEW.recovery_generation);
 IF TG_OP='INSERT' THEN
  PERFORM plugin_append_event(NEW.workspace_id,CASE WHEN child THEN 'dag.node.attempt_created' ELSE 'run.created' END,'run',NEW.id::text,payload,NEW.runner_id);
  IF NEW.source='schedule' THEN PERFORM plugin_append_event(NEW.workspace_id,'schedule.occurrence_created','run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF NEW.source='schedule' THEN PERFORM plugin_append_event(NEW.workspace_id,'schedule.evaluated','run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF NEW.retry_of IS NOT NULL THEN PERFORM plugin_append_event(NEW.workspace_id,CASE WHEN child THEN 'dag.node.retry_scheduled' ELSE 'run.retry_requested' END,'run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF NEW.state='queued' AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'run.queued','run',NEW.id::text,payload,NEW.runner_id); END IF;
 ELSE
  IF OLD.state IS DISTINCT FROM NEW.state THEN
   PERFORM plugin_append_event(NEW.workspace_id,CASE WHEN child THEN 'dag.node.state_changed' ELSE 'run.state_changed' END,'run',NEW.id::text,payload,NEW.runner_id);
   IF NEW.state='unknown' AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'run.uncertain','run',NEW.id::text,payload,NEW.runner_id); END IF;
   IF OLD.state='unknown' AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'run.reconciled','run',NEW.id::text,payload,NEW.runner_id); END IF;
  END IF;
  IF OLD.started_at IS NULL AND NEW.started_at IS NOT NULL AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'run.started','run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF NOT OLD.cancel_requested AND NEW.cancel_requested THEN PERFORM plugin_append_event(NEW.workspace_id,'run.cancel_requested','run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF OLD.finished_at IS NOT NULL AND NEW.finished_at IS NULL AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'run.reopened','run',NEW.id::text,payload,NEW.runner_id); END IF;
  IF NEW.recovery_generation IS DISTINCT FROM OLD.recovery_generation AND NOT child THEN PERFORM plugin_append_event(NEW.workspace_id,'dag.recovered','run',NEW.id::text,payload,NEW.runner_id); END IF;
 END IF;
 IF NEW.finished_at IS NOT NULL AND (TG_OP='INSERT' OR OLD.finished_at IS NULL) THEN
  payload:=payload||jsonb_build_object('result',NEW.result,'format',COALESCE(NEW.execution_snapshot->>'format','text'),'config',NEW.config);
  PERFORM plugin_append_event(NEW.workspace_id,CASE WHEN child THEN 'dag.node.attempt_finished' ELSE 'run.finished' END,'run',NEW.id::text,payload,NEW.runner_id);
 END IF;
 IF TG_OP='UPDATE' AND NEW.session IS DISTINCT FROM OLD.session AND NEW.session IS NOT NULL THEN
  PERFORM plugin_append_event(NEW.workspace_id,CASE WHEN OLD.session IS NULL THEN 'session.bound' ELSE 'session.availability_changed' END,'run',NEW.id::text,jsonb_build_object('run_id',NEW.id,'session',NEW.session-'command'),NEW.runner_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_run_changes AFTER INSERT OR UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION plugin_run_changes();

CREATE FUNCTION plugin_definition_changes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ws uuid; kind text; key text; payload jsonb; old_payload jsonb;
BEGIN
 payload:=to_jsonb(NEW); old_payload:=CASE WHEN TG_OP='UPDATE' THEN to_jsonb(OLD) ELSE '{}'::jsonb END;
 ws:=(payload->>'workspace_id')::uuid; key:=payload->>'id'; kind:=CASE WHEN TG_TABLE_NAME='tasks' THEN 'task' ELSE 'template' END;
 IF TG_OP='INSERT' OR payload->'revision' IS DISTINCT FROM old_payload->'revision' THEN
  PERFORM plugin_append_event(ws,kind||CASE WHEN TG_OP='INSERT' THEN '.created' ELSE '.updated' END,kind,key,payload-ARRAY['created_by','updated_by']);
 END IF;
 IF TG_OP='UPDATE' AND payload->'archived_at' IS DISTINCT FROM old_payload->'archived_at' THEN PERFORM plugin_append_event(ws,kind||CASE WHEN NEW.archived_at IS NULL THEN '.restored' ELSE '.archived' END,kind,key,jsonb_build_object('id',key,'revision',NEW.revision)); END IF;
 IF kind='task' AND TG_OP='INSERT' AND payload->'template_origin' IS NOT NULL AND payload->'template_origin'<>'null'::jsonb THEN PERFORM plugin_append_event(ws,'template.instantiated','task',key,jsonb_build_object('task_id',key,'template_origin',payload->'template_origin')); END IF;
 IF kind='task' AND TG_OP='UPDATE' THEN
  IF payload->'enabled' IS DISTINCT FROM old_payload->'enabled' THEN PERFORM plugin_append_event(ws,'task.enabled_changed',kind,key,jsonb_build_object('id',key,'enabled',payload->'enabled')); END IF;
  IF payload#>'{config,schedule}' IS DISTINCT FROM old_payload#>'{config,schedule}' THEN PERFORM plugin_append_event(ws,'task.schedule_changed',kind,key,jsonb_build_object('id',key,'schedule',payload#>'{config,schedule}')); END IF;
  IF payload->'next_at' IS DISTINCT FROM old_payload->'next_at' THEN PERFORM plugin_append_event(ws,'schedule.next_changed',kind,key,jsonb_build_object('id',key,'next_at',payload->'next_at')); END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_task_changes AFTER INSERT OR UPDATE ON tasks FOR EACH ROW WHEN (pg_trigger_depth()=0) EXECUTE FUNCTION plugin_definition_changes();
CREATE TRIGGER plugin_template_changes AFTER INSERT OR UPDATE ON task_templates FOR EACH ROW EXECUTE FUNCTION plugin_definition_changes();

CREATE FUNCTION plugin_device_changes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE payload jsonb;
BEGIN
 payload:=jsonb_build_object('id',NEW.id,'name',NEW.name,'revoked',NEW.revoked,'capabilities',NEW.agents);
 IF TG_OP='INSERT' THEN PERFORM plugin_append_event(NEW.workspace_id,'device.enrolled','device',NEW.id::text,payload,NEW.id);
 ELSE
  IF NOT OLD.revoked AND NEW.revoked THEN PERFORM plugin_append_event(NEW.workspace_id,'device.revoked','device',NEW.id::text,payload,NEW.id); END IF;
  IF NEW.agents IS DISTINCT FROM OLD.agents THEN PERFORM plugin_append_event(NEW.workspace_id,'device.capabilities_changed','device',NEW.id::text,payload,NEW.id); END IF;

 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_device_changes AFTER INSERT OR UPDATE ON runners FOR EACH ROW EXECUTE FUNCTION plugin_device_changes();

CREATE FUNCTION plugin_workspace_changes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE payload jsonb; ws uuid; key text; typ text;
BEGIN
 payload:=CASE WHEN TG_OP='DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
 IF TG_TABLE_NAME='workspaces' THEN
  ws:=(payload->>'id')::uuid; key:=ws::text;
  typ:=CASE WHEN TG_OP='INSERT' THEN 'workspace.created' ELSE 'workspace.settings_changed' END;
 ELSIF TG_TABLE_NAME='settings' THEN
  ws:=(payload->>'workspace_id')::uuid;key:=ws::text;typ:='workspace.settings_changed';
 ELSE
  ws:=(payload->>'workspace_id')::uuid;key:=payload->>'user_id';
  typ:=CASE WHEN TG_OP='INSERT' THEN 'member.joined' WHEN TG_OP='DELETE' THEN 'member.left' ELSE 'member.role_changed' END;
 END IF;
 IF TG_OP='UPDATE' AND to_jsonb(NEW)=to_jsonb(OLD) THEN RETURN NEW; END IF;
 PERFORM plugin_append_event(ws,typ,CASE WHEN TG_TABLE_NAME='workspace_members' THEN 'member' ELSE 'workspace' END,key,payload);
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;RETURN NEW;
END $$;
CREATE TRIGGER plugin_workspace_changes AFTER INSERT OR UPDATE ON workspaces FOR EACH ROW EXECUTE FUNCTION plugin_workspace_changes();
CREATE TRIGGER plugin_settings_changes AFTER UPDATE ON settings FOR EACH ROW EXECUTE FUNCTION plugin_workspace_changes();
CREATE TRIGGER plugin_member_changes AFTER INSERT OR UPDATE OR DELETE ON workspace_members FOR EACH ROW EXECUTE FUNCTION plugin_workspace_changes();

CREATE FUNCTION plugin_dag_changes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ws uuid;payload jsonb;
BEGIN
 ws:=NEW.workspace_id;
 payload:=jsonb_build_object('run_id',NEW.parent_run_id,'node_id',NEW.node_id,'state',NEW.state,'execution_id',NEW.execution_id,'reason',NEW.reason);
 IF TG_OP='INSERT' OR OLD.state IS DISTINCT FROM NEW.state THEN
  PERFORM plugin_append_event(ws,'dag.node.state_changed','dag_node',NEW.parent_run_id::text||':'||NEW.node_id,payload);
  IF NEW.state='queued' THEN PERFORM plugin_append_event(ws,'dag.node.ready','dag_node',NEW.parent_run_id::text||':'||NEW.node_id,payload); END IF;
  IF NEW.finished_at IS NOT NULL THEN PERFORM plugin_append_event(ws,'dag.node.finished','dag_node',NEW.parent_run_id::text||':'||NEW.node_id,payload); END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_dag_changes AFTER INSERT OR UPDATE ON dag_run_nodes FOR EACH ROW EXECUTE FUNCTION plugin_dag_changes();
CREATE TABLE plugin_auth_flows (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 runner_id uuid NOT NULL REFERENCES runners(id),instance_id uuid NOT NULL REFERENCES plugin_instances(id),
 state_hash text NOT NULL UNIQUE,status text NOT NULL DEFAULT 'pending',expires_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX plugin_delivery_replay_identity ON plugin_deliveries(replay_id,binding_id,event_id) WHERE replay_id IS NOT NULL;

CREATE TABLE plugin_device_presence(runner_id uuid PRIMARY KEY REFERENCES runners(id) ON DELETE CASCADE,online boolean NOT NULL,updated_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE plugin_operation_tokens (
 hash text PRIMARY KEY, operation_id uuid NOT NULL REFERENCES plugin_operations(id) ON DELETE CASCADE,
 actor_id uuid NOT NULL REFERENCES users(id),workspace_id uuid NOT NULL REFERENCES workspaces(id),expires_at timestamptz NOT NULL
);
CREATE FUNCTION plugin_complete_business_operation() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE op uuid; payload jsonb;
BEGIN
 op:=NULLIF(current_setting('wakeit.operation_id',true),'')::uuid;
 IF op IS NULL THEN RETURN NEW; END IF;
 IF EXISTS(SELECT 1 FROM plugin_operations WHERE id=op AND request->>'path' LIKE '%/instantiate') THEN RETURN NEW; END IF;
 payload:=CASE WHEN TG_TABLE_NAME='settings' THEN to_jsonb(NEW)->'data' ELSE to_jsonb(NEW) END;
 UPDATE plugin_operations SET state='committed',result=payload,revision=revision+1,updated_at=now(),locked_until=NULL WHERE id=op AND kind='business' AND state='running';
 RETURN NEW;
END $$;
CREATE TRIGGER plugin_complete_task_operation AFTER INSERT OR UPDATE ON tasks FOR EACH ROW EXECUTE FUNCTION plugin_complete_business_operation();
CREATE TRIGGER plugin_complete_template_operation AFTER INSERT OR UPDATE ON task_templates FOR EACH ROW EXECUTE FUNCTION plugin_complete_business_operation();
CREATE TRIGGER plugin_complete_settings_operation AFTER UPDATE ON settings FOR EACH ROW EXECUTE FUNCTION plugin_complete_business_operation();
