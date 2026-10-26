-- Mail cleanup: categories for mail, guessed on the computer from headers and
-- sender history, and confirmed by the user.

-- Hints from list and bulk-mail headers (`mail_core::categories::signals`
-- bits), NULL until those headers have been looked at; and the link to
-- leave the list the message came through.
ALTER TABLE messages ADD COLUMN signals INTEGER;
ALTER TABLE messages ADD COLUMN unsubscribe TEXT;

-- The category the user chose for everything from a sender.
CREATE TABLE sender_categories (
    email    TEXT    PRIMARY KEY COLLATE NOCASE,
    category TEXT    NOT NULL,
    updated  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- A category the user chose or accepted for one message.
CREATE TABLE message_categories (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    category   TEXT    NOT NULL
);
