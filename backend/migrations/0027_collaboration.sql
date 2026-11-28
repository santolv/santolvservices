-- Real-time collaboration engine: the durable state of each collaborative document (a Drive
-- file). The latest snapshot plus the updates after it rebuild the document after a restart.
CREATE SCHEMA collaboration;

CREATE TABLE collaboration.updates (
    seq bigserial PRIMARY KEY,
    node_id uuid NOT NULL REFERENCES public.nodes(id) ON DELETE CASCADE,
    -- The client's operation id: a retried operation is stored once.
    op_id uuid NOT NULL,
    user_id uuid REFERENCES public.users(id) ON DELETE SET NULL,
    payload bytea NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 262144),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (node_id, op_id)
);
CREATE INDEX collaboration_updates_node ON collaboration.updates(node_id, seq);

CREATE TABLE collaboration.snapshots (
    node_id uuid PRIMARY KEY REFERENCES public.nodes(id) ON DELETE CASCADE,
    seq bigint NOT NULL,
    state bytea NOT NULL CHECK (octet_length(state) <= 33554432),
    created_at timestamptz NOT NULL DEFAULT now()
);
