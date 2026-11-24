-- Existing admin accounts stay in place, but can no longer hold API sessions.
DELETE FROM sessions USING users WHERE sessions.user_id=users.id AND users.account_type='admin';
CREATE UNIQUE INDEX one_server_administrator ON users(account_type) WHERE account_type='admin';
