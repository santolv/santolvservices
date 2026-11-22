ALTER TABLE objects ADD COLUMN original_name text;
UPDATE objects o SET original_name=COALESCE(
 (SELECT name FROM uploads WHERE id=o.id),
 (SELECT n.name FROM versions v JOIN nodes n ON n.id=v.node_id WHERE v.object_id=o.id ORDER BY v.id LIMIT 1),
 o.id::text||'.bin');
ALTER TABLE objects ALTER COLUMN original_name SET NOT NULL;
