//! SQLite-Index der Bibliothek. Die Dateien sind die Wahrheit; der Index ist
//! ein schneller Katalog darüber (Galerie, später Volltextsuche).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

/// 1: ein Ordner pro Aufnahme · 2: flache Monatsordner · 3: Suche, Export
pub(crate) const SCHEMA_VERSION: i64 = 3;

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
    add_column(&conn, "ocr_done", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(&conn, "export_path", "TEXT")?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS captures_fts USING fts5(
            id UNINDEXED,
            text,
            tokenize = 'unicode61 remove_diacritics 2'
        );",
    )?;
    Ok(conn)
}

fn add_column(conn: &Connection, name: &str, def: &str) -> rusqlite::Result<()> {
    let exists: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('captures') WHERE name = ?1")?
        .exists([name])?;
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE captures ADD COLUMN {name} {def}"))?;
    }
    Ok(())
}

/// Aufnahmen ohne Texterkennung, neueste zuerst.
pub(crate) fn pending_ocr(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM captures WHERE ocr_done = 0 ORDER BY created_at DESC LIMIT ?1",
    )?;
    stmt.query_map([limit], |r| r.get(0))?.collect()
}

pub(crate) fn set_ocr_text(conn: &Connection, id: &str, text: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM captures_fts WHERE id = ?1", [id])?;
    conn.execute(
        "INSERT INTO captures_fts (id, text) VALUES (?1, ?2)",
        params![id, text],
    )?;
    conn.execute("UPDATE captures SET ocr_done = 1 WHERE id = ?1", [id])?;
    Ok(())
}

/// Volltextsuche; jedes Wort muss (als Wortanfang) vorkommen.
/// Filter und Sortierung für die Galerie.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureQuery {
    /// Volltext über den erkannten Text; leer = alle.
    pub text: String,
    /// Ab diesem Zeitpunkt (inklusive, Unix-Millisekunden).
    pub from_ms: Option<i64>,
    /// Vor diesem Zeitpunkt (exklusive).
    pub to_ms: Option<i64>,
    /// Nur Aufnahmen dieser Art (`region`, `window`, `screen`, `scroll`).
    pub mode: Option<String>,
    pub starred_only: bool,
    pub oldest_first: bool,
    pub limit: u32,
}

/// Wörter für die Volltextsuche (Präfixsuche je Wort).
fn fts_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect()
}

pub(crate) fn query(
    conn: &Connection,
    root: &Path,
    q: &CaptureQuery,
) -> rusqlite::Result<Vec<CaptureRecord>> {
    let mut filters: Vec<&str> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let terms = fts_terms(&q.text);
    if !terms.is_empty() {
        filters.push("id IN (SELECT id FROM captures_fts WHERE captures_fts MATCH ?)");
        args.push(terms.join(" ").into());
    }
    if let Some(from) = q.from_ms {
        filters.push("created_at >= ?");
        args.push(from.into());
    }
    if let Some(to) = q.to_ms {
        filters.push("created_at < ?");
        args.push(to.into());
    }
    if let Some(mode) = &q.mode {
        filters.push("mode = ?");
        args.push(mode.clone().into());
    }
    if q.starred_only {
        filters.push("starred = 1");
    }
    let filter = if filters.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", filters.join(" AND "))
    };
    let order = if q.oldest_first { "ASC" } else { "DESC" };
    args.push(i64::from(q.limit.max(1)).into());
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM captures {filter} ORDER BY created_at {order}, id {order} LIMIT ?"
    ))?;
    stmt.query_map(rusqlite::params_from_iter(args), |r| row(root, r))?
        .collect()
}

pub(crate) fn search(
    conn: &Connection,
    root: &Path,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<CaptureRecord>> {
    let terms = fts_terms(query);
    if terms.is_empty() {
        return recent(conn, root, limit);
    }
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM captures WHERE id IN
           (SELECT id FROM captures_fts WHERE captures_fts MATCH ?1)
         ORDER BY created_at DESC LIMIT ?2"
    ))?;
    stmt.query_map(params![terms.join(" "), limit], |r| row(root, r))?
        .collect()
}

pub(crate) fn set_export_path(conn: &Connection, id: &str, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE captures SET export_path = ?2 WHERE id = ?1",
        params![id, path],
    )?;
    Ok(())
}

pub(crate) fn export_path(conn: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT export_path FROM captures WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
}

/// IDs älter als `before_ms`, ohne Stern.
pub(crate) fn expired(conn: &Connection, before_ms: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM captures WHERE created_at < ?1 AND starred = 0")?;
    stmt.query_map([before_ms], |r| r.get(0))?.collect()
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
    conn.execute("DELETE FROM captures_fts WHERE id = ?1", [id])?;
    Ok(())
}

pub(crate) fn set_starred(conn: &Connection, id: &str, starred: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE captures SET starred = ?2 WHERE id = ?1",
        params![id, starred],
    )?;
    Ok(())
}
