-- index character_alias_name
CREATE INDEX character_alias_name ON character_alias (alias);

-- index collects_collected
CREATE INDEX collects_collected ON collects (collected_issue_id);

-- index copy_added_at
CREATE INDEX copy_added_at ON copy (added_at);

-- index copy_box
CREATE INDEX copy_box ON copy (box_id, box_position);

-- index copy_edition
CREATE INDEX copy_edition ON copy (edition_id);

-- index copy_grade
CREATE INDEX copy_grade ON copy (grade);

-- index copy_purchase_date
CREATE INDEX copy_purchase_date ON copy (purchase_date);

-- index copy_tag_tag
CREATE INDEX copy_tag_tag ON copy_tag (tag_id);

-- index copy_trash
CREATE INDEX copy_trash ON copy (trash_id);

-- index creator_alias_name
CREATE INDEX creator_alias_name ON creator_alias (alias);

-- index credit_creator
CREATE INDEX credit_creator ON credit (creator_id, role);

-- index edition_cover_artist_creator
CREATE INDEX edition_cover_artist_creator ON edition_cover_artist (creator_id);

-- index edition_isbn
CREATE INDEX edition_isbn ON edition (isbn);

-- index edition_issue
CREATE INDEX edition_issue ON edition (issue_id);

-- index edition_trash
CREATE INDEX edition_trash ON edition (trash_id);

-- index issue_character_character
CREATE INDEX issue_character_character ON issue_character (character_id);

-- index issue_cover_date
CREATE INDEX issue_cover_date ON issue (cover_year, cover_month);

-- index issue_number_in_series
CREATE UNIQUE INDEX issue_number_in_series
    ON issue (series_id, number_sort)
    WHERE number IS NOT NULL AND trash_id IS NULL;

-- index issue_series_order
CREATE INDEX issue_series_order ON issue (series_id, number_sort);

-- index issue_story_arc_arc
CREATE INDEX issue_story_arc_arc ON issue_story_arc (story_arc_id, part);

-- index issue_team_team
CREATE INDEX issue_team_team ON issue_team (team_id);

-- index issue_trash
CREATE INDEX issue_trash ON issue (trash_id);

-- index series_character_character
CREATE INDEX series_character_character ON series_character (character_id);

-- index series_name
CREATE INDEX series_name ON series (name COLLATE NOCASE);

-- index series_publisher
CREATE INDEX series_publisher ON series (publisher_id);

-- index series_team_team
CREATE INDEX series_team_team ON series_team (team_id);

-- index series_trash
CREATE INDEX series_trash ON series (trash_id);

-- index signature_creator
CREATE INDEX signature_creator ON signature (creator_id);

-- index sqlite_autoindex_character_1, made by SQLite

-- index sqlite_autoindex_character_2, made by SQLite

-- index sqlite_autoindex_character_alias_1, made by SQLite

-- index sqlite_autoindex_collects_1, made by SQLite

-- index sqlite_autoindex_copy_1, made by SQLite

-- index sqlite_autoindex_copy_tag_1, made by SQLite

-- index sqlite_autoindex_cover_photo_1, made by SQLite

-- index sqlite_autoindex_cover_photo_2, made by SQLite

-- index sqlite_autoindex_creator_1, made by SQLite

-- index sqlite_autoindex_creator_2, made by SQLite

-- index sqlite_autoindex_creator_alias_1, made by SQLite

-- index sqlite_autoindex_credit_1, made by SQLite

-- index sqlite_autoindex_edition_1, made by SQLite

-- index sqlite_autoindex_edition_cover_artist_1, made by SQLite

-- index sqlite_autoindex_exchange_rate_1, made by SQLite

-- index sqlite_autoindex_hidden_section_1, made by SQLite

-- index sqlite_autoindex_issue_1, made by SQLite

-- index sqlite_autoindex_issue_character_1, made by SQLite

-- index sqlite_autoindex_issue_story_arc_1, made by SQLite

-- index sqlite_autoindex_issue_team_1, made by SQLite

-- index sqlite_autoindex_meta_1, made by SQLite

-- index sqlite_autoindex_publisher_1, made by SQLite

