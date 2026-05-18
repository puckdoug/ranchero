CREATE TABLE athletes (
    id         INTEGER PRIMARY KEY NOT NULL,
    fname      TEXT,
    lname      TEXT,
    ftp        INTEGER,
    weight     REAL,
    badges     TEXT,
    last_seen  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX athletes_last_seen ON athletes (last_seen DESC);
