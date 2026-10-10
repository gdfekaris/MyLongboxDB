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
    signed_by TEXT NOT NULL DEFAULT '',
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
    collected_issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE RESTRICT,
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
