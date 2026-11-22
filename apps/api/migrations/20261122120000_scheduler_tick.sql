-- The installation's last successful poller tick, per 00003 D10.
CREATE TABLE pipewhere.scheduler_tick (
    singleton boolean PRIMARY KEY CHECK (singleton),
    last_completed_at timestamptz NOT NULL
);
