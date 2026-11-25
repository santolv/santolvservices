ALTER TABLE photos.spaces DROP CONSTRAINT safe_storage_directory;
UPDATE photos.spaces
SET legacy_storage_directory = storage_directory,
    storage_directory = 'usuarios/' || owner_id::text || '/drive/Photos';
ALTER TABLE photos.spaces ADD CONSTRAINT safe_storage_directory CHECK (
 storage_directory = 'usuarios/' || owner_id::text || '/drive/Photos'
);

