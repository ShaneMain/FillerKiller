-- Account deletion now DELETES the user's authored guides (0007 had them
-- anonymized/retained). Guides are titled, personal content, so they're removed
-- with the account. Votes are unaffected — they stay retained-anonymized (0005).
--
-- Switch skip_guide.author_id's FK from ON DELETE SET NULL to ON DELETE CASCADE.
-- The column stays nullable (no rows are ever NULL in practice now: guides are
-- always created with an author and are deleted, not orphaned, on account
-- removal). Entries and likes already cascade from skip_guide.
ALTER TABLE skip_guide DROP CONSTRAINT skip_guide_author_id_fkey;

ALTER TABLE skip_guide
    ADD CONSTRAINT skip_guide_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES app_user (id) ON DELETE CASCADE;
