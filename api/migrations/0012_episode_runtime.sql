-- Add nullable runtime (minutes) to the episode table.
-- Episodes heal as shows are refreshed from TMDB.
ALTER TABLE episode ADD COLUMN runtime_minutes INTEGER;
