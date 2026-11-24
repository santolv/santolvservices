CREATE TABLE users (
 id uuid PRIMARY KEY, email text NOT NULL UNIQUE, password_hash text NOT NULL, verified boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE sessions (
 token_hash text PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id),
 csrf_hash text NOT NULL, expires_at timestamptz NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE spaces (
 id uuid PRIMARY KEY, name text NOT NULL, kind text NOT NULL CHECK(kind IN ('personal','organization')),
 owner_id uuid NOT NULL REFERENCES users(id), quota bigint NOT NULL CHECK(quota>0),
 used_bytes bigint NOT NULL DEFAULT 0 CHECK(used_bytes>=0), reserved_bytes bigint NOT NULL DEFAULT 0 CHECK(reserved_bytes>=0)
);
CREATE TABLE members (
 space_id uuid NOT NULL REFERENCES spaces(id), user_id uuid NOT NULL REFERENCES users(id),
 role smallint NOT NULL CHECK(role BETWEEN 1 AND 3), PRIMARY KEY(space_id,user_id)
);
CREATE TABLE nodes (
 id uuid PRIMARY KEY, space_id uuid NOT NULL REFERENCES spaces(id), parent_id uuid,
 name text NOT NULL, kind text NOT NULL CHECK(kind IN ('file','folder')),
 revision bigint NOT NULL DEFAULT 1, trashed_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(space_id,id), FOREIGN KEY(space_id,parent_id) REFERENCES nodes(space_id,id)
);
CREATE INDEX nodes_children ON nodes(space_id,parent_id,id) WHERE trashed_at IS NULL;
CREATE INDEX nodes_trash ON nodes(space_id,id) WHERE trashed_at IS NOT NULL;
CREATE INDEX nodes_all_children ON nodes(parent_id,id);
CREATE TABLE grants (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 node_id uuid NOT NULL REFERENCES nodes(id), user_id uuid REFERENCES users(id),
 subject_space_id uuid REFERENCES spaces(id), role smallint NOT NULL CHECK(role BETWEEN 1 AND 3),
 CHECK ((user_id IS NULL) <> (subject_space_id IS NULL))
);
CREATE UNIQUE INDEX grants_user ON grants(node_id,user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX grants_space ON grants(node_id,subject_space_id) WHERE subject_space_id IS NOT NULL;
CREATE INDEX grants_node ON grants(node_id);
CREATE INDEX grants_user_discovery ON grants(user_id,node_id) WHERE user_id IS NOT NULL;
CREATE INDEX grants_space_discovery ON grants(subject_space_id,node_id) WHERE subject_space_id IS NOT NULL;
CREATE TABLE objects (
 id uuid PRIMARY KEY, space_id uuid NOT NULL REFERENCES spaces(id), size bigint NOT NULL CHECK(size>=0),
 sha256 text NOT NULL, state text NOT NULL DEFAULT 'ready' CHECK(state IN ('ready','missing','deleting')),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE versions (
 id uuid PRIMARY KEY, node_id uuid NOT NULL REFERENCES nodes(id), object_id uuid NOT NULL REFERENCES objects(id),
 created_by uuid NOT NULL REFERENCES users(id), created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX versions_node ON versions(node_id,created_at DESC,id DESC);
CREATE INDEX versions_cursor ON versions(node_id,id);
CREATE INDEX versions_object ON versions(object_id);
ALTER TABLE nodes ADD COLUMN current_version uuid REFERENCES versions(id);
CREATE TABLE uploads (
 id uuid PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id), space_id uuid NOT NULL REFERENCES spaces(id),
 parent_id uuid, node_id uuid REFERENCES nodes(id), name text NOT NULL, expected_revision bigint,
 size bigint NOT NULL CHECK(size>=0), received bigint NOT NULL DEFAULT 0 CHECK(received>=0 AND received<=size),
 sha256 text NOT NULL, state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','ready','cancelled')),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '24 hours', result_node uuid REFERENCES nodes(id),
 complete_requested boolean NOT NULL DEFAULT false, completion_error text,
 FOREIGN KEY(space_id,parent_id) REFERENCES nodes(space_id,id)
);
CREATE INDEX uploads_pending ON uploads(expires_at,id) WHERE state='pending';
CREATE INDEX uploads_finalization ON uploads(expires_at,id) WHERE state='pending' AND complete_requested;
CREATE INDEX uploads_node ON uploads(node_id);
CREATE INDEX uploads_parent ON uploads(parent_id);
CREATE INDEX uploads_result ON uploads(result_node);
CREATE TABLE recovery (
 token_hash text PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id), expires_at timestamptz NOT NULL
);
CREATE TABLE verification (token_hash text PRIMARY KEY,user_id uuid NOT NULL REFERENCES users(id),expires_at timestamptz NOT NULL);
CREATE TABLE mail_jobs (
 id uuid PRIMARY KEY, recipient text NOT NULL, body text NOT NULL, attempts int NOT NULL DEFAULT 0,
 available_at timestamptz NOT NULL DEFAULT now(), created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE rate_limits (
 key text PRIMARY KEY, window_start timestamptz NOT NULL DEFAULT now(), count int NOT NULL DEFAULT 1
);
CREATE INDEX rate_limits_expiry ON rate_limits(window_start);
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE INDEX recovery_expiry ON recovery(expires_at);
CREATE INDEX verification_expiry ON verification(expires_at);
CREATE INDEX mail_jobs_available ON mail_jobs(available_at,id);
CREATE TABLE idempotency (
 user_id uuid NOT NULL REFERENCES users(id), key text NOT NULL, fingerprint text NOT NULL,
 result jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(user_id,key)
);
CREATE TABLE audit (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, actor uuid NOT NULL REFERENCES users(id),
 space_id uuid NOT NULL REFERENCES spaces(id), action text NOT NULL, target uuid,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX idempotency_expiry ON idempotency(created_at);
CREATE INDEX audit_space ON audit(space_id,id);
CREATE TABLE purge_jobs(node_id uuid PRIMARY KEY REFERENCES nodes(id), created_at timestamptz NOT NULL DEFAULT now());
