CREATE TABLE task_templates (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 id text NOT NULL,
 revision bigint NOT NULL DEFAULT 1,
 definition jsonb NOT NULL,
 created_by uuid NOT NULL REFERENCES users(id),
 updated_by uuid NOT NULL REFERENCES users(id),
 archived_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (workspace_id,id)
);
CREATE INDEX task_templates_recent ON task_templates(workspace_id,updated_at DESC,id);
ALTER TABLE tasks ADD COLUMN template_origin jsonb;
ALTER TABLE runs ADD COLUMN template_origin jsonb;
CREATE TABLE template_operations (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 actor_id uuid NOT NULL REFERENCES users(id),
 operation text NOT NULL,
 request_key text NOT NULL,
 request_hash text NOT NULL,
 response jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(workspace_id,actor_id,operation,request_key)
);
