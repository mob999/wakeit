-- Historical attempts remain child Runs, with at most one unsettled attempt per node.
DROP INDEX runs_parent_node;
CREATE UNIQUE INDEX runs_parent_node_active ON runs(parent_run_id,node_id)
 WHERE parent_run_id IS NOT NULL AND finished_at IS NULL;
CREATE INDEX runs_node_attempts ON runs(parent_run_id,node_id,created_at,id)
 WHERE parent_run_id IS NOT NULL;
CREATE UNIQUE INDEX dag_retry_successor ON runs(retry_of) WHERE parent_run_id IS NOT NULL AND retry_of IS NOT NULL;
ALTER TABLE runs ADD CONSTRAINT dag_retry_owner FOREIGN KEY(workspace_id,retry_of,parent_run_id,node_id)
 REFERENCES runs(workspace_id,id,parent_run_id,node_id);
