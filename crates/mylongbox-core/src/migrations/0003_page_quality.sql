-- How white a comic's or magazine's pages still are, on CGC's scale.
-- Collected editions use book condition instead, so theirs stays empty.
-- Existing copies start with none recorded.
ALTER TABLE copy ADD COLUMN page_quality TEXT CHECK (page_quality IN (
    'white', 'off_white_to_white', 'off_white', 'cream_to_off_white',
    'cream', 'light_tan_to_off_white', 'light_tan_to_cream', 'light_tan',
    'tan_to_off_white', 'tan_to_cream', 'tan', 'dark_tan_to_off_white',
    'dark_tan', 'brown_to_off_white', 'brown_to_tan', 'brown',
    'brown_brittle'
));
