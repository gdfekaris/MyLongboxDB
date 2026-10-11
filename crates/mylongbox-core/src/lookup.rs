//! Shared name lists: publishers, storage boxes, creators, characters,
//! teams, story arcs, and tags. Each can be renamed, merged, and given
//! alternate names where the list supports them.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{Database, run_steps};
use crate::error::{Error, Result};
use crate::model::{
    BoxId, CharacterId, CreatorId, PublisherId, RowUuid, StoryArcId, TagId, TeamId,
};

/// One entry in a name list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup<I> {
    pub id: I,
    pub uuid: RowUuid,
    pub name: String,
    /// Other spellings that find this entry, such as `J. Kirby`. Always
    /// empty for lists without alternate names.
    pub aliases: Vec<String>,
}

/// The fixed SQL for one name list.
#[doc(hidden)]
pub struct LookupSql {
    /// Lowercase singular noun for messages, e.g. "creator".
    noun: &'static str,
    insert: &'static str,
    get: &'static str,
    list: &'static str,
    find_name: &'static str,
    rename: &'static str,
    delete: &'static str,
    usage: &'static str,
    suggest: &'static str,
    aliases: Option<AliasSql>,
    /// Moves every link from ?1 to ?2 without duplicating any, then
    /// deletes ?1. For lists with aliases, ?1's name and aliases are
    /// added to ?2's before ?1 goes.
    merge: &'static [&'static str],
}

struct AliasSql {
    find: &'static str,
    list: &'static str,
    insert: &'static str,
    delete: &'static str,
}

/// An id type whose entries form a name list.
pub trait LookupId: Copy + sealed::Sealed {
    #[doc(hidden)]
    const SQL: &'static LookupSql;
    #[doc(hidden)]
    fn from_raw(raw: i64) -> Self;
    #[doc(hidden)]
    fn raw(self) -> i64;
}

mod sealed {
    pub trait Sealed {}
}

/// Builds a [`LookupSql`] from literal table names, so every statement is
/// one complete string fixed at compile time.
macro_rules! lookup_sql {
    (
        noun: $noun:literal,
        table: $t:literal,
        usage: $usage:literal,
        $(aliases: ($a:literal, $fk:literal),)?
        merge: [$($merge:literal),+ $(,)?],
    ) => {
        LookupSql {
            noun: $noun,
            insert: concat!("INSERT INTO ", $t, " (uuid, name) VALUES (?1, ?2)"),
            get: concat!("SELECT id, uuid, name FROM ", $t, " WHERE id = ?1"),
            list: concat!("SELECT id, uuid, name FROM ", $t, " ORDER BY name COLLATE NOCASE, id"),
            find_name: concat!("SELECT id, name FROM ", $t, " WHERE name = ?1"),
            rename: concat!("UPDATE ", $t, " SET name = ?2 WHERE id = ?1"),
            delete: concat!("DELETE FROM ", $t, " WHERE id = ?1"),
            usage: $usage,
            suggest: lookup_sql!(@suggest $t $(, $a, $fk)?),
            aliases: lookup_sql!(@aliases $t $(, $a, $fk)?),
            merge: &[$($merge),+],
        }
    };
    (@suggest $t:literal) => {
        concat!(
            "SELECT id, uuid, name FROM ", $t,
            " WHERE name LIKE ?1 ESCAPE '\\'",
            " ORDER BY name LIKE ?2 ESCAPE '\\' DESC, name COLLATE NOCASE, id LIMIT ?3"
        )
    };
    (@suggest $t:literal, $a:literal, $fk:literal) => {
        concat!(
            "SELECT id, uuid, name FROM ", $t,
            " WHERE name LIKE ?1 ESCAPE '\\' OR id IN (SELECT ", $fk, " FROM ", $a,
            " WHERE alias LIKE ?1 ESCAPE '\\')",
            " ORDER BY name LIKE ?2 ESCAPE '\\' DESC, name COLLATE NOCASE, id LIMIT ?3"
        )
    };
    (@aliases $t:literal) => { None };
    (@aliases $t:literal, $a:literal, $fk:literal) => {
        Some(AliasSql {
            find: concat!(
                "SELECT ", $fk, ", alias, (SELECT name FROM ", $t, " WHERE id = ", $fk, ") FROM ",
                $a, " WHERE alias = ?1"
            ),
            list: concat!("SELECT alias FROM ", $a, " WHERE ", $fk, " = ?1 ORDER BY alias"),
            insert: concat!("INSERT INTO ", $a, " (", $fk, ", alias) VALUES (?1, ?2)"),
            delete: concat!("DELETE FROM ", $a, " WHERE ", $fk, " = ?1 AND alias = ?2"),
        })
    };
}

