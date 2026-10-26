//! Pair adjacent, complete message occurrences, never a file-wide set of text.
//! An unmatched/ambiguous projection is preferable to deleting a real turn.
use crate::models::Message;
use anyhow::{Result, bail};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};

struct Occurrence {
    first: i64,
    last: i64,
    line: usize,
    role: String,
    projection: bool,
    digest: Sha256,
}
fn finish(db: &Connection, previous: &mut Option<Occurrence>, current: Occurrence) -> Result<()> {
    if let Some(prior) = previous.as_ref()
        && prior.projection != current.projection
        && prior.role == current.role
        && prior.digest.clone().finalize() == current.digest.clone().finalize()
    {
        let projection = if prior.projection { prior } else { &current };
        db.execute(
            "DELETE FROM messages WHERE id BETWEEN ?1 AND ?2",
            params![projection.first, projection.last],
        )?;
        // Each normalized occurrence can consume at most one projection. A later
        // repeated prompt must not be paired against an already-used message.
        *previous = None;
    } else {
        *previous = Some(current);
    }
    Ok(())
}
pub(super) fn deduplicate(db: &Connection, file: &str, cancel: &AtomicBool) -> Result<()> {
    let mut statement = db.prepare("SELECT id,data FROM messages WHERE file=?1 ORDER BY id")?;
    let mut rows = statement.query([file])?;
    let mut current: Option<Occurrence> = None;
    let mut previous = None;
    while let Some(row) = rows.next()? {
        if cancel.load(Ordering::Relaxed) {
            bail!("Indexing cancelled");
        }
        let id: i64 = row.get(0)?;
        let message: Message = serde_json::from_str(&row.get::<_, String>(1)?)?;
        if current.as_ref().is_some_and(|c| c.line != message.line) {
            finish(db, &mut previous, current.take().unwrap())?;
        }
        let occurrence = current.get_or_insert_with(|| Occurrence {
            first: id,
            last: id,
            line: message.line,
            role: message.role,
            projection: message.projection,
            digest: Sha256::new(),
        });
        occurrence.last = id;
        // Include boundaries so only complete, identically chunked messages pair.
        occurrence
            .digest
            .update((message.text.len() as u64).to_le_bytes());
        occurrence.digest.update(message.text.as_bytes());
    }
    if let Some(current) = current {
        finish(db, &mut previous, current)?;
    }
    Ok(())
}
