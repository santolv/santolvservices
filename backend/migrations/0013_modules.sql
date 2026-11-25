SET LOCAL search_path TO public;

CREATE TABLE service_settings (
    id text PRIMARY KEY,
    enabled boolean NOT NULL DEFAULT true
);
INSERT INTO service_settings(id) VALUES ('drive'), ('photos');

CREATE SCHEMA photos;

-- Keep the existing Drive tables and their IDs. Move only rows owned by Fotos.
-- Abort atomically if an old grant links the two services.
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM public.grants g JOIN public.nodes n ON n.id=g.node_id
        JOIN public.spaces owner ON owner.id=n.space_id
        JOIN public.spaces recipient ON recipient.id=g.subject_space_id
        WHERE owner.service<>recipient.service
    ) THEN
        RAISE EXCEPTION 'Remove cross-service space grants before the modular migration';
    END IF;
END $$;

DO $$ DECLARE
    tables text[] := ARRAY['spaces','members','nodes','grants','objects','versions','uploads',
        'audit','purge_jobs','workspace_entries','workspace_jobs','workspace_moves',
        'workspace_publications','recent_nodes'];
    name text;
    target text;
    definition text;
    fk record;
BEGIN
    FOREACH name IN ARRAY tables LOOP
        EXECUTE format('CREATE TABLE photos.%I (LIKE public.%I INCLUDING ALL)',name,name);
    END LOOP;
    -- Add the foreign keys after copying the circular nodes/versions relationship.
    INSERT INTO photos.spaces SELECT * FROM public.spaces WHERE service='photos';
    INSERT INTO photos.members SELECT m.* FROM public.members m JOIN photos.spaces s ON s.id=m.space_id;
    INSERT INTO photos.nodes SELECT n.* FROM public.nodes n JOIN photos.spaces s ON s.id=n.space_id;
    INSERT INTO photos.grants SELECT g.* FROM public.grants g JOIN photos.nodes n ON n.id=g.node_id;
    INSERT INTO photos.objects SELECT o.* FROM public.objects o JOIN photos.spaces s ON s.id=o.space_id;
    INSERT INTO photos.versions SELECT v.* FROM public.versions v JOIN photos.nodes n ON n.id=v.node_id;
    INSERT INTO photos.uploads SELECT u.* FROM public.uploads u JOIN photos.spaces s ON s.id=u.space_id;
    INSERT INTO photos.audit OVERRIDING SYSTEM VALUE SELECT a.* FROM public.audit a JOIN photos.spaces s ON s.id=a.space_id;
    PERFORM setval(pg_get_serial_sequence('photos.audit','id'),coalesce((SELECT max(id)+1 FROM photos.audit),1),false);
    INSERT INTO photos.purge_jobs SELECT j.* FROM public.purge_jobs j JOIN photos.nodes n ON n.id=j.node_id;
    INSERT INTO photos.workspace_entries SELECT e.* FROM public.workspace_entries e JOIN photos.spaces s ON s.id=e.space_id;
    INSERT INTO photos.workspace_jobs SELECT j.* FROM public.workspace_jobs j JOIN photos.spaces s ON s.id=j.space_id;
    INSERT INTO photos.workspace_moves SELECT m.* FROM public.workspace_moves m JOIN photos.workspace_entries e ON e.node_id=m.node_id
        UNION SELECT m.* FROM public.workspace_moves m JOIN photos.workspace_jobs j ON j.node_id=m.node_id
        UNION SELECT m.* FROM public.workspace_moves m JOIN photos.nodes n ON n.id=m.node_id;
    INSERT INTO photos.workspace_publications SELECT p.* FROM public.workspace_publications p JOIN photos.workspace_entries e ON e.node_id=p.node_id
        UNION SELECT p.* FROM public.workspace_publications p JOIN photos.workspace_jobs j ON j.node_id=p.node_id
        UNION SELECT p.* FROM public.workspace_publications p JOIN photos.nodes n ON n.id=p.node_id;
    INSERT INTO photos.recent_nodes SELECT r.* FROM public.recent_nodes r JOIN photos.nodes n ON n.id=r.node_id;
    FOREACH name IN ARRAY tables LOOP
        FOR fk IN SELECT conname, pg_get_constraintdef(oid) AS ddl FROM pg_constraint
                  WHERE conrelid=format('public.%I',name)::regclass AND contype='f' LOOP
            definition := fk.ddl;
            FOREACH target IN ARRAY tables LOOP
                definition := replace(definition,'REFERENCES '||target||'(', 'REFERENCES photos.'||target||'(');
            END LOOP;
            EXECUTE format('ALTER TABLE photos.%I ADD CONSTRAINT %I %s',name,fk.conname,definition);
        END LOOP;
    END LOOP;
END $$;

ALTER TABLE public.photo_metadata DROP CONSTRAINT photo_metadata_node_id_fkey;
ALTER TABLE public.photo_metadata SET SCHEMA photos;
ALTER TABLE photos.photo_metadata ADD FOREIGN KEY(node_id) REFERENCES photos.nodes(id) ON DELETE CASCADE;
DELETE FROM public.workspace_moves WHERE node_id IN (SELECT node_id FROM photos.workspace_moves);
DELETE FROM public.workspace_publications WHERE node_id IN (SELECT node_id FROM photos.workspace_publications);
DELETE FROM public.workspace_jobs WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.workspace_entries WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.purge_jobs WHERE node_id IN (SELECT id FROM photos.nodes);
DELETE FROM public.uploads WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.grants WHERE node_id IN (SELECT id FROM photos.nodes);
UPDATE public.nodes SET current_version=NULL WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.versions WHERE node_id IN (SELECT id FROM photos.nodes);
DELETE FROM public.nodes WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.objects WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.members WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.audit WHERE space_id IN (SELECT id FROM photos.spaces);
DELETE FROM public.spaces WHERE id IN (SELECT id FROM photos.spaces);
ALTER TABLE public.spaces ADD CHECK(service='drive');
ALTER TABLE photos.spaces ADD CHECK(service='photos');
