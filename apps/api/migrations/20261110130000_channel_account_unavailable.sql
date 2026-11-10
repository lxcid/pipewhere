-- An instance that reports a channel's account unavailable makes it unable to
-- publish, per 00004 D12.
ALTER TABLE pipewhere.channel
    DROP CONSTRAINT channel_status_check,
    ADD CONSTRAINT channel_status_check CHECK (
        status IN ('active', 'disconnected', 'access_revoked', 'account_unavailable')
    );
