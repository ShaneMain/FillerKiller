-- Provider-keyed OAuth identities + session revocation support.
--
-- Identity was previously keyed on email alone (app_user.email UNIQUE): any
-- provider asserting a given verified email landed on the same account, so a
-- future provider with weaker email verification could take over an existing
-- account. Bind logins to the provider's stable subject id ((provider,
-- subject)) instead; email becomes profile data, used once to auto-link a
-- first login from a new provider to an existing account.

-- Emails are compared case-insensitively from now on; fold existing rows.
-- (Safe on current data: emails were only ever provider-verified or seeded,
-- and no case-variant duplicates exist.)
UPDATE app_user SET email = lower(btrim(email));

-- Belt-and-braces: catches any future code path that skips normalization.
CREATE UNIQUE INDEX app_user_email_lower_key ON app_user (lower(email));

-- Bumping token_version invalidates all of a user's outstanding sessions:
-- each JWT carries the version it was issued with, and verification rejects a
-- mismatch.
ALTER TABLE app_user ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0;

CREATE TABLE oauth_identity (
    provider   TEXT NOT NULL,
    -- The provider's stable user id (Google `sub`, GitHub numeric `id`) —
    -- unlike email, it never changes for a given provider account.
    subject    TEXT NOT NULL,
    user_id    UUID NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, subject)
);
CREATE INDEX oauth_identity_user_idx ON oauth_identity (user_id);

-- Make episode slot-uniqueness deferrable. The import re-matches existing
-- episodes by their stable tmdb_id and updates `episode_number` in place, so a
-- TMDB renumber/shift transiently puts two rows in one (show, season, number)
-- slot mid-statement before settling to a consistent final state. A deferred
-- check validates only at COMMIT — once every episode holds its final number —
-- so the refresh succeeds without recreating rows (votes, which FK to the
-- episode id, are preserved). The name is Postgres's inline-constraint default
-- from 0001.
ALTER TABLE episode
    DROP CONSTRAINT episode_show_id_season_number_episode_number_key,
    ADD CONSTRAINT episode_show_id_season_number_episode_number_key
        UNIQUE (show_id, season_number, episode_number) DEFERRABLE INITIALLY DEFERRED;
