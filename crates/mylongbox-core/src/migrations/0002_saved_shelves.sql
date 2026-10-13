-- Saved shelves and the sidebar's hidden entries.
--
-- Both describe how the owner browses this collection rather than what
-- it holds, so they carry no UUID and are not part of CSV or full
-- exports; a copy of the whole collection keeps them.

-- A filter set the owner named, listed in the sidebar by `position`.
-- `rules` is the set as JSON, as written by shelves.rs; a newer version
-- of the app may write rules this one cannot read, so it is kept as
-- text and only read when the shelf is shown.
CREATE TABLE saved_shelf (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (name <> ''),
    position INTEGER NOT NULL,
    rules TEXT NOT NULL CHECK (json_valid(rules))
) STRICT;

-- Built-in sidebar entries the owner chose not to show, by the code the
-- app gives each.
CREATE TABLE hidden_section (
    code TEXT PRIMARY KEY CHECK (code <> '')
) STRICT;
