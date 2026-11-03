CREATE TABLE pipewhere.content (
    id text PRIMARY KEY CHECK (pipewhere.is_typeid(id, 'cnt')),
    organization_id text NOT NULL REFERENCES auth.organization (id),
    body text NOT NULL
);
