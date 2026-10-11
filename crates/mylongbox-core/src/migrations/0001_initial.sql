-- Initial schema.
--
-- Conventions:
-- * Every table is STRICT, so a value of the wrong type is an error.
-- * Entity tables carry a UUID that names the row in exports and matches
--   it on re-import; the integer id is only for joins.
-- * Money is a whole number of the currency's smallest unit plus its
--   code: USD cents, XAU ten-thousandths of a troy ounce, BTC satoshis.
-- * Dates are YYYY-MM-DD text; timestamps are UTC YYYY-MM-DDTHH:MM:SSZ.
-- * Grades are tenths: 9.8 is stored as 98.
-- * Flags are 0 or 1.
-- * Series, issues, editions, and copies are moved to Trash by setting
--   trash_id; children trashed with a parent share its trash_id, so the
--   group restores together. Deleting a trash row deletes every row in
--   its group, and the cascades below remove everything beneath them, so
--   restoring must clear trash_id before it deletes the trash row.

CREATE TABLE trash (
    id INTEGER PRIMARY KEY,
    trashed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    -- What the user deleted, for the Trash list, e.g. "Amazing Spider-Man #121".
    label TEXT NOT NULL
) STRICT;

CREATE TABLE publisher (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    notes TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE series (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL CHECK (name <> ''),
    category TEXT NOT NULL CHECK (category IN ('comic', 'magazine', 'collected')),
    publisher_id INTEGER REFERENCES publisher (id) ON DELETE RESTRICT,
    volume INTEGER CHECK (volume >= 1),
    start_year INTEGER CHECK (start_year BETWEEN 1 AND 9999),
    -- For gap detection; unknown when NULL.
    known_issue_count INTEGER CHECK (known_issue_count >= 0),
    -- Magazines only.
    magazine_format TEXT CHECK (magazine_format IN ('standard', 'treasury', 'digest')),
    black_and_white INTEGER CHECK (black_and_white IN (0, 1)),
    notes TEXT NOT NULL DEFAULT '',
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE,
    CHECK (category = 'magazine' OR (magazine_format IS NULL AND black_and_white IS NULL))
) STRICT;

CREATE INDEX series_publisher ON series (publisher_id);
CREATE INDEX series_trash ON series (trash_id);
CREATE INDEX series_name ON series (name COLLATE NOCASE);

CREATE TABLE issue (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    series_id INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    -- As typed. NULL only for a standalone collected edition.
    number TEXT CHECK (number <> ''),
    -- IssueNumber::sort_key, or '' when number is NULL.
    number_sort TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    cover_year INTEGER CHECK (cover_year BETWEEN 1 AND 9999),
    cover_month INTEGER CHECK (cover_month BETWEEN 1 AND 12),
    release_date TEXT CHECK (release_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    page_count INTEGER CHECK (page_count >= 0),
    synopsis TEXT NOT NULL DEFAULT '',
    is_key INTEGER NOT NULL DEFAULT 0 CHECK (is_key IN (0, 1)),
    key_reason TEXT NOT NULL DEFAULT '',
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE,
    CHECK (cover_month IS NULL OR cover_year IS NOT NULL),
    CHECK ((number IS NULL) = (number_sort = ''))
) STRICT;

-- One live issue per number in a series. "Annual 3" and "annual 3" share
-- a sort key, so they count as the same number.
CREATE UNIQUE INDEX issue_number_in_series
    ON issue (series_id, number_sort)
    WHERE number IS NOT NULL AND trash_id IS NULL;
CREATE INDEX issue_series_order ON issue (series_id, number_sort);
CREATE INDEX issue_trash ON issue (trash_id);
CREATE INDEX issue_cover_date ON issue (cover_year, cover_month);

CREATE TABLE edition (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    -- e.g. "Cover B", "1:25 incentive"; '' for the standard cover.
    variant TEXT NOT NULL DEFAULT '',
    printing INTEGER CHECK (printing >= 1),
    distribution TEXT CHECK (distribution IN ('direct', 'newsstand')),
    cover_price_units INTEGER CHECK (cover_price_units >= 0),
    cover_price_currency TEXT CHECK (cover_price_currency IN ('USD', 'XAU', 'BTC')),
    barcode TEXT,
    -- Collected editions only.
    isbn TEXT CHECK (isbn GLOB '[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]'),
    binding TEXT CHECK (binding IN ('hardcover', 'softcover', 'oversized_hardcover')),
    has_dust_jacket INTEGER CHECK (has_dust_jacket IN (0, 1)),
    has_slipcase INTEGER CHECK (has_slipcase IN (0, 1)),
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE,
    CHECK ((cover_price_units IS NULL) = (cover_price_currency IS NULL))
) STRICT;

CREATE INDEX edition_issue ON edition (issue_id);
CREATE INDEX edition_trash ON edition (trash_id);
CREATE INDEX edition_isbn ON edition (isbn);

CREATE TABLE storage_box (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    notes TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE copy (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    edition_id INTEGER NOT NULL REFERENCES edition (id) ON DELETE CASCADE,
    -- Comics and magazines.
    grade INTEGER CHECK (grade BETWEEN 5 AND 100),
    grade_type TEXT CHECK (grade_type IN ('raw', 'cgc', 'cbcs', 'pgx')),
    cert_number TEXT,
    label_type TEXT,
    -- Collected editions.
    book_condition TEXT CHECK (book_condition IN
        ('sealed', 'new', 'like_new', 'very_good', 'good', 'fair', 'poor')),
    dust_jacket_condition TEXT CHECK (dust_jacket_condition IN
        ('sealed', 'new', 'like_new', 'very_good', 'good', 'fair', 'poor')),
    purchase_price_units INTEGER CHECK (purchase_price_units >= 0),
    purchase_price_currency TEXT CHECK (purchase_price_currency IN ('USD', 'XAU', 'BTC')),
    purchase_date TEXT CHECK (purchase_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    seller TEXT NOT NULL DEFAULT '',
    box_id INTEGER REFERENCES storage_box (id) ON DELETE RESTRICT,
    box_position INTEGER CHECK (box_position >= 1),
    notes TEXT NOT NULL DEFAULT '',
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE,
    CHECK ((purchase_price_units IS NULL) = (purchase_price_currency IS NULL)),
    CHECK (box_position IS NULL OR box_id IS NOT NULL)
) STRICT;

CREATE INDEX copy_edition ON copy (edition_id);
CREATE INDEX copy_box ON copy (box_id, box_position);
CREATE INDEX copy_trash ON copy (trash_id);
CREATE INDEX copy_grade ON copy (grade);
CREATE INDEX copy_purchase_date ON copy (purchase_date);
CREATE INDEX copy_added_at ON copy (added_at);

-- Each signature on a copy, in the order signed. A signer is a creator,
-- so alternate names match and a creator's shelf includes what they
-- signed; the same person may sign one copy more than once.
CREATE TABLE signature (
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 1),
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    -- How the signature is known to be genuine: 'unverified', 'witnessed'
    -- (by the owner), 'coa' (another issuer's certificate), 'jsa' (a JSA
    -- letter or sticker on a raw book), 'cgc_ss' (CGC Signature Series),
    -- 'cgc_jsa' (CGC Signature Verified through JSA), 'cbcs' (CBCS
    -- Verified Signature), 'pgx' (PGX Signature Verified).
    verification TEXT NOT NULL CHECK (verification IN
        ('unverified', 'witnessed', 'coa', 'jsa', 'cgc_ss', 'cgc_jsa', 'cbcs', 'pgx')),
    -- A certificate's number, when it has one.
    cert_number TEXT CHECK (cert_number <> ''),
    signed_on TEXT CHECK (signed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    -- Where it was signed, e.g. "NYCC 2015".
    place TEXT NOT NULL DEFAULT '',
    -- A sketch (remark) drawn with the signature.
    sketch INTEGER NOT NULL DEFAULT 0 CHECK (sketch IN (0, 1)),
    PRIMARY KEY (copy_id, position)
) STRICT;

CREATE INDEX signature_creator ON signature (creator_id);

CREATE TABLE value_entry (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    date TEXT NOT NULL CHECK (date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    value_units INTEGER NOT NULL CHECK (value_units >= 0),
    value_currency TEXT NOT NULL CHECK (value_currency IN ('USD', 'XAU', 'BTC')),
    -- Free text, e.g. "GoCollect", "eBay sold".
    source TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE INDEX value_entry_copy_date ON value_entry (copy_id, date);

-- Kept apart from copy so that searches never read image data.
CREATE TABLE cover_photo (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    side TEXT NOT NULL CHECK (side IN ('front', 'back')),
    full_jpeg BLOB NOT NULL,
    thumb_jpeg BLOB NOT NULL,
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    UNIQUE (copy_id, side)
) STRICT;

CREATE TABLE creator (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

CREATE TABLE creator_alias (
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (creator_id, alias)
) STRICT;

CREATE INDEX creator_alias_name ON creator_alias (alias);

CREATE TABLE credit (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK (role IN (
        'writer', 'penciller', 'inker', 'colorist', 'letterer', 'cover_artist', 'editor',
        -- Magazines.
        'article', 'interview'
    )),
    PRIMARY KEY (issue_id, creator_id, role)
) STRICT;

CREATE INDEX credit_creator ON credit (creator_id, role);

-- Cover artists can differ between variants of the same issue.
CREATE TABLE edition_cover_artist (
    edition_id INTEGER NOT NULL REFERENCES edition (id) ON DELETE CASCADE,
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    PRIMARY KEY (edition_id, creator_id)
) STRICT;

CREATE INDEX edition_cover_artist_creator ON edition_cover_artist (creator_id);

CREATE TABLE character (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

CREATE TABLE character_alias (
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (character_id, alias)
) STRICT;

CREATE INDEX character_alias_name ON character_alias (alias);

CREATE TABLE issue_character (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE RESTRICT,
    first_appearance INTEGER NOT NULL DEFAULT 0 CHECK (first_appearance IN (0, 1)),
    PRIMARY KEY (issue_id, character_id)
) STRICT;

CREATE INDEX issue_character_character ON issue_character (character_id);

CREATE TABLE team (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

CREATE TABLE team_alias (
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (team_id, alias)
) STRICT;

CREATE INDEX team_alias_name ON team_alias (alias);

CREATE TABLE issue_team (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE RESTRICT,
    first_appearance INTEGER NOT NULL DEFAULT 0 CHECK (first_appearance IN (0, 1)),
    PRIMARY KEY (issue_id, team_id)
) STRICT;

CREATE INDEX issue_team_team ON issue_team (team_id);

CREATE TABLE story_arc (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

CREATE TABLE story_arc_alias (
    story_arc_id INTEGER NOT NULL REFERENCES story_arc (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (story_arc_id, alias)
) STRICT;

CREATE INDEX story_arc_alias_name ON story_arc_alias (alias);

CREATE TABLE issue_story_arc (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    story_arc_id INTEGER NOT NULL REFERENCES story_arc (id) ON DELETE RESTRICT,
    -- Part number within the arc, when known.
    part INTEGER CHECK (part >= 1),
    PRIMARY KEY (issue_id, story_arc_id)
) STRICT;

CREATE INDEX issue_story_arc_arc ON issue_story_arc (story_arc_id, part);

CREATE TABLE tag (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

CREATE TABLE copy_tag (
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag (id) ON DELETE CASCADE,
    PRIMARY KEY (copy_id, tag_id)
) STRICT;

CREATE INDEX copy_tag_tag ON copy_tag (tag_id);

-- The issues a collected edition reprints, in reading order. The
-- collecting issue is a volume of a 'collected' series; the collected
-- issues are ordinary catalog issues, owned or not.
CREATE TABLE collects (
    collection_issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    -- NO ACTION rather than RESTRICT: SQLite checks it once the whole
    -- statement is done, so emptying Trash may delete a collected edition
    -- together with an issue it reprints, but never the issue alone.
    collected_issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE NO ACTION,
    position INTEGER NOT NULL CHECK (position >= 1),
    PRIMARY KEY (collection_issue_id, collected_issue_id),
    CHECK (collection_issue_id <> collected_issue_id)
) STRICT;

CREATE INDEX collects_collected ON collects (collected_issue_id);

-- User-entered prices; the app cannot look them up.
CREATE TABLE exchange_rate (
    date TEXT PRIMARY KEY CHECK (date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    usd_cents_per_ounce INTEGER NOT NULL CHECK (usd_cents_per_ounce > 0),
    usd_cents_per_bitcoin INTEGER NOT NULL CHECK (usd_cents_per_bitcoin > 0)
) STRICT;



-- Quick search. `search_source` holds one row per searchable name or
-- text, kept in step with its source by the triggers below. Its id is the
-- source row's id times 16 plus its kind, so each source row has exactly
-- one; `owner` is the entry it finds books through (for an alternate
-- name, the entry it belongs to).
CREATE TABLE search_source (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    kind INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 14),
    owner INTEGER NOT NULL
) STRICT;

-- The index over it. Trigrams make every part of a word findable,
-- ignoring case and accents. Keeping the text in `search_source` lets the
-- index be emptied and rebuilt without reading what was in it. The
-- triggers below change both together.
CREATE VIRTUAL TABLE search_text USING fts5 (
    text,
    content = 'search_source',
    content_rowid = 'id',
    tokenize = 'trigram remove_diacritics 1'
);

CREATE TRIGGER search_series_insert AFTER INSERT ON series BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 1, new.name, 1, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 1, new.name);
END;

CREATE TRIGGER search_series_update AFTER UPDATE OF name ON series BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 1, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 1;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 1, new.name);
END;

CREATE TRIGGER search_series_delete AFTER DELETE ON series BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 1, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 1;
END;

CREATE TRIGGER search_issue_insert AFTER INSERT ON issue BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 2, new.title, 2, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 2, new.title);
END;

CREATE TRIGGER search_issue_update AFTER UPDATE OF title ON issue BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 2, old.title);
    UPDATE search_source SET text = new.title WHERE id = old.id * 16 + 2;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 2, new.title);
END;

CREATE TRIGGER search_issue_delete AFTER DELETE ON issue BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 2, old.title);
    DELETE FROM search_source WHERE id = old.id * 16 + 2;
END;

CREATE TRIGGER search_publisher_insert AFTER INSERT ON publisher BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 3, new.name, 3, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 3, new.name);
END;

CREATE TRIGGER search_publisher_update AFTER UPDATE OF name ON publisher BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 3, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 3;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 3, new.name);
END;

CREATE TRIGGER search_publisher_delete AFTER DELETE ON publisher BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 3, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 3;
END;

CREATE TRIGGER search_creator_insert AFTER INSERT ON creator BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 4, new.name, 4, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 4, new.name);
END;

CREATE TRIGGER search_creator_update AFTER UPDATE OF name ON creator BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 4, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 4;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 4, new.name);
END;

CREATE TRIGGER search_creator_delete AFTER DELETE ON creator BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 4, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 4;
END;

CREATE TRIGGER search_creator_alias_insert AFTER INSERT ON creator_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 5, new.alias, 5, new.creator_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 5, new.alias);
END;

CREATE TRIGGER search_creator_alias_update AFTER UPDATE OF alias, creator_id ON creator_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 5, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.creator_id WHERE id = old.rowid * 16 + 5;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 5, new.alias);
END;

