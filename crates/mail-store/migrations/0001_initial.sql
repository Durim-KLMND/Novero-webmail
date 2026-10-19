-- Initial schema. See the "Local data model" section of the build plan.
-- The server is the source of truth: everything here can be rebuilt by resyncing.

CREATE TABLE accounts (
    id            INTEGER PRIMARY KEY,
    display_name  TEXT    NOT NULL,
    email         TEXT    NOT NULL,
    imap_host     TEXT    NOT NULL,
    imap_port     INTEGER NOT NULL,
    imap_security TEXT    NOT NULL CHECK (imap_security IN ('implicit_tls', 'starttls', 'insecure')),
    smtp_host     TEXT    NOT NULL,
    smtp_port     INTEGER NOT NULL,
    smtp_security TEXT    NOT NULL CHECK (smtp_security IN ('implicit_tls', 'starttls', 'insecure')),
    auth_kind     TEXT    NOT NULL CHECK (auth_kind IN ('password', 'oauth2')),
    created_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE folders (
    id            INTEGER PRIMARY KEY,
    account_id    INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name          TEXT    NOT NULL,
    delimiter     TEXT,
    role          TEXT    CHECK (role IN ('inbox', 'sent', 'drafts', 'trash', 'junk', 'archive')),
    uidvalidity   INTEGER,
    uidnext       INTEGER,
    highestmodseq INTEGER,
    last_sync     INTEGER,
    UNIQUE (account_id, name)
);

CREATE TABLE threads (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    root_message_id TEXT,
    latest_date     INTEGER,
    unread_count    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id      TEXT,
    thread_id       INTEGER REFERENCES threads(id) ON DELETE SET NULL,
    from_addr       TEXT,
    to_addrs        TEXT,
    cc_addrs        TEXT,
    subject         TEXT,
    date            INTEGER,
    size            INTEGER,
    flags           INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    body_state      TEXT    NOT NULL DEFAULT 'none' CHECK (body_state IN ('none', 'fetched', 'evicted')),
    UNIQUE (folder_id, uid)
);
CREATE INDEX messages_folder_date ON messages (folder_id, date DESC);
CREATE INDEX messages_message_id ON messages (message_id);
CREATE INDEX messages_thread ON messages (thread_id);

CREATE TABLE message_bodies (
    message_id   INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    preview      TEXT,
    html_path    TEXT,
    raw_path     TEXT
);

CREATE TABLE attachments (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    part_id     TEXT    NOT NULL,
    filename    TEXT,
    mime_type   TEXT,
    size        INTEGER,
    saved_path  TEXT
);

CREATE TABLE op_queue (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,
    payload    TEXT    NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE outbox (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    raw_path   TEXT    NOT NULL,
    state      TEXT    NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'sending', 'failed', 'sent')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    next_try   INTEGER
);

CREATE VIRTUAL TABLE messages_fts USING fts5 (
    subject, from_addr, to_addrs, body,
    content = '',
    contentless_delete = 1
);
