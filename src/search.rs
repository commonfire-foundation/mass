use crate::{
    index,
    models::{Hit, Page, Session},
    reference,
};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Debug, Clone, Default, Serialize, Deserialize, clap::Args)]
pub struct Filters {
    #[arg(long, value_parser=["pi","claude","codex"])]
    pub agent: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub before: Option<String>,
    #[arg(long,default_value_t=20,value_parser=clap::value_parser!(u32).range(1..=200))]
    pub limit: u32,
    #[arg(long)]
    pub cursor: Option<String>,
}
// One insertion, deletion, or substitution for metadata words only.
fn near(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.chars().take(128).collect();
    let b: Vec<_> = b.chars().take(128).collect();
    if b.len() < 4 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut errors) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        errors += 1;
        if errors > 1 {
            return false;
        }
        if a.len() >= b.len() {
            i += 1;
        }
        if b.len() >= a.len() {
            j += 1;
        }
    }
    errors + (a.len() - i) + (b.len() - j) <= 1
}
#[derive(Default)]
struct Token {
    text: String,
    phrase: bool,
    // Only an unquoted prefix introduces a filter: project:"path with spaces"
    // is a filter, while "project:value" is searchable literal text.
    filter: Option<usize>,
}
fn tokens(query: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut word = Token::default();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.phrase = true;
            }
            c if c.is_whitespace() && !quoted => {
                if !word.text.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            _ => {
                if c == ':' && !quoted && !word.phrase && word.filter.is_none() {
                    word.filter = Some(word.text.len());
                }
                word.text.push(c);
            }
        }
    }
    if quoted {
        bail!(crate::Usage("Unclosed quote in query".into()));
    }
    if !word.text.is_empty() {
        out.push(word);
    }
    Ok(out)
}
struct Budget<'a>(&'a Connection);
impl Drop for Budget<'_> {
    fn drop(&mut self) {
        self.0.progress_handler(0, None::<fn() -> bool>);
    }
}
pub fn query(db: &Connection, query: &str, filters: &Filters) -> Result<Page> {
    query_cancellable(db, query, filters, Arc::new(AtomicBool::new(false)))
}
pub fn query_cancellable(
    db: &Connection,
    query: &str,
    filters: &Filters,
    cancel: Arc<AtomicBool>,
) -> Result<Page> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Search cancelled");
    }
    let snapshot = db.unchecked_transaction()?;
    let db = &snapshot;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    db.progress_handler(
        1000,
        Some(move || cancel.load(Ordering::Relaxed) || std::time::Instant::now() > deadline),
    );
    let _budget = Budget(db);
    let mut f = filters.clone();
    if f.agent
        .as_ref()
        .is_some_and(|a| !matches!(a.as_str(), "pi" | "claude" | "codex"))
    {
        bail!(crate::Usage("Unknown agent filter".into()));
    }
    let mut terms = Vec::new();
    let mut fuzzy_terms = Vec::new();
    for token in tokens(query)? {
        let word = token.text;
        if let Some(colon) = token.filter {
            let (key, rest) = word.split_at(colon);
            let value = &rest[1..];
            match key {
                "agent" => {
                    if !matches!(value, "pi" | "claude" | "codex") {
                        bail!(crate::Usage(format!("Unknown agent: {value}")));
                    }
                    f.agent = Some(value.into());
                }
                "project" => f.project = Some(value.into()),
                _ => bail!(crate::Usage(format!("Unknown filter: {key}"))),
            };
        } else {
            fuzzy_terms.push((word.to_lowercase(), token.phrase));
            terms.push(format!(
                "\"{}\"{}",
                word.replace('"', "\"\""),
                if token.phrase { "" } else { "*" }
            ));
        }
    }
    if let Some(p) = &f.project {
        let path = match std::fs::canonicalize(p) {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::path::absolute(p)?,
            Err(e) => return Err(e.into()),
        };
        f.project = Some(path.to_string_lossy().into());
    }
    for d in [&f.since, &f.before].into_iter().flatten() {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| crate::Usage("Dates must be YYYY-MM-DD".into()))?;
    }
    let stamp: String = db.query_row(
        "SELECT coalesce((SELECT value FROM state WHERE key='coverage'),'') || coalesce((SELECT value FROM state WHERE key='revision'),'0')",
        [],
        |r| r.get(0),
    )?;
    let mut signature = f.clone();
    signature.cursor = None;
    let signature = reference(&[query, &serde_json::to_string(&signature)?, &stamp]);
    let offset = if let Some(cursor) = &f.cursor {
        let (sig, n) = cursor
            .split_once(':')
            .ok_or_else(|| crate::Usage("Invalid cursor".into()))?;
        if sig != signature {
            bail!("Cursor expired or belongs to different filters; restart search");
        }
        n.parse::<u64>()
            .map_err(|_| crate::Usage("Invalid cursor offset".into()))?
    } else {
        0
    };
    if offset > i64::MAX as u64 {
        bail!(crate::Usage("Cursor out of range".into()));
    }
    let limit = if f.limit == 0 { 20 } else { f.limit.min(200) };
    let text = terms.join(" AND ");
    // Omit absent predicates so SQLite can use the metadata indexes, rather
    // than scanning JSON through optional-parameter OR expressions.
    let mut predicates = vec!["1=1"];
    if f.agent.is_some() {
        predicates.push("f.agent=?2");
    }
    if f.project.is_some() {
        predicates.push("f.workspace=?3");
    }
    if f.since.is_some() {
        predicates.push("f.activity>=?4");
    }
    if f.before.is_some() {
        predicates.push("f.activity<?5");
    }
    let where_filter = predicates.join(" AND ");
    if !text.is_empty() && predicates.len() > 1 {
        // An impossible scope needs no FTS posting traversal at all. Keep this
        // in the same snapshot and after cursor/query validation.
        let eligible: Option<i32> = db
            .query_row(
                &format!("SELECT 1 FROM files f WHERE {where_filter} LIMIT ?6"),
                params![text, f.agent, f.project, f.since, f.before, 1],
                |r| r.get(0),
            )
            .optional()?;
        if eligible.is_none() {
            return Ok(Page {
                version: 1,
                results: vec![],
                next_cursor: None,
                coverage: index::coverage(db)?,
            });
        }
    }
    let sql = if text.is_empty() {
        format!(
            "SELECT f.data, '', NULL, 0.0 FROM files f WHERE {where_filter} AND ?1='' ORDER BY f.activity DESC,f.ref LIMIT ?6 OFFSET ?7"
        )
    } else {
        format!(
            "WITH eligible AS MATERIALIZED (
                SELECT f.key,f.ref,f.activity FROM files f WHERE {where_filter}
            ), matches AS MATERIALIZED (
                SELECT m.file,m.ref,m.id AS message_id,bm25(fts,5.0,2.0,1.0) AS rank
                FROM fts JOIN messages m ON m.id=fts.rowid JOIN eligible e ON e.key=m.file
                WHERE fts MATCH ?1
                UNION ALL
                SELECT f.key,'',NULL,1.0 FROM eligible e JOIN files f ON f.key=e.key
                WHERE mass_fuzzy(f.title || ' ' || f.cwd)
            ), ranked AS (
                SELECT m.*,e.ref AS session_ref,e.activity,
                    row_number() OVER (PARTITION BY e.ref ORDER BY m.rank,m.ref) AS n
                FROM matches m JOIN eligible e ON e.key=m.file
            ), page AS MATERIALIZED (
                SELECT * FROM ranked WHERE n=1 ORDER BY rank,activity DESC,session_ref LIMIT ?6 OFFSET ?7
            ) SELECT f.data,p.ref,p.message_id,p.rank FROM page p JOIN files f ON f.key=p.file
              ORDER BY p.rank,p.activity DESC,p.session_ref"
        )
    };
    db.create_scalar_function(
        "mass_fuzzy",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            let metadata: String = ctx.get(0)?;
            let metadata = metadata.to_lowercase();
            Ok(!fuzzy_terms.is_empty()
                && fuzzy_terms.iter().all(|(term, phrase)| {
                    if *phrase {
                        metadata.contains(term)
                    } else {
                        metadata
                            .split(|c: char| !c.is_alphanumeric())
                            .any(|word| word.starts_with(term) || near(word, term))
                    }
                }))
        },
    )?;
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            text,
            f.agent,
            f.project,
            f.since,
            f.before,
            limit + 1,
            offset
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, f64>(3)?,
            ))
        },
    )?;
    // Rank/group first; excerpt generation is only needed for the returned page.
    let mut excerpt_query = db.prepare(
        "SELECT snippet(fts,2,'','', ' … ',40) FROM fts WHERE rowid=?1 AND fts MATCH ?2",
    )?;
    let mut hits = Vec::new();
    for row in rows {
        let (s, message_ref, message_id, rank) = row?;
        let session: Session = serde_json::from_str(&s)?;
        let excerpt = if let Some(id) = message_id {
            excerpt_query.query_row(params![id, text], |r| r.get(0))?
        } else if text.is_empty() {
            String::new()
        } else {
            session.title.clone()
        };
        hits.push(Hit {
            session,
            message_ref,
            excerpt,
            rank,
        });
    }
    let more = hits.len() > limit as usize;
    hits.truncate(limit as usize);
    Ok(Page {
        version: 1,
        results: hits,
        next_cursor: more.then(|| format!("{signature}:{}", offset + limit as u64)),
        coverage: index::coverage(db)?,
    })
}
