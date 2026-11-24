ALTER TABLE spaces ADD COLUMN service text NOT NULL DEFAULT 'drive' CHECK(service IN ('drive','photos'));
ALTER TABLE spaces ADD COLUMN legacy_storage_directory text;
ALTER TABLE spaces DROP CONSTRAINT safe_storage_directory;
ALTER TABLE spaces ADD CONSTRAINT safe_storage_directory CHECK (
 storage_directory ~ '^(usuarios/[a-z0-9_.-]{3,64}|usuarios_compat/[0-9a-f-]{36}|organizaciones/[0-9a-f-]{36}|usuarios/[0-9a-f-]{36}/(drive|photos))$'
);
UPDATE spaces SET legacy_storage_directory=storage_directory,
 storage_directory='usuarios/'||owner_id::text||'/drive' WHERE kind='personal';
CREATE UNIQUE INDEX personal_service ON spaces(owner_id,service) WHERE kind='personal';
CREATE TABLE photo_metadata (
 node_id uuid PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
 favorite boolean NOT NULL DEFAULT false
);
