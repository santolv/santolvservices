CREATE SCHEMA calendar;

CREATE TABLE calendar.calendars (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    is_default boolean NOT NULL DEFAULT false
);
CREATE INDEX calendar_owner ON calendar.calendars(owner_id);
CREATE UNIQUE INDEX calendar_default ON calendar.calendars(owner_id) WHERE is_default;

CREATE TABLE calendar.members (
    calendar_id uuid NOT NULL REFERENCES calendar.calendars(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('reader', 'editor')),
    PRIMARY KEY (calendar_id, user_id)
);
CREATE INDEX calendar_member_user ON calendar.members(user_id, calendar_id);

CREATE TABLE calendar.events (
    id uuid PRIMARY KEY,
    calendar_id uuid NOT NULL REFERENCES calendar.calendars(id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    description text NOT NULL CHECK (char_length(description) <= 4000),
    local_start timestamp(0) NOT NULL,
    local_end timestamp(0) NOT NULL,
    zone text NOT NULL,
    all_day boolean NOT NULL,
    busy boolean NOT NULL,
    repeat_kind text CHECK (repeat_kind IN ('daily', 'weekly', 'monthly', 'yearly')),
    repeat_until date,
    repeat_count integer CHECK (repeat_count BETWEEN 1 AND 10000),
    revision bigint NOT NULL DEFAULT 1,
    CHECK (local_end > local_start AND local_end - local_start <= interval '31 days'),
    CHECK (NOT all_day OR (local_start::time = time '00:00' AND local_end::time = time '00:00')),
    CHECK (repeat_until IS NULL OR (repeat_kind IS NOT NULL AND repeat_until >= local_start::date)),
    CHECK (repeat_count IS NULL OR (repeat_kind IS NOT NULL AND repeat_until IS NULL))
);
CREATE INDEX calendar_event_range ON calendar.events(calendar_id, local_start);

CREATE TABLE calendar.exceptions (
    event_id uuid NOT NULL REFERENCES calendar.events(id) ON DELETE CASCADE,
    original_start timestamp(0) NOT NULL,
    cancelled boolean NOT NULL,
    title text,
    description text,
    local_start timestamp(0),
    local_end timestamp(0),
    zone text,
    all_day boolean,
    busy boolean,
    PRIMARY KEY (event_id, original_start)
);

CREATE TABLE calendar.reminders (
    event_id uuid NOT NULL REFERENCES calendar.events(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    minutes_before integer NOT NULL CHECK (minutes_before BETWEEN 0 AND 10080),
    PRIMARY KEY (event_id, user_id)
);
CREATE INDEX calendar_reminder_user ON calendar.reminders(user_id, event_id);

CREATE TABLE calendar.dismissals (
    event_id uuid NOT NULL REFERENCES calendar.events(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    original_start timestamp(0) NOT NULL,
    due_at timestamptz NOT NULL,
    PRIMARY KEY (event_id, user_id, original_start, due_at)
);

CREATE FUNCTION calendar.period_index(start_date date, target_date date, kind text)
RETURNS integer LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE kind
        WHEN 'daily' THEN target_date - start_date
        WHEN 'weekly' THEN (target_date - start_date) / 7
        WHEN 'monthly' THEN (extract(year FROM target_date)::int - extract(year FROM start_date)::int) * 12
                             + extract(month FROM target_date)::int - extract(month FROM start_date)::int
        WHEN 'yearly' THEN extract(year FROM target_date)::int - extract(year FROM start_date)::int
        ELSE 0 END
$$;

CREATE FUNCTION calendar.slot(start_at timestamp, kind text, occurrence_index integer)
RETURNS timestamp LANGUAGE sql IMMUTABLE AS $$
    SELECT start_at + CASE kind
        WHEN 'daily' THEN make_interval(days => occurrence_index)
        WHEN 'weekly' THEN make_interval(weeks => occurrence_index)
        WHEN 'monthly' THEN make_interval(months => occurrence_index)
        WHEN 'yearly' THEN make_interval(years => occurrence_index)
        ELSE interval '0' END
$$;

CREATE FUNCTION calendar.is_slot(e calendar.events, original timestamp)
RETURNS boolean LANGUAGE sql IMMUTABLE AS $$
    SELECT original >= e.local_start
       AND (e.repeat_kind IS NOT NULL OR original = e.local_start)
       AND original = calendar.slot(e.local_start, e.repeat_kind, occurrence_index)
       AND (e.repeat_until IS NULL OR original::date <= e.repeat_until)
       AND (e.repeat_count IS NULL OR occurrence_index < e.repeat_count)
    FROM calendar.period_index(e.local_start::date, original::date, e.repeat_kind) AS occurrence_index
$$;

CREATE FUNCTION calendar.occurrences(viewer uuid, since timestamptz, until timestamptz,
    only_calendar uuid, only_event uuid, reminders_only boolean)
RETURNS TABLE(event_id uuid, calendar_id uuid, calendar_name text, role text, revision bigint,
    original_start timestamp, title text, description text, zone text, all_day boolean, busy boolean,
    local_start timestamp, local_end timestamp, starts_at timestamptz, ends_at timestamptz)
LANGUAGE sql STABLE AS $$
    WITH visible AS MATERIALIZED (
        SELECT e.*, c.name AS calendar_name,
            CASE WHEN c.owner_id = viewer THEN 'owner' ELSE m.role END AS role
        FROM calendar.events e
        JOIN calendar.calendars c ON c.id = e.calendar_id
        LEFT JOIN calendar.members m ON m.calendar_id = c.id AND m.user_id = viewer
        WHERE (c.owner_id = viewer OR m.user_id IS NOT NULL)
          AND (only_calendar IS NULL OR e.calendar_id = only_calendar)
          AND (only_event IS NULL OR e.id = only_event)
          AND (NOT reminders_only OR EXISTS (
              SELECT 1 FROM calendar.reminders r WHERE r.event_id = e.id AND r.user_id = viewer))
    ), slots AS (
        SELECT e.id, e.calendar_id, e.calendar_name, e.role, e.revision, slot.original,
               e.title, e.description, e.zone, e.all_day, e.busy,
               slot.original AS local_start, slot.original + (e.local_end - e.local_start) AS local_end,
               CASE WHEN NOT e.all_day THEN
                   (e.local_end AT TIME ZONE e.zone) - (e.local_start AT TIME ZONE e.zone)
               END AS elapsed
        FROM visible e
        CROSS JOIN LATERAL generate_series(
            greatest(0, calendar.period_index(e.local_start::date,
                (since AT TIME ZONE e.zone)::date - (e.local_end::date - e.local_start::date) - 2, e.repeat_kind)),
            least(coalesce(e.repeat_count - 1, 2147483647),
                calendar.period_index(e.local_start::date,
                    least((until AT TIME ZONE e.zone)::date + 2, e.repeat_until), e.repeat_kind))
        ) AS n(occurrence_index)
        CROSS JOIN LATERAL (SELECT calendar.slot(e.local_start, e.repeat_kind, n.occurrence_index) AS original) slot
        WHERE NOT EXISTS (SELECT 1 FROM calendar.exceptions x WHERE x.event_id = e.id AND x.original_start = slot.original)
          AND (e.repeat_until IS NULL OR slot.original::date <= e.repeat_until)
        UNION ALL
        SELECT e.id, e.calendar_id, e.calendar_name, e.role, e.revision, x.original_start,
               x.title, x.description, x.zone, x.all_day, x.busy, x.local_start, x.local_end, NULL
        FROM visible e JOIN calendar.exceptions x ON x.event_id = e.id
        WHERE NOT x.cancelled
    ), resolved AS (
        SELECT s.*, s.local_start AT TIME ZONE s.zone AS starts_at,
               coalesce((s.local_start AT TIME ZONE s.zone) + s.elapsed,
                        s.local_end AT TIME ZONE s.zone) AS ends_at
        FROM slots s
    )
    SELECT id, r.calendar_id, r.calendar_name, r.role, r.revision, original, r.title, r.description,
           r.zone, r.all_day, r.busy,
           CASE WHEN r.all_day THEN r.local_start ELSE r.starts_at AT TIME ZONE r.zone END,
           CASE WHEN r.all_day THEN r.local_end ELSE r.ends_at AT TIME ZONE r.zone END,
           r.starts_at, r.ends_at
    FROM resolved r
    WHERE r.starts_at < until AND r.ends_at > since
$$;

CREATE FUNCTION calendar.pending_reminders(viewer uuid)
RETURNS TABLE(event_id uuid, original_start timestamp, title text, due_at timestamptz)
LANGUAGE sql STABLE AS $$
    SELECT o.event_id, o.original_start, o.title, due.at
    FROM calendar.occurrences(viewer, now() - interval '8 days', now() + interval '8 days', NULL, NULL, true) o
    JOIN calendar.reminders r ON r.event_id=o.event_id AND r.user_id=viewer
    CROSS JOIN LATERAL (
        SELECT (CASE WHEN o.all_day THEN (o.local_start::date + time '09:00') AT TIME ZONE o.zone
                     ELSE o.starts_at END) - make_interval(mins => r.minutes_before) AS at
    ) due
    WHERE due.at BETWEEN now() - interval '7 days' AND now()
      AND NOT EXISTS (SELECT 1 FROM calendar.dismissals d
          WHERE d.user_id=viewer AND d.event_id=o.event_id AND d.original_start=o.original_start AND d.due_at=due.at)
$$;

INSERT INTO public.service_settings(id, enabled) VALUES ('calendar', false);
