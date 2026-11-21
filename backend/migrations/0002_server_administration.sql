-- Preserve account IDs, password hashes, memberships and content from v0.1.
ALTER TABLE users RENAME COLUMN email TO username;
ALTER TABLE users DROP COLUMN verified;
ALTER TABLE users ADD COLUMN account_type text NOT NULL DEFAULT 'user' CHECK(account_type IN ('user','admin'));
ALTER TABLE users ADD COLUMN disabled boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN must_change_password boolean NOT NULL DEFAULT false;
DELETE FROM sessions;
DROP TABLE recovery;
DROP TABLE verification;
DROP TABLE mail_jobs;
CREATE TABLE admin_audit (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 actor uuid NOT NULL REFERENCES users(id), target uuid REFERENCES users(id),
 action text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX admin_audit_cursor ON admin_audit(id);
CREATE INDEX users_administration ON users(account_type,id);
