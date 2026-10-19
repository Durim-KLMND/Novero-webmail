-- Messages being written. `content` is the draft as JSON. Its copy in the
-- Drafts folder is replaced whenever `version` moves past `uploaded_version`.
CREATE TABLE drafts (
    id               INTEGER PRIMARY KEY,
    account_id       INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    message_id       TEXT    NOT NULL,
    content          TEXT    NOT NULL,
    version          INTEGER NOT NULL DEFAULT 1,
    uploaded_version INTEGER NOT NULL DEFAULT 0,
    -- Sent or thrown away: the server copy is removed, then the row.
    discarded        INTEGER NOT NULL DEFAULT 0,
    updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Nothing could be queued before this version, so the outbox is redefined.
-- 'sent' means the server took the message and only the copy for the Sent
-- folder is left to save.
DROP TABLE outbox;
CREATE TABLE outbox (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    message_id  TEXT    NOT NULL,
    subject     TEXT    NOT NULL,
    recipients  TEXT    NOT NULL,
    state       TEXT    NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'failed', 'sent')),
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
