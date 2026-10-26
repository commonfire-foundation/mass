//! Discovery records for copies that do not own searchable rows.
use super::fingerprint;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::HashMap, path::PathBuf};

pub(super) fn unchanged(
    db: &Connection,
    key: &str,
    stamp: &str,
    config: &str,
    discovered: &HashMap<String, PathBuf>,
) -> Result<bool> {
    let canonical: Option<(String, String, String)> = db
        .query_row(
            "SELECT f.key,f.path,f.fingerprint FROM duplicates d JOIN files f ON f.key=d.canonical
         WHERE d.key=?1 AND d.fingerprint=?2 AND d.canonical_fingerprint=f.fingerprint",
            params![key, stamp],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(canonical.is_some_and(|(key, path, stamp)| {
        // Check discovery/exclusions and disk metadata too: the canonical file
        // may be visited later in this scan or have changed to .jsonl.zst.
        discovered
            .get(&key)
            .is_some_and(|p| p == &PathBuf::from(&path))
            && fingerprint(std::path::Path::new(&path), config).is_ok_and(|s| s == stamp)
    }))
}
