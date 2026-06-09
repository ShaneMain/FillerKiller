-- User-authored skip guides: a curated per-episode verdict for a show that
-- other users can browse, share, and like. Distinct from the algorithmic skip
-- guide (derived live from votes in scoring.rs).

CREATE TYPE guide_disposition AS ENUM ('WATCH', 'OPTIONAL', 'SKIP');

CREATE TABLE skip_guide (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    show_id      UUID NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    -- Author is retained-but-anonymized on account deletion (mirrors votes):
    -- deleting an account dissociates their guides rather than removing them.
    author_id    UUID REFERENCES app_user (id) ON DELETE SET NULL,
    title        TEXT NOT NULL,
    description  TEXT,
    is_published BOOLEAN NOT NULL DEFAULT FALSE,
    -- Denormalized like tally, kept current by a trigger (see below).
    like_count   INTEGER NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX skip_guide_show_idx ON skip_guide (show_id);
CREATE INDEX skip_guide_author_idx ON skip_guide (author_id);

CREATE TABLE skip_guide_entry (
    guide_id    UUID NOT NULL REFERENCES skip_guide (id) ON DELETE CASCADE,
    episode_id  UUID NOT NULL REFERENCES episode (id) ON DELETE CASCADE,
    disposition guide_disposition NOT NULL,
    PRIMARY KEY (guide_id, episode_id)
);

CREATE TABLE skip_guide_like (
    guide_id   UUID NOT NULL REFERENCES skip_guide (id) ON DELETE CASCADE,
    -- A like is ephemeral; deleting the user removes their likes (the trigger
    -- decrements the count). One like per user per guide.
    user_id    UUID NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guide_id, user_id)
);

-- Keep skip_guide.like_count current (mirrors the episode_score trigger).
CREATE OR REPLACE FUNCTION refresh_guide_like_count(g UUID)
RETURNS void AS $$
    UPDATE skip_guide
    SET like_count = (SELECT COUNT(*) FROM skip_guide_like WHERE guide_id = g)
    WHERE id = g;
$$ LANGUAGE sql;

CREATE OR REPLACE FUNCTION trg_refresh_guide_like_count()
RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM refresh_guide_like_count(OLD.guide_id);
        RETURN OLD;
    END IF;
    PERFORM refresh_guide_like_count(NEW.guide_id);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS guide_like_refresh ON skip_guide_like;
CREATE TRIGGER guide_like_refresh
    AFTER INSERT OR DELETE ON skip_guide_like
    FOR EACH ROW EXECUTE FUNCTION trg_refresh_guide_like_count();
