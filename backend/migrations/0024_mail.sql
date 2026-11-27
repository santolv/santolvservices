CREATE SCHEMA mail;

CREATE TABLE mail.messages (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('draft', 'received', 'sent')),
    sender text CHECK (char_length(sender) <= 320),
    to_addresses text[] NOT NULL DEFAULT '{}',
    cc_addresses text[] NOT NULL DEFAULT '{}',
    bcc_addresses text[] NOT NULL DEFAULT '{}',
    subject text NOT NULL CHECK (char_length(subject) <= 250),
    content text NOT NULL CHECK (char_length(content) <= 30000),
    starred boolean NOT NULL DEFAULT false,
    unread boolean NOT NULL DEFAULT false,
    archived boolean NOT NULL DEFAULT false,
    trashed boolean NOT NULL DEFAULT false,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CHECK (cardinality(to_addresses) + cardinality(cc_addresses) + cardinality(bcc_addresses) <= 50),
    CHECK (kind <> 'received' OR sender IS NOT NULL),
    CHECK (kind = 'received' OR NOT unread)
);

CREATE INDEX mail_messages_page ON mail.messages (owner_id, updated_at DESC, id DESC);
INSERT INTO public.service_settings (id, enabled) VALUES ('mail', false);
