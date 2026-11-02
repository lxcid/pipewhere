-- Each table's id check: CHECK (pipewhere.is_typeid(id, '<prefix>')), with the
-- prefix listed for that table in PREFIXES in src/ids.rs.
CREATE FUNCTION pipewhere.is_typeid(id text, prefix text) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
RETURN prefix ~ '^[a-z]{3,5}$'
    AND id ~ ('^' || prefix || '_[0-7][0-9a-hjkmnp-tv-z]{25}$');
