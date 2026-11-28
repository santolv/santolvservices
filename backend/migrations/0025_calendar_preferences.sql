-- Personal calendar preferences: each user chooses the colour and visibility of every
-- calendar in their list (their own or shared with them) without affecting anyone else.
CREATE TABLE calendar.preferences (
    calendar_id uuid NOT NULL REFERENCES calendar.calendars(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    color text CHECK (color ~ '^#[0-9a-f]{6}$'),
    visible boolean NOT NULL DEFAULT true,
    PRIMARY KEY (calendar_id, user_id)
);
CREATE INDEX calendar_preferences_user ON calendar.preferences(user_id);