-- index sqlite_autoindex_publisher_2, made by SQLite

-- index sqlite_autoindex_saved_shelf_1, made by SQLite

-- index sqlite_autoindex_series_1, made by SQLite

-- index sqlite_autoindex_series_character_1, made by SQLite

-- index sqlite_autoindex_series_team_1, made by SQLite

-- index sqlite_autoindex_signature_1, made by SQLite

-- index sqlite_autoindex_storage_box_1, made by SQLite

-- index sqlite_autoindex_storage_box_2, made by SQLite

-- index sqlite_autoindex_story_arc_1, made by SQLite

-- index sqlite_autoindex_story_arc_2, made by SQLite

-- index sqlite_autoindex_story_arc_alias_1, made by SQLite

-- index sqlite_autoindex_tag_1, made by SQLite

-- index sqlite_autoindex_tag_2, made by SQLite

-- index sqlite_autoindex_team_1, made by SQLite

-- index sqlite_autoindex_team_2, made by SQLite

-- index sqlite_autoindex_team_alias_1, made by SQLite

-- index sqlite_autoindex_value_entry_1, made by SQLite

-- index story_arc_alias_name
CREATE INDEX story_arc_alias_name ON story_arc_alias (alias);

-- index team_alias_name
CREATE INDEX team_alias_name ON team_alias (alias);

-- index value_entry_copy_date
CREATE INDEX value_entry_copy_date ON value_entry (copy_id, date);

-- table character
CREATE TABLE character (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

-- table character_alias
CREATE TABLE character_alias (
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (character_id, alias)
) STRICT;

-- table collects
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

-- table copy
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
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE, page_quality TEXT CHECK (page_quality IN (
    'white', 'off_white_to_white', 'off_white', 'cream_to_off_white',
    'cream', 'light_tan_to_off_white', 'light_tan_to_cream', 'light_tan',
    'tan_to_off_white', 'tan_to_cream', 'tan', 'dark_tan_to_off_white',
    'dark_tan', 'brown_to_off_white', 'brown_to_tan', 'brown',
    'brown_brittle'
)), pedigree TEXT NOT NULL DEFAULT '',
    CHECK ((purchase_price_units IS NULL) = (purchase_price_currency IS NULL)),
    CHECK (box_position IS NULL OR box_id IS NOT NULL)
) STRICT;

