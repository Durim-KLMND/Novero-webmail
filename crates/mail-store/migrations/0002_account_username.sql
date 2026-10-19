-- The login name can differ from the email address.
ALTER TABLE accounts ADD COLUMN username TEXT NOT NULL DEFAULT '';
