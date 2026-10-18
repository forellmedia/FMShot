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
    /// Nur bei Videos: Länge in Millisekunden.
    pub duration_ms: Option<i64>,
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
    add_column(&conn, "duration_ms", "INTEGER")?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS captures_fts USING fts5(
            id UNINDEXED,
            text,
            tokenize = 'unicode61 remove_diacritics 2'
        );
        -- Bildinhalt (ADR 0010): ein Vektor je Aufnahme und Modell. Leerer
        -- Vektor = liess sich nicht berechnen, nicht immer wieder versuchen.
        CREATE TABLE IF NOT EXISTS embeddings (
            id      TEXT PRIMARY KEY,
            model   TEXT NOT NULL,
            vector  BLOB NOT NULL
        );
        -- Stichwörter aus dem Bildinhalt, je Aufnahme höchstens drei.
        CREATE TABLE IF NOT EXISTS capture_tags (
            id     TEXT NOT NULL,
            term   TEXT NOT NULL,
            score  REAL NOT NULL,
            PRIMARY KEY (id, term)
        );
        CREATE INDEX IF NOT EXISTS capture_tags_term ON capture_tags(term);",
    )?;
    // Mit welcher Begriffsliste die Stichwörter berechnet wurden.
    add_column_to(&conn, "embeddings", "tags_version", "TEXT")?;
    Ok(conn)
}

fn add_column(conn: &Connection, name: &str, def: &str) -> rusqlite::Result<()> {
    add_column_to(conn, "captures", name, def)
}

fn add_column_to(conn: &Connection, table: &str, name: &str, def: &str) -> rusqlite::Result<()> {
    let exists: bool = conn
        .prepare(&format!(
            "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"
        ))?
        .exists([name])?;
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {name} {def}"))?;
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

/// Aufnahmen ohne Vektor des Modells `model`, neueste zuerst.
pub(crate) fn pending_embeddings(
    conn: &Connection,
    model: &str,
    limit: u32,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.id FROM captures c LEFT JOIN embeddings e ON e.id = c.id AND e.model = ?1
         WHERE e.id IS NULL ORDER BY c.created_at DESC LIMIT ?2",
    )?;
    stmt.query_map(params![model, limit], |r| r.get(0))?
        .collect()
}

pub(crate) fn set_embedding(
    conn: &Connection,
    id: &str,
    model: &str,
    vector: &[u8],
) -> rusqlite::Result<()> {
    // Nur für bestehende Aufnahmen (sie kann inzwischen gelöscht sein).
    conn.execute(
        "INSERT OR REPLACE INTO embeddings (id, model, vector)
         SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM captures WHERE id = ?1)",
        params![id, model, vector],
    )?;
    Ok(())
}