-- table copy_tag
CREATE TABLE copy_tag (
    copy_id INTEGER NOT NULL REFERENCES copy (id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag (id) ON DELETE CASCADE,
    PRIMARY KEY (copy_id, tag_id)
) STRICT;

-- table cover_photo
CREATE TABLE "cover_photo" (
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

-- table creator
CREATE TABLE creator (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

-- table creator_alias
CREATE TABLE creator_alias (
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (creator_id, alias)
) STRICT;

-- table credit
CREATE TABLE "credit" (
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

-- table edition
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
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE, canadian_cover_price_cents INTEGER
    CHECK (canadian_cover_price_cents >= 0), published TEXT CHECK (
    published GLOB '[0-9][0-9][0-9][0-9]'
    OR published GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'
    OR published GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9]'
),
    CHECK ((cover_price_units IS NULL) = (cover_price_currency IS NULL))
) STRICT;

-- table edition_cover_artist
CREATE TABLE edition_cover_artist (
    edition_id INTEGER NOT NULL REFERENCES edition (id) ON DELETE CASCADE,
    creator_id INTEGER NOT NULL REFERENCES creator (id) ON DELETE RESTRICT,
    PRIMARY KEY (edition_id, creator_id)
) STRICT;

-- table exchange_rate
CREATE TABLE exchange_rate (
    date TEXT PRIMARY KEY CHECK (date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    usd_cents_per_ounce INTEGER NOT NULL CHECK (usd_cents_per_ounce > 0),
    usd_cents_per_bitcoin INTEGER NOT NULL CHECK (usd_cents_per_bitcoin > 0)
) STRICT;

-- table hidden_section
CREATE TABLE hidden_section (
    code TEXT PRIMARY KEY CHECK (code <> '')
) STRICT;

-- table issue
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
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE, legacy_number INTEGER CHECK (legacy_number >= 0),
    CHECK (cover_month IS NULL OR cover_year IS NOT NULL),
    CHECK ((number IS NULL) = (number_sort = ''))
) STRICT;

-- table issue_character
CREATE TABLE issue_character (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE RESTRICT,
    first_appearance INTEGER NOT NULL DEFAULT 0 CHECK (first_appearance IN (0, 1)),
    PRIMARY KEY (issue_id, character_id)
) STRICT;

-- table issue_story_arc
CREATE TABLE issue_story_arc (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    story_arc_id INTEGER NOT NULL REFERENCES story_arc (id) ON DELETE RESTRICT,
    -- Part number within the arc, when known.
    part INTEGER CHECK (part >= 1),
    PRIMARY KEY (issue_id, story_arc_id)
) STRICT;

-- table issue_team
CREATE TABLE issue_team (
    issue_id INTEGER NOT NULL REFERENCES issue (id) ON DELETE CASCADE,
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE RESTRICT,
    first_appearance INTEGER NOT NULL DEFAULT 0 CHECK (first_appearance IN (0, 1)),
    PRIMARY KEY (issue_id, team_id)
) STRICT;

-- table meta
CREATE TABLE meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

-- table publisher
CREATE TABLE publisher (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    notes TEXT NOT NULL DEFAULT ''
) STRICT;

-- table saved_shelf
CREATE TABLE saved_shelf (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    position INTEGER NOT NULL,
    rules TEXT NOT NULL CHECK (json_valid(rules))
) STRICT;

-- table search_source
CREATE TABLE search_source (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    kind INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 14),
    owner INTEGER NOT NULL
) STRICT;

-- table search_text
CREATE VIRTUAL TABLE search_text USING fts5 (
    text,
    content = 'search_source',
    content_rowid = 'id',
    tokenize = 'trigram remove_diacritics 1'
);

-- table search_text_config
CREATE TABLE 'search_text_config'(k PRIMARY KEY, v) WITHOUT ROWID;

-- table search_text_data
CREATE TABLE 'search_text_data'(id INTEGER PRIMARY KEY, block BLOB);

-- table search_text_docsize
CREATE TABLE 'search_text_docsize'(id INTEGER PRIMARY KEY, sz BLOB);

-- table search_text_idx
CREATE TABLE 'search_text_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;

-- table series
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
    trash_id INTEGER REFERENCES trash (id) ON DELETE CASCADE, imprint TEXT NOT NULL DEFAULT '',
    CHECK (category = 'magazine' OR (magazine_format IS NULL AND black_and_white IS NULL))
) STRICT;

-- table series_character
CREATE TABLE series_character (
    series_id INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE RESTRICT,
    PRIMARY KEY (series_id, character_id)
) STRICT;

-- table series_team
CREATE TABLE series_team (
    series_id INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE RESTRICT,
    PRIMARY KEY (series_id, team_id)
) STRICT;

-- table signature
CREATE TABLE "signature" (
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

-- table storage_box
CREATE TABLE storage_box (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    notes TEXT NOT NULL DEFAULT ''
) STRICT;

-- table story_arc
CREATE TABLE story_arc (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

-- table story_arc_alias
CREATE TABLE story_arc_alias (
    story_arc_id INTEGER NOT NULL REFERENCES story_arc (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (story_arc_id, alias)
) STRICT;

-- table tag
CREATE TABLE tag (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

-- table team
CREATE TABLE team (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE CHECK (length(uuid) = 36),
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> '')
) STRICT;

-- table team_alias
CREATE TABLE team_alias (
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE CASCADE,
    alias TEXT NOT NULL COLLATE NOCASE CHECK (alias <> ''),
    PRIMARY KEY (team_id, alias)
) STRICT;

-- table trash
CREATE TABLE trash (
    id INTEGER PRIMARY KEY,
    trashed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    -- What the user deleted, for the Trash list, e.g. "Amazing Spider-Man #121".
    label TEXT NOT NULL
) STRICT;

-- table value_entry
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

-- trigger search_character_alias_delete
CREATE TRIGGER search_character_alias_delete AFTER DELETE ON character_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 7, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 7;
END;

-- trigger search_character_alias_insert
CREATE TRIGGER search_character_alias_insert AFTER INSERT ON character_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 7, new.alias, 7, new.character_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 7, new.alias);
END;

-- trigger search_character_alias_update
CREATE TRIGGER search_character_alias_update AFTER UPDATE OF alias, character_id ON character_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 7, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.character_id WHERE id = old.rowid * 16 + 7;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 7, new.alias);
END;

-- trigger search_character_delete
CREATE TRIGGER search_character_delete AFTER DELETE ON character BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 6, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 6;
END;

-- trigger search_character_insert
CREATE TRIGGER search_character_insert AFTER INSERT ON character BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 6, new.name, 6, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 6, new.name);
END;

