-- Intent survives a filesystem rename followed by a rolled-back metadata transaction.
CREATE TABLE workspace_moves (
    node_id uuid PRIMARY KEY,
    source text NOT NULL,
    target text NOT NULL
);
CREATE INDEX workspace_object_lookup ON workspace_entries(object_id) WHERE object_id IS NOT NULL;
CREATE INDEX workspace_trash_lookup ON workspace_entries(space_id,lower(component)) WHERE trashed;
-- Only the current content remains accessible. Obsolete blobs are collected by the worker.
DELETE FROM versions v WHERE NOT EXISTS (SELECT 1 FROM nodes n WHERE n.current_version=v.id);
UPDATE objects o SET state='deleting' WHERE NOT EXISTS (SELECT 1 FROM versions v WHERE v.object_id=o.id);
INSERT INTO workspace_jobs(node_id,space_id) SELECT id,space_id FROM nodes
ON CONFLICT(node_id) DO UPDATE SET audit=false,attempted_at='-infinity';
