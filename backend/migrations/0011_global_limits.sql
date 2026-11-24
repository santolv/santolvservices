CREATE INDEX uploads_pending_user ON uploads(user_id) WHERE state='pending';
