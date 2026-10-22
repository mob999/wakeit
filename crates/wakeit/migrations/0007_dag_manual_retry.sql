-- Recovery keeps original Run/attempt identities and uses a separate active clock.
ALTER TABLE runs ADD COLUMN attempt_origin text NOT NULL DEFAULT 'initial'
 CHECK(attempt_origin IN ('initial','automatic','manual','continuation'));
ALTER TABLE runs ADD COLUMN recovery_generation bigint NOT NULL DEFAULT 0 CHECK(recovery_generation>=0);
ALTER TABLE runs ADD COLUMN window_started_at timestamptz;
ALTER TABLE runs ADD COLUMN stop_cause text CHECK(stop_cause IN ('dependency','graph_cancel','graph_timeout'));
ALTER TABLE dag_run_nodes ADD COLUMN stop_cause text CHECK(stop_cause IN ('dependency','graph_cancel','graph_timeout'));
UPDATE runs SET attempt_origin='automatic' WHERE parent_run_id IS NOT NULL AND retry_of IS NOT NULL;
UPDATE runs SET stop_cause=CASE WHEN state='timed_out' THEN 'graph_timeout' ELSE 'graph_cancel' END WHERE parent_run_id IS NULL AND config ? 'dag' AND cancel_requested;
UPDATE runs SET window_started_at=started_at WHERE parent_run_id IS NULL AND config ? 'dag';
UPDATE dag_run_nodes n SET stop_cause=CASE WHEN p.state='timed_out' THEN 'graph_timeout' ELSE 'graph_cancel' END
 FROM runs p WHERE p.id=n.parent_run_id AND p.cancel_requested AND p.state IN ('cancelled','timed_out') AND n.state IN ('cancelled','skipped');
UPDATE runs r SET stop_cause=n.stop_cause FROM dag_run_nodes n WHERE n.execution_id=r.id AND n.stop_cause IS NOT NULL;
UPDATE dag_run_nodes n SET stop_cause='dependency' WHERE n.state='skipped' AND n.stop_cause IS NULL AND n.execution_id IS NULL
 AND EXISTS(SELECT 1 FROM dag_run_nodes p WHERE p.parent_run_id=n.parent_run_id AND n.config->'pre' ? p.node_id AND p.state IN ('failed','timed_out','interrupted','cancelled','skipped'));

-- Separate request identities also cover acceleration of an existing automatic attempt.
CREATE TABLE dag_retry_requests (
 workspace_id uuid NOT NULL, user_id uuid NOT NULL REFERENCES users(id),
 parent_run_id uuid NOT NULL, node_id text NOT NULL, request_key text NOT NULL,
 expected_execution uuid, resume_stopped boolean NOT NULL, execution_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,user_id,parent_run_id,node_id,request_key),
 FOREIGN KEY(workspace_id,parent_run_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE,
 FOREIGN KEY(workspace_id,execution_id,parent_run_id,node_id) REFERENCES runs(workspace_id,id,parent_run_id,node_id) ON DELETE CASCADE
);