CREATE TRIGGER search_creator_alias_delete AFTER DELETE ON creator_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 5, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 5;
END;

CREATE TRIGGER search_character_insert AFTER INSERT ON character BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 6, new.name, 6, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 6, new.name);
END;

CREATE TRIGGER search_character_update AFTER UPDATE OF name ON character BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 6, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 6;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 6, new.name);
END;

CREATE TRIGGER search_character_delete AFTER DELETE ON character BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 6, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 6;
END;

CREATE TRIGGER search_character_alias_insert AFTER INSERT ON character_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 7, new.alias, 7, new.character_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 7, new.alias);
END;

CREATE TRIGGER search_character_alias_update AFTER UPDATE OF alias, character_id ON character_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 7, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.character_id WHERE id = old.rowid * 16 + 7;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 7, new.alias);
END;

CREATE TRIGGER search_character_alias_delete AFTER DELETE ON character_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 7, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 7;
END;

CREATE TRIGGER search_team_insert AFTER INSERT ON team BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 8, new.name, 8, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 8, new.name);
END;

CREATE TRIGGER search_team_update AFTER UPDATE OF name ON team BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 8, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 8;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 8, new.name);
END;

CREATE TRIGGER search_team_delete AFTER DELETE ON team BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 8, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 8;
END;

