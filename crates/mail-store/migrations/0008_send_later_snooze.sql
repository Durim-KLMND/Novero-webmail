-- Scheduled send: a queued message waits in the outbox until this time.
ALTER TABLE outbox ADD COLUMN send_at INTEGER;

-- Snoozed messages are hidden from their folder until `until`, then come
-- back unread.
CREATE TABLE snoozes (
    message INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    until   INTEGER NOT NULL
);
CREATE INDEX snoozes_until ON snoozes (until);