/// Alle berechneten Vektoren des Modells (ohne die leeren).
pub(crate) fn embeddings(
    conn: &Connection,
    model: &str,
) -> rusqlite::Result<Vec<(String, Vec<u8>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, vector FROM embeddings WHERE model = ?1 AND length(vector) > 0",
    )?;
    stmt.query_map([model], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// Vektoren, deren Stichwörter fehlen oder mit einer anderen Begriffsliste
/// berechnet wurden.
pub(crate) fn pending_tags(
    conn: &Connection,
    model: &str,
    version: &str,
    limit: u32,
) -> rusqlite::Result<Vec<(String, Vec<u8>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, vector FROM embeddings
         WHERE model = ?1 AND length(vector) > 0 AND (tags_version IS NULL OR tags_version != ?2)
         LIMIT ?3",
    )?;
    stmt.query_map(params![model, version, limit], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?
    .collect()
}

pub(crate) fn set_tags(
    conn: &Connection,
    id: &str,
    version: &str,
    tags: &[(&str, f32)],
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM capture_tags WHERE id = ?1", [id])?;
    for (term, score) in tags {
        conn.execute(
            "INSERT OR REPLACE INTO capture_tags (id, term, score) VALUES (?1, ?2, ?3)",
            params![id, term, score],
        )?;
    }
    conn.execute(
        "UPDATE embeddings SET tags_version = ?2 WHERE id = ?1",
        params![id, version],
    )?;
    Ok(())
}

/// Stichwörter je Aufnahme, bestes zuerst.
pub(crate) fn tags(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<std::collections::HashMap<String, Vec<String>>> {
    let mut out: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut stmt =
        conn.prepare_cached("SELECT term FROM capture_tags WHERE id = ?1 ORDER BY score DESC")?;
    for id in ids {
        let terms: Vec<String> = stmt
            .query_map([id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if !terms.is_empty() {
            out.insert(id.clone(), terms);
        }
    }
    Ok(out)
}

/// IDs, deren erkannter Text zur Suche passt.
pub(crate) fn text_matches(conn: &Connection, text: &str) -> rusqlite::Result<Vec<String>> {
    let terms = fts_terms(text);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt =
        conn.prepare_cached("SELECT id FROM captures_fts WHERE captures_fts MATCH ?1")?;
    stmt.query_map([terms.join(" ")], |r| r.get(0))?.collect()
}

/// (erledigt, alle) – für die Fortschrittsanzeige.
pub(crate) fn embedding_progress(conn: &Connection, model: &str) -> rusqlite::Result<(u32, u32)> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM embeddings e JOIN captures c ON c.id = e.id
                 WHERE e.model = ?1),
                (SELECT count(*) FROM captures)",
        [model],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// Volltextsuche; jedes Wort muss (als Wortanfang) vorkommen.
/// Filter und Sortierung für die Galerie.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureQuery {
    /// Volltext über den erkannten Text; leer = alle.
    pub text: String,
    /// Passen zum Text, auch wenn er nicht darin steht: Aufnahmen mit
    /// passendem Bildinhalt (ADR 0010) …
    pub content_ids: Vec<String>,
    /// … und Aufnahmen mit einem dieser Stichwörter.
    pub text_tags: Vec<String>,
    /// Nur Aufnahmen mit mindestens einem dieser Stichwörter (Schnellfilter).
    pub any_tags: Vec<String>,
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

/// WHERE-Teil und Parameter für die Filter der Galerie.
fn where_clause(q: &CaptureQuery) -> (String, Vec<rusqlite::types::Value>) {
    let mut filters: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let terms = fts_terms(&q.text);
    if !terms.is_empty() {
        let mut any =
            vec!["id IN (SELECT id FROM captures_fts WHERE captures_fts MATCH ?)".to_string()];
        args.push(terms.join(" ").into());
        if !q.content_ids.is_empty() {
            any.push(format!("id IN ({})", placeholders(q.content_ids.len())));
            args.extend(q.content_ids.iter().map(|id| id.clone().into()));
        }
        if !q.text_tags.is_empty() {
            any.push(format!(
                "id IN (SELECT id FROM capture_tags WHERE term IN ({}))",
                placeholders(q.text_tags.len())
            ));
            args.extend(q.text_tags.iter().map(|t| t.clone().into()));
        }
        filters.push(format!("({})", any.join(" OR ")));
    }
    if !q.any_tags.is_empty() {
        filters.push(format!(
            "id IN (SELECT id FROM capture_tags WHERE term IN ({}))",
            placeholders(q.any_tags.len())
        ));
        args.extend(q.any_tags.iter().map(|t| t.clone().into()));
    }
    if let Some(from) = q.from_ms {
        filters.push("created_at >= ?".into());
        args.push(from.into());
    }
    if let Some(to) = q.to_ms {
        filters.push("created_at < ?".into());
        args.push(to.into());
    }
    if let Some(mode) = &q.mode {
        filters.push("mode = ?".into());
        args.push(mode.clone().into());
    }
    if q.starred_only {
        filters.push("starred = 1".into());
    }
    let clause = if filters.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", filters.join(" AND "))
    };
    (clause, args)
}

pub(crate) fn query(
    conn: &Connection,
    root: &Path,
    q: &CaptureQuery,
) -> rusqlite::Result<Vec<CaptureRecord>> {
    let (filter, mut args) = where_clause(q);
    let order = if q.oldest_first { "ASC" } else { "DESC" };
    args.push(i64::from(q.limit.max(1)).into());
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM captures {filter} ORDER BY created_at {order}, id {order} LIMIT ?"
    ))?;
    stmt.query_map(rusqlite::params_from_iter(args), |r| row(root, r))?
        .collect()
}

/// (ID, Stichwort) aller Aufnahmen, die zu den Filtern passen – ohne
/// Begrenzung. Für die Anzahl je Schnellfilter.
pub(crate) fn tagged(
    conn: &Connection,
    q: &CaptureQuery,
) -> rusqlite::Result<Vec<(String, String)>> {
    let (filter, args) = where_clause(q);
    let mut stmt = conn.prepare(&format!(
        "SELECT id, term FROM capture_tags WHERE id IN (SELECT id FROM captures {filter})"
    ))?;
    stmt.query_map(rusqlite::params_from_iter(args), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?
    .collect()
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
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
        "INSERT INTO captures
         (id, created_at, mode, width, height, scale_factor, rel_dir, starred, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            rec.id,
            rec.created_at_ms,
            rec.mode,
            rec.width,
            rec.height,
            rec.scale_factor,
            rel.to_string_lossy(),
            rec.starred,
            rec.duration_ms,
        ],
    )?;
    Ok(())
}

const COLUMNS: &str =
    "id, created_at, mode, width, height, scale_factor, rel_dir, starred, duration_ms";

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
        duration_ms: r.get(8)?,
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
    conn.execute("DELETE FROM embeddings WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM capture_tags WHERE id = ?1", [id])?;
    Ok(())
}

pub(crate) fn set_starred(conn: &Connection, id: &str, starred: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE captures SET starred = ?2 WHERE id = ?1",
        params![id, starred],
    )?;
    Ok(())
}
