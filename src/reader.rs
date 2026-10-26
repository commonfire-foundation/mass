use crate::models::{Message, Session};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    pub version: u32,
    pub session: Session,
    pub messages: Vec<Message>,
    pub at: usize,
    pub total: usize,
    pub more: bool,
    pub next: Option<String>,
    pub previous: Option<String>,
}
pub fn resolve(db: &Connection, reference: &str) -> Result<Session> {
    if reference.len() < 4 || !reference.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!(crate::Usage(
            "Use at least four hexadecimal reference characters".into()
        ));
    }
    let rows = db
        .prepare("SELECT data FROM files WHERE substr(ref,1,length(?1))=?1 LIMIT 2")?
        .query_map([reference], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    match rows.len() {
        0 => bail!("Session not found"),
        1 => Ok(serde_json::from_str(&rows[0])?),
        _ => bail!("Ambiguous reference; use more characters"),
    }
}
/// Reuse a caller's transaction, or own one for the entire multi-query read.
/// This also lets find/read/export compose without nested BEGIN statements.
pub(crate) fn snapshot(db: &Connection) -> rusqlite::Result<Option<rusqlite::Transaction<'_>>> {
    db.is_autocommit()
        .then(|| db.unchecked_transaction())
        .transpose()
}
pub fn read(db: &Connection, reference: &str, at: Option<&str>) -> Result<Reading> {
    read_window(db, reference, at, 256)
}
pub fn read_window(
    db: &Connection,
    reference: &str,
    at: Option<&str>,
    limit: usize,
) -> Result<Reading> {
    let snapshot = snapshot(db)?;
    let db = snapshot.as_deref().unwrap_or(db);
    let limit = limit.clamp(1, 256);
    let session = resolve(db, reference)?;
    let file: String = db.query_row(
        "SELECT key FROM files WHERE ref=?1",
        [&session.reference],
        |r| r.get(0),
    )?;
    let start = if let Some(at) = at {
        db.query_row(
            "SELECT id FROM messages WHERE file=?1 AND ref=?2",
            [&file, at],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|_| anyhow::anyhow!("Message does not belong to this session"))?
    } else {
        0
    };
    // Bounded reader window; the CLI streams full history separately.
    let rows = db
        .prepare("SELECT data FROM messages WHERE file=?1 AND id>=?2 ORDER BY id LIMIT ?3")?
        .query_map(rusqlite::params![file, start, limit + 1], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let at: usize = db.query_row(
        "SELECT count(*) FROM messages WHERE file=?1 AND id<?2",
        rusqlite::params![file, start],
        |r| r.get(0),
    )?;
    let total: usize = db.query_row(
        "SELECT count(*) FROM messages WHERE file=?1",
        [&file],
        |r| r.get(0),
    )?;
    let more = rows.len() > limit;
    let next = rows
        .get(limit)
        .map(|s| serde_json::from_str::<Message>(s).map(|m| m.reference))
        .transpose()?;
    let previous = db.query_row("SELECT ref FROM (SELECT id,ref FROM messages WHERE file=?1 AND id<?2 ORDER BY id DESC LIMIT ?3) ORDER BY id LIMIT 1", rusqlite::params![file,start,limit], |r|r.get(0)).optional()?;
    let messages = rows
        .into_iter()
        .take(limit)
        .map(|s| serde_json::from_str(&s))
        .collect::<Result<_, _>>()?;
    Ok(Reading {
        version: 1,
        session,
        messages,
        at,
        total,
        more,
        next,
        previous,
    })
}
/// Literal, Unicode-aware find across indexed history with a visible execution budget.
/// Starts beyond `after`; wraps only after scanning the remaining history.
pub fn find_match(
    db: &Connection,
    reference: &str,
    needle: &str,
    after: &str,
    backwards: bool,
) -> Result<Option<String>> {
    let snapshot = snapshot(db)?;
    let db = snapshot.as_deref().unwrap_or(db);
    let session = resolve(db, reference)?;
    let file: String = db.query_row(
        "SELECT key FROM files WHERE ref=?1",
        [&session.reference],
        |r| r.get(0),
    )?;
    let start: i64 = db
        .query_row(
            "SELECT id FROM messages WHERE file=?1 AND ref=?2",
            [&file, after],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let needle = needle.to_lowercase();
    for wrap in [false, true] {
        let comparison = match (backwards, wrap) {
            (false, false) => ">",
            (false, true) => "<=",
            (true, false) => "<",
            (true, true) => ">=",
        };
        let order = if backwards { "DESC" } else { "ASC" };
        let mut statement = db.prepare(&format!(
            "SELECT ref,text FROM messages WHERE file=?1 AND id{comparison}?2 ORDER BY id {order}"
        ))?;
        for row in statement.query_map(rusqlite::params![file, start], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            if std::time::Instant::now() > deadline {
                bail!(
                    "Conversation find exceeded its two-second budget; use a more specific search"
                );
            }
            let (reference, text) = row?;
            if text.to_lowercase().contains(&needle) {
                return Ok(Some(reference));
            }
        }
    }
    Ok(None)
}
pub fn stream(
    db: &Connection,
    reference: &str,
    at: Option<&str>,
    mut emit: impl FnMut(Message) -> Result<()>,
) -> Result<Session> {
    let snapshot = snapshot(db)?;
    let db = snapshot.as_deref().unwrap_or(db);
    let session = resolve(db, reference)?;
    let start = if let Some(at) = at {
        db.query_row(
            "SELECT m.id FROM messages m JOIN files f ON f.key=m.file WHERE f.ref=?1 AND m.ref=?2",
            [&session.reference, at],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|_| anyhow::anyhow!("Message does not belong to this session"))?
    } else {
        0
    };
    let mut statement=db.prepare("SELECT m.data FROM messages m JOIN files f ON f.key=m.file WHERE f.ref=?1 AND m.id>=?2 ORDER BY m.id")?;
    for row in statement.query_map(rusqlite::params![session.reference, start], |r| {
        r.get::<_, String>(0)
    })? {
        emit(serde_json::from_str(&row?)?)?;
    }
    Ok(session)
}
