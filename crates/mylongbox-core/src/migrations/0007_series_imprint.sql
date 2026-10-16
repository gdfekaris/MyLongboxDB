-- The imprint a series is published under, such as Vertigo for DC or
-- Epic for Marvel. Kept as text on the series, since imprints are only
-- ever chosen for a series, never browsed on their own. Existing series
-- start with none.
ALTER TABLE series ADD COLUMN imprint TEXT NOT NULL DEFAULT '';
