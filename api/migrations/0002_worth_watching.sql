-- Add the third vote category, WORTH_WATCHING. Postgres allows
-- ALTER TYPE ... ADD VALUE inside a transaction on PG12+ as long as the new value
-- is not used in the same transaction; this migration only adds it.
ALTER TYPE vote_value ADD VALUE IF NOT EXISTS 'WORTH_WATCHING';
