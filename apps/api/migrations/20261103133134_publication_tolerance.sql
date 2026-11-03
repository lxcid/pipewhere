-- 00003 D1 and D11: how late a publication's provider commit may start. The
-- range is checked in src/publications.rs too; change the two together.
-- Existing publications get the 30 minutes each would have recorded, per 00002
-- D12. The default then goes, so a writer that omits the tolerance fails
-- instead of quietly taking 30 minutes.
ALTER TABLE pipewhere.publication
    ADD COLUMN tolerance_seconds integer NOT NULL DEFAULT 1800
        CHECK (tolerance_seconds BETWEEN 300 AND 86400);
ALTER TABLE pipewhere.publication ALTER COLUMN tolerance_seconds DROP DEFAULT;
