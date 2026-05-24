-- Migrate athletes from fixed-column schema to JSON-blob schema.
--
-- Existing rows are converted: fname/lname/ftp/weight are folded into a JSON
-- object stored in the data column.  The old column-per-field layout is dropped.
-- last_seen index is recreated on the renamed table.

CREATE TABLE athletes_new (
    id        INTEGER PRIMARY KEY NOT NULL,
    data      TEXT NOT NULL DEFAULT '{}',
    last_seen INTEGER NOT NULL DEFAULT 0
);

INSERT INTO athletes_new (id, data, last_seen)
SELECT
    id,
    json_object('firstName', fname, 'lastName', lname, 'ftp', ftp, 'weight', weight),
    last_seen
FROM athletes;

DROP TABLE athletes;
ALTER TABLE athletes_new RENAME TO athletes;
CREATE INDEX athletes_last_seen ON athletes (last_seen DESC);
