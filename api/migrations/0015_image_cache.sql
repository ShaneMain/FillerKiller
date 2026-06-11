-- Server-side cache of TMDB images, served same-origin via
-- GET /img/t/p/{size}/{file}. `path` is the cache key "{size}/{file}",
-- e.g. "w154/lP4zwr0F7hWTbAFltfoFTc2AxRG.jpg".
--
-- TMDB image paths are content-unique (new art gets a new file name), so a
-- cached body can never go stale — rows are evicted for storage hygiene only:
-- pinned rows (poster files of the currently-popular shows, any size) are kept
-- indefinitely; unpinned rows expire after a TTL. Both the pin set and the
-- pruning are maintained by the popular-shows read path (see
-- db::sync_pinned_images).
CREATE TABLE image_cache (
    path         TEXT PRIMARY KEY,
    content_type TEXT NOT NULL,
    body         BYTEA NOT NULL,
    pinned       BOOLEAN NOT NULL DEFAULT FALSE,
    fetched_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Serves the prune scan (unpinned, oldest first).
CREATE INDEX image_cache_prune_idx ON image_cache (pinned, fetched_at);
