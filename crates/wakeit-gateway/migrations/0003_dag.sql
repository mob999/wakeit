-- Graph rounds never borrow a node's Runner identity.
ALTER TABLE tasks ALTER COLUMN runner_id DROP NOT NULL;
ALTER TABLE runs ALTER COLUMN runner_id DROP NOT NULL;
ALTER TABLE runs ADD COLUMN parent_run_id uuid;
ALTER TABLE runs ADD COLUMN node_id text;
ALTER TABLE runs ADD CONSTRAINT runs_parent_workspace_fk FOREIGN KEY(workspace_id,parent_run_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE;
ALTER TABLE runs ADD CONSTRAINT runs_node_parent CHECK((parent_run_id IS NULL)=(node_id IS NULL));
ALTER TABLE runs ADD CONSTRAINT runs_execution_runner CHECK(runner_id IS NOT NULL OR (parent_run_id IS NULL AND config ? 'dag'));
ALTER TABLE tasks ADD CONSTRAINT tasks_execution_runner CHECK((runner_id IS NULL)=(config ? 'dag'));
CREATE UNIQUE INDEX runs_parent_node ON runs(parent_run_id,node_id) WHERE parent_run_id IS NOT NULL;
CREATE INDEX runs_parent_lookup ON runs(workspace_id,parent_run_id,id);
DROP INDEX runs_occurrence;
CREATE UNIQUE INDEX runs_occurrence ON runs(workspace_id,task_id,scheduled_at) WHERE source='schedule' AND parent_run_id IS NULL;

CREATE TABLE dag_nodes (
 workspace_id uuid NOT NULL, task_id text NOT NULL, node_id text NOT NULL, runner_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,task_id,node_id),
 FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,id) ON DELETE CASCADE,
 FOREIGN KEY(workspace_id,runner_id) REFERENCES runners(workspace_id,id)
);
CREATE TABLE dag_run_nodes (
 workspace_id uuid NOT NULL, parent_run_id uuid NOT NULL, node_id text NOT NULL,
 position integer NOT NULL, runner_id uuid NOT NULL, config jsonb NOT NULL,
 state text NOT NULL DEFAULT 'waiting', reason text, execution_id uuid,
 started_at timestamptz, finished_at timestamptz,
 PRIMARY KEY(parent_run_id,node_id),
 FOREIGN KEY(workspace_id,parent_run_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE,
 FOREIGN KEY(workspace_id,execution_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE,
 FOREIGN KEY(workspace_id,runner_id) REFERENCES runners(workspace_id,id)
);
CREATE INDEX dag_round_pending ON runs(created_at) WHERE config ? 'dag' AND finished_at IS NULL;

ALTER TABLE runs ADD UNIQUE(workspace_id,id,parent_run_id,node_id);
ALTER TABLE dag_run_nodes ADD CONSTRAINT dag_execution_owner FOREIGN KEY(workspace_id,execution_id,parent_run_id,node_id) REFERENCES runs(workspace_id,id,parent_run_id,node_id) ON DELETE CASCADE;
