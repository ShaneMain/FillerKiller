-- Maintain `episode_score` as a denormalized per-episode vote tally so the read
-- path is a single indexed lookup instead of aggregating the whole `vote` table
-- on every request.
--
-- This table stores RAW COUNTS only. The displayed status and the canonical
-- filler_score are still derived in code (scoring.rs) from these counts — that
-- stays the single source of truth. `filler_score` is kept here
-- purely as a denormalized convenience for any future SQL-side sorting; the API
-- never reads it.

-- 0002 added WORTH_WATCHING; the score table predates it.
ALTER TABLE episode_score
    ADD COLUMN IF NOT EXISTS worth_watching_votes INTEGER NOT NULL DEFAULT 0;

-- Recount one episode from the source `vote` rows and upsert its score row.
-- An episode with no votes still yields a single zero row (aggregate without
-- GROUP BY always returns one row), so deleting the last vote leaves zeros
-- rather than a stale tally.
CREATE OR REPLACE FUNCTION refresh_episode_score(ep UUID)
RETURNS void AS $$
    INSERT INTO episode_score (
        episode_id, filler_votes, worth_watching_votes, canon_votes,
        filler_score, updated_at
    )
    SELECT
        ep,
        COUNT(*) FILTER (WHERE value = 'FILLER'),
        COUNT(*) FILTER (WHERE value = 'WORTH_WATCHING'),
        COUNT(*) FILTER (WHERE value = 'CANON'),
        CASE WHEN COUNT(*) = 0 THEN NULL
             ELSE COUNT(*) FILTER (WHERE value = 'FILLER')::float8 / COUNT(*)
        END,
        now()
    FROM vote WHERE episode_id = ep
    ON CONFLICT (episode_id) DO UPDATE SET
        filler_votes         = EXCLUDED.filler_votes,
        worth_watching_votes = EXCLUDED.worth_watching_votes,
        canon_votes          = EXCLUDED.canon_votes,
        filler_score         = EXCLUDED.filler_score,
        updated_at           = EXCLUDED.updated_at;
$$ LANGUAGE sql;

-- Keep the tally current on every vote write. The trigger runs inside the same
-- statement's transaction as the vote change, so a read right after a vote sees
-- the fresh counts.
CREATE OR REPLACE FUNCTION trg_refresh_episode_score()
RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM refresh_episode_score(OLD.episode_id);
        RETURN OLD;
    END IF;
    -- INSERT or UPDATE. A vote never moves between episodes in practice, but
    -- refresh both sides if it ever did.
    IF TG_OP = 'UPDATE' AND NEW.episode_id IS DISTINCT FROM OLD.episode_id THEN
        PERFORM refresh_episode_score(OLD.episode_id);
    END IF;
    PERFORM refresh_episode_score(NEW.episode_id);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS vote_refresh_score ON vote;
CREATE TRIGGER vote_refresh_score
    AFTER INSERT OR UPDATE OR DELETE ON vote
    FOR EACH ROW EXECUTE FUNCTION trg_refresh_episode_score();

-- Backfill from any votes that already exist (episodes with no votes are left
-- out; the read path COALESCEs a missing row to zero).
INSERT INTO episode_score (
    episode_id, filler_votes, worth_watching_votes, canon_votes,
    filler_score, updated_at
)
SELECT
    e.id,
    COUNT(v.id) FILTER (WHERE v.value = 'FILLER'),
    COUNT(v.id) FILTER (WHERE v.value = 'WORTH_WATCHING'),
    COUNT(v.id) FILTER (WHERE v.value = 'CANON'),
    CASE WHEN COUNT(v.id) = 0 THEN NULL
         ELSE COUNT(v.id) FILTER (WHERE v.value = 'FILLER')::float8 / COUNT(v.id)
    END,
    now()
FROM episode e
JOIN vote v ON v.episode_id = e.id
GROUP BY e.id
ON CONFLICT (episode_id) DO UPDATE SET
    filler_votes         = EXCLUDED.filler_votes,
    worth_watching_votes = EXCLUDED.worth_watching_votes,
    canon_votes          = EXCLUDED.canon_votes,
    filler_score         = EXCLUDED.filler_score,
    updated_at           = EXCLUDED.updated_at;
