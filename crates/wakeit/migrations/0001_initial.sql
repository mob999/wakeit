CREATE TABLE users (
 id uuid PRIMARY KEY, username text NOT NULL UNIQUE, password_hash text NOT NULL,
 role text NOT NULL CHECK (role IN ('admin','operator','viewer')), active boolean NOT NULL DEFAULT true,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE credentials (
 hash text PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id), kind text NOT NULL,
 name text NOT NULL DEFAULT '', expires_at timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE invites (
 hash text PRIMARY KEY, role text NOT NULL CHECK(role IN ('admin','operator','viewer')),
 expires_at timestamptz NOT NULL, used_at timestamptz
);
CREATE TABLE enrollments (hash text PRIMARY KEY, expires_at timestamptz NOT NULL, used_at timestamptz);
CREATE TABLE runners (
 id uuid PRIMARY KEY, name text NOT NULL UNIQUE, token_hash text NOT NULL UNIQUE, public_key text NOT NULL UNIQUE,
 revoked boolean NOT NULL DEFAULT false, runtime_id uuid, last_seen timestamptz,
 agents jsonb NOT NULL DEFAULT '[]', directory_roots jsonb NOT NULL DEFAULT '[]', info jsonb NOT NULL DEFAULT '{}', created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE tasks (
 id text PRIMARY KEY, revision bigint NOT NULL DEFAULT 1, runner_id uuid NOT NULL REFERENCES runners(id),
 config jsonb NOT NULL, enabled boolean NOT NULL, next_at timestamptz,
 created_by uuid NOT NULL REFERENCES users(id), updated_by uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz
);
CREATE INDEX tasks_due ON tasks(next_at) WHERE enabled AND archived_at IS NULL;
CREATE TABLE runs (
 id uuid PRIMARY KEY, task_id text NOT NULL REFERENCES tasks(id), task_revision bigint NOT NULL,
 runner_id uuid NOT NULL REFERENCES runners(id), config jsonb NOT NULL,
 state text NOT NULL DEFAULT 'queued', reason text, source text NOT NULL,
 scheduled_at timestamptz NOT NULL, deadline timestamptz NOT NULL,
 claim_id uuid, runtime_id uuid, agent_digest text, job_id text,
 cancel_requested boolean NOT NULL DEFAULT false, result jsonb, execution_snapshot jsonb, last_state_seq bigint NOT NULL DEFAULT 0,
 session jsonb, session_seq bigint NOT NULL DEFAULT 0,
 created_at timestamptz NOT NULL DEFAULT now(), started_at timestamptz, finished_at timestamptz,
 last_seen timestamptz, retry_of uuid REFERENCES runs(id) ON DELETE SET NULL,
 request_key text UNIQUE, log_bytes bigint NOT NULL DEFAULT 0, log_truncated boolean NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX runs_occurrence ON runs(task_id,scheduled_at) WHERE source='schedule';
CREATE INDEX runs_task_active ON runs(task_id,state);
CREATE INDEX runs_created ON runs(created_at DESC);
CREATE INDEX runs_session_active ON runs(runner_id,(config->>'agent'),(COALESCE(session->>'id',config->'session'->>'id'))) WHERE state IN ('starting','running','unknown');
CREATE TABLE events (
 id bigserial PRIMARY KEY, run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
 seq bigint NOT NULL, kind text NOT NULL, data jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(run_id,seq)
);
CREATE INDEX events_run_cursor ON events(run_id,id);
CREATE TABLE outbox (
 id bigserial PRIMARY KEY, subject text NOT NULL, payload jsonb NOT NULL, message_id text NOT NULL UNIQUE,
 stream text, stream_seq bigint, cleared_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(), published_at timestamptz
);
CREATE INDEX outbox_pending ON outbox(id) WHERE published_at IS NULL;
CREATE TABLE audit (
 id bigserial PRIMARY KEY, user_id uuid REFERENCES users(id), action text NOT NULL, target text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE settings (id boolean PRIMARY KEY DEFAULT true CHECK(id), data jsonb NOT NULL);
INSERT INTO settings(data) VALUES ('{"timezone":"Asia/Shanghai","log_retention_days":7,"run_retention_days":90}');

CREATE TABLE login_attempts (username text NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
CREATE INDEX login_attempts_user_time ON login_attempts(username,created_at);

-- Browser-confirmed CLI grants; raw codes and credentials are never stored here.
CREATE TABLE device_grants (
 device_hash text PRIMARY KEY, user_hash text NOT NULL UNIQUE, intent jsonb NOT NULL,
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','approved','denied','consumed')),
 user_id uuid REFERENCES users(id), approval_credential text, api_hash text,
 expires_at timestamptz NOT NULL, last_poll timestamptz, poll_interval integer NOT NULL DEFAULT 5,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE device_auth_limits(key text NOT NULL,created_at timestamptz NOT NULL DEFAULT now());
CREATE INDEX device_auth_limits_time ON device_auth_limits(created_at);
CREATE TABLE device_registrations (
 request_id uuid PRIMARY KEY, runner_id uuid NOT NULL UNIQUE REFERENCES runners(id),
 public_key text NOT NULL UNIQUE, user_id uuid NOT NULL REFERENCES users(id)
);
