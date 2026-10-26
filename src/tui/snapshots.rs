use super::{
    State,
    document::Document,
    presentation::Theme,
    render::{self, View},
};
use crate::{
    models::{Coverage, Hit, Message, Page, Session},
    reader::Reading,
    search::Filters,
};
use ratatui::{backend::TestBackend, prelude::*, widgets::ListState};

#[test]
fn terminal_size_and_state_matrix() {
    let session = Session {
        reference: "0123456789abcdef".into(),
        source: "fixture".into(),
        agent: "pi".into(),
        native_id: "one".into(),
        title: "Fix the OAuth callback — Unicode café 你好 and a deliberately long title".into(),
        cwd: "/home/me/Projects/website".into(),
        path: "/private/transcript.jsonl".into(),
        ..Default::default()
    };
    let second = Session {
        reference: "abcdef0123456789".into(),
        agent: "claude".into(),
        title: "Investigate login failures".into(),
        cwd: "/work/client/website".into(),
        ..session.clone()
    };
    let page = Page {
        version: 1,
        results: vec![
            Hit {
                session: session.clone(),
                message_ref: "one".into(),
                excerpt: "…the callback URI was encoded twice…".into(),
                rank: -1.0,
            },
            Hit {
                session: second,
                message_ref: "two".into(),
                excerpt: "Validate the callback before setting the cookie.".into(),
                rank: -0.5,
            },
        ],
        next_cursor: Some("fixture:2".into()),
        // A usable index can contain importer limitations. These must not
        // become a warning count or an incomplete-index alarm in the TUI.
        coverage: Coverage {
            warnings: vec!["pi: Missing native session identity".into(); 182],
            ..Coverage::default()
        },
    };
    let reading=Reading{version:1,session,messages:vec![Message{reference:"one".into(),native_id:"one".into(),parent:String::new(),role:"user".into(),text:"Why does the callback fail?".into(),line:1,chunk:0,projection:false},Message{reference:"two".into(),native_id:"two".into(),parent:String::new(),role:"assistant".into(),text:"# Redirect handling\nThe callback URI was encoded twice. Preserve the original path.\n\n```rust\n    let callback = decode_once(uri);\n    assert_eq!(callback, expected);\n```\nUnicode paths: café / 你好".into(),line:2,chunk:0,projection:false}],at:0,total:2,more:false,next:None,previous:None};
    for (width, height) in [(55, 20), (80, 24), (120, 35), (160, 45)] {
        for mode in [
            "results", "reader", "empty", "error", "indexing", "loading", "startup", "details",
        ] {
            for color in [false, true] {
                let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
                let state = State {
                    query: if mode == "startup" { "" } else { "callback" }.into(),
                    cursor: if mode == "startup" { 0 } else { 8 },
                    reader: mode == "reader",
                    ..Default::default()
                };
                let mut list = ListState::default();
                let mut dialog_scroll = 0;
                let filters = Filters::default();
                let empty = Page {
                    results: vec![],
                    ..page.clone()
                };
                let doc = Document::build(
                    &reading,
                    render::reader_area(Rect::new(0, 0, width, height), state.reader).width
                        as usize,
                    &state.query,
                    "",
                    "callback URI",
                    Theme::new(color),
                );
                term.draw(|f| {
                    render::draw_themed(
                        f,
                        View {
                            state: &state,
                            page: if matches!(mode, "error" | "startup") {
                                None
                            } else if mode == "empty" {
                                Some(&empty)
                            } else {
                                Some(&page)
                            },
                            reading: Some(&reading),
                            notice: if mode == "indexing" {
                                "Pi: checking file 25 of 100"
                            } else if mode == "startup" {
                                "Pi: 80 transcript files\nClaude: 10 transcript files"
                            } else {
                                ""
                            },
                            error: (mode == "error")
                                .then_some("No index yet. Press Ctrl-L to build a private cache."),
                            indexing: mode == "indexing",
                            first: mode == "startup",
                            help: false,
                            filter: false,
                            modal: false,
                            filters: &filters,
                            list: &mut list,
                            details: mode == "details",
                            document: &doc,
                            finding: false,
                            find_text: "",
                            find_cursor: 0,
                            searching: mode == "loading",
                            dialog_scroll: &mut dialog_scroll,
                        },
                        Theme::new(color),
                    )
                })
                .unwrap();
                let buffer = term.backend().buffer();
                if width >= 90 && mode == "results" {
                    let divider = (0..width)
                        .find(|&x| buffer[(x, 5)].symbol() == "│")
                        .unwrap();
                    for y in 5..height - 3 {
                        assert_eq!(buffer[(divider - 1, y)].symbol(), " ");
                        assert_eq!(buffer[(divider + 1, y)].symbol(), " ");
                    }
                }
                if !color {
                    assert!(
                        buffer
                            .content
                            .iter()
                            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
                    );
                }
                if mode == "results" {
                    assert!(
                        buffer
                            .content
                            .iter()
                            .any(|c| c.modifier.contains(Modifier::REVERSED))
                    );
                    assert!(
                        buffer
                            .content
                            .iter()
                            .any(|c| c.modifier.contains(Modifier::UNDERLINED))
                    );
                }
                let mut text = String::new();
                for y in 0..height {
                    let mut x = 0;
                    let mut row = String::new();
                    while x < width {
                        let symbol = buffer[(x, y)].symbol();
                        row.push_str(symbol);
                        x += Span::raw(symbol).width().max(1) as u16;
                    }
                    text.push_str(row.trim_end());
                    text.push('\n');
                }
                assert!(!text.contains("182 warnings"));
                assert!(!text.contains("Not yet fully indexed"));
                assert!(!text.contains("Missing native session identity"));
                assert_eq!(page.coverage.warnings.len(), 182);
                if mode != "details" {
                    assert!(!text.contains("/home/me/Projects"));
                }
                let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/snapshots/tui/{mode}-{width}x{height}.txt"));
                if std::env::var_os("MASS_UPDATE_SNAPSHOTS").is_some() {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, &text).unwrap();
                }
                assert_eq!(
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|_| panic!("Missing snapshot {}", path.display())),
                    text,
                    "{} (color={color})",
                    path.display()
                );
            }
        }
    }
}
