-- An attempt belongs to its publication's organization, per 00002 D6, so it
-- holds no organization of its own. Adding a result means extending this check
-- and AttemptResult in src/publications.rs together.
CREATE TABLE pipewhere.publication_attempt (
    id text PRIMARY KEY CHECK (pipewhere.is_typeid(id, 'pubat')),
    publication_id text NOT NULL REFERENCES pipewhere.publication (id),
    result text CHECK (result IN ('published', 'failed', 'outcome_unknown')),
    reason text,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    -- A running attempt has neither, and an ended one has both.
    CONSTRAINT publication_attempt_ended_check CHECK ((result IS NULL) = (ended_at IS NULL)),
    -- 00002 D4: the operator reads why an attempt failed or ended unknown.
    CONSTRAINT publication_attempt_reason_check CHECK (
        result IS NULL OR result = 'published' OR reason IS NOT NULL
    )
);
