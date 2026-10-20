-- When an edition came out, for one published long after the issue
-- first was, such as a facsimile or a later printing. Often known only
-- to the year or month, so kept as YYYY, YYYY-MM, or YYYY-MM-DD, which
-- sort in date order as text. Existing editions start with none.
ALTER TABLE edition ADD COLUMN published TEXT CHECK (
    published GLOB '[0-9][0-9][0-9][0-9]'
    OR published GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'
    OR published GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9]'
);
