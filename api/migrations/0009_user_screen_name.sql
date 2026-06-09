-- Optional user-chosen screen name, shown instead of the OAuth-provided name.
-- `display_name` keeps mirroring the OAuth profile name (refreshed on each login);
-- the effective display name is COALESCE(screen_name, display_name).
ALTER TABLE app_user ADD COLUMN screen_name TEXT;
