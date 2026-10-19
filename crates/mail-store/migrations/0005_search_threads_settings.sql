-- Phase 5: conversations, search over message text, settings and signatures.

-- Conversations are grouped by the Message-IDs a message replies to. Headers
-- cached so far were fetched without References, so they are dropped and
-- fetched again on the next sync. Cached message files are kept on disk and
-- replaced when a message is opened again.
DELETE FROM messages;
DELETE FROM threads;
UPDATE folders SET uidnext = NULL, highestmodseq = NULL;

-- References and In-Reply-To of each message, to find its conversation when
-- a reply arrives before the message it answers.
CREATE TABLE message_refs (
    message INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    ref     TEXT    NOT NULL
);
CREATE INDEX message_refs_ref ON message_refs (ref);
CREATE INDEX message_refs_message ON message_refs (message);
CREATE INDEX messages_folder_thread ON messages (folder_id, thread_id, date DESC);

-- A conversation goes when its last message does.
CREATE TRIGGER messages_thread_cleanup AFTER DELETE ON messages
WHEN old.thread_id IS NOT NULL
BEGIN
    DELETE FROM threads WHERE id = old.thread_id
        AND NOT EXISTS (SELECT 1 FROM messages WHERE thread_id = old.thread_id);
END;

-- The search index, rebuilt with accents folded (so "cafe" finds "café") and
-- prefix indexes for search-as-you-type. Headers are indexed when a message
-- is cached; its text is added once the body is downloaded.
DROP TABLE messages_fts;
CREATE VIRTUAL TABLE messages_fts USING fts5 (
    subject, from_addr, to_addrs, body,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, subject, from_addr, to_addrs, body)
    VALUES (new.id, coalesce(new.subject, ''), coalesce(new.from_addr, ''),
            trim(coalesce(new.to_addrs, '') || ' ' || coalesce(new.cc_addrs, '')), '');
END;

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;

-- App-wide preferences as JSON values, e.g. the theme.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Plain-text signature added to new messages, replies and forwards.
ALTER TABLE accounts ADD COLUMN signature TEXT NOT NULL DEFAULT '';
