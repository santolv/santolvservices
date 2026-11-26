ALTER TABLE tasks.tasks ADD COLUMN archived_at timestamptz;
CREATE INDEX tasks_active_page ON tasks.tasks(owner_id,created_at DESC,id DESC) WHERE archived_at IS NULL;
CREATE TABLE calendar.record_links (
 event_id uuid NOT NULL REFERENCES calendar.events(id) ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
 note_id uuid REFERENCES notes.notes(id) ON DELETE SET NULL,
 task_id uuid REFERENCES tasks.tasks(id) ON DELETE SET NULL,
 PRIMARY KEY(event_id,user_id)
);
