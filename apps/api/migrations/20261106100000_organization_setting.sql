-- 00003 D1 and D11: an organization's default lateness tolerance. An organization
-- without a row uses 30 minutes, which src/settings.rs supplies. The range is
-- repeated there; `checked_tolerance` lists each copy.
CREATE TABLE pipewhere.organization_setting (
    organization_id text PRIMARY KEY REFERENCES auth.organization (id),
    tolerance_seconds integer NOT NULL CHECK (tolerance_seconds BETWEEN 300 AND 86400)
);