macro_rules! lookup_id {
    ($id:ident, $sql:expr) => {
        impl sealed::Sealed for $id {}
        impl LookupId for $id {
            const SQL: &'static LookupSql = &$sql;
            fn from_raw(raw: i64) -> Self {
                Self(raw)
            }
            fn raw(self) -> i64 {
                self.0
            }
        }
    };
}

lookup_id!(
    PublisherId,
    lookup_sql! {
        noun: "publisher",
        table: "publisher",
        usage: "SELECT count(*) FROM series WHERE publisher_id = ?1",
        merge: [
            "UPDATE series SET publisher_id = ?2 WHERE publisher_id = ?1",
            "DELETE FROM publisher WHERE id = ?1",
        ],
    }
);

lookup_id!(
    BoxId,
    lookup_sql! {
        noun: "box",
        table: "storage_box",
        usage: "SELECT count(*) FROM copy WHERE box_id = ?1",
        merge: [
            "UPDATE copy SET box_id = ?2 WHERE box_id = ?1",
            "DELETE FROM storage_box WHERE id = ?1",
        ],
    }
);

lookup_id!(
    TagId,
    lookup_sql! {
        noun: "tag",
        table: "tag",
        usage: "SELECT count(*) FROM copy_tag WHERE tag_id = ?1",
        merge: [
            "INSERT INTO copy_tag (copy_id, tag_id)
             SELECT copy_id, ?2 FROM copy_tag AS old WHERE tag_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM copy_tag WHERE copy_id = old.copy_id AND tag_id = ?2)",
            "DELETE FROM tag WHERE id = ?1",
        ],
    }
);

lookup_id!(
    CreatorId,
    lookup_sql! {
        noun: "creator",
        table: "creator",
        usage: "SELECT (SELECT count(*) FROM credit WHERE creator_id = ?1)
                    + (SELECT count(*) FROM edition_cover_artist WHERE creator_id = ?1)",
        aliases: ("creator_alias", "creator_id"),
        merge: [
            "INSERT INTO credit (issue_id, creator_id, role)
             SELECT issue_id, ?2, role FROM credit AS old WHERE creator_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM credit
                     WHERE issue_id = old.issue_id AND creator_id = ?2 AND role = old.role)",
            "DELETE FROM credit WHERE creator_id = ?1",
            "INSERT INTO edition_cover_artist (edition_id, creator_id)
             SELECT edition_id, ?2 FROM edition_cover_artist AS old WHERE creator_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM edition_cover_artist
                     WHERE edition_id = old.edition_id AND creator_id = ?2)",
            "DELETE FROM edition_cover_artist WHERE creator_id = ?1",
            "INSERT INTO creator_alias (creator_id, alias)
             SELECT ?2, moved FROM (
                 SELECT alias AS moved FROM creator_alias WHERE creator_id = ?1
                 UNION SELECT name FROM creator WHERE id = ?1)
             WHERE NOT EXISTS (SELECT 1 FROM creator_alias WHERE creator_id = ?2 AND alias = moved)",
            "DELETE FROM creator WHERE id = ?1",
        ],
    }
);

lookup_id!(
    CharacterId,
    lookup_sql! {
        noun: "character",
        table: "character",
        usage: "SELECT count(*) FROM issue_character WHERE character_id = ?1",
        aliases: ("character_alias", "character_id"),
        merge: [
            // Keep a first-appearance flag from either side.
            "UPDATE issue_character SET first_appearance = 1
             WHERE character_id = ?2 AND issue_id IN (
                 SELECT issue_id FROM issue_character
                 WHERE character_id = ?1 AND first_appearance = 1)",
            "INSERT INTO issue_character (issue_id, character_id, first_appearance)
             SELECT issue_id, ?2, first_appearance FROM issue_character AS old
             WHERE character_id = ?1 AND NOT EXISTS (SELECT 1 FROM issue_character
                 WHERE issue_id = old.issue_id AND character_id = ?2)",
            "DELETE FROM issue_character WHERE character_id = ?1",
            "INSERT INTO character_alias (character_id, alias)
             SELECT ?2, moved FROM (
                 SELECT alias AS moved FROM character_alias WHERE character_id = ?1
                 UNION SELECT name FROM character WHERE id = ?1)
             WHERE NOT EXISTS (SELECT 1 FROM character_alias WHERE character_id = ?2 AND alias = moved)",
            "DELETE FROM character WHERE id = ?1",
        ],
    }
);

