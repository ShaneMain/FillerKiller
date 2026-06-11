-- Watch progress: a user can mark episodes as watched. Personal data with no
-- aggregate value, so both FKs use ON DELETE CASCADE: deleting a user or
-- removing an episode drops all associated watch rows automatically.
--
-- The episode FK cascade satisfies the "delete with the account" requirement
-- stated in the spec WITHOUT any extra application code in the delete_user path:
-- `DELETE FROM app_user WHERE id = $1` cascades through
-- app_user → episode_watch (via user_id FK) and
-- episode   → episode_watch (via episode_id FK).
--
-- No trigger or maintained tally is needed: watch progress is per-user and
-- queried directly (a COUNT for the summary, a correlated EXISTS for per-episode).
CREATE TABLE episode_watch (
    user_id    UUID NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
    episode_id UUID NOT NULL REFERENCES episode   (id) ON DELETE CASCADE,
    watched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, episode_id)
);

-- Fast lookup by user for the COUNT-per-show query (covers the
-- `WHERE user_id = $1` in watch_count_for_show).
CREATE INDEX episode_watch_user_idx ON episode_watch (user_id);
