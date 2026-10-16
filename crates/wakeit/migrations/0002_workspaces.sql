CREATE TABLE workspaces (
 id uuid PRIMARY KEY,
 name text NOT NULL CHECK(length(btrim(name)) BETWEEN 1 AND 100),
 is_default boolean NOT NULL DEFAULT false,
 is_demo boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX workspaces_default ON workspaces(is_default) WHERE is_default;
CREATE UNIQUE INDEX workspaces_demo ON workspaces(is_demo) WHERE is_demo;
INSERT INTO workspaces(id,name,is_default) VALUES(gen_random_uuid(),'Default',true);

ALTER TABLE users ADD COLUMN instance_admin boolean NOT NULL DEFAULT false;
ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;
CREATE TABLE user_identities (
 provider text NOT NULL CHECK(provider='github'), subject text NOT NULL,
 user_id uuid NOT NULL REFERENCES users(id), PRIMARY KEY(provider,subject),
 UNIQUE(provider,user_id)
);
CREATE TABLE workspace_members (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 user_id uuid NOT NULL REFERENCES users(id),
 role text NOT NULL CHECK(role IN ('admin','operator','viewer')),
 active boolean NOT NULL DEFAULT true, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(workspace_id,user_id)
);
INSERT INTO workspace_members(workspace_id,user_id,role)
 SELECT w.id,u.id,u.role FROM workspaces w CROSS JOIN users u WHERE w.is_default;

ALTER TABLE credentials ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE credentials SET workspace_id=(SELECT id FROM workspaces WHERE is_default) WHERE kind='api';
ALTER TABLE credentials ADD CONSTRAINT credentials_scope CHECK(
 (kind='session' AND workspace_id IS NULL) OR (kind='api' AND workspace_id IS NOT NULL)
);
CREATE INDEX credentials_workspace_user ON credentials(workspace_id,user_id);
ALTER TABLE invites ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
ALTER TABLE invites ADD COLUMN created_by uuid REFERENCES users(id);
UPDATE invites SET workspace_id=(SELECT id FROM workspaces WHERE is_default),expires_at=LEAST(expires_at,now());
ALTER TABLE invites ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE enrollments ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE enrollments SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE enrollments ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE runners ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE runners SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE runners ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE runners DROP CONSTRAINT runners_name_key;
ALTER TABLE runners ADD UNIQUE(workspace_id,name), ADD UNIQUE(workspace_id,id);

ALTER TABLE tasks ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE tasks SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE tasks ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE runs ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE runs SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE runs ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE runs DROP CONSTRAINT runs_task_id_fkey;
ALTER TABLE tasks DROP CONSTRAINT tasks_pkey;
ALTER TABLE tasks ADD PRIMARY KEY(workspace_id,id);
ALTER TABLE tasks DROP CONSTRAINT tasks_runner_id_fkey;
ALTER TABLE tasks ADD FOREIGN KEY(workspace_id,runner_id) REFERENCES runners(workspace_id,id);
ALTER TABLE runs DROP CONSTRAINT runs_runner_id_fkey;
ALTER TABLE runs DROP CONSTRAINT runs_retry_of_fkey;
ALTER TABLE runs ADD UNIQUE(workspace_id,id);
ALTER TABLE runs ADD FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,id);
ALTER TABLE runs ADD FOREIGN KEY(workspace_id,runner_id) REFERENCES runners(workspace_id,id);
ALTER TABLE runs ADD FOREIGN KEY(workspace_id,retry_of) REFERENCES runs(workspace_id,id) ON DELETE SET NULL (retry_of);
ALTER TABLE runs DROP CONSTRAINT runs_request_key_key;
ALTER TABLE runs ADD UNIQUE(workspace_id,request_key);
DROP INDEX runs_occurrence;
CREATE UNIQUE INDEX runs_occurrence ON runs(workspace_id,task_id,scheduled_at) WHERE source='schedule';
DROP INDEX runs_task_active;
CREATE INDEX runs_task_active ON runs(workspace_id,task_id,state);
CREATE INDEX runs_workspace_created ON runs(workspace_id,created_at DESC,id DESC);

ALTER TABLE events ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE events SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE events ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE events DROP CONSTRAINT events_run_id_fkey;
ALTER TABLE events ADD FOREIGN KEY(workspace_id,run_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE;
ALTER TABLE outbox ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE outbox SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE outbox ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE audit ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE audit SET workspace_id=(SELECT id FROM workspaces WHERE is_default);

ALTER TABLE settings ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE settings SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE settings DROP CONSTRAINT settings_pkey;
ALTER TABLE settings DROP COLUMN id;
ALTER TABLE settings ADD PRIMARY KEY(workspace_id);

DELETE FROM device_grants;
ALTER TABLE device_grants ADD COLUMN workspace_id uuid NOT NULL REFERENCES workspaces(id);
ALTER TABLE device_registrations ADD COLUMN workspace_id uuid REFERENCES workspaces(id);
UPDATE device_registrations SET workspace_id=(SELECT id FROM workspaces WHERE is_default);
ALTER TABLE device_registrations ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE device_registrations ADD FOREIGN KEY(workspace_id,runner_id) REFERENCES runners(workspace_id,id);

CREATE TABLE github_login_transactions (
 state_hash text PRIMARY KEY, browser_hash text NOT NULL, verifier text NOT NULL,
 return_to text NOT NULL, expires_at timestamptz NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
