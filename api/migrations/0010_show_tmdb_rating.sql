-- Cache TMDB's overall show rating, alongside the per-episode ratings (0006).
-- Nullable; backfilled by re-importing shows (the `refresh-catalog` subcommand).
ALTER TABLE show
    ADD COLUMN IF NOT EXISTS tmdb_vote_average DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS tmdb_vote_count   INTEGER;
