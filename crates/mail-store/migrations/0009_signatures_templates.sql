-- Phase 9: several signatures per account, and message templates.

-- An account's signatures. `text` is plain; `html` is the formatted version
-- (cleaned HTML, pictures as data: URLs), or '' when it has no formatting.
CREATE TABLE signatures (
    id INTEGER PRIMARY KEY,
    account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    text TEXT NOT NULL DEFAULT '',
    html TEXT NOT NULL DEFAULT ''
);
CREATE INDEX signatures_account ON signatures (account);

-- Which signature new messages, and replies and forwards, start with.
ALTER TABLE accounts ADD COLUMN signature_new INTEGER REFERENCES signatures(id) ON DELETE SET NULL;
ALTER TABLE accounts ADD COLUMN signature_reply INTEGER REFERENCES signatures(id) ON DELETE SET NULL;

-- The one signature an account had becomes its first, used for everything.
-- (accounts.signature and signature_html are no longer read.)
INSERT INTO signatures (account, name, text, html)
    SELECT id, 'Signature', signature, signature_html FROM accounts
    WHERE signature <> '' OR signature_html <> '';
UPDATE accounts SET
    signature_new = (SELECT s.id FROM signatures s WHERE s.account = accounts.id),
    signature_reply = (SELECT s.id FROM signatures s WHERE s.account = accounts.id);

-- Messages saved to start new ones from: a subject and the text.
CREATE TABLE templates (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    html TEXT NOT NULL DEFAULT ''
);
