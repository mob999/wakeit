ALTER TABLE user_identities DROP CONSTRAINT user_identities_provider_check;
ALTER TABLE user_identities ADD CONSTRAINT user_identities_provider_check
 CHECK (provider IN ('github', 'gitlab'));

CREATE TABLE gitlab_login_transactions (
 state_hash text PRIMARY KEY,
 origin text NOT NULL,
 browser_hash text NOT NULL,
 verifier text NOT NULL,
 return_to text NOT NULL,
 expires_at timestamptz NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
