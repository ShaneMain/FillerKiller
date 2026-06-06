-- FillerKiller initial schema. Mirrors the design notes.
-- Catalog (show/season/episode) is a TMDB cache; vote/episode_score is the
-- opinion layer we own. Keep in sync with the spec — changes there are spec
-- changes.

CREATE TYPE vote_value AS ENUM ('FILLER', 'CANON');

CREATE TABLE show (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id        INTEGER NOT NULL UNIQUE,
    name           TEXT NOT NULL,
    first_air_year INTEGER,
    poster_path    TEXT,
    overview       TEXT,
    last_synced_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX show_name_idx ON show (name);

CREATE TABLE season (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    show_id       UUID NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    name          TEXT,
    UNIQUE (show_id, season_number)
);

CREATE TABLE episode (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    show_id        UUID NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    season_id      UUID NOT NULL REFERENCES season (id) ON DELETE CASCADE,
    tmdb_id        INTEGER NOT NULL UNIQUE,
    season_number  INTEGER NOT NULL,
    episode_number INTEGER NOT NULL,
    name           TEXT,
    overview       TEXT,
    air_date       DATE,
    still_path     TEXT,
    UNIQUE (show_id, season_number, episode_number)
);
CREATE INDEX episode_show_idx ON episode (show_id);

CREATE TABLE app_user (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email        TEXT NOT NULL UNIQUE,
    display_name TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE vote (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
    episode_id UUID NOT NULL REFERENCES episode (id) ON DELETE CASCADE,
    value      vote_value NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One active vote per user per episode.
    UNIQUE (user_id, episode_id)
);
CREATE INDEX vote_episode_idx ON vote (episode_id);

-- Cached aggregate per episode. In MVP this can be recomputed live; promote to
-- a maintained table when read volume warrants.
CREATE TABLE episode_score (
    episode_id   UUID PRIMARY KEY REFERENCES episode (id) ON DELETE CASCADE,
    filler_votes INTEGER NOT NULL DEFAULT 0,
    canon_votes  INTEGER NOT NULL DEFAULT 0,
    filler_score DOUBLE PRECISION,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
