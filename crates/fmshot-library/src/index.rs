//! SQLite-Index der Bibliothek. Die Dateien sind die Wahrheit; der Index ist
//! ein schneller Katalog darüber (Galerie, später Volltextsuche).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

/// 1: ein Ordner pro Aufnahme · 2: flache Monatsordner
pub(crate) const SCHEMA_VERSION: i64 = 2;

/// Eine Aufnahme, wie die Galerie sie braucht.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureRecord {
    pub id: String,
    /// Unix-Zeit in Millisekunden
    pub created_at_ms: i64,
    /// z. B. `region`, `screen`
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    /// Absoluter Monatsordner, in dem die Dateien der Aufnahme liegen
    pub dir: PathBuf,
    pub starred: bool,
}

pub(crate) fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS captures (
            id            TEXT PRIMARY KEY,
            created_at    INTEGER NOT NULL,
            mode          TEXT    NOT NULL,
            width         INTEGER NOT NULL,
            height        INTEGER NOT NULL,
            scale_factor  REAL    NOT NULL,
            rel_dir       TEXT    NOT NULL,
            starred       INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS captures_created ON captures(created_at DESC);",
    )?;
    Ok(conn)
}

pub(crate) fn version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
}

pub(crate) fn set_version(conn: &Connection, version: i64) -> rusqlite::Result<()> {
    conn.pragma_update(None, "user_version", version)
}

/// Alle (id, rel_dir) – für Migrationen.
pub(crate) fn all_dirs(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, rel_dir FROM captures")?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

pub(crate) fn set_dir(conn: &Connection, id: &str, rel_dir: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE captures SET rel_dir = ?2 WHERE id = ?1",
        params![id, rel_dir],
    )?;
    Ok(())
}

pub(crate) fn insert(conn: &Connection, root: &Path, rec: &CaptureRecord) -> rusqlite::Result<()> {
    let rel = rec.dir.strip_prefix(root).unwrap_or(&rec.dir);
    conn.execute(
        "INSERT INTO captures (id, created_at, mode, width, height, scale_factor, rel_dir, starred)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            rec.id,
            rec.created_at_ms,
            rec.mode,
            rec.width,
            rec.height,
            rec.scale_factor,
            rel.to_string_lossy(),
            rec.starred,
        ],
    )?;
    Ok(())
}

const COLUMNS: &str = "id, created_at, mode, width, height, scale_factor, rel_dir, starred";

fn row(root: &Path, r: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureRecord> {
    Ok(CaptureRecord {
        id: r.get(0)?,
        created_at_ms: r.get(1)?,
        mode: r.get(2)?,
        width: r.get(3)?,
        height: r.get(4)?,
        scale_factor: r.get(5)?,
        dir: root.join(r.get::<_, String>(6)?),
        starred: r.get(7)?,
    })
}

pub(crate) fn recent(
    conn: &Connection,
    root: &Path,
    limit: u32,
) -> rusqlite::Result<Vec<CaptureRecord>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM captures ORDER BY created_at DESC, id DESC LIMIT ?1"
    ))?;
    stmt.query_map([limit], |r| row(root, r))?.collect()
}

pub(crate) fn get(
    conn: &Connection,
    root: &Path,
    id: &str,
) -> rusqlite::Result<Option<CaptureRecord>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM captures WHERE id = ?1"))?;
    let mut rows = stmt.query_map([id], |r| row(root, r))?;
    rows.next().transpose()
}

pub(crate) fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM captures WHERE id = ?1", [id])?;
    Ok(())
}

pub(crate) fn set_starred(conn: &Connection, id: &str, starred: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE captures SET starred = ?2 WHERE id = ?1",
        params![id, starred],
    )?;
    Ok(())
}
