ALTER TABLE spaces ADD COLUMN storage_directory text;
UPDATE spaces s SET storage_directory = CASE
 WHEN s.kind='organization' THEN 'organizaciones/'||s.id::text
 WHEN u.username ~ '^[a-z0-9_.-]{3,64}$' AND u.username !~ '\.$'
  AND split_part(u.username,'.',1) !~* '^(con|prn|aux|nul|com[0-9]|lpt[0-9])$'
 THEN 'usuarios/'||u.username
 ELSE 'usuarios_compat/'||u.id::text END
FROM users u WHERE u.id=s.owner_id;
ALTER TABLE spaces ALTER COLUMN storage_directory SET NOT NULL;
ALTER TABLE spaces ADD CONSTRAINT safe_storage_directory CHECK (
 storage_directory ~ '^(usuarios/[a-z0-9_.-]{3,64}|usuarios_compat/[0-9a-f-]{36}|organizaciones/[0-9a-f-]{36})$'
);
CREATE UNIQUE INDEX spaces_storage_directory ON spaces(storage_directory);
