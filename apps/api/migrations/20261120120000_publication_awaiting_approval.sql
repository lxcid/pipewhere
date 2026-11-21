-- An agent's publication waits for a person to approve it, per 00010 D4.
ALTER TABLE pipewhere.publication
    DROP CONSTRAINT publication_state_check,
    ADD CONSTRAINT publication_state_check CHECK (state IN (
        'awaiting_approval', 'scheduled', 'preparing', 'publishing', 'published', 'failed',
        'outcome_unknown', 'cancelled'
    ));
