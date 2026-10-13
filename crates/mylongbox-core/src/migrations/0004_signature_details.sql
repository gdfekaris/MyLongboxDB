-- Signatures: CBCS's witnessed program, where on the book each one is,
-- and a photo of a signature inside the book.
--
-- SQLite cannot change a column's CHECK in place, so the two tables are
-- rebuilt: made anew under a working name, filled from the old one,
-- which is then dropped, and renamed. Nothing refers to either table,
-- so foreign keys elsewhere are untouched. Every row is kept as it was;
-- placement starts empty.

CREATE TABLE signature_rebuilt (
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 1),
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    verification TEXT NOT NULL CHECK (verification IN
        ('unverified', 'witnessed', 'coa', 'jsa', 'cgc_ss', 'cgc_jsa', 'cbcs',
         'cbcs_witnessed', 'pgx')),
    cert_number TEXT CHECK (cert_number <> ''),
    signed_on TEXT CHECK (signed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    place TEXT NOT NULL DEFAULT '',
    sketch INTEGER NOT NULL DEFAULT 0 CHECK (sketch IN (0, 1)),
    -- Where on the book it is signed.
    placement TEXT CHECK (placement IN ('front_cover', 'inside_front_cover', 'first_page',
        'interior_page', 'inside_back_cover', 'back_cover')),
    PRIMARY KEY (copy_id, position)
) STRICT;

INSERT INTO signature_rebuilt (copy_id, position, creator_id, verification, cert_number,
    signed_on, place, sketch)
SELECT copy_id, position, creator_id, verification, cert_number, signed_on, place, sketch
FROM signature;

DROP TABLE signature;
ALTER TABLE signature_rebuilt RENAME TO signature;
CREATE INDEX signature_creator ON signature (creator_id);

-- A copy may have a photo of a signature as well as its two covers.
CREATE TABLE cover_photo_rebuilt (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    side TEXT NOT NULL CHECK (side IN ('front', 'back', 'signature')),
    full_jpeg BLOB NOT NULL,
    thumb_jpeg BLOB NOT NULL,
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    UNIQUE (copy_id, side)
) STRICT;

INSERT INTO cover_photo_rebuilt (id, uuid, copy_id, side, full_jpeg, thumb_jpeg, width,
    height, sha256)
SELECT id, uuid, copy_id, side, full_jpeg, thumb_jpeg, width, height, sha256
FROM cover_photo;

DROP TABLE cover_photo;
ALTER TABLE cover_photo_rebuilt RENAME TO cover_photo;
