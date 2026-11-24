ALTER TABLE spaces ADD COLUMN deleted_at timestamptz;
ALTER TABLE spaces ADD COLUMN storage_removed boolean NOT NULL DEFAULT false;
CREATE INDEX spaces_pending_deletion ON spaces(id) WHERE deleted_at IS NOT NULL AND NOT storage_removed;
CREATE TABLE recent_nodes (
 user_id uuid NOT NULL REFERENCES users(id),
 node_id uuid NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
 used_at bigint NOT NULL,
 PRIMARY KEY(user_id,node_id)
);
CREATE INDEX recent_nodes_order ON recent_nodes(user_id,used_at DESC,node_id DESC);
CREATE INDEX recent_nodes_node ON recent_nodes(node_id);
CREATE INDEX objects_space ON objects(space_id);
CREATE INDEX uploads_space ON uploads(space_id);
