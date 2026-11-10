-- A provider that no longer accepts a channel's token revokes its access, per
-- 00004 D9.
ALTER TABLE pipewhere.channel
    DROP CONSTRAINT channel_status_check,
    ADD CONSTRAINT channel_status_check CHECK (
        status IN ('active', 'disconnected', 'access_revoked')
    );
