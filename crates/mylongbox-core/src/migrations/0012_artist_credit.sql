-- An artist credit, for art credited as a whole when who pencilled and
-- who inked is not known.
--
-- SQLite cannot change a column's CHECK in place, so the table is
-- rebuilt: made anew under a working name, filled from the old one,
-- which is then dropped, and renamed. Nothing refers to the table, so
-- foreign keys elsewhere are untouched. Every row is kept as it was.

CREATE TABLE credit_rebuilt (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK (role IN (
        'writer', 'artist', 'penciller', 'inker', 'colorist', 'letterer', 'cover_artist',
        'editor',
        -- Magazines.
        'article', 'interview'
    )),
    PRIMARY KEY (issue_id, creator_id, role)
) STRICT;

INSERT INTO credit_rebuilt (issue_id, creator_id, role)
SELECT issue_id, creator_id, role
FROM credit;

DROP TABLE credit;
ALTER TABLE credit_rebuilt RENAME TO credit;
CREATE INDEX credit_creator ON credit (creator_id, role);
