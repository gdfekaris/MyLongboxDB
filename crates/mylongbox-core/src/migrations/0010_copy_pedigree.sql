-- The named collection a copy once belonged to, such as the Mile High or
-- Duke Caldwell collection, as a grading label gives it. Kept as text on
-- the copy, like where it was bought, since a pedigree is only ever
-- given to copies. Existing copies start with none.
ALTER TABLE copy ADD COLUMN pedigree TEXT NOT NULL DEFAULT '';
