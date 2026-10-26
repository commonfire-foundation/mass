use mass::{
    adapters,
    config::{Config, Source},
    index, reader,
    search::{self, Filters},
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    config: Config,
    db: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        let config = Config {
            sources: vec![Source {
                id: "test".into(),
                agent: "pi".into(),
                root: root.join("sessions"),
                resume: false,
                optional: false,
            }],
            exclusions: vec![],
        };
        Self {
            db: root.join("index.sqlite3"),
            root,
            config,
        }
    }
    fn pi(&self, name: &str, text: &str) {
        self.records(name,&[json!({"type":"session","version":3,"id":name,"cwd":self.root}),json!({"type":"message","id":"entry","parentId":"parent","timestamp":"2026-01-02T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":text}]}})]);
    }
    fn records(&self, name: &str, records: &[serde_json::Value]) {
        std::fs::write(
            self.root.join("sessions").join(format!("{name}.jsonl")),
            records
                .iter()
                .map(|r| r.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
    }
    fn refresh(&self) -> mass::models::Coverage {
        index::refresh(&self.db, &self.config, &AtomicBool::new(false), |_| {}).unwrap()
    }
    fn query(&self, q: &str) -> mass::models::Page {
        search::query(
            &index::open(&self.db, false).unwrap(),
            q,
            &Filters {
                limit: 20,
                ..Default::default()
            },
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn beyond_caps_identity_and_incremental() {
    let f = Fixture::new();
    f.pi(
        "one",
        &format!("{} hidden needle beyond caps", "é ".repeat(100_000)),
    );
    let before = std::fs::read(f.root.join("sessions/one.jsonl")).unwrap();
    assert_eq!(f.refresh().indexed, 1);
    let p = f.query("\"hidden needle\"");
    assert_eq!(p.results.len(), 1);
    assert!(p.results[0].excerpt.contains("hidden needle"));
    let r = p.results[0].session.reference.clone();
    assert_eq!(f.refresh().unchanged, 1);
    let db = index::open(&f.db, false).unwrap();
    let reading = reader::read(&db, &r, Some(&p.results[0].message_ref)).unwrap();
    assert!(reading.messages[0].text.contains("hidden needle"));
    drop(db);
    std::fs::remove_file(&f.db).unwrap();
    f.refresh();
    assert_eq!(r, f.query("needle").results[0].session.reference);
    assert_eq!(
        before,
        std::fs::read(f.root.join("sessions/one.jsonl")).unwrap()
    );
}
#[test]
fn missing_store_is_not_deletion() {
    let f = Fixture::new();
    f.pi("one", "unique searchable content");
    f.refresh();
    std::fs::rename(f.root.join("sessions"), f.root.join("offline")).unwrap();
    assert!(!f.refresh().warnings.is_empty());
    assert_eq!(f.query("unique").results.len(), 1);
    std::fs::rename(f.root.join("offline"), f.root.join("sessions")).unwrap();
    std::fs::remove_file(f.root.join("sessions/one.jsonl")).unwrap();
    assert_eq!(f.refresh().removed, 1);
    assert!(f.query("unique").results.is_empty());
}
#[test]
fn distinct_session_pagination_and_cursor_binding() {
    let f = Fixture::new();
    for n in 0..6 {
        f.pi(&n.to_string(), "exact answer");
    }
    f.refresh();
    let db = index::open(&f.db, false).unwrap();
    let mut filters = Filters {
        limit: 2,
        ..Default::default()
    };
    let mut refs = std::collections::HashSet::new();
    loop {
        let p = search::query(&db, "answer", &filters).unwrap();
        for h in p.results {
            assert!(refs.insert(h.session.reference));
        }
        match p.next_cursor {
            Some(c) => filters.cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(refs.len(), 6);
    assert!(search::query(&db, "different", &filters).is_err());
}
#[test]
fn passage_terms_do_not_cross_turns() {
    let f = Fixture::new();
    f.records(
        "one",
        &[
            json!({"type":"session","id":"one"}),
            json!({"type":"session_info","name":"neutral"}),
            json!({"type":"message","id":"a","message":{"role":"assistant","content":"aardvark"}}),
            json!({"type":"message","id":"b","message":{"role":"assistant","content":"zebra"}}),
        ],
    );
    f.refresh();
    assert!(f.query("aardvark zebra").results.is_empty());
}
#[test]
fn malformed_and_oversized_coverage_is_persistent() {
    let f = Fixture::new();
    f.pi("one", "safe content");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(f.root.join("sessions/one.jsonl"))
        .unwrap();
    writeln!(file, "{}", "x".repeat(17 * 1024 * 1024)).unwrap();
    writeln!(file, "{{unfinished").unwrap();
    drop(file);
    assert!(!f.refresh().warnings.is_empty());
    assert!(!f.refresh().warnings.is_empty());
    assert_eq!(f.query("safe").results.len(), 1);
}
#[test]
fn other_adapters_and_compression() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "claude".into();
    f.records("claude",&[json!({"type":"user","sessionId":"claude-native","cwd":f.root,"uuid":"native-entry","parentUuid":"parent","isSidechain":true,"message":{"content":[{"type":"text","text":"Claude answer"}]}})]);
    f.refresh();
    let hit = &f.query("Claude").results[0];
    assert!(hit.session.child);
    let r = reader::read(
        &index::open(&f.db, false).unwrap(),
        &hit.session.reference,
        None,
    )
    .unwrap();
    assert_eq!(r.messages[0].native_id, "native-entry");
    std::fs::remove_file(f.root.join("sessions/claude.jsonl")).unwrap();
    f.config.sources[0].agent = "codex".into();
    let text=[json!({"type":"session_meta","payload":{"id":"codex-native","cwd":f.root,"parent_thread_id":"parent"}}),json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"compressed answer"}]}})].iter().map(|v|v.to_string()+"\n").collect::<String>();
    std::fs::write(
        f.root.join("sessions/codex.jsonl.zst"),
        zstd::encode_all(text.as_bytes(), 1).unwrap(),
    )
    .unwrap();
    f.refresh();
    assert_eq!(f.query("compressed").results.len(), 1);
}
#[test]
fn stable_source_qualified_references() {
    let f = Fixture::new();
    f.pi("one", "answer");
    let path = f.root.join("sessions/one.jsonl");
    let a = adapters::parse(&path, &f.config.sources[0], |_| Ok(())).unwrap();
    let mut other = f.config.sources[0].clone();
    other.id = "other".into();
    let b = adapters::parse(&path, &other, |_| Ok(())).unwrap();
    assert_ne!(a.reference, b.reference);
}
#[test]
fn filters_and_errors() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let f = Fixture::new();
    f.pi("one", "naïve snake_case callback-uri");
    f.refresh();
    assert_eq!(f.query("naïve agent:pi").results.len(), 1);
    assert_eq!(f.query("snake callback").results.len(), 1);
    assert_eq!(f.query("agent:claude").results.len(), 0);
    let db = index::open(&f.db, false).unwrap();
    assert!(search::query(&db, "bad:value", &Filters::default()).is_err());
    assert!(search::query(&db, "\"unfinished", &Filters::default()).is_err());
    assert!(
        search::query(
            &db,
            "",
            &Filters {
                since: Some("bad".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
    // An impossible scope must return before even preparing an FTS read, but
    // must not bypass cursor validation. Denying FTS makes this deterministic.
    db.authorizer(Some(|ctx: AuthContext<'_>| {
        if matches!(ctx.action, AuthAction::Read { table_name, .. } if table_name.starts_with("fts")) {
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    }));
    assert!(
        search::query(&db, "callback agent:claude", &Filters::default())
            .unwrap()
            .results
            .is_empty()
    );
    assert!(search::query(&db, "callback agent:pi", &Filters::default()).is_err());
    assert!(
        search::query(
            &db,
            "callback agent:claude",
            &Filters {
                cursor: Some("invalid".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn quoted_colons_are_literals_but_filter_values_can_be_quoted() {
    let f = Fixture::new();
    f.pi("literal", "Error: timeout std::vector agent:pi");
    f.pi("other", "unrelated conversation");
    f.refresh();
    for query in ["\"Error: timeout\"", "\"std::vector\"", "\"agent:pi\""] {
        let page = f.query(query);
        assert_eq!(page.results.len(), 1, "{query}");
        assert_eq!(page.results[0].session.native_id, "literal");
        assert!(!page.results[0].message_ref.is_empty());
    }
    assert_eq!(f.query("agent:\"pi\"").results.len(), 2);
    let project = f.root.join("path with spaces:and-colons");
    f.records(
        "spaces",
        &[
            json!({"type":"session","id":"spaces","cwd":project}),
            json!({"type":"message","id":"entry","message":{"role":"user","content":"needle"}}),
        ],
    );
    f.refresh();
    assert_eq!(
        f.query(&format!("project:\"{}\" needle", project.display()))
            .results
            .len(),
        1
    );
}
#[test]
fn resume_plan_is_read_only_and_disabled() {
    let f = Fixture::new();
    f.pi("one", "answer");
    f.refresh();
    let s = f.query("answer").results.remove(0).session;
    let p = mass::resume::plan(&s, &f.config).unwrap();
    assert!(!p.supported);
    assert_eq!(p.argv[0], "--session");
}
#[test]
fn cancellation_and_disabled_resume_do_not_read_transcripts() {
    let mut f = Fixture::new();
    f.pi("one", "needle");
    f.refresh();
    let session = f.query("needle").results.remove(0).session;
    std::fs::write(&session.path, "not valid JSON\n").unwrap();
    let plan = mass::resume::plan(&session, &f.config).unwrap();
    assert!(!plan.supported);
    assert!(plan.warnings.iter().any(|w| w.contains("not checked")));
    f.config.sources[0].resume = true;
    assert!(mass::resume::plan(&session, &f.config).is_err());
    let cancel = std::sync::Arc::new(AtomicBool::new(true));
    assert!(
        mass::resume::plan_cancellable(&session, &f.config, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(
        adapters::parse_cancellable(
            std::path::Path::new("missing.jsonl"),
            &f.config.sources[0],
            &cancel,
            |_| Ok(())
        )
        .unwrap_err()
        .to_string()
        .contains("cancelled")
    );
    assert!(
        mass::discovery::scan(&f.config.sources[0], &[], &cancel)
            .err()
            .unwrap()
            .to_string()
            .contains("cancelled")
    );
    let db = index::open(&f.db, false).unwrap();
    assert!(
        search::query_cancellable(&db, "needle", &Filters::default(), cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert_eq!(
        search::query(&db, "needle", &Filters::default())
            .unwrap()
            .results
            .len(),
        1
    );
}
#[test]
fn discovery_uses_only_eligible_plain_files_to_shadow_archives() {
    let f = Fixture::new();
    f.pi("one", "needle");
    let archive = f.root.join("sessions/one.jsonl.zst");
    std::fs::write(&archive, b"filename-only fixture").unwrap();
    let found = mass::discovery::scan(&f.config.sources[0], &[], &AtomicBool::new(false)).unwrap();
    assert_eq!(found.paths.len(), 1);
    assert!(found.paths[0].ends_with("one.jsonl"));
    std::fs::remove_file(f.root.join("sessions/one.jsonl")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&archive, f.root.join("sessions/one.jsonl")).unwrap();
    let found = mass::discovery::scan(&f.config.sources[0], &[], &AtomicBool::new(false)).unwrap();
    assert_eq!(found.paths, vec![archive]);
    let found = mass::discovery::scan(
        &f.config.sources[0],
        &["one.jsonl".into()],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(found.paths.is_empty());
}
#[test]
fn tui_rejects_stale_generations() {
    let s = mass::tui::State {
        generation: 3,
        ..Default::default()
    };
    assert!(!s.accepts(2));
    assert!(s.accepts(3));
}
#[test]
fn renamed_transcript_keeps_identity_in_one_scan() {
    let f = Fixture::new();
    f.pi("one", "moved answer");
    f.refresh();
    let old = f.query("moved").results.remove(0);
    std::fs::rename(
        f.root.join("sessions/one.jsonl"),
        f.root.join("sessions/renamed.jsonl"),
    )
    .unwrap();
    assert!(f.refresh().warnings.is_empty());
    let new = f.query("moved").results.remove(0);
    assert_eq!(old.session.reference, new.session.reference);
    assert_eq!(old.message_ref, new.message_ref);
}
#[test]
fn writer_lock_refuses_second_writer() {
    use fs2::FileExt;
    let f = Fixture::new();
    f.pi("one", "answer");
    f.refresh();
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(f.db.with_extension("lock"))
        .unwrap();
    lock.try_lock_exclusive().unwrap();
    let e = index::refresh(&f.db, &f.config, &AtomicBool::new(false), |_| {}).unwrap_err();
    assert!(e.to_string().contains("Indexing elsewhere"));
    assert_eq!(f.query("answer").results.len(), 1);
}
#[test]
fn foreign_prototype_database_is_rejected() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(&f.db).unwrap();
    db.execute_batch("CREATE TABLE sessions(id INTEGER); PRAGMA user_version=1;")
        .unwrap();
    drop(db);
    assert!(index::open(&f.db, true).is_err());
    let db = rusqlite::Connection::open(&f.db).unwrap();
    let n: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='files'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
#[test]
fn cancelled_update_keeps_old_data() {
    let f = Fixture::new();
    f.pi("one", "retained answer");
    f.refresh();
    std::fs::remove_file(f.root.join("sessions/one.jsonl")).unwrap();
    let c = index::refresh(&f.db, &f.config, &AtomicBool::new(true), |_| {}).unwrap();
    assert!(c.warnings.iter().any(|w| w.contains("cancelled")));
    assert_eq!(f.query("retained").results.len(), 1);
}
#[test]
fn codex_event_only_and_duplicate_projections() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "codex".into();
    f.records("one",&[
        json!({"type":"session_meta","payload":{"id":"one"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"unique projection"}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"duplicate answer"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"duplicate answer"}]}}),
    ]);
    f.refresh();
    let hit = &f.query("projection").results[0];
    let r = reader::read(
        &index::open(&f.db, false).unwrap(),
        &hit.session.reference,
        None,
    )
    .unwrap();
    assert_eq!(r.messages.len(), 2);
}
#[test]
fn projection_pairs_preserve_repeated_turns_and_complete_chunks() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "codex".into();
    let event =
        |text: &str| json!({"type":"event_msg","payload":{"type":"user_message","message":text}});
    let native = |text: &str| json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}});
    let long = format!("{} unique tail", "same prefix ".repeat(4000));
    let different = format!("{} other tail", "same prefix ".repeat(4000));
    f.records("one", &[
        json!({"type":"session_meta","payload":{"id":"one"}}),
        event("repeat me"), native("repeat me"),
        // Neither another projection nor a distant occurrence may reuse that pair.
        event("repeat me"),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"first answer"}]}}),
        event("repeat me"),
        // Equal prefixes are not equal messages; keep all their chunks.
        event(&long), native(&different),
        // Reverse order also pairs, once, including every chunk of a long message.
        native(&long), event(&long), event(&long),
    ]);
    assert!(f.refresh().warnings.is_empty());
    let reference = &f.query("repeat").results[0].session.reference;
    let db = index::open(&f.db, false).unwrap();
    let mut messages = Vec::new();
    mass::export::messages(&db, reference, None, |m| {
        messages.push(m.text);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        messages,
        vec![
            "repeat me",
            "repeat me",
            "first answer",
            "repeat me",
            &long,
            &different,
            &long,
            &long
        ]
    );
}
#[test]
fn fuzzy_metadata_is_weaker_than_direct_text() {
    let f = Fixture::new();
    f.pi("one", "migration discussion");
    f.pi("two", "migraton direct");
    f.refresh();
    let p = f.query("migraton");
    assert_eq!(p.results.len(), 2);
    assert_eq!(p.results[0].session.native_id, "two");
}
#[test]
fn pagination_groups_before_limiting_messages() {
    let f = Fixture::new();
    let mut records = vec![json!({"type":"session","id":"large"})];
    for i in 0..2100 {
        records.push(json!({"type":"message","id":i.to_string(),"message":{"role":"assistant","content":"shared answer"}}));
    }
    f.records("large", &records);
    f.pi("small", "shared answer");
    f.refresh();
    assert_eq!(f.query("shared").results.len(), 2);
    let hit = f
        .query("shared")
        .results
        .into_iter()
        .find(|h| h.session.native_id == "large")
        .unwrap();
    let db = index::open(&f.db, false).unwrap();
    let first = reader::read(&db, &hit.session.reference, None).unwrap();
    assert!(first.more);
    let second = reader::read(&db, &hit.session.reference, first.next.as_deref()).unwrap();
    assert_eq!(
        first.messages.last().unwrap().line + 1,
        second.messages.first().unwrap().line
    );
    assert_eq!(second.previous.as_ref(), Some(&first.messages[0].reference));
}
#[test]
fn conversation_find_crosses_reader_windows_and_wraps() {
    let f = Fixture::new();
    let mut records = vec![json!({"type":"session","id":"long"})];
    for i in 0..80 {
        records.push(json!({"type":"message","id":i.to_string(),"message":{"role":"assistant","content":if i==70{"Distant CAFÉ needle"}else if i==5{"Earlier CAFÉ needle"}else{"ordinary text"}}}));
    }
    f.records("long", &records);
    f.refresh();
    let reference = f.query("needle").results.remove(0).session.reference;
    let db = index::open(&f.db, false).unwrap();
    let first = reader::read_window(&db, &reference, None, 32).unwrap();
    assert_eq!(first.messages.len(), 32);
    assert_eq!(first.total, 80);
    let at = reader::find_match(
        &db,
        &reference,
        "café needle",
        &first.messages.last().unwrap().reference,
        false,
    )
    .unwrap()
    .unwrap();
    let found = reader::read_window(&db, &reference, Some(&at), 32).unwrap();
    assert_eq!(found.at, 70);
    assert_eq!(found.messages[0].native_id, "70");
    let earlier = first.messages[5].reference.clone();
    assert_eq!(
        reader::find_match(&db, &reference, "café needle", &at, false).unwrap(),
        Some(earlier.clone())
    );
    assert_eq!(
        reader::find_match(&db, &reference, "café needle", &at, true).unwrap(),
        Some(earlier.clone())
    );
    assert_eq!(
        reader::find_match(&db, &reference, "café needle", &earlier, true).unwrap(),
        Some(at)
    );
}
#[test]
fn reader_window_keeps_one_snapshot_during_index_commits() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let f = Fixture::new();
    f.pi("one", "original passage");
    f.refresh();
    let hit = f.query("original").results.remove(0);
    f.records("one", &[
        json!({"type":"session","id":"one","cwd":f.root}),
        json!({"type":"message","id":"entry","message":{"role":"user","content":"replacement passage"}}),
        json!({"type":"message","id":"second","message":{"role":"assistant","content":"extra turn"}}),
    ]);
    let db = index::open(&f.db, false).unwrap();
    let committed = std::sync::Arc::new(AtomicBool::new(false));
    let flag = committed.clone();
    let writer_path = f.db.clone();
    let config = f.config.clone();
    // Commit on a separate connection exactly between resolving the session and
    // preparing the message read. This deterministically reproduces the race.
    db.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "messages",
                ..
            }
        ) && !flag.swap(true, Ordering::Relaxed)
        {
            assert!(
                index::refresh(&writer_path, &config, &AtomicBool::new(false), |_| {})
                    .unwrap()
                    .warnings
                    .is_empty()
            );
        }
        Authorization::Allow
    }));
    let reading =
        reader::read_window(&db, &hit.session.reference, Some(&hit.message_ref), 1).unwrap();
    assert!(committed.load(Ordering::Relaxed));
    assert_eq!(reading.total, 1);
    assert!(!reading.more);
    assert_eq!(reading.messages[0].text, "original passage");
    db.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let latest = reader::read(&db, &hit.session.reference, None).unwrap();
    assert_eq!(latest.total, 2);
    assert_eq!(latest.messages[0].text, "replacement passage");
    // The same public API composes inside an already-open caller transaction.
    let tx = db.unchecked_transaction().unwrap();
    assert_eq!(
        reader::read(&tx, &hit.session.reference, None)
            .unwrap()
            .total,
        2
    );
}
#[test]
fn pi_exports_have_child_scoped_read_only_identities() {
    let f = Fixture::new();
    for child in 0..2 {
        f.records(&format!("export-{child}"),&[
            json!({"version":1,"recordType":"message","sourceEventType":"initial_prompt","runId":"run-one","childIndex":child,"cwd":f.root,"message":{"role":"user","content":"export needle"}}),
            json!({"version":1,"recordType":"message","sourceEventType":"message_end","runId":"run-one","childIndex":child,"cwd":f.root,"message":{"role":"user","content":"export needle"}}),
            json!({"version":1,"recordType":"message","sourceEventType":"message_end","runId":"run-one","childIndex":child,"cwd":f.root,"message":{"role":"assistant","content":[{"type":"text","text":"child answer"}]}}),
        ]);
    }
    assert!(f.refresh().warnings.is_empty());
    let page = f.query("export needle");
    assert_eq!(page.results.len(), 2);
    assert_ne!(
        page.results[0].session.reference,
        page.results[1].session.reference
    );
    let db = index::open(&f.db, false).unwrap();
    for hit in page.results {
        assert!(hit.session.read_only && hit.session.child);
        let r = reader::read(&db, &hit.session.reference, None).unwrap();
        assert_eq!(r.messages.len(), 2);
        assert!(
            !mass::resume::plan(&hit.session, &f.config)
                .unwrap()
                .supported
        );
    }
}
#[test]
fn claude_children_do_not_collide_with_parent_or_siblings() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "claude".into();
    f.records("parent",&[json!({"type":"user","sessionId":"parent-native","cwd":f.root,"uuid":"shared-entry","message":{"content":"family needle"}})]);
    for child in ["one", "two"] {
        f.records(child,&[json!({"type":"user","sessionId":"parent-native","agentId":child,"isSidechain":true,"cwd":f.root,"uuid":"shared-entry","message":{"content":"family needle"}})]);
    }
    assert!(f.refresh().warnings.is_empty());
    let page = f.query("family");
    assert_eq!(page.results.len(), 3);
    let refs: std::collections::HashSet<_> = page.results.iter().map(|h| &h.message_ref).collect();
    assert_eq!(refs.len(), 3);
    let before = page
        .results
        .iter()
        .find(|h| h.session.identity_scope == "child:one")
        .unwrap()
        .session
        .reference
        .clone();
    std::fs::rename(
        f.root.join("sessions/one.jsonl"),
        f.root.join("sessions/renamed.jsonl"),
    )
    .unwrap();
    assert!(f.refresh().warnings.is_empty());
    assert_eq!(
        f.query("family")
            .results
            .iter()
            .find(|h| h.session.identity_scope == "child:one")
            .unwrap()
            .session
            .reference,
        before
    );
}
#[test]
fn duplicate_prefixes_keep_fuller_history_and_survive_copy_removal() {
    let f = Fixture::new();
    let mut records = vec![
        json!({"type":"session","id":"same","cwd":f.root}),
        json!({"type":"message","id":"first","message":{"role":"user","content":"shared needle"}}),
    ];
    f.records("a", &records);
    records.push(json!({"type":"message","id":"second","message":{"role":"assistant","content":"additional tail"}}));
    f.records("b", &records);
    f.records("c", &records);
    assert!(f.refresh().warnings.is_empty());
    let hit = f.query("needle").results.remove(0);
    assert_eq!(f.query("needle").results.len(), 1);
    assert_eq!(f.query("tail").results.len(), 1);
    assert_eq!(
        reader::read(
            &index::open(&f.db, false).unwrap(),
            &hit.session.reference,
            None
        )
        .unwrap()
        .messages
        .len(),
        2
    );
    let again = f.refresh();
    assert!(again.warnings.is_empty());
    assert_eq!(again.duplicates, 2);
    std::fs::remove_file(f.root.join("sessions/b.jsonl")).unwrap();
    assert!(f.refresh().warnings.is_empty());
    assert_eq!(
        f.query("tail").results[0].session.reference,
        hit.session.reference
    );
}
#[test]
fn duplicate_fingerprints_skip_parsing_and_invalidate_safely() {
    for change in ["delete", "exclude", "rewrite", "append-copy"] {
        let mut f = Fixture::new();
        let records = vec![
            json!({"type":"session","id":"same","cwd":f.root}),
            json!({"type":"message","id":"one","message":{"role":"user","content":"original needle"}}),
        ];
        // The canonical path sorts after its duplicate: updates must still run first.
        f.records("z-owner", &records);
        f.refresh();
        f.records("a-copy", &records);
        f.refresh();
        let reference = f.query("needle").results[0].session.reference.clone();
        let unchanged = f.refresh();
        assert_eq!(unchanged.duplicates_unchanged, 1);
        assert_eq!(unchanged.duplicates, 1);
        assert_eq!(unchanged.indexed, 0);
        match change {
            "delete" => std::fs::remove_file(f.root.join("sessions/z-owner.jsonl")).unwrap(),
            "exclude" => f.config.exclusions.push("z-owner.jsonl".into()),
            "rewrite" => f.records("z-owner", &[
                records[0].clone(),
                json!({"type":"message","id":"one","message":{"role":"user","content":"divergent replacement"}}),
            ]),
            _ => {
                let mut appended = records.clone();
                appended.push(json!({"type":"message","id":"two","message":{"role":"assistant","content":"appended tail"}}));
                f.records("a-copy", &appended);
            }
        }
        let report = f.refresh();
        assert!(
            report.warnings.is_empty(),
            "{change}: {:?}",
            report.warnings
        );
        assert_eq!(report.duplicates_unchanged, 0, "{change}");
        let old = f.query("needle");
        assert_eq!(old.results.len(), 1, "{change}");
        if change == "rewrite" {
            assert_eq!(f.query("replacement").results.len(), 1);
            assert!(old.results[0].session.read_only);
        } else {
            assert_eq!(old.results[0].session.reference, reference);
        }
        if change == "append-copy" {
            assert_eq!(f.query("tail").results.len(), 1);
        }
        let db = index::open(&f.db, true).unwrap();
        db.execute("INSERT INTO fts(fts,rank) VALUES ('integrity-check',1)", [])
            .unwrap();
        drop(db);
        f.config.sources.clear();
        f.refresh();
        let db = index::open(&f.db, false).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM duplicates", [], |r| r
                .get::<_, usize>(0))
                .unwrap(),
            0
        );
    }
}
#[test]
fn divergent_copies_remain_readable_and_cannot_resume() {
    let mut f = Fixture::new();
    f.config.sources[0].resume = true;
    for side in ["left", "right"] {
        f.records(side,&[json!({"type":"session","id":"same","cwd":f.root}),json!({"type":"message","id":"first","message":{"role":"user","content":"shared seed"}}),json!({"type":"message","id":"second","message":{"role":"assistant","content":format!("{side} branch")}})]);
    }
    assert!(f.refresh().warnings.is_empty());
    let page = f.query("branch");
    assert_eq!(page.results.len(), 2);
    for h in &page.results {
        assert!(h.session.read_only);
        let r = reader::read(
            &index::open(&f.db, false).unwrap(),
            &h.session.reference,
            Some(&h.message_ref),
        )
        .unwrap();
        assert!(r.messages[0].text.contains("branch"));
        let p = mass::resume::plan(&h.session, &f.config).unwrap();
        assert!(!p.supported);
        assert!(p.warnings.iter().any(|w| w.contains("read-only")));
    }
    // Reparse the canonical file while its sibling remains unchanged.
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(f.root.join("sessions/left.jsonl"))
            .unwrap(),
        "{{\"type\":\"session_info\",\"name\":\"new title\"}}"
    )
    .unwrap();
    assert!(f.refresh().warnings.is_empty());
    assert!(
        f.query("branch")
            .results
            .iter()
            .all(|h| h.session.read_only)
    );
    let right_ref = page
        .results
        .iter()
        .find(|h| h.session.path.ends_with("right.jsonl"))
        .unwrap()
        .session
        .reference
        .clone();
    std::fs::remove_file(f.root.join("sessions/left.jsonl")).unwrap();
    assert!(f.refresh().warnings.is_empty());
    writeln!(std::fs::OpenOptions::new().append(true).open(f.root.join("sessions/right.jsonl")).unwrap(),"{{\"type\":\"message\",\"id\":\"third\",\"message\":{{\"role\":\"assistant\",\"content\":\"new tail\"}}}}").unwrap();
    assert!(f.refresh().warnings.is_empty());
    let hit = f.query("tail").results.remove(0);
    assert_eq!(hit.session.reference, right_ref);
    assert!(hit.session.read_only);
}
#[test]
fn empty_placeholders_are_ignored_but_truncation_retains_cached_history() {
    let f = Fixture::new();
    std::fs::write(f.root.join("sessions/one.jsonl"), " \n").unwrap();
    let c = f.refresh();
    assert!(c.warnings.is_empty());
    assert_eq!(c.ignored_empty, 1);
    f.pi("one", "retained needle");
    assert!(f.refresh().warnings.is_empty());
    std::fs::write(f.root.join("sessions/one.jsonl"), "").unwrap();
    assert!(!f.refresh().warnings.is_empty());
    assert_eq!(f.query("retained").results.len(), 1);
}
#[test]
fn large_multimodal_records_keep_text_without_indexing_image_data() {
    let f = Fixture::new();
    f.records("image",&[json!({"type":"session","id":"image"}),json!({"type":"message","id":"large","message":{"role":"user","content":[{"type":"text","text":"before image needle"},{"type":"image","data":"x".repeat(17*1024*1024),"mimeType":"image/png"},{"type":"text","text":"after image needle"}]}})]);
    let raw = std::fs::read(f.root.join("sessions/image.jsonl")).unwrap();
    std::fs::write(
        f.root.join("sessions/image.jsonl.zst"),
        zstd::stream::encode_all(raw.as_slice(), 1).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(f.root.join("sessions/image.jsonl")).unwrap();
    let c = f.refresh();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(f.query("\"before image\"").results.len(), 1);
    assert_eq!(f.query("\"after image\"").results.len(), 1);
    let hit = f.query("needle").results.remove(0);
    let r = reader::read(
        &index::open(&f.db, false).unwrap(),
        &hit.session.reference,
        None,
    )
    .unwrap();
    assert!(r.messages[0].text.len() < 100);
}
#[test]
fn retained_text_budget_and_malformed_records_do_not_hide_later_messages() {
    let f = Fixture::new();
    f.records("large",&[json!({"type":"session","id":"large"}),json!({"type":"message","id":"large","message":{"role":"user","content":"x".repeat(17*1024*1024)}}),json!({"type":"message","id":"safe","message":{"role":"user","content":"safe later needle"}})]);
    let c = f.refresh();
    assert!(
        c.warnings
            .iter()
            .any(|w| w.contains("retained text/metadata"))
    );
    assert_eq!(f.query("later").results.len(), 1);
    assert!(
        f.refresh()
            .warnings
            .iter()
            .any(|w| w.contains("retained text/metadata"))
    );
}
#[test]
fn mixed_export_identities_roll_back_instead_of_merging_children() {
    let f = Fixture::new();
    f.records("mixed",&(0..2).map(|i|json!({"version":1,"recordType":"message","runId":"run","childIndex":i,"message":{"role":"user","content":"must not merge"}})).collect::<Vec<_>>());
    assert!(
        f.refresh()
            .warnings
            .iter()
            .any(|w| w.contains("Mixed native"))
    );
    assert!(f.query("merge").results.is_empty());
}
#[test]
fn codex_forks_keep_own_identity_across_explicit_ancestor_headers() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "codex".into();
    for id in ["parent", "child-one", "child-two"] {
        let mut records = vec![
            json!({"type":"session_meta","payload":{"id":id,"cwd":f.root,"parent_thread_id":if id=="parent"{""}else{"parent"},"forked_from_id":if id=="parent"{""}else{"parent"}}}),
        ];
        if id != "parent" {
            records.push(
                json!({"type":"session_meta","payload":{"id":"parent","cwd":"/ancestor/project"}}),
            );
        }
        records.push(json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"inherited fork needle"}]}}));
        f.records(id, &records);
    }
    assert!(f.refresh().warnings.is_empty());
    let page = f.query("fork needle");
    assert_eq!(page.results.len(), 3);
    for h in page.results {
        assert_eq!(h.session.cwd, f.root.to_string_lossy());
        assert_eq!(h.session.child, h.session.native_id != "parent");
    }
    f.records("unrelated",&[json!({"type":"session_meta","payload":{"id":"a"}}),json!({"type":"session_meta","payload":{"id":"b"}}),json!({"type":"event_msg","payload":{"type":"user_message","message":"unrelated secret"}})]);
    assert!(
        f.refresh()
            .warnings
            .iter()
            .any(|w| w.contains("Mixed native"))
    );
    assert!(f.query("unrelated secret").results.is_empty());
}
#[test]
fn codex_large_image_url_does_not_discard_neighboring_text() {
    let mut f = Fixture::new();
    f.config.sources[0].agent = "codex".into();
    f.records("image",&[json!({"type":"session_meta","payload":{"id":"image"}}),json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_image","image_url":format!("data:image/png;base64,{}","A".repeat(17*1024*1024))},{"type":"input_text","text":"retained multimodal needle"}]}})]);
    let c = f.refresh();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(f.query("multimodal needle").results.len(), 1);
}
#[test]
fn incomplete_copies_are_not_silently_deduplicated() {
    let f = Fixture::new();
    let records = [
        json!({"type":"session","id":"same"}),
        json!({"type":"message","id":"first","message":{"role":"user","content":"copy needle"}}),
    ];
    f.records("a", &records);
    f.records("b", &records);
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(f.root.join("sessions/b.jsonl"))
            .unwrap(),
        "{{malformed"
    )
    .unwrap();
    let c = f.refresh();
    assert!(
        c.warnings
            .iter()
            .any(|w| w.contains("b.jsonl") && w.contains("invalid JSON") && w.contains("line 3"))
    );
    assert_eq!(f.query("copy needle").results.len(), 2);
}
#[test]
fn project_filter_keeps_deleted_workspaces_searchable() {
    let f = Fixture::new();
    let project = f.root.join("removed-project");
    std::fs::create_dir(&project).unwrap();
    f.records("archived", &[
        json!({"type":"session","id":"archived","cwd":project}),
        json!({"type":"message","id":"entry","message":{"role":"user","content":"archived workspace needle"}}),
    ]);
    f.refresh();
    std::fs::remove_dir(&project).unwrap();
    let db = index::open(&f.db, false).unwrap();
    for path in [
        project.clone(),
        project
            .strip_prefix(std::env::current_dir().unwrap())
            .unwrap()
            .to_path_buf(),
    ] {
        let page = search::query(
            &db,
            "workspace needle",
            &Filters {
                project: Some(path.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.results.len(), 1);
    }
}
#[test]
fn deferred_excerpts_still_point_to_matching_passages() {
    let f = Fixture::new();
    f.pi(
        "passage",
        &format!(
            "{} uniquephrase needle {}",
            "ordinary context ".repeat(200),
            "following context ".repeat(200)
        ),
    );
    f.refresh();
    let page = f.query("\"uniquephrase needle\"");
    assert_eq!(page.results.len(), 1);
    let hit = &page.results[0];
    assert!(hit.excerpt.contains("uniquephrase needle"));
    let db = index::open(&f.db, false).unwrap();
    let reading = reader::read(&db, &hit.session.reference, Some(&hit.message_ref)).unwrap();
    assert!(
        reading
            .messages
            .iter()
            .any(|m| m.reference == hit.message_ref && m.text.contains("uniquephrase needle"))
    );
}
#[test]
fn exports_join_chunks_and_scope_selected_messages_without_path_metadata() {
    use mass::export::{self, Format};
    let f = Fixture::new();
    let text = format!(
        "{}\n```make\nall:\n\tprintf hello\n```\n```python\nif True:\n\tprint('café')\n```\r\n",
        "café Unicode passage ".repeat(3000)
    );
    f.records("export", &[
        json!({"type":"session","id":"export","cwd":f.root}),
        json!({"type":"message","id":"long","message":{"role":"user","content":text}}),
        json!({"type":"message","id":"short","message":{"role":"assistant","content":"second message"}}),
    ]);
    f.refresh();
    let reference = f.query("Unicode passage").results[0]
        .session
        .reference
        .clone();
    let db = index::open(&f.db, false).unwrap();
    let reading = reader::read(&db, &reference, None).unwrap();
    assert!(reading.messages.len() > 3);
    let mut output = Vec::new();
    export::write(&db, &reference, None, Format::Json, false, &mut output).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(document["messages"].as_array().unwrap().len(), 2);
    assert_eq!(document["messages"][0]["text"], text);
    assert_eq!(document["native_backup"], false);
    assert!(document["session"].get("cwd").is_none());
    assert!(
        !String::from_utf8(output)
            .unwrap()
            .contains(f.root.to_str().unwrap())
    );
    let selected = &reading.messages[2].reference;
    let mut output = Vec::new();
    export::write(
        &db,
        &reference,
        Some(selected),
        Format::Json,
        true,
        &mut output,
    )
    .unwrap();
    let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(document["messages"].as_array().unwrap().len(), 1);
    assert_eq!(document["messages"][0]["text"], text);
    assert_eq!(document["session"]["cwd"], f.root.to_str().unwrap());
    let mut output = Vec::new();
    export::write(&db, &reference, None, Format::Markdown, false, &mut output).unwrap();
    let markdown = String::from_utf8(output).unwrap();
    assert!(markdown.contains(&text));
    assert!(markdown.contains("## assistant"));
}
#[test]
fn export_files_are_private_and_never_overwritten_or_left_partial() {
    use mass::export::{self, Format};
    let f = Fixture::new();
    f.pi("export", "export needle");
    f.refresh();
    let reference = f.query("needle").results[0].session.reference.clone();
    let db = index::open(&f.db, false).unwrap();
    let path = f.root.join("export.json");
    export::to_file(&db, &reference, None, Format::Json, false, &path).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(export::to_file(&db, &reference, None, Format::Markdown, false, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let bad = f.root.join("invalid.md");
    assert!(
        export::to_file(
            &db,
            &reference,
            Some("not-a-message"),
            Format::Markdown,
            false,
            &bad
        )
        .is_err()
    );
    assert!(!bad.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = f.root.join("link.md");
        symlink(&path, &link).unwrap();
        assert!(export::to_file(&db, &reference, None, Format::Markdown, false, &link).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
#[test]
fn reviewed_resume_commands_quote_arguments_and_reject_changes() {
    use mass::resume::{Plan, shell_command, verify_review};
    let f = Fixture::new();
    let marker = f.root.join("must-not-exist");
    let argument = format!("literal ' ; $(touch {}) ; #", marker.display());
    let plan = Plan {
        version: 1,
        reference: "reference".into(),
        executable: "/usr/bin/printf".into(),
        argv: vec!["%s".into(), argument.clone()],
        cwd: f.root.to_string_lossy().into_owned(),
        environment: Default::default(),
        supported: true,
        warnings: vec![],
    };
    verify_review(&plan, &plan).unwrap();
    let mut changed = plan.clone();
    changed.argv.push("unexpected".into());
    assert!(verify_review(&plan, &changed).is_err());
    changed = plan.clone();
    changed.supported = false;
    assert!(verify_review(&plan, &changed).is_err());
    assert!(shell_command(&changed).is_err());
    #[cfg(unix)]
    {
        let output = std::process::Command::new("/bin/sh")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", &shell_command(&plan).unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), argument);
        assert!(!marker.exists());
    }
}
#[test]
fn terminal_controls_are_removed() {
    assert_eq!(mass::clean("a\u{1b}\u{7}\u{202e}b"), "ab");
    assert_eq!(
        mass::clean_text("\tcode\r\n\u{1b}\u{7}\u{202e}"),
        "\tcode\r\n"
    );
}
