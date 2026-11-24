CREATE TABLE workspace_entries (
 node_id uuid PRIMARY KEY, space_id uuid NOT NULL REFERENCES spaces(id), parent_id uuid,
 component text NOT NULL, source_name text NOT NULL, kind text NOT NULL,
 object_id uuid, content_name text, trashed boolean NOT NULL
);
CREATE INDEX workspace_entries_parent ON workspace_entries(parent_id);
CREATE UNIQUE INDEX workspace_entries_names ON workspace_entries(space_id,COALESCE(parent_id,'00000000-0000-0000-0000-000000000000'::uuid),lower(component)) WHERE NOT trashed;
CREATE INDEX nodes_workspace_names ON nodes(space_id,COALESCE(parent_id,'00000000-0000-0000-0000-000000000000'::uuid),lower(name)) WHERE trashed_at IS NULL;
CREATE INDEX uploads_workspace_names ON uploads(space_id,lower(name),parent_id) WHERE state='pending' AND node_id IS NULL;
CREATE TABLE workspace_jobs (
 node_id uuid PRIMARY KEY, space_id uuid NOT NULL REFERENCES spaces(id),
 attempted_at timestamptz NOT NULL DEFAULT now(), last_error text, audit boolean NOT NULL DEFAULT false
);
INSERT INTO workspace_jobs(node_id,space_id) SELECT id,space_id FROM nodes;
CREATE INDEX workspace_jobs_space ON workspace_jobs(space_id);
CREATE INDEX workspace_jobs_order ON workspace_jobs(attempted_at,node_id);