-- trigger search_character_update
CREATE TRIGGER search_character_update AFTER UPDATE OF name ON character BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 6, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 6;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 6, new.name);
END;

-- trigger search_copy_delete
CREATE TRIGGER search_copy_delete AFTER DELETE ON copy BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 14, old.notes);
    DELETE FROM search_source WHERE id = old.id * 16 + 14;
END;

-- trigger search_copy_insert
CREATE TRIGGER search_copy_insert AFTER INSERT ON copy BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 14, new.notes, 14, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 14, new.notes);
END;

-- trigger search_copy_update
CREATE TRIGGER search_copy_update AFTER UPDATE OF notes ON copy BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 14, old.notes);
    UPDATE search_source SET text = new.notes WHERE id = old.id * 16 + 14;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 14, new.notes);
END;

-- trigger search_creator_alias_delete
CREATE TRIGGER search_creator_alias_delete AFTER DELETE ON creator_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 5, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 5;
END;

-- trigger search_creator_alias_insert
CREATE TRIGGER search_creator_alias_insert AFTER INSERT ON creator_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 5, new.alias, 5, new.creator_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 5, new.alias);
END;

-- trigger search_creator_alias_update
CREATE TRIGGER search_creator_alias_update AFTER UPDATE OF alias, creator_id ON creator_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 5, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.creator_id WHERE id = old.rowid * 16 + 5;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 5, new.alias);
END;

-- trigger search_creator_delete
CREATE TRIGGER search_creator_delete AFTER DELETE ON creator BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 4, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 4;
END;

-- trigger search_creator_insert
CREATE TRIGGER search_creator_insert AFTER INSERT ON creator BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 4, new.name, 4, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 4, new.name);
END;

-- trigger search_creator_update
CREATE TRIGGER search_creator_update AFTER UPDATE OF name ON creator BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 4, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 4;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 4, new.name);
END;

-- trigger search_issue_delete
CREATE TRIGGER search_issue_delete AFTER DELETE ON issue BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 2, old.title);
    DELETE FROM search_source WHERE id = old.id * 16 + 2;
END;

-- trigger search_issue_insert
CREATE TRIGGER search_issue_insert AFTER INSERT ON issue BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 2, new.title, 2, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 2, new.title);
END;

-- trigger search_issue_update
CREATE TRIGGER search_issue_update AFTER UPDATE OF title ON issue BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 2, old.title);
    UPDATE search_source SET text = new.title WHERE id = old.id * 16 + 2;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 2, new.title);
END;

-- trigger search_publisher_delete
CREATE TRIGGER search_publisher_delete AFTER DELETE ON publisher BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 3, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 3;
END;

-- trigger search_publisher_insert
CREATE TRIGGER search_publisher_insert AFTER INSERT ON publisher BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 3, new.name, 3, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 3, new.name);
END;

-- trigger search_publisher_update
CREATE TRIGGER search_publisher_update AFTER UPDATE OF name ON publisher BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 3, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 3;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 3, new.name);
END;

-- trigger search_series_delete
CREATE TRIGGER search_series_delete AFTER DELETE ON series BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 1, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 1;
END;

-- trigger search_series_insert
CREATE TRIGGER search_series_insert AFTER INSERT ON series BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 1, new.name, 1, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 1, new.name);
END;

