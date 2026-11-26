-- Move tables atomically: preserve IDs, ownership, indexes and the existing link FK.
CREATE SCHEMA notes;
CREATE SCHEMA tasks;
ALTER TABLE notes_tasks.notes SET SCHEMA notes;
ALTER TABLE notes_tasks.tasks SET SCHEMA tasks;
INSERT INTO public.service_settings (id,enabled)
SELECT next.id, old.enabled FROM public.service_settings old
CROSS JOIN (VALUES ('notes'),('tasks')) next(id) WHERE old.id='notes-tasks';
INSERT INTO public.service_user_denials(user_id,service_id)
SELECT old.user_id,next.id FROM public.service_user_denials old
CROSS JOIN (VALUES ('notes'),('tasks')) next(id) WHERE old.service_id='notes-tasks';
DELETE FROM public.service_user_denials WHERE service_id='notes-tasks';
DELETE FROM public.service_settings WHERE id='notes-tasks';
DROP SCHEMA notes_tasks;
