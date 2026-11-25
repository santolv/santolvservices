ALTER TABLE photos.derivatives DROP COLUMN thumbnail;
-- Requeue failed originals after raising decoder limits.
INSERT INTO photos.derivative_jobs(node_id,version_id)
SELECT node_id,version_id FROM photos.derivatives WHERE error
ON CONFLICT(node_id) DO UPDATE SET version_id=excluded.version_id;
DELETE FROM photos.derivatives WHERE error;
