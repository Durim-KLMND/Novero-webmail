-- Emoji reactions sent as mail (mail_mime::reaction). The reaction mail
-- stays in its folder but is left out of lists; its emoji shows on the
-- message it reacts to instead.

-- The emoji, when the message is a reaction, and the Message-ID it is to.
ALTER TABLE messages ADD COLUMN reaction TEXT;
ALTER TABLE messages ADD COLUMN reaction_to TEXT;
-- Set once the message has been looked at for a reaction.
ALTER TABLE messages ADD COLUMN reaction_checked INTEGER NOT NULL DEFAULT 0;

CREATE INDEX messages_reaction_to ON messages (reaction_to) WHERE reaction_to IS NOT NULL;
