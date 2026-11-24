-- Never let sessions issued by an older binary outlive the new absolute limit.
UPDATE sessions SET expires_at=LEAST(expires_at,now()+interval '2 hours');
