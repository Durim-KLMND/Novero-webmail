-- Calendars synced with Google and Microsoft accounts.

-- An online account the calendars come from; its sign-in is in the
-- credential store under `calendar-<id>`.
CREATE TABLE calendar_accounts (
    id           INTEGER PRIMARY KEY,
    provider     TEXT    NOT NULL,
    email        TEXT    NOT NULL,
    synced_at    INTEGER,
    error        TEXT,
    needs_signin INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE calendars ADD COLUMN source INTEGER REFERENCES calendar_accounts(id) ON DELETE CASCADE;
-- The provider's id for the calendar.
ALTER TABLE calendars ADD COLUMN remote_id TEXT;
ALTER TABLE calendars ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0;
-- Where the last pull left off (Google's sync token).
ALTER TABLE calendars ADD COLUMN sync_token TEXT;

-- The provider's id and version of the event.
ALTER TABLE events ADD COLUMN remote_id TEXT;
ALTER TABLE events ADD COLUMN etag TEXT;
-- For one occurrence kept as its own event: the series' id, and the start
-- the occurrence had before it was moved.
ALTER TABLE events ADD COLUMN series TEXT;
ALTER TABLE events ADD COLUMN original_start INTEGER;
-- Bumped by each local change not sent yet; 0 when in step with the server.
ALTER TABLE events ADD COLUMN dirty INTEGER NOT NULL DEFAULT 0;
CREATE INDEX events_remote ON events (calendar_id, remote_id);
CREATE INDEX events_series ON events (calendar_id, series);

-- Events deleted here and not yet on the server.
CREATE TABLE calendar_deletions (
    calendar_id INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    remote_id   TEXT    NOT NULL,
    PRIMARY KEY (calendar_id, remote_id)
);
