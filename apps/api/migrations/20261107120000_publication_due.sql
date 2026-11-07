-- Worker's poller reads this every 5 seconds, per 00003 D5. It holds only the
-- publications still waiting, so it stays as small as the waiting work.
CREATE INDEX publication_due ON pipewhere.publication (planned_at)
    WHERE state = 'scheduled';
