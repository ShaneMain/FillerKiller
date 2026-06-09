-- Give each show a URL slug so links read as the title ("star-trek-the-next-
-- generation") instead of an opaque UUID. UUIDs still resolve for back-compat.

ALTER TABLE show ADD COLUMN slug TEXT;

-- Backfill: slugify(name), then guarantee uniqueness by checking the slugs we've
-- already assigned and suffixing with "-<tmdb_id>" (then "-<tmdb_id>-N") until a
-- free one is found. This mirrors db::pick_unique_slug exactly, and — because it
-- tests the actual column rather than assuming a suffixed form is free — it can't
-- produce a duplicate even when one show's suffixed slug equals another's bare
-- slug (e.g. "Foo" #100 → "foo-100" vs a show literally named "Foo 100").
-- Names that slugify to empty (e.g. all-symbol) fall back to the tmdb id.
DO $$
DECLARE
    r         RECORD;
    base      TEXT;
    candidate TEXT;
    n         INT;
BEGIN
    FOR r IN SELECT id, tmdb_id, name FROM show ORDER BY tmdb_id LOOP
        base := NULLIF(trim(both '-' from regexp_replace(lower(r.name), '[^a-z0-9]+', '-', 'g')), '');
        IF base IS NULL THEN
            base := r.tmdb_id::text;
        END IF;
        candidate := base;
        n := 0;
        WHILE EXISTS (SELECT 1 FROM show WHERE slug = candidate) LOOP
            n := n + 1;
            candidate := CASE WHEN n = 1
                THEN base || '-' || r.tmdb_id
                ELSE base || '-' || r.tmdb_id || '-' || n END;
        END LOOP;
        UPDATE show SET slug = candidate WHERE id = r.id;
    END LOOP;
END $$;

ALTER TABLE show ALTER COLUMN slug SET NOT NULL;
CREATE UNIQUE INDEX show_slug_key ON show (slug);
