-- Phase 9: a calendar kept on this computer.

CREATE TABLE calendars (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    color      TEXT    NOT NULL,
    -- The account invitations to it are answered from.
    account_id INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    visible    INTEGER NOT NULL DEFAULT 1,
    position   INTEGER NOT NULL DEFAULT 0
);
INSERT INTO calendars (name, color, position) VALUES ('Calendar', '#2e7d6b', 0);

-- Times are UTC seconds; see mail_core::calendar. Lists are JSON.
CREATE TABLE events (
    id          INTEGER PRIMARY KEY,
    calendar_id INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    uid         TEXT    NOT NULL UNIQUE,
    title       TEXT    NOT NULL DEFAULT '',
    location    TEXT    NOT NULL DEFAULT '',
    notes       TEXT    NOT NULL DEFAULT '',
    starts      INTEGER NOT NULL,
    ends        INTEGER NOT NULL,
    all_day     INTEGER NOT NULL DEFAULT 0,
    tz          TEXT,
    rrule       TEXT,
    exdates     TEXT    NOT NULL DEFAULT '[]',
    organizer   TEXT,
    attendees   TEXT    NOT NULL DEFAULT '[]',
    response    TEXT,
    reminder    INTEGER,
    sequence    INTEGER NOT NULL DEFAULT 0,
    cancelled   INTEGER NOT NULL DEFAULT 0,
    -- When the last occurrence ends; NULL when it repeats without end.
    last_ends   INTEGER,
    -- The start of the occurrence last reminded about.
    reminded    INTEGER
);
CREATE INDEX events_range ON events (starts, last_ends);