lookup_id!(
    TeamId,
    lookup_sql! {
        noun: "team",
        table: "team",
        usage: "SELECT count(*) FROM issue_team WHERE team_id = ?1",
        aliases: ("team_alias", "team_id"),
        merge: [
            "UPDATE issue_team SET first_appearance = 1
             WHERE team_id = ?2 AND issue_id IN (
                 SELECT issue_id FROM issue_team WHERE team_id = ?1 AND first_appearance = 1)",
            "INSERT INTO issue_team (issue_id, team_id, first_appearance)
             SELECT issue_id, ?2, first_appearance FROM issue_team AS old
             WHERE team_id = ?1 AND NOT EXISTS (SELECT 1 FROM issue_team
                 WHERE issue_id = old.issue_id AND team_id = ?2)",
            "DELETE FROM issue_team WHERE team_id = ?1",
            "INSERT INTO team_alias (team_id, alias)
             SELECT ?2, moved FROM (
                 SELECT alias AS moved FROM team_alias WHERE team_id = ?1
                 UNION SELECT name FROM team WHERE id = ?1)
             WHERE NOT EXISTS (SELECT 1 FROM team_alias WHERE team_id = ?2 AND alias = moved)",
            "DELETE FROM team WHERE id = ?1",
        ],
    }
);

lookup_id!(
    StoryArcId,
    lookup_sql! {
        noun: "story arc",
        table: "story_arc",
        usage: "SELECT count(*) FROM issue_story_arc WHERE story_arc_id = ?1",
        aliases: ("story_arc_alias", "story_arc_id"),
        merge: [
            // Keep a part number from either side, preferring the survivor's.
            "UPDATE issue_story_arc SET part = (
                 SELECT old.part FROM issue_story_arc AS old
                 WHERE old.story_arc_id = ?1 AND old.issue_id = issue_story_arc.issue_id)
             WHERE story_arc_id = ?2 AND part IS NULL",
            "INSERT INTO issue_story_arc (issue_id, story_arc_id, part)
             SELECT issue_id, ?2, part FROM issue_story_arc AS old
             WHERE story_arc_id = ?1 AND NOT EXISTS (SELECT 1 FROM issue_story_arc
                 WHERE issue_id = old.issue_id AND story_arc_id = ?2)",
            "DELETE FROM issue_story_arc WHERE story_arc_id = ?1",
            "INSERT INTO story_arc_alias (story_arc_id, alias)
             SELECT ?2, moved FROM (
                 SELECT alias AS moved FROM story_arc_alias WHERE story_arc_id = ?1
                 UNION SELECT name FROM story_arc WHERE id = ?1)
             WHERE NOT EXISTS (SELECT 1 FROM story_arc_alias WHERE story_arc_id = ?2 AND alias = moved)",
            "DELETE FROM story_arc WHERE id = ?1",
        ],
    }
);

