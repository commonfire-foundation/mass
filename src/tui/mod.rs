mod actions;
mod document;
mod filters;
mod input;
mod presentation;
mod render;
#[cfg(test)]
mod snapshots;

use crate::{
    config::Config,
    index,
    models::Page,
    reader::{self, Reading},
    search::{self, Filters},
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use ratatui_crossterm::CrosstermBackend;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            crossterm::cursor::Show,
            event::DisableBracketedPaste
        );
    }
}
#[derive(Debug)]
enum Request {
    Search(u64, String, Filters),
    Read(u64, String, String),
    Index,
    Discover,
    Projects,
    Find(u64, String, String, String, bool),
    Plan(u64, Box<crate::models::Session>, Arc<AtomicBool>),
    Action(u64, actions::Job),
}
#[derive(Debug)]
enum Reply {
    Page(u64, Result<Page, String>),
    Read(u64, Result<Box<Reading>, String>),
    Progress(String),
    Indexed(Result<(), String>),
    Projects(Result<Vec<String>, String>),
    Plan(u64, Result<Box<crate::resume::Plan>, String>),
    Found(u64, bool, Result<Option<Box<Reading>>, String>),
    Action(u64, Result<String, String>),
}
#[derive(Clone, Copy)]
enum ReadPosition {
    Match,
    MatchLast,
    Start,
    End,
}
#[derive(Default)]
pub struct State {
    pub generation: u64,
    pub selected: usize,
    pub query: String,
    pub cursor: usize,
    pub reader: bool,
    pub scroll: usize,
    pub focus: usize,
}
impl State {
    pub fn cycle_focus(&mut self, backwards: bool, wide: bool) {
        let panes = if wide { 3 } else { 2 };
        self.focus = (self.focus + if backwards { panes - 1 } else { 1 }) % panes;
    }
    pub fn open_reader(&mut self, has_selection: bool) -> bool {
        if has_selection {
            self.reader = true;
        }
        has_selection
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
}
fn worker_connection(
    path: &std::path::Path,
    cancel: &Arc<AtomicBool>,
) -> Result<rusqlite::Connection> {
    let db = index::open(path, false)?;
    let cancel = cancel.clone();
    db.progress_handler(1000, Some(move || cancel.load(Ordering::Relaxed)));
    Ok(db)
}
fn request_plan(
    send: &mpsc::Sender<Request>,
    generation: &mut u64,
    cancel: &mut Arc<AtomicBool>,
    session: Box<crate::models::Session>,
) -> Result<()> {
    cancel.store(true, Ordering::Relaxed);
    *cancel = Arc::new(AtomicBool::new(false));
    *generation += 1;
    send.send(Request::Plan(*generation, session, cancel.clone()))?;
    Ok(())
}
pub fn run(
    path: PathBuf,
    config: Config,
    query: String,
    config_path: Option<PathBuf>,
) -> Result<i32> {
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_config = config.clone();
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let mut writer: Option<std::thread::JoinHandle<()>> = None;
        let mut pending = None;
        while let Ok(mut request) = pending.take().map(Ok).unwrap_or_else(|| requests.recv()) {
            // Only coalesce consecutive searches: never discard reader or index requests.
            if matches!(request, Request::Search(..)) {
                while let Ok(next) = requests.try_recv() {
                    if matches!(next, Request::Search(..)) {
                        request = next;
                    } else {
                        pending = Some(next);
                        break;
                    }
                }
            }
            // Never execute a queued read, export, or launch review after exit.
            if worker_cancel.load(Ordering::Relaxed) {
                break;
            }
            match request {
                Request::Find(g, reference, needle, after, backwards) => {
                    let result = worker_connection(&worker_path, &worker_cancel)
                        .and_then(|db| {
                            let snapshot = db.unchecked_transaction()?;
                            reader::find_match(&snapshot, &reference, &needle, &after, backwards)?
                                .map(|at| {
                                    reader::read_window(&snapshot, &reference, Some(&at), 32)
                                        .map(Box::new)
                                })
                                .transpose()
                        })
                        .map_err(|e| e.to_string());
                    let _ = replies.send(Reply::Found(g, backwards, result));
                }
                Request::Projects => {
                    let result = worker_connection(&worker_path,&worker_cancel).and_then(|db| {
                        let mut stmt=db.prepare("SELECT DISTINCT workspace FROM files WHERE workspace!='' ORDER BY 1 LIMIT 10000")?;
                        let projects=stmt.query_map([],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
                        Ok(projects)
                    }).map_err(|e|e.to_string());
                    let _ = replies.send(Reply::Projects(result));
                }
                Request::Plan(g, session, cancel) => {
                    // A refresh may have discovered an ambiguous history copy
                    // since this search result was rendered.
                    let result = worker_connection(&worker_path, &cancel)
                        .and_then(|db| reader::resolve(&db, &session.reference))
                        .and_then(|current| {
                            crate::resume::plan_cancellable(&current, &worker_config, &cancel)
                        })
                        .map(Box::new)
                        .map_err(|e| e.to_string());
                    let _ = replies.send(Reply::Plan(g, result));
                }
                Request::Action(g, job) => {
                    let result = worker_connection(&worker_path, &worker_cancel)
                        .and_then(|db| {
                            job.run(
                                &db,
                                worker_path
                                    .parent()
                                    .filter(|p| !p.as_os_str().is_empty())
                                    .unwrap_or(std::path::Path::new(".")),
                            )
                        })
                        .map_err(|e| e.to_string());
                    let _ = replies.send(Reply::Action(g, result));
                }
                Request::Discover => {
                    let mut lines = Vec::new();
                    for source in &worker_config.sources {
                        match crate::discovery::scan(
                            source,
                            &worker_config.exclusions,
                            &worker_cancel,
                        ) {
                            Ok(found) => {
                                lines.push(format!(
                                    "{}: {} transcript files",
                                    source.id,
                                    found.paths.len()
                                ));
                                lines.extend(found.warnings);
                            }
                            Err(e) => {
                                lines.push(e.to_string());
                                break;
                            }
                        }
                    }
                    let _ = replies.send(Reply::Progress(lines.join("\n")));
                }
                Request::Search(g, q, f) => {
                    let result = worker_connection(&worker_path, &worker_cancel)
                        .and_then(|db| {
                            search::query_cancellable(&db, &q, &f, worker_cancel.clone())
                        })
                        .map_err(|e| e.to_string());
                    let _ = replies.send(Reply::Page(g, result));
                }
                Request::Read(g, r, at) => {
                    let result = worker_connection(&worker_path, &worker_cancel)
                        .and_then(|db| {
                            reader::read_window(
                                &db,
                                &r,
                                if at.is_empty() { None } else { Some(&at) },
                                32,
                            )
                        })
                        .map_err(|e| e.to_string());
                    let _ = replies.send(Reply::Read(g, result.map(Box::new)));
                }
                Request::Index => {
                    if writer.as_ref().is_some_and(|w| !w.is_finished()) {
                        continue;
                    }
                    if let Some(w) = writer.take() {
                        let _ = w.join();
                    }
                    let path = worker_path.clone();
                    let config = worker_config.clone();
                    let cancel = worker_cancel.clone();
                    let replies = replies.clone();
                    writer = Some(std::thread::spawn(move || {
                        let result = index::refresh(&path, &config, &cancel, |s| {
                            let _ = replies.send(Reply::Progress(s));
                        })
                        // Per-record import diagnostics remain in the persisted
                        // coverage report. Only a failed refresh is a TUI error.
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                        let _ = replies.send(Reply::Indexed(result));
                    }));
                }
            }
            if worker_cancel.load(Ordering::Relaxed) {
                break;
            }
        }
        if let Some(w) = writer {
            let _ = w.join();
        }
    });
    terminal::enable_raw_mode()?;
    let guard = Guard;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        event::EnableBracketedPaste
    )?;
    let mut term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut state = State {
        cursor: query.len(),
        query,
        ..Default::default()
    };
    let mut list = ratatui::widgets::ListState::default();
    let mut details = false;
    let mut dialog_scroll = 0;
    let mut filters = Filters {
        limit: 50,
        ..Default::default()
    };
    let mut page: Option<Page> = None;
    let mut page_history: Vec<(Option<String>, usize)> = Vec::new();
    let mut pending_page: Option<(Option<String>, usize, bool)> = None;
    let mut reading: Option<Reading> = None;
    let mut document = document::Document::default();
    let mut document_revision = 0u64;
    let mut document_key = None;
    let mut read_pending = false;
    let mut pending_position = ReadPosition::Match;
    let mut next_position = None;
    let mut notice = "Loading conversations…".to_owned();
    let mut query_error = None;
    let mut displayed_generation = u64::MAX;
    let mut index_progress = String::new();
    let mut last_index_query = std::time::Instant::now();
    let mut first = !path.is_file();
    let mut indexing = false;
    let mut help = false;
    let mut sheet: Option<filters::Sheet> = None;
    let mut resume_review: Option<Result<Option<Box<crate::resume::Plan>>, String>> = None;
    let mut resume_generation = 0;
    let mut plan_cancel = Arc::new(AtomicBool::new(false));
    let mut actions: Option<actions::Sheet> = None;
    let mut action_generation = 0;
    let mut read_generation = 0;
    let mut preview_ref = String::new();
    let mut finding = false;
    let mut find_text = String::new();
    let mut find_cursor = 0;
    if first {
        send.send(Request::Discover)?;
    }
    if !first {
        send.send(Request::Search(
            state.generation,
            state.query.clone(),
            filters.clone(),
        ))?;
    }
    let outcome = (|| -> Result<i32> {
        loop {
            while let Ok(reply) = receive.try_recv() {
                match reply {
                    Reply::Page(g, result) if state.accepts(g) => match result {
                        Ok(p) => {
                            displayed_generation = g;
                            query_error = None;
                            let keep = page
                                .as_ref()
                                .and_then(|p| p.results.get(state.selected))
                                .map(|h| h.session.reference.clone());
                            state.selected =
                                if let Some((cursor, selected, backwards)) = pending_page.take() {
                                    if backwards {
                                        page_history
                                            .pop()
                                            .map(|(_, selected)| selected)
                                            .unwrap_or(0)
                                    } else {
                                        page_history.push((cursor, selected));
                                        0
                                    }
                                } else {
                                    keep.and_then(|r| {
                                        p.results.iter().position(|h| h.session.reference == r)
                                    })
                                    .unwrap_or(0)
                                }
                                .min(p.results.len().saturating_sub(1));
                            notice.clear();
                            page = Some(p);
                        }
                        Err(e) => {
                            displayed_generation = g;
                            if let Some((cursor, selected, _)) = pending_page.take() {
                                filters.cursor = cursor;
                                state.selected = selected;
                            }
                            query_error = Some(e.clone());
                            notice = format!("{e} · showing previous results");
                        }
                    },
                    Reply::Read(g, result) if g == read_generation => {
                        read_pending = false;
                        match result {
                            Ok(r) => {
                                reading = Some(*r);
                                document_revision += 1;
                                next_position = Some(pending_position);
                            }
                            Err(e) => notice = e,
                        }
                    }
                    Reply::Found(g, backwards, result) if g == read_generation => {
                        read_pending = false;
                        match result {
                            Ok(Some(r)) => {
                                reading = Some(*r);
                                document_revision += 1;
                                next_position = Some(if backwards {
                                    ReadPosition::MatchLast
                                } else {
                                    ReadPosition::Match
                                });
                                notice = "Match found · n/N for next/previous".into();
                            }
                            Ok(None) => notice = "No matches in this conversation".into(),
                            Err(e) => notice = e,
                        }
                    }
                    Reply::Projects(result) => {
                        if let Some(sheet) = &mut sheet {
                            match result {
                                Ok(projects) => sheet.projects = projects,
                                Err(e) => sheet.error = Some(e),
                            }
                        }
                    }
                    Reply::Plan(g, result) if g == resume_generation && resume_review.is_some() => {
                        resume_review = Some(result.map(Some));
                    }
                    Reply::Action(g, result) if g == action_generation => {
                        if let Some(sheet) = &mut actions {
                            sheet.finish(result);
                        } else {
                            notice = result.unwrap_or_else(|e| e);
                        }
                    }
                    Reply::Progress(s) => {
                        if indexing {
                            index_progress = s;
                        } else {
                            notice = s;
                        }
                    }
                    Reply::Indexed(result) => {
                        indexing = false;
                        notice = result.err().unwrap_or_else(|| "Index refreshed".into());
                        state.generation += 1;
                        filters.cursor = None;
                        page_history.clear();
                        pending_page = None;
                        preview_ref.clear();
                        send.send(Request::Search(
                            state.generation,
                            state.query.clone(),
                            filters.clone(),
                        ))?;
                    }
                    _ => {}
                }
            }
            if !state.reader {
                if let Some(hit) = page.as_ref().and_then(|p| p.results.get(state.selected)) {
                    let key = format!("{}:{}", hit.session.reference, hit.message_ref);
                    if preview_ref != key {
                        preview_ref = key;
                        find_text.clear();
                        find_cursor = 0;
                        reading = None;
                        read_generation += 1;
                        read_pending = true;
                        pending_position = ReadPosition::Match;
                        send.send(Request::Read(
                            read_generation,
                            hit.session.reference.clone(),
                            hit.message_ref.clone(),
                        ))?;
                    }
                } else {
                    reading = None;
                }
            }
            if indexing
                && !state.reader
                && filters.cursor.is_none()
                && last_index_query.elapsed() >= Duration::from_secs(1)
            {
                // Show committed files before the full scan finishes.
                last_index_query = std::time::Instant::now();
                send.send(Request::Search(
                    state.generation,
                    state.query.clone(),
                    filters.clone(),
                ))?;
            }
            if term.size()?.width < 90 && state.focus == 2 {
                state.focus = 0;
            }
            let terminal_area = Rect::new(0, 0, term.size()?.width, term.size()?.height);
            let reader_area = render::reader_area(terminal_area, state.reader);
            let key = (
                document_revision,
                reader_area.width,
                state.query.clone(),
                find_text.clone(),
            );
            if document_key.as_ref() != Some(&key)
                && let Some(r) = &reading
            {
                let anchor = document.anchor(state.scroll);
                let query_changed =
                    document_key
                        .as_ref()
                        .is_some_and(|k: &(u64, u16, String, String)| {
                            k.2 != state.query || k.3 != find_text
                        });
                let excerpt = page
                    .as_ref()
                    .and_then(|p| p.results.get(state.selected))
                    .map(|h| h.excerpt.as_str())
                    .unwrap_or("");
                document = document::Document::build(
                    r,
                    reader_area.width as usize,
                    &state.query,
                    &find_text,
                    if find_text.is_empty() {
                        excerpt
                    } else {
                        &find_text
                    },
                    presentation::Theme::terminal(),
                );
                state.scroll = match next_position.take() {
                    Some(ReadPosition::Start) => 0,
                    Some(ReadPosition::End) => document.max_scroll(reader_area.height as usize),
                    Some(ReadPosition::Match) => document.target,
                    Some(ReadPosition::MatchLast) => {
                        let end = document
                            .anchors
                            .get(1)
                            .map(|a| a.0)
                            .unwrap_or(document.lines.len());
                        document
                            .matches
                            .iter()
                            .rev()
                            .find(|i| **i < end)
                            .copied()
                            .unwrap_or(document.target)
                    }
                    None if query_changed => document.target,
                    None => anchor.map(|a| document.restore(&a)).unwrap_or(0),
                }
                .min(document.max_scroll(reader_area.height as usize));
                document_key = Some(key);
            }
            term.draw(|frame| {
                render::draw(
                    frame,
                    render::View {
                        state: &state,
                        page: page.as_ref(),
                        reading: reading.as_ref(),
                        notice: if indexing {&index_progress} else {&notice},
                        error: query_error.as_deref(),
                        indexing,
                        first,
                        help,
                        filter: sheet.is_some(),
                        modal: resume_review.is_some() || actions.is_some(),
                        filters: &filters,
                        list: &mut list,
                        details,
                        document: &document,
                        finding,
                        find_text: &find_text,
                        find_cursor,
                        searching: displayed_generation!=state.generation,
                        dialog_scroll:&mut dialog_scroll,
                    },
                );
                if let Some(sheet)=&sheet {sheet.draw(frame);}
                if let Some(sheet)=&mut actions {sheet.draw(frame,presentation::Theme::terminal());}
                if let Some(review)=&resume_review {
                    let text=match review {
                        Ok(Some(p))=>format!("{}\n\nExperimental native reopening. This launches an agent with its own settings and permissions. Live-session status is unknown.\n\n{}\n\nExecutable: {}\nArguments: {:?}\nDirectory: {}\nStore: {:?}",if p.supported{"Y Launch agent · C Copy command · Esc Cancel"}else{"Resume unavailable · Esc Back"},p.warnings.iter().skip(1).cloned().collect::<Vec<_>>().join("\n"),p.executable,p.argv,p.cwd,p.environment),
                        Ok(None)=>"Checking source identity and resume support…\n\nEsc Cancel".into(),
                        Err(e)=>format!("Resume unavailable\n\n{e}\n\nEsc Back"),
                    };
                    render::dialog(frame,"Resume review",crate::clean(&text),&mut dialog_scroll);
                }
            })?;
            if !event::poll(Duration::from_millis(40))? {
                continue;
            }
            let event = event::read()?;
            if let Event::Paste(text) = &event {
                if let Some(sheet) = &mut actions {
                    sheet.paste(text);
                } else if let Some(sheet) = &mut sheet {
                    sheet.paste(text);
                } else if finding {
                    input::insert(&mut find_text, &mut find_cursor, text);
                } else if !first && !help && !details && resume_review.is_none() && !state.reader {
                    input::insert(&mut state.query, &mut state.cursor, text);
                    state.focus = 0;
                    query_error = None;
                    filters.cursor = None;
                    page_history.clear();
                    pending_page = None;
                    state.generation += 1;
                    send.send(Request::Search(
                        state.generation,
                        state.query.clone(),
                        filters.clone(),
                    ))?;
                }
                continue;
            }
            let Event::Key(key) = event else {
                continue;
            };
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(130);
            }
            if let Some(sheet) = &mut actions {
                match sheet.key(key) {
                    actions::Outcome::Keep => {}
                    actions::Outcome::Close => {
                        actions = None;
                        action_generation += 1;
                    }
                    actions::Outcome::Run(job) => {
                        action_generation += 1;
                        send.send(Request::Action(action_generation, job))?;
                    }
                    actions::Outcome::Resume(session) => {
                        actions = None;
                        dialog_scroll = 0;
                        resume_review = Some(Ok(None));
                        request_plan(&send, &mut resume_generation, &mut plan_cancel, session)?;
                    }
                }
                continue;
            }
            if (resume_review.is_some() || help || details)
                && matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
                )
            {
                dialog_scroll = match key.code {
                    KeyCode::Up => dialog_scroll.saturating_sub(1),
                    KeyCode::PageUp => dialog_scroll.saturating_sub(8),
                    KeyCode::Down => dialog_scroll.saturating_add(1),
                    _ => dialog_scroll.saturating_add(8),
                };
                continue;
            }
            if let Some(review) = &resume_review {
                if key.code == KeyCode::Esc {
                    resume_review = None;
                    resume_generation += 1;
                    plan_cancel.store(true, Ordering::Relaxed);
                } else if matches!(key.code, KeyCode::Char('c' | 'C'))
                    && let Ok(Some(plan)) = review
                    && plan.supported
                {
                    if let Some(helper) = crate::clipboard::detect() {
                        let text = crate::resume::shell_command(plan)?;
                        action_generation += 1;
                        send.send(Request::Action(
                            action_generation,
                            actions::Job::Copy {
                                helper,
                                text: Some(text),
                                reference: plan.reference.clone(),
                                message: None,
                            },
                        ))?;
                        resume_review = None;
                        notice = "Copying reviewed command…".into();
                    } else {
                        resume_review = Some(Err("Clipboard unavailable: configure wl-copy or xclip; no helper was installed".into()));
                    }
                } else if key.code == KeyCode::Char('y')
                    && let Ok(Some(plan)) = review
                    && plan.supported
                {
                    terminal::disable_raw_mode()?;
                    execute!(
                        std::io::stdout(),
                        LeaveAlternateScreen,
                        event::DisableBracketedPaste,
                        crossterm::cursor::Show
                    )?;
                    let result = crate::resume::launch(plan, true, || {
                        let config = crate::config::load(config_path.as_deref())?;
                        let db = index::open(&path, false)?;
                        let current = reader::resolve(&db, &plan.reference)?;
                        crate::resume::plan(&current, &config)
                    });
                    terminal::enable_raw_mode()?;
                    execute!(
                        std::io::stdout(),
                        EnterAlternateScreen,
                        event::EnableBracketedPaste
                    )?;
                    term.clear()?;
                    notice = match result {
                        Ok(0) => "Agent finished · returned to MASS".into(),
                        Ok(n) => format!("Agent exited with status {n}"),
                        Err(e) => e.to_string(),
                    };
                    resume_review = None;
                }
                continue;
            }
            if let Some(form) = &mut sheet {
                match form.key(key) {
                    filters::Action::Cancel => sheet = None,
                    filters::Action::Apply(f) => {
                        filters = f;
                        page_history.clear();
                        pending_page = None;
                        sheet = None;
                        query_error = None;
                        state.generation += 1;
                        send.send(Request::Search(
                            state.generation,
                            state.query.clone(),
                            filters.clone(),
                        ))?;
                    }
                    filters::Action::Keep => {}
                }
                continue;
            }
            if key.code == KeyCode::Esc {
                if details {
                    details = false;
                    continue;
                }
                if finding {
                    finding = false;
                    continue;
                }
                if indexing {
                    cancel.store(true, Ordering::Relaxed);
                    return Ok(130);
                }
                if help {
                    help = false;
                } else if state.reader {
                    // A late search response may change the best excerpt for this session.
                    // Back must retain the passage the person actually navigated to.
                    if let Some(hit) = page.as_ref().and_then(|p| p.results.get(state.selected))
                        && reading
                            .as_ref()
                            .is_some_and(|r| r.session.reference == hit.session.reference)
                    {
                        preview_ref = format!("{}:{}", hit.session.reference, hit.message_ref);
                    }
                    state.reader = false;
                    state.focus = 0;
                } else {
                    return Ok(0);
                }
                continue;
            }
            if details {
                continue;
            }
            if first {
                if key.code == KeyCode::Enter {
                    first = false;
                    indexing = true;
                    notice = "Starting index… You can type to search while files commit.".into();
                    query_error = None;
                    send.send(Request::Index)?;
                }
                continue;
            }
            if help {
                continue;
            }
            if finding {
                if key.code == KeyCode::Enter {
                    finding = false;
                    notice = "n/N Next / previous match across this conversation".into();
                    if !find_text.is_empty()
                        && document.matches.is_empty()
                        && let Some(r) = &reading
                    {
                        read_generation += 1;
                        read_pending = true;
                        let after = r
                            .messages
                            .last()
                            .map(|m| m.reference.clone())
                            .unwrap_or_default();
                        send.send(Request::Find(
                            read_generation,
                            r.session.reference.clone(),
                            find_text.clone(),
                            after,
                            false,
                        ))?;
                    }
                } else {
                    input::edit(&mut find_text, &mut find_cursor, key);
                }
                continue;
            }
            if (state.reader || state.focus == 2)
                && matches!(
                    key.code,
                    KeyCode::Down
                        | KeyCode::Up
                        | KeyCode::PageDown
                        | KeyCode::PageUp
                        | KeyCode::Right
                        | KeyCode::Left
                )
            {
                let forward =
                    matches!(key.code, KeyCode::Down | KeyCode::PageDown | KeyCode::Right);
                let step = if matches!(key.code, KeyCode::PageDown | KeyCode::PageUp) {
                    reader_area.height.saturating_sub(1).max(1) as usize
                } else {
                    1
                };
                let maximum = document.max_scroll(reader_area.height as usize);
                let crossing = if forward {
                    state.scroll >= maximum
                } else {
                    state.scroll == 0
                };
                if (crossing || matches!(key.code, KeyCode::Left | KeyCode::Right))
                    && !read_pending
                    && let Some(r) = &reading
                {
                    let cursor = if forward { &r.next } else { &r.previous };
                    if let Some(cursor) = cursor {
                        read_generation += 1;
                        read_pending = true;
                        pending_position = if forward {
                            ReadPosition::Start
                        } else {
                            ReadPosition::End
                        };
                        send.send(Request::Read(
                            read_generation,
                            r.session.reference.clone(),
                            cursor.clone(),
                        ))?;
                    }
                } else if !read_pending {
                    state.scroll = if forward {
                        (state.scroll + step).min(maximum)
                    } else {
                        state.scroll.saturating_sub(step)
                    };
                }
                continue;
            }
            if !state.reader
                && let Some(changed) = input::edit(&mut state.query, &mut state.cursor, key)
            {
                state.focus = 0;
                if changed {
                    query_error = None;
                    filters.cursor = None;
                    page_history.clear();
                    pending_page = None;
                    state.generation += 1;
                    send.send(Request::Search(
                        state.generation,
                        state.query.clone(),
                        filters.clone(),
                    ))?;
                }
                continue;
            }
            match key.code {
                KeyCode::Char('/') if state.reader => {
                    finding = true;
                    find_text.clear();
                    find_cursor = 0;
                    notice = "Find in conversation · Enter search · Esc cancel".into();
                }
                KeyCode::Char('n' | 'N') if state.reader && !read_pending => {
                    let backwards = key.code == KeyCode::Char('N');
                    let next = if backwards {
                        document.matches.iter().rev().find(|i| **i < state.scroll)
                    } else {
                        document.matches.iter().find(|i| **i > state.scroll)
                    };
                    if let Some(line) = next {
                        state.scroll = *line;
                    } else if let Some(r) = &reading {
                        let needle = if find_text.is_empty() {
                            presentation::terms(&state.query)
                                .first()
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            find_text.clone()
                        };
                        if !needle.is_empty() {
                            let after = if backwards {
                                r.messages.first()
                            } else {
                                r.messages.last()
                            }
                            .map(|m| m.reference.clone())
                            .unwrap_or_default();
                            read_generation += 1;
                            read_pending = true;
                            send.send(Request::Find(
                                read_generation,
                                r.session.reference.clone(),
                                needle,
                                after,
                                backwards,
                            ))?;
                        }
                    }
                }
                KeyCode::PageDown
                    if !state.reader
                        && !indexing
                        && displayed_generation == state.generation
                        && query_error.is_none() =>
                {
                    if let Some(cursor) = page.as_ref().and_then(|p| p.next_cursor.clone()) {
                        pending_page = Some((filters.cursor.clone(), state.selected, false));
                        filters.cursor = Some(cursor);
                        state.generation += 1;
                        send.send(Request::Search(
                            state.generation,
                            state.query.clone(),
                            filters.clone(),
                        ))?;
                    }
                }
                KeyCode::PageUp
                    if !state.reader
                        && !indexing
                        && displayed_generation == state.generation
                        && query_error.is_none() =>
                {
                    if let Some((cursor, _)) = page_history.last() {
                        pending_page = Some((filters.cursor.clone(), state.selected, true));
                        filters.cursor = cursor.clone();
                        state.generation += 1;
                        send.send(Request::Search(
                            state.generation,
                            state.query.clone(),
                            filters.clone(),
                        ))?;
                    }
                }
                KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    help = true;
                    dialog_scroll = 0;
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    details = true;
                    dialog_scroll = 0;
                }
                KeyCode::Char('l')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && !indexing =>
                {
                    indexing = true;
                    send.send(Request::Index)?;
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    sheet = Some(filters::Sheet::new(&filters));
                    send.send(Request::Projects)?;
                }
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let hit = page.as_ref().and_then(|p| p.results.get(state.selected));
                    let target = if state.reader {
                        reading.as_ref().map(|r| actions::Target {
                            session: r.session.clone(),
                            message: document
                                .anchors
                                .iter()
                                .rev()
                                .find(|(line, _)| *line <= state.scroll)
                                .map(|(_, reference)| reference.clone())
                                .or_else(|| r.messages.first().map(|m| m.reference.clone())),
                            excerpt: hit
                                .filter(|h| h.session.reference == r.session.reference)
                                .map(|h| h.excerpt.clone())
                                .unwrap_or_default(),
                        })
                    } else {
                        hit.map(|h| actions::Target {
                            session: h.session.clone(),
                            message: (!h.message_ref.is_empty()).then(|| h.message_ref.clone()),
                            excerpt: h.excerpt.clone(),
                        })
                    };
                    if let Some(target) = target {
                        actions = Some(actions::Sheet::new(target));
                    } else {
                        notice = "No conversation selected".into();
                    }
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let session = if state.reader {
                        reading.as_ref().map(|r| &r.session)
                    } else {
                        page.as_ref()
                            .and_then(|p| p.results.get(state.selected))
                            .map(|h| &h.session)
                    };
                    if let Some(session) = session {
                        dialog_scroll = 0;
                        resume_review = Some(Ok(None));
                        request_plan(
                            &send,
                            &mut resume_generation,
                            &mut plan_cancel,
                            Box::new(session.clone()),
                        )?;
                    }
                }
                KeyCode::Tab if !state.reader => state.cycle_focus(false, term.size()?.width >= 90),
                KeyCode::BackTab if !state.reader => {
                    state.cycle_focus(true, term.size()?.width >= 90)
                }
                KeyCode::Enter => {
                    if !state.open_reader(page.as_ref().is_some_and(|p| !p.results.is_empty())) {
                        notice = if indexing {
                            "Indexing: waiting for searchable conversations"
                        } else {
                            "No conversation selected. Type a query or press Ctrl-L to index."
                        }
                        .into();
                    }
                }
                KeyCode::Up => state.selected = state.selected.saturating_sub(1),
                KeyCode::Down => {
                    state.selected = (state.selected + 1).min(
                        page.as_ref()
                            .map(|p| p.results.len().saturating_sub(1))
                            .unwrap_or(0),
                    )
                }
                _ => {}
            }
        }
    })();
    cancel.store(true, Ordering::Relaxed);
    plan_cancel.store(true, Ordering::Relaxed);
    drop(send);
    // Restore the terminal immediately, even if a filesystem syscall has not
    // returned to a cooperative cancellation point yet.
    drop(term);
    drop(guard);
    let _ = worker.join();
    outcome
}
