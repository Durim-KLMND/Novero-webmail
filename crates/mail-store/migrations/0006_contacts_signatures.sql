-- Phase 8: address suggestions, formatted signatures and rules.

-- People the user writes to or hears from, for address suggestions.
-- Filled from cached headers (and on the first start after this migration,
-- from everything already cached) and from sent mail.
CREATE TABLE contacts (
    email     TEXT    PRIMARY KEY COLLATE NOCASE,
    name      TEXT,
    -- Messages the user sent to them, and received from them.
    sent      INTEGER NOT NULL DEFAULT 0,
    received  INTEGER NOT NULL DEFAULT 0,
    last_seen INTEGER,
    -- Removed from suggestions by the user; kept so sync doesn't add it back.
    hidden    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX contacts_name ON contacts (name COLLATE NOCASE);

-- A formatted signature (cleaned HTML, pictures as data: URLs). `signature`
-- stays its plain-text version, used for plain-text messages.
ALTER TABLE accounts ADD COLUMN signature_html TEXT NOT NULL DEFAULT '';

-- Rules run on new inbox mail, in `position` order. Conditions and actions
-- are JSON, as `mail_core::rules` writes them.
CREATE TABLE rules (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL DEFAULT '',
    enabled    INTEGER NOT NULL DEFAULT 1,
    match_all  INTEGER NOT NULL DEFAULT 1,
    conditions TEXT    NOT NULL,
    actions    TEXT    NOT NULL,
    position   INTEGER NOT NULL
);
CREATE INDEX rules_account ON rules (account_id, position);