CREATE TRIGGER search_team_alias_insert AFTER INSERT ON team_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 9, new.alias, 9, new.team_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 9, new.alias);
END;

CREATE TRIGGER search_team_alias_update AFTER UPDATE OF alias, team_id ON team_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 9, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.team_id WHERE id = old.rowid * 16 + 9;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 9, new.alias);
END;

CREATE TRIGGER search_team_alias_delete AFTER DELETE ON team_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 9, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 9;
END;

CREATE TRIGGER search_story_arc_insert AFTER INSERT ON story_arc BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 10, new.name, 10, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 10, new.name);
END;

CREATE TRIGGER search_story_arc_update AFTER UPDATE OF name ON story_arc BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 10, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 10;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 10, new.name);
END;

CREATE TRIGGER search_story_arc_delete AFTER DELETE ON story_arc BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 10, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 10;
END;

CREATE TRIGGER search_story_arc_alias_insert AFTER INSERT ON story_arc_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 11, new.alias, 11, new.story_arc_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 11, new.alias);
END;

CREATE TRIGGER search_story_arc_alias_update AFTER UPDATE OF alias, story_arc_id ON story_arc_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 11, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.story_arc_id WHERE id = old.rowid * 16 + 11;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 11, new.alias);
END;

CREATE TRIGGER search_story_arc_alias_delete AFTER DELETE ON story_arc_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 11, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 11;
END;

