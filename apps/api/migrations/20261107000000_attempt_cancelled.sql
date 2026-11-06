-- A run that finds its publication cancelled ends its attempt with a result of
-- its own, per 00003 D7. Nothing went wrong, so it needs no reason.
ALTER TABLE pipewhere.publication_attempt
    DROP CONSTRAINT publication_attempt_result_check,
    ADD CONSTRAINT publication_attempt_result_check CHECK (
        result IN ('published', 'failed', 'outcome_unknown', 'cancelled')
    ),
    DROP CONSTRAINT publication_attempt_reason_check,
    ADD CONSTRAINT publication_attempt_reason_check CHECK (
        result IS NULL OR result IN ('published', 'cancelled') OR reason IS NOT NULL
    );
