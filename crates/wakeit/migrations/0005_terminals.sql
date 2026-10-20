CREATE TABLE terminals (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    run_id uuid NOT NULL,
    runner_id uuid NOT NULL,
    runtime_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users(id),
    credential text NOT NULL,
    request_id uuid NOT NULL,
    agent text NOT NULL,
    session_id text NOT NULL,
    state text NOT NULL DEFAULT 'created' CHECK (state IN ('created','starting','running','closing','closed','unknown')),
    generation bigint NOT NULL DEFAULT 0,
    attached boolean NOT NULL DEFAULT false,
    lease_until timestamptz NOT NULL DEFAULT now() + interval '30 seconds',
    ticket_hash text,
    ticket_expires timestamptz,
    reason text,
    exit_code integer,
    created_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    FOREIGN KEY (workspace_id,run_id) REFERENCES runs(workspace_id,id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id,runner_id) REFERENCES runners(workspace_id,id),
    UNIQUE (workspace_id,user_id,request_id)
);
CREATE UNIQUE INDEX terminals_active_session ON terminals(runner_id,agent,session_id) WHERE state <> 'closed';
CREATE INDEX terminals_workspace ON terminals(workspace_id,id);