CREATE TRIGGER search_tag_insert AFTER INSERT ON tag BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 12, new.name, 12, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 12, new.name);
END;

CREATE TRIGGER search_tag_update AFTER UPDATE OF name ON tag BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 12, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 12;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 12, new.name);
END;

CREATE TRIGGER search_tag_delete AFTER DELETE ON tag BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 12, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 12;
END;

CREATE TRIGGER search_storage_box_insert AFTER INSERT ON storage_box BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 13, new.name, 13, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 13, new.name);
END;

CREATE TRIGGER search_storage_box_update AFTER UPDATE OF name ON storage_box BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 13, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 13;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 13, new.name);
END;

CREATE TRIGGER search_storage_box_delete AFTER DELETE ON storage_box BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 13, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 13;
END;

CREATE TRIGGER search_copy_insert AFTER INSERT ON copy BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 14, new.notes, 14, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 14, new.notes);
END;

CREATE TRIGGER search_copy_update AFTER UPDATE OF notes ON copy BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 14, old.notes);
    UPDATE search_source SET text = new.notes WHERE id = old.id * 16 + 14;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 14, new.notes);
END;

CREATE TRIGGER search_copy_delete AFTER DELETE ON copy BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 14, old.notes);
    DELETE FROM search_source WHERE id = old.id * 16 + 14;
END;
