-- 00004 D2: every provider's channel shares the handle and status. Channels
-- stored before 00004 hold no credentials, so they start disconnected, and so
-- does a row inserted without a status.
ALTER TABLE pipewhere.channel
    ADD COLUMN handle text,
    ADD COLUMN status text NOT NULL DEFAULT 'disconnected'
        CHECK (status IN ('active', 'disconnected'));

-- 00004 D4: what belongs to Mastodon's adapter. Secrets are sealed by
-- src/credentials.rs, per 00004 D5.
CREATE TABLE pipewhere.mastodon_instance (
    host text PRIMARY KEY,
    redirect_uri text NOT NULL,
    client_id text NOT NULL,
    client_secret bytea NOT NULL
);

CREATE TABLE pipewhere.mastodon_authorization (
    id text PRIMARY KEY CHECK (pipewhere.is_typeid(id, 'mauth')),
    organization_id text NOT NULL REFERENCES auth.organization (id),
    host text NOT NULL REFERENCES pipewhere.mastodon_instance (host),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE pipewhere.mastodon_channel (
    channel_id text PRIMARY KEY REFERENCES pipewhere.channel (id),
    host text NOT NULL REFERENCES pipewhere.mastodon_instance (host),
    account_id text NOT NULL,
    access_token bytea NOT NULL
);
