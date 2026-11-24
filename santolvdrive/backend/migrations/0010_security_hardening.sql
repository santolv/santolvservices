CREATE TABLE workspace_publications (
 node_id uuid PRIMARY KEY,
 target text NOT NULL
);
CREATE TABLE login_failures (
 key text PRIMARY KEY,
 failures integer NOT NULL,
 next_attempt timestamptz NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX login_failures_expiry ON login_failures(updated_at);
