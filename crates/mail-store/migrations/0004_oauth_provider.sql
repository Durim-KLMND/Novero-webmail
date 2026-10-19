-- Accounts that sign in with OAuth2 record whose sign-in page they use.
-- The refresh token itself lives in the OS credential store.
ALTER TABLE accounts ADD COLUMN oauth_provider TEXT
    CHECK (oauth_provider IS NULL OR oauth_provider IN ('google', 'microsoft'));
