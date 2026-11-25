ALTER TABLE public.users
    ADD COLUMN display_name text CHECK (char_length(display_name) BETWEEN 1 AND 48),
    ADD COLUMN avatar_color text NOT NULL DEFAULT 'blue'
        CHECK (avatar_color IN ('blue', 'teal', 'plum', 'rose', 'slate'));
