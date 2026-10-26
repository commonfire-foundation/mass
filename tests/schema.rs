use mass::{
    adapters,
    config::{Config, Source},
    index, reader,
    search::{self, Filters},
};
use rusqlite::{Connection, params};
use serde_json::json;
use std::{path::PathBuf, sync::atomic::AtomicBool};

struct Fixture {
    directory: tempfile::TempDir,
    source: Source,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            tempfile::tempdir_in(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let source = Source {
            id: "schema".into(),
            agent: "pi".into(),
            root: directory.path().join("sessions"),
            resume: false,
            optional: false,
        };
        std::fs::create_dir(&source.root).unwrap();
        let path = directory.path().join("index.sqlite3");
        Self {
            directory,
            source,
            path,
        }
    }
    fn transcript(&self, name: &str, text: &str) -> PathBuf {
        let path = self.source.root.join(format!("{name}.jsonl"));
        let records = [
            json!({"type":"session","id":name,"cwd":self.directory.path()}),
            json!({"type":"session_info","name":"Neutral title"}),
            json!({"type":"message","id":"one","timestamp":"2026-01-02T00:00:00Z","message":{"role":"assistant","content":text}}),
        ];
        std::fs::write(
            &path,
            records.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        path
    }
    fn refresh(&self) {
        let config = Config {
            sources: vec![self.source.clone()],
            exclusions: vec![],
        };
        assert!(
            index::refresh(&self.path, &config, &AtomicBool::new(false), |_| {})
                .unwrap()
                .warnings
                .is_empty()
        );
    }
    fn legacy(&self) -> (String, Vec<serde_json::Value>) {
        let path = self.transcript(
            "one",
            &format!("{}\n\tneedle phrase café", "background ".repeat(4000)),
        );
        let mut messages = Vec::new();
        let session = adapters::parse(&path, &self.source, |m| {
            messages.push(m);
            Ok(())
        })
        .unwrap();
        let db = Connection::open(&self.path).unwrap();
        db.execute_batch(include_str!("fixtures/schema-v1.sql"))
            .unwrap();
        let key = mass::reference(&[&self.source.id, "one.jsonl"]);
        db.execute(
            "INSERT INTO files VALUES (?1,?2,?3,'legacy',?4,?5)",
            params![
                key,
                self.source.id,
                path.to_str().unwrap(),
                session.reference,
                serde_json::to_string(&session).unwrap()
            ],
        )
        .unwrap();
        for m in &messages {
            db.execute(
                "INSERT INTO messages(file,ref,data) VALUES (?1,?2,?3)",
                params![key, m.reference, serde_json::to_string(m).unwrap()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO fts(rowid,title,project,body) VALUES (?1,?2,?3,?4)",
                params![db.last_insert_rowid(), session.title, session.cwd, m.text],
            )
            .unwrap();
        }
        (
            session.reference,
            messages
                .iter()
                .map(|m| serde_json::to_value(m).unwrap())
                .collect(),
        )
    }
}
fn integrity(db: &Connection) {
    db.execute("INSERT INTO fts(fts,rank) VALUES ('integrity-check',1)", [])
        .unwrap();
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}
#[test]
fn schema_upgrade_is_explicit_and_preserves_history() {
    let f = Fixture::new();
    let (reference, messages) = f.legacy();
    let bytes = std::fs::read(&f.path).unwrap();
    assert!(
        index::open(&f.path, false)
            .unwrap_err()
            .to_string()
            .contains("upgrade")
    );
    assert_eq!(std::fs::read(&f.path).unwrap(), bytes);
    let db = index::open(&f.path, true).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let reading = reader::read(&db, &reference, None).unwrap();
    assert_eq!(
        serde_json::to_value(reading.messages).unwrap(),
        json!(messages)
    );
    assert_eq!(
        search::query(&db, "needle", &Filters::default())
            .unwrap()
            .results
            .len(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='fts_content'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        0
    );
    integrity(&db);
    drop(db);
    // Refresh reparses old fingerprints without changing public identities.
    f.refresh();
    f.transcript("one", "replacement unique text");
    f.refresh();
    let db = index::open(&f.path, true).unwrap();
    assert!(
        search::query(&db, "needle", &Filters::default())
            .unwrap()
            .results
            .is_empty()
    );
    assert_eq!(
        search::query(&db, "replacement", &Filters::default())
            .unwrap()
            .results[0]
            .session
            .reference,
        reference
    );
    integrity(&db);
}
#[test]
fn failed_upgrade_rolls_back_schema_and_history() {
    let f = Fixture::new();
    f.legacy();
    let db = Connection::open(&f.path).unwrap();
    db.execute(
        "INSERT INTO messages(file,ref,data) VALUES ('file','bad','invalid JSON')",
        [],
    )
    .unwrap();
    drop(db);
    assert!(index::open(&f.path, true).is_err());
    let db = Connection::open(&f.path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT data FROM messages WHERE ref='bad'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "invalid JSON"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM fts WHERE fts MATCH 'needle'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='legacy_messages'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn external_content_preserves_legacy_ranking_excerpts_and_filters() {
    let f = Fixture::new();
    for n in 0..12 {
        f.transcript(
            &n.to_string(),
            &format!(
                "{} needle phrase café {}",
                "background ".repeat(n * 2000),
                "needle ".repeat(n)
            ),
        );
    }
    f.refresh();
    let db = index::open(&f.path, true).unwrap();
    // Internal-content FTS and the old rank/group/filter/excerpt ordering form
    // an independent baseline over identical chunks and document statistics.
    db.execute_batch("CREATE VIRTUAL TABLE old_fts USING fts5(title,project,body,tokenize='unicode61',prefix='2 3');
        INSERT INTO old_fts(rowid,title,project,body) SELECT id,title,project,body FROM search_content;").unwrap();
    for (query, expression) in [
        ("needle", "\"needle\"*"),
        ("café", "\"café\"*"),
        ("\"needle phrase\"", "\"needle phrase\""),
    ] {
        for agent in [None, Some("pi"), Some("claude")] {
            for before in [None, Some("2026-01-02"), Some("2026-01-03")] {
                let filters = Filters {
                    agent: agent.map(str::to_owned),
                    before: before.map(str::to_owned),
                    limit: 5,
                    ..Default::default()
                };
                let actual = search::query(&db, query, &filters).unwrap();
                let mut stmt=db.prepare("WITH matches AS MATERIALIZED (
                    SELECT m.file,m.ref,bm25(old_fts,5.0,2.0,1.0) AS rank,snippet(old_fts,2,'','',' … ',40) AS excerpt
                    FROM old_fts JOIN messages m ON m.id=old_fts.rowid WHERE old_fts MATCH ?1
                ), ranked AS (
                    SELECT f.ref AS session_ref,m.ref,m.rank,m.excerpt,f.activity,
                        row_number() OVER (PARTITION BY f.ref ORDER BY m.rank,m.ref) AS n
                    FROM matches m JOIN files f ON f.key=m.file
                    WHERE (?2 IS NULL OR f.agent=?2) AND (?3 IS NULL OR substr(f.activity,1,10)<?3)
                ) SELECT session_ref,ref,excerpt,rank FROM ranked WHERE n=1 ORDER BY rank,activity DESC,session_ref LIMIT 5").unwrap();
                let expected = stmt
                    .query_map(params![expression, agent, before], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, f64>(3)?,
                        ))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                let actual = actual
                    .results
                    .into_iter()
                    .map(|h| (h.session.reference, h.message_ref, h.excerpt, h.rank))
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{query} {agent:?} {before:?}");
            }
        }
    }
    integrity(&db);
}
