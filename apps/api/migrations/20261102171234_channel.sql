-- Adding a provider means extending this check and the Provider enum in
-- src/channels.rs together.
CREATE TABLE pipewhere.channel (
    id text PRIMARY KEY CHECK (pipewhere.is_typeid(id, 'chn')),
    organization_id text NOT NULL REFERENCES auth.organization (id),
    provider text NOT NULL CHECK (provider IN ('mastodon', 'threads'))
);
