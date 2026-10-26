mod duplicates;
mod projections;
mod schema;

use crate::{
    adapters,
    config::Config,
    models::{Coverage, Session},
    reference,
};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const INSERT_MESSAGE: &str = "INSERT INTO messages(file,ref,native_id,parent,role,text,line,chunk,projection) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)";

pub fn open(path: &Path, write: bool) -> Result<Connection> {
    if path.is_symlink() {
        bail!("Refusing a symlink index");
    }
    if !write && !path.is_file() {
        bail!("No index yet. Run mass index to build a private plaintext cache.");
    }
    if write {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)?;
        }
    }
    let db = Connection::open_with_flags(
        path,
        if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let application: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
    if version != 0 && application != 0x4d415353 {
        bail!(
            "Not a Rust MASS index; choose a fresh --db path. Prototype migration is rebuild-only."
        );
    }
    if version == 0 {
        let tables: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )?;
        if tables != 0 {
            bail!("Refusing to initialize a nonempty foreign database");
        }
    }
    if !(0..=schema::VERSION).contains(&version) {
        bail!("Unsupported index schema; rebuild at a new --db path");
    }
    if write {
        db.pragma_update(None, "journal_mode", "WAL")?;
        schema::initialize(&db, version)?;
    } else if version != schema::VERSION {
        bail!("Index needs initialization or a schema upgrade; run mass index");
    }
    Ok(db)
}
pub fn coverage(db: &Connection) -> Result<Coverage> {
    let raw: Option<String> = db
        .query_row("SELECT value FROM state WHERE key='coverage'", [], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}
fn save(db: &Connection, c: &Coverage) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO state VALUES ('coverage',?1)",
        [serde_json::to_string(c)?],
    )?;
    Ok(())
}
fn fingerprint(path: &Path, config: &str) -> Result<String> {
    let m = path.metadata()?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{}", m.dev(), m.ino(), m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let identity = String::new();
    Ok(reference(&[
        &m.len().to_string(),
        &format!("{:?}", m.modified()?),
        &identity,
        config,
        &adapters::VERSION.to_string(),
    ]))
}
// Compare normalized searchable history, not timestamps or serialization noise.
// Some(order) means one sequence is an exact prefix of the other.
fn history_order(
    db: &Connection,
    new_key: &str,
    old_key: &str,
    cancel: &AtomicBool,
) -> Result<Option<std::cmp::Ordering>> {
    let sql = "SELECT role,text FROM messages WHERE file=?1 AND role!='metadata' ORDER BY id";
    let mut a = db.prepare(sql)?;
    let mut b = db.prepare(sql)?;
    let mut a = a.query([new_key])?;
    let mut b = b.query([old_key])?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("Indexing cancelled");
        }
        match (a.next()?, b.next()?) {
            (None, None) => return Ok(Some(std::cmp::Ordering::Equal)),
            (None, Some(_)) => return Ok(Some(std::cmp::Ordering::Less)),
            (Some(_), None) => return Ok(Some(std::cmp::Ordering::Greater)),
            (Some(a), Some(b)) => {
                if a.get::<_, String>(0)? != b.get::<_, String>(0)?
                    || a.get::<_, String>(1)? != b.get::<_, String>(1)?
                {
                    return Ok(None);
                }
            }
        }
    }
}
pub fn refresh(
    path: &Path,
    config: &Config,
    cancel: &AtomicBool,
    mut progress: impl FnMut(String),
) -> Result<Coverage> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("lock");
    if lock_path.is_symlink() {
        bail!("Refusing symlink writer lock");
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock_exclusive()
        .context("Indexing elsewhere; retry when the other writer finishes")?;
    let mut db = open(path, true)?;
    let previous = coverage(&db)?;
    let mut report = Coverage {
        last_attempt: chrono::Utc::now().to_rfc3339(),
        last_success: previous.last_success,
        ..Default::default()
    };
    report
        .warnings
        .push("Indexing in progress; coverage is not yet reconciled".into());
    save(&db, &report)?;
    report.warnings.clear();
    for source in &config.sources {
        if cancel.load(Ordering::Relaxed) {
            report.warnings.push("Indexing cancelled".into());
            break;
        }
        if !source.root.is_dir() || source.root.is_symlink() {
            let cached: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE source=?1)",
                [&source.id],
                |r| r.get(0),
            )?;
            if !source.optional || cached {
                report.warnings.push(format!(
                    "{}: source unavailable; cached history retained",
                    source.id
                ));
            }
            progress(format!("{}: source unavailable", source.id));
            continue;
        }
        let found = match crate::discovery::scan(source, &config.exclusions, cancel) {
            Ok(found) => found,
            Err(e) => {
                report.warnings.push(e.to_string());
                break;
            }
        };
        let mut complete = found.warnings.is_empty();
        report.warnings.extend(found.warnings);
        let mut paths = found.paths;
        // Refresh existing owners before their aliases, even when a duplicate
        // sorts earlier. Otherwise a copy could be discarded against stale
        // canonical content that is about to change later in this same scan.
        let owners = db
            .prepare("SELECT path FROM files WHERE source=?1")?
            .query_map([&source.id], |r| {
                r.get::<_, String>(0).map(std::path::PathBuf::from)
            })?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        paths.sort_by_key(|p| !owners.contains(p));
        progress(format!("{}: {} transcript files", source.id, paths.len()));
        let config_key = serde_json::to_string(source)?;
        let discovered = paths
            .iter()
            .map(|file| {
                let logical = file
                    .strip_prefix(&source.root)
                    .expect("discovered inside source")
                    .to_string_lossy();
                (
                    reference(&[&source.id, logical.strip_suffix(".zst").unwrap_or(&logical)]),
                    file.clone(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let file_count = paths.len();
        for (position, file) in paths.into_iter().enumerate() {
            if position % 25 == 0 {
                progress(format!(
                    "{}: checking file {} of {}",
                    source.id,
                    position + 1,
                    file_count
                ));
            }
            if cancel.load(Ordering::Relaxed) {
                complete = false;
                report.warnings.push("Indexing cancelled".into());
                break;
            }
            let logical = file.strip_prefix(&source.root)?.to_string_lossy();
            let key = reference(&[&source.id, logical.strip_suffix(".zst").unwrap_or(&logical)]);
            let result = (|| -> Result<()> {
                let stamp = fingerprint(&file, &config_key)?;
                if duplicates::unchanged(&db, &key, &stamp, &config_key, &discovered)? {
                    report.duplicates += 1;
                    report.duplicates_unchanged += 1;
                    return Ok(());
                }
                let old: Option<(String, String)> = db
                    .query_row(
                        "SELECT fingerprint,data FROM files WHERE key=?1",
                        [&key],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((f, data)) = old
                    && f == stamp
                {
                    let s: Session = serde_json::from_str(&data)?;
                    report.warnings.extend(
                        s.warnings
                            .iter()
                            .map(|w| format!("{}: {}: {w}", source.id, file.display())),
                    );
                    report.unchanged += 1;
                    return Ok(());
                }
                let tx = db.transaction()?;
                tx.execute("DELETE FROM messages WHERE file=?1", [&key])?;
                let mut has_projections = false;
                let mut session = {
                    let mut insert = tx.prepare_cached(INSERT_MESSAGE)?;
                    adapters::parse_cancellable(&file, source, cancel, |m| {
                        has_projections |= m.projection;
                        if cancel.load(Ordering::Relaxed) {
                            bail!("Indexing cancelled");
                        }
                        insert.execute(params![
                            key,
                            m.reference,
                            m.native_id,
                            m.parent,
                            m.role,
                            m.text,
                            m.line,
                            m.chunk,
                            m.projection
                        ])?;
                        Ok(())
                    })?
                };
                if fingerprint(&file, &config_key)? != stamp {
                    bail!("File changed while reading; retry indexing");
                }
                if has_projections {
                    projections::deduplicate(&tx, &key, cancel)?;
                }
                let base_reference = session.reference.clone();
                let collision: Option<(String, String, String, String)> = tx
                    .query_row(
                        "SELECT key,path,data,fingerprint FROM files WHERE ref=?1 AND key!=?2",
                        params![session.reference, key],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?;
                if let Some((old_key, old_path, data, old_stamp)) = collision {
                    let mut previous: Session = serde_json::from_str(&data)?;
                    let order = if session.warnings.is_empty()
                        && previous.warnings.is_empty()
                        && session.cwd == previous.cwd
                        && session.child == previous.child
                        && session.parent == previous.parent
                    {
                        history_order(&tx, &key, &old_key, cancel)?
                    } else {
                        None
                    };
                    if complete
                        && (!discovered.contains_key(&old_key) || !Path::new(&old_path).exists())
                        || order == Some(std::cmp::Ordering::Greater)
                    {
                        tx.execute("DELETE FROM messages WHERE file=?1", [&old_key])?;
                        tx.execute("DELETE FROM files WHERE key=?1", [&old_key])?;
                    } else if matches!(
                        order,
                        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                    ) {
                        tx.execute("DELETE FROM messages WHERE file=?1", [&key])?;
                        tx.execute("DELETE FROM files WHERE key=?1", [&key])?;
                        tx.execute("INSERT OR REPLACE INTO duplicates(key,source,path,fingerprint,canonical,canonical_fingerprint) VALUES (?1,?2,?3,?4,?5,?6)", params![key,source.id,file.to_string_lossy(),stamp,old_key,old_stamp])?;
                        tx.commit()?;
                        report.duplicates += 1;
                        return Ok(());
                    } else {
                        // Divergent copies are separate readable histories, never a
                        // guessed merge or an automatic resume into the wrong branch.
                        session.reference = reference(&[&session.reference, "copy", &key]);
                        session.read_only = true;
                        previous.read_only = true;
                        tx.execute(
                            "UPDATE files SET data=?1 WHERE key=?2",
                            params![serde_json::to_string(&previous)?, old_key],
                        )?;
                    }
                }
                // Keep an existing copy's reference and read-only guard on append,
                // including after its formerly canonical sibling disappears.
                let former: Option<String> = tx
                    .query_row("SELECT data FROM files WHERE key=?1", [&key], |r| r.get(0))
                    .optional()?;
                if let Some(data) = former {
                    let former: Session = serde_json::from_str(&data)?;
                    if former.read_only
                        && former.native_id == session.native_id
                        && former.identity_scope == session.identity_scope
                        && former.agent == session.agent
                    {
                        session.reference = former.reference;
                        session.read_only = true;
                    }
                }
                if session.reference != base_reference {
                    let mut stmt =
                        tx.prepare("SELECT id,data FROM messages WHERE file=?1 ORDER BY id")?;
                    let mut rows = stmt.query([&key])?;
                    while let Some(row) = rows.next()? {
                        if cancel.load(Ordering::Relaxed) {
                            bail!("Indexing cancelled");
                        }
                        let id: i64 = row.get(0)?;
                        let mut m: crate::models::Message =
                            serde_json::from_str(&row.get::<_, String>(1)?)?;
                        m.reference =
                            reference(&[&session.reference, &m.native_id, &m.chunk.to_string()]);
                        tx.execute(
                            "UPDATE messages SET ref=?1 WHERE id=?2",
                            params![m.reference, id],
                        )?;
                    }
                }
                // A changed canonical file must not lose its ambiguity guard while
                // an unchanged divergent copy remains in the cache.
                session.read_only |= tx.query_row("SELECT EXISTS(SELECT 1 FROM files WHERE source=?1 AND json_extract(data,'$.native_id')=?2 AND COALESCE(json_extract(data,'$.identity_scope'),'')=?3 AND key!=?4 AND ref!=?5)",params![source.id,session.native_id,session.identity_scope,key,session.reference],|r|r.get::<_,bool>(0))?;
                tx.execute("DELETE FROM duplicates WHERE key=?1", [&key])?;
                tx.execute(
                    "INSERT OR REPLACE INTO files(key,source,path,fingerprint,ref,data) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        key,
                        source.id,
                        file.to_string_lossy(),
                        stamp,
                        session.reference,
                        serde_json::to_string(&session)?
                    ],
                )?;
                let count: i64 =
                    tx.query_row("SELECT count(*) FROM messages WHERE file=?1", [&key], |r| {
                        r.get(0)
                    })?;
                if count == 0 {
                    let m = crate::models::Message {
                        reference: reference(&[&session.reference, "metadata"]),
                        native_id: "metadata".into(),
                        parent: String::new(),
                        role: "metadata".into(),
                        text: String::new(),
                        line: 0,
                        chunk: 0,
                        projection: false,
                    };
                    tx.execute(
                        INSERT_MESSAGE,
                        params![
                            key,
                            m.reference,
                            m.native_id,
                            m.parent,
                            m.role,
                            m.text,
                            m.line,
                            m.chunk,
                            m.projection
                        ],
                    )?;
                }
                tx.execute("INSERT INTO fts(rowid,title,project,body) SELECT id,?1,?2,text FROM messages WHERE file=?3",params![session.title,session.cwd,key])?;
                tx.execute("UPDATE messages SET indexed=1 WHERE file=?1", [&key])?;
                tx.commit()?;
                report.indexed += 1;
                report.warnings.extend(
                    session
                        .warnings
                        .iter()
                        .map(|w| format!("{}: {}: {w}", source.id, file.display())),
                );
                Ok(())
            })();
            if let Err(e) = result {
                if e.downcast_ref::<adapters::EmptyTranscript>().is_some()
                    && !db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM files WHERE key=?1)",
                        [&key],
                        |r| r.get::<_, bool>(0),
                    )?
                {
                    report.ignored_empty += 1;
                    continue;
                }
                report
                    .warnings
                    .push(format!("{}: {}: {e}", source.id, file.display()));
            }
        }
        if complete && !cancel.load(Ordering::Relaxed) {
            let old = db
                .prepare("SELECT key FROM files WHERE source=?1 UNION SELECT key FROM duplicates WHERE source=?1")?
                .query_map([&source.id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let tx = db.transaction()?;
            let mut removed = 0;
            for key in old {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if !discovered.contains_key(&key) {
                    tx.execute("DELETE FROM messages WHERE file=?1", [&key])?;
                    removed += tx.execute("DELETE FROM files WHERE key=?1", [&key])?;
                    tx.execute("DELETE FROM duplicates WHERE key=?1", [&key])?;
                }
            }
            if !cancel.load(Ordering::Relaxed) {
                tx.commit()?;
                report.removed += removed;
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        if !report.warnings.iter().any(|w| w == "Indexing cancelled") {
            report.warnings.push("Indexing cancelled".into());
        }
        save(&db, &report)?;
        return Ok(report);
    }
    // Removed configuration is not an unavailable mount: purge its derived cache.
    let old = db
        .prepare("SELECT source FROM files UNION SELECT source FROM duplicates")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for source in old {
        if cancel.load(Ordering::Relaxed) {
            report.warnings.push("Indexing cancelled".into());
            break;
        }
        if !config.sources.iter().any(|s| s.id == source) {
            let tx = db.transaction()?;
            tx.execute(
                "DELETE FROM messages WHERE file IN (SELECT key FROM files WHERE source=?1)",
                [&source],
            )?;
            let removed = tx.execute("DELETE FROM files WHERE source=?1", [&source])?;
            tx.execute("DELETE FROM duplicates WHERE source=?1", [&source])?;
            if cancel.load(Ordering::Relaxed) {
                report.warnings.push("Indexing cancelled".into());
                break;
            }
            tx.commit()?;
            report.removed += removed;
        }
    }
    if report.warnings.is_empty() {
        report.last_success = report.last_attempt.clone();
    }
    save(&db, &report)?;
    Ok(report)
}
