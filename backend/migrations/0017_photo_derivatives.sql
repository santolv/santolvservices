CREATE TABLE photos.derivatives (
 node_id uuid PRIMARY KEY REFERENCES photos.nodes(id) ON DELETE CASCADE,
 version_id uuid NOT NULL,
 thumbnail bytea,
 preview bytea,
 error boolean NOT NULL DEFAULT false
);
CREATE TABLE photos.derivative_jobs (
 node_id uuid PRIMARY KEY REFERENCES photos.nodes(id) ON DELETE CASCADE,
 version_id uuid NOT NULL
);
CREATE FUNCTION photos.queue_derivatives() RETURNS trigger
LANGUAGE plpgsql SET search_path=photos,public AS $$
BEGIN
 IF NEW.trashed_at IS DISTINCT FROM OLD.trashed_at THEN
   WITH RECURSIVE subtree AS (
     SELECT id FROM photos.nodes WHERE id=NEW.id
     UNION ALL SELECT n.id FROM photos.nodes n JOIN subtree p ON n.parent_id=p.id
   ) DELETE FROM photos.derivatives WHERE node_id IN (SELECT id FROM subtree);
   WITH RECURSIVE subtree AS (
     SELECT id FROM photos.nodes WHERE id=NEW.id
     UNION ALL SELECT n.id FROM photos.nodes n JOIN subtree p ON n.parent_id=p.id
   ) DELETE FROM photos.derivative_jobs WHERE node_id IN (SELECT id FROM subtree);
   IF NEW.trashed_at IS NULL THEN
     WITH RECURSIVE subtree AS (
       SELECT * FROM photos.nodes WHERE id=NEW.id AND trashed_at IS NULL
       UNION ALL SELECT n.* FROM photos.nodes n JOIN subtree p ON n.parent_id=p.id WHERE n.trashed_at IS NULL
     ) INSERT INTO photos.derivative_jobs SELECT id,current_version FROM subtree
       WHERE kind='file' AND current_version IS NOT NULL ON CONFLICT DO NOTHING;
   END IF;
 END IF;
 IF NEW.current_version IS DISTINCT FROM OLD.current_version THEN
   DELETE FROM photos.derivatives WHERE node_id=NEW.id;
   DELETE FROM photos.derivative_jobs WHERE node_id=NEW.id;
   IF NEW.current_version IS NOT NULL AND NEW.trashed_at IS NULL AND NEW.kind='file' THEN
     INSERT INTO photos.derivative_jobs VALUES(NEW.id,NEW.current_version);
   END IF;
 END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER photo_derivative_changes AFTER UPDATE OF current_version,trashed_at
 ON photos.nodes FOR EACH ROW EXECUTE FUNCTION photos.queue_derivatives();
WITH RECURSIVE live AS (
 SELECT * FROM photos.nodes WHERE parent_id IS NULL AND trashed_at IS NULL
 UNION ALL SELECT n.* FROM photos.nodes n JOIN live p ON n.parent_id=p.id WHERE n.trashed_at IS NULL
) INSERT INTO photos.derivative_jobs SELECT id,current_version FROM live
 WHERE kind='file' AND current_version IS NOT NULL;