impl Database {
    /// Adds an entry, refusing a name that already names another entry.
    pub fn create_lookup<I: LookupId>(&self, name: &str) -> Result<I> {
        let sql = I::SQL;
        let name = clean_name(sql, name)?;
        let tx = self.conn.unchecked_transaction()?;
        check_name_free::<I>(&tx, &name, None)?;
        tx.execute(sql.insert, params![RowUuid::new_random(), name])?;
        let id = I::from_raw(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }

    pub fn lookup<I: LookupId>(&self, id: I) -> Result<Lookup<I>> {
        let (raw, uuid, name) = self
            .conn
            .query_row(I::SQL.get, [id.raw()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(Lookup {
            id: I::from_raw(raw),
            uuid,
            name,
            aliases: aliases_of::<I>(&self.conn, raw)?,
        })
    }

    /// Every entry in the list, by name.
    pub fn list_lookups<I: LookupId>(&self) -> Result<Vec<Lookup<I>>> {
        self.lookups_from(I::SQL.list, ())
    }

    /// The entry whose name or alternate name is `name`, ignoring case.
    pub fn find_lookup<I: LookupId>(&self, name: &str) -> Result<Option<I>> {
        Ok(find::<I>(&self.conn, name.trim())?.map(|(id, _)| id))
    }

    /// Entries whose name or an alternate name contains `text`, ignoring
    /// case, with names that start with it first. For autocomplete.
    pub fn suggest_lookups<I: LookupId>(&self, text: &str, limit: u32) -> Result<Vec<Lookup<I>>> {
        let escaped = escape_like(text.trim());
        self.lookups_from(
            I::SQL.suggest,
            params![format!("%{escaped}%"), format!("{escaped}%"), limit],
        )
    }

    pub fn rename_lookup<I: LookupId>(&self, id: I, name: &str) -> Result<()> {
        let sql = I::SQL;
        let name = clean_name(sql, name)?;
        let tx = self.conn.unchecked_transaction()?;
        self.lookup(id)?;
        check_name_free::<I>(&tx, &name, Some(id))?;
        // Renaming to one of its own alternate names swaps them over.
        if let Some(aliases) = &sql.aliases {
            tx.execute(aliases.delete, params![id.raw(), name])?;
        }
        tx.execute(sql.rename, params![id.raw(), name])?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_alias<I: LookupId>(&self, id: I, alias: &str) -> Result<()> {
        let sql = I::SQL;
        let Some(aliases) = &sql.aliases else {
            return Err(Error::Invalid(format!(
                "a {} has no alternate names",
                sql.noun
            )));
        };
        let alias = clean_name(sql, alias)?;
        let tx = self.conn.unchecked_transaction()?;
        let entry = self.lookup(id)?;
        if entry.name.eq_ignore_ascii_case(&alias)
            || entry.aliases.iter().any(|a| a.eq_ignore_ascii_case(&alias))
        {
            return Ok(());
        }
        check_name_free::<I>(&tx, &alias, None)?;
        tx.execute(aliases.insert, params![id.raw(), alias])?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_alias<I: LookupId>(&self, id: I, alias: &str) -> Result<()> {
        if let Some(aliases) = &I::SQL.aliases {
            self.conn
                .execute(aliases.delete, params![id.raw(), alias.trim()])?;
        }
        Ok(())
    }

    /// Folds `from` into `into`: every link moves over, and `from`'s name
    /// and alternate names become alternate names of `into`, so they still
    /// find it. `from` is then gone.
    pub fn merge_lookups<I: LookupId>(&self, from: I, into: I) -> Result<()> {
        if from.raw() == into.raw() {
            return Err(Error::Invalid(format!(
                "choose a different {} to merge into",
                I::SQL.noun
            )));
        }
        let tx = self.conn.unchecked_transaction()?;
        self.lookup(from)?;
        self.lookup(into)?;
        run_steps(&tx, I::SQL.merge, [from.raw(), into.raw()])?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes an entry that nothing links to.
    pub fn delete_lookup<I: LookupId>(&self, id: I) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.lookup(id)?;
        let uses: i64 = tx.query_row(I::SQL.usage, [id.raw()], |row| row.get(0))?;
        let uses = u64::try_from(uses).unwrap_or(0);
        if uses > 0 {
            return Err(Error::InUse { uses });
        }
        tx.execute(I::SQL.delete, [id.raw()])?;
        tx.commit()?;
        Ok(())
    }

    fn lookups_from<I: LookupId, P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<Lookup<I>>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params, |row| {
                Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<(i64, RowUuid, String)>>>()?;
        rows.into_iter()
            .map(|(raw, uuid, name)| {
                Ok(Lookup {
                    id: I::from_raw(raw),
                    uuid,
                    name,
                    aliases: aliases_of::<I>(&self.conn, raw)?,
                })
            })
            .collect()
    }
}

fn clean_name(sql: &LookupSql, name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(Error::Invalid(format!("enter a {} name", sql.noun)));
    }
    Ok(name)
}

/// The entry named `name` or with `name` as an alternate name, and the
/// entry's own name.
fn find<I: LookupId>(conn: &Connection, name: &str) -> Result<Option<(I, String)>> {
    let sql = I::SQL;
    let by_name: Option<(i64, String)> = conn
        .query_row(sql.find_name, [name], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    if let Some((raw, existing)) = by_name {
        return Ok(Some((I::from_raw(raw), existing)));
    }
    let Some(aliases) = &sql.aliases else {
        return Ok(None);
    };
    let by_alias: Option<(i64, String)> = conn
        .query_row(aliases.find, [name], |row| Ok((row.get(0)?, row.get(2)?)))
        .optional()?;
    Ok(by_alias.map(|(raw, existing)| (I::from_raw(raw), existing)))
}

fn check_name_free<I: LookupId>(conn: &Connection, name: &str, except: Option<I>) -> Result<()> {
    match find::<I>(conn, name)? {
        Some((owner, existing)) if except.is_none_or(|e| e.raw() != owner.raw()) => {
            Err(Error::NameTaken {
                name: name.to_owned(),
                existing,
            })
        }
        _ => Ok(()),
    }
}

fn aliases_of<I: LookupId>(conn: &Connection, raw: i64) -> Result<Vec<String>> {
    let Some(aliases) = &I::SQL.aliases else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(aliases.list)?;
    let rows = stmt.query_map([raw], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Escapes LIKE wildcards so typed `%` and `_` match themselves.
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
