CREATE SCHEMA contacts;

CREATE TABLE contacts.people (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    given_name text NOT NULL CHECK (char_length(given_name) <= 100),
    family_name text NOT NULL CHECK (char_length(family_name) <= 100),
    company text NOT NULL CHECK (char_length(company) <= 160),
    display_name text GENERATED ALWAYS AS (
        coalesce(nullif(btrim(given_name || ' ' || family_name), ''), company)
    ) STORED,
    email text NOT NULL CHECK (char_length(email) <= 254),
    phone text NOT NULL CHECK (char_length(phone) <= 80),
    job_title text NOT NULL CHECK (char_length(job_title) <= 120),
    address text NOT NULL CHECK (char_length(address) <= 500),
    website text NOT NULL CHECK (char_length(website) <= 500),
    notes text NOT NULL CHECK (char_length(notes) <= 4000),
    favorite boolean NOT NULL DEFAULT false,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (length(display_name) > 0)
);
CREATE INDEX contacts_by_name ON contacts.people (owner_id, lower(display_name), id);
CREATE INDEX contacts_favorites_by_name ON contacts.people (owner_id, lower(display_name), id)
    WHERE favorite;

INSERT INTO public.service_settings(id, enabled) VALUES ('contacts', false);
