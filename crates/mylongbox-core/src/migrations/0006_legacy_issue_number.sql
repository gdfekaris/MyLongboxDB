-- The number an issue has in its title's whole history, across
-- relaunches that restart at #1, such as Amazing Spider-Man (2018) #45
-- being #846. Existing issues start with none recorded.
ALTER TABLE issue ADD COLUMN legacy_number INTEGER CHECK (legacy_number >= 0);
