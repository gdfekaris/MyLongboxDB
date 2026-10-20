-- Characters and teams in every issue of a series, such as Spider-Man
-- in Amazing Spider-Man, so they need not be added to each issue. An
-- issue counts for these as well as for those linked to it alone. First
-- appearances stay with the issue. Existing series start with none.
CREATE TABLE series_character (
    series_id INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    character_id INTEGER NOT NULL REFERENCES character (id) ON DELETE RESTRICT,
    PRIMARY KEY (series_id, character_id)
) STRICT;

CREATE INDEX series_character_character ON series_character (character_id);

CREATE TABLE series_team (
    series_id INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    team_id INTEGER NOT NULL REFERENCES team (id) ON DELETE RESTRICT,
    PRIMARY KEY (series_id, team_id)
) STRICT;

CREATE INDEX series_team_team ON series_team (team_id);
