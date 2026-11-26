CREATE SCHEMA notes_tasks;

CREATE TABLE notes_tasks.notes (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    content text NOT NULL CHECK (char_length(content) <= 50000),
    archived_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (owner_id, id)
);

CREATE TABLE notes_tasks.tasks (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    status text NOT NULL CHECK (status IN ('pending', 'in_progress', 'done')),
    priority text NOT NULL CHECK (priority IN ('low', 'normal', 'high')),
    due_at timestamptz,
    note_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (owner_id, note_id) REFERENCES notes_tasks.notes(owner_id, id)
);

CREATE INDEX notes_tasks_active_notes_page
    ON notes_tasks.notes (owner_id, created_at DESC, id DESC)
    WHERE archived_at IS NULL;
CREATE INDEX notes_tasks_archived_notes_page
    ON notes_tasks.notes (owner_id, created_at DESC, id DESC)
    WHERE archived_at IS NOT NULL;
CREATE INDEX notes_tasks_tasks_page
    ON notes_tasks.tasks (owner_id, created_at DESC, id DESC);
CREATE INDEX notes_tasks_tasks_note_page
    ON notes_tasks.tasks (owner_id, note_id, created_at DESC, id DESC)
    WHERE note_id IS NOT NULL;

INSERT INTO public.service_settings (id, enabled) VALUES ('notes-tasks', false);
