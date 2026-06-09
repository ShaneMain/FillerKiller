-- Account deletion KEEPS votes. When a user deletes their account we dissociate
-- their votes (user_id -> NULL) instead of cascading the delete, so the
-- community totals and the maintained `episode_score` stay intact — the vote
-- simply loses its owner.
--
-- user_id becomes nullable and the foreign key switches ON DELETE CASCADE ->
-- ON DELETE SET NULL. NULLs are distinct in the UNIQUE(user_id, episode_id)
-- index, so any number of anonymized votes can coexist on one episode, while the
-- one-vote-per-user rule still holds for live (non-null) users.
ALTER TABLE vote ALTER COLUMN user_id DROP NOT NULL;

ALTER TABLE vote DROP CONSTRAINT vote_user_id_fkey;

ALTER TABLE vote
    ADD CONSTRAINT vote_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES app_user (id) ON DELETE SET NULL;
