CREATE TABLE leaderboards (
    segment_id   INTEGER PRIMARY KEY NOT NULL,
    payload      BLOB    NOT NULL,
    inserted_at  INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL
);
CREATE INDEX leaderboards_expires_at ON leaderboards (expires_at);
