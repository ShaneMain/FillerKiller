-- Cache TMDB's own per-episode rating alongside our crowd-sourced verdict.
-- Nullable: existing rows backfill on their next TMDB sync, and TMDB reports
-- no rating (count 0 / absent) for unaired or unrated episodes.
ALTER TABLE episode
    ADD COLUMN IF NOT EXISTS tmdb_vote_average DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS tmdb_vote_count   INTEGER;
