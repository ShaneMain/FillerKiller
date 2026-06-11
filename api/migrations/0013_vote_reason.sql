-- Vote reason tags: an optional single-tag annotation the voter can attach to
-- their vote explaining why they voted that way. Tags are value-scoped (the set
-- of valid tags depends on the vote value). Stored as plain text with a CHECK
-- constraint that lists all nine allowed values across all vote categories.
--
-- Design notes:
--   • Nullable — the tag is always optional; absence means the voter didn't tag.
--   • A PUT that changes the vote value clears the reason automatically via the
--     application layer (reason is always re-set alongside value in the upsert).
--   • Account deletion already sets user_id to NULL (migration 0005); the reason
--     column rides along untouched — anonymous votes retain their reason for
--     aggregate accuracy, exactly as they retain their vote value.

ALTER TABLE vote
    ADD COLUMN reason TEXT
        CHECK (reason IN (
            -- FILLER reasons
            'recap',
            'side-story',
            'fun-but-skippable',
            -- WORTH_WATCHING reasons
            'self-contained-gem',
            'character-moment',
            'worldbuilding',
            -- CANON reasons
            'major-plot',
            'character-development',
            'arc-setup'
        ));