-- trigger search_series_update
CREATE TRIGGER search_series_update AFTER UPDATE OF name ON series BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 1, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 1;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 1, new.name);
END;

-- trigger search_storage_box_delete
CREATE TRIGGER search_storage_box_delete AFTER DELETE ON storage_box BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 13, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 13;
END;

-- trigger search_storage_box_insert
CREATE TRIGGER search_storage_box_insert AFTER INSERT ON storage_box BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 13, new.name, 13, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 13, new.name);
END;

-- trigger search_storage_box_update
CREATE TRIGGER search_storage_box_update AFTER UPDATE OF name ON storage_box BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 13, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 13;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 13, new.name);
END;

-- trigger search_story_arc_alias_delete
CREATE TRIGGER search_story_arc_alias_delete AFTER DELETE ON story_arc_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 11, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 11;
END;

-- trigger search_story_arc_alias_insert
CREATE TRIGGER search_story_arc_alias_insert AFTER INSERT ON story_arc_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 11, new.alias, 11, new.story_arc_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 11, new.alias);
END;

-- trigger search_story_arc_alias_update
CREATE TRIGGER search_story_arc_alias_update AFTER UPDATE OF alias, story_arc_id ON story_arc_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 11, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.story_arc_id WHERE id = old.rowid * 16 + 11;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 11, new.alias);
END;

-- trigger search_story_arc_delete
CREATE TRIGGER search_story_arc_delete AFTER DELETE ON story_arc BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 10, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 10;
END;

-- trigger search_story_arc_insert
CREATE TRIGGER search_story_arc_insert AFTER INSERT ON story_arc BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 10, new.name, 10, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 10, new.name);
END;

-- trigger search_story_arc_update
CREATE TRIGGER search_story_arc_update AFTER UPDATE OF name ON story_arc BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 10, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 10;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 10, new.name);
END;

-- trigger search_tag_delete
CREATE TRIGGER search_tag_delete AFTER DELETE ON tag BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 12, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 12;
END;

-- trigger search_tag_insert
CREATE TRIGGER search_tag_insert AFTER INSERT ON tag BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 12, new.name, 12, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 12, new.name);
END;

-- trigger search_tag_update
CREATE TRIGGER search_tag_update AFTER UPDATE OF name ON tag BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 12, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 12;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 12, new.name);
END;

-- trigger search_team_alias_delete
CREATE TRIGGER search_team_alias_delete AFTER DELETE ON team_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 9, old.alias);
    DELETE FROM search_source WHERE id = old.rowid * 16 + 9;
END;

-- trigger search_team_alias_insert
CREATE TRIGGER search_team_alias_insert AFTER INSERT ON team_alias BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.rowid * 16 + 9, new.alias, 9, new.team_id);
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 9, new.alias);
END;

-- trigger search_team_alias_update
CREATE TRIGGER search_team_alias_update AFTER UPDATE OF alias, team_id ON team_alias BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.rowid * 16 + 9, old.alias);
    UPDATE search_source SET text = new.alias, owner = new.team_id WHERE id = old.rowid * 16 + 9;
    INSERT INTO search_text (rowid, text) VALUES (new.rowid * 16 + 9, new.alias);
END;

-- trigger search_team_delete
CREATE TRIGGER search_team_delete AFTER DELETE ON team BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 8, old.name);
    DELETE FROM search_source WHERE id = old.id * 16 + 8;
END;

-- trigger search_team_insert
CREATE TRIGGER search_team_insert AFTER INSERT ON team BEGIN
    INSERT INTO search_source (id, text, kind, owner)
    VALUES (new.id * 16 + 8, new.name, 8, new.id);
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 8, new.name);
END;

-- trigger search_team_update
CREATE TRIGGER search_team_update AFTER UPDATE OF name ON team BEGIN
    INSERT INTO search_text (search_text, rowid, text)
    VALUES ('delete', old.id * 16 + 8, old.name);
    UPDATE search_source SET text = new.name WHERE id = old.id * 16 + 8;
    INSERT INTO search_text (rowid, text) VALUES (new.id * 16 + 8, new.name);
END;
