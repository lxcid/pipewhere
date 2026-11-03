-- Adding a state means extending this check and PublicationState in
-- src/publications.rs together.
CREATE TABLE pipewhere.publication (
    id text PRIMARY KEY CHECK (pipewhere.is_typeid(id, 'pub')),
    organization_id text NOT NULL REFERENCES auth.organization (id),
    channel_id text NOT NULL REFERENCES pipewhere.channel (id),
    content_id text NOT NULL REFERENCES pipewhere.content (id),
    body text NOT NULL,
    state text NOT NULL CHECK (state IN (
        'scheduled', 'preparing', 'publishing', 'published', 'failed', 'outcome_unknown', 'cancelled'
    )),
    planned_at timestamptz NOT NULL
);

-- A key's row is inserted before its publication, in the same transaction, so
-- the reference is checked at commit.
CREATE TABLE pipewhere.idempotency_key (
    organization_id text NOT NULL REFERENCES auth.organization (id),
    key text NOT NULL,
    request_hash bytea NOT NULL,
    publication_id text NOT NULL REFERENCES pipewhere.publication (id) DEFERRABLE INITIALLY DEFERRED,
    PRIMARY KEY (organization_id, key)
);
