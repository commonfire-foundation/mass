use super::{
    State,
    presentation::{self as p, Theme},
};
use crate::{models::Page, reader::Reading, search::Filters};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

const TITLE: &str = "M.A.S.S. - Multi Agent Session Search";
const VERSION_LABEL: &str = concat!(" v", env!("CARGO_PKG_VERSION"));

fn title_line(width: usize, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            p::clip(TITLE, width.saturating_sub(VERSION_LABEL.len())),
            theme.title,
        ),
        Span::styled(VERSION_LABEL, theme.quiet),
    ])
}

pub(super) struct View<'a> {
    pub state: &'a State,
    pub page: Option<&'a Page>,
    pub reading: Option<&'a Reading>,
    pub notice: &'a str,
    pub error: Option<&'a str>,
    pub indexing: bool,
    pub first: bool,
    pub help: bool,
    pub filter: bool,
    pub modal: bool,
    pub filters: &'a Filters,
    pub list: &'a mut ListState,
    pub details: bool,
    pub document: &'a super::document::Document,
    pub finding: bool,
    pub find_text: &'a str,
    pub find_cursor: usize,
    pub searching: bool,
    pub dialog_scroll: &'a mut usize,
}

pub(super) fn dialog(
    frame: &mut Frame,
    title: &str,
    body: impl Into<Text<'static>>,
    scroll: &mut usize,
) -> Rect {
    let area = frame.area();
    let width = area.width.saturating_sub(4).min(76);
    let height = area.height.saturating_sub(4).min(20);
    let area = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let inner = area.inner(Margin::new(1, 1));
    let body: Text = body.into();
    let lines: Vec<_> = body
        .lines
        .iter()
        .flat_map(|line| {
            super::document::wrap(
                &crate::clean(&line.to_string()),
                inner.width as usize,
                false,
            )
        })
        .collect();
    *scroll = (*scroll).min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Block::bordered().title(format!(" {title} · ↑↓ scroll ")),
        area,
    );
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(*scroll)
                .take(inner.height as usize)
                .map(Line::raw)
                .collect::<Vec<_>>(),
        ),
        inner,
    );
    area
}
fn panel(frame: &mut Frame, area: Rect, title: &str, body: &str, theme: Theme) {
    let inner = area.inner(Margin::new(2, 1));
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(title.to_owned(), theme.title),
            Line::default(),
        ])
        .block(Block::default()),
        inner,
    );
    let content = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    frame.render_widget(
        Paragraph::new(crate::clean(body)).wrap(Wrap { trim: false }),
        content,
    );
}
pub(super) fn reader_area(area: Rect, full: bool) -> Rect {
    let width = area.width.saturating_sub(2);
    let body = Rect::new(area.x + 1, area.y + 5, width, area.height.saturating_sub(8));
    let content = if full || area.width < 90 {
        body
    } else {
        let panes = Layout::horizontal([
            Constraint::Percentage(42),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(body);
        panes[2].inner(Margin::new(1, 0))
    };
    Rect::new(
        content.x,
        content.y + 2,
        content.width,
        content.height.saturating_sub(3),
    )
}
pub(super) fn draw(frame: &mut Frame, view: View<'_>) {
    draw_themed(frame, view, Theme::terminal());
}
pub(super) fn draw_themed(frame: &mut Frame, view: View<'_>, theme: Theme) {
    let state = view.state;
    if frame.area().width < 35 || frame.area().height < 12 {
        frame.render_widget(
            Paragraph::new(vec![
                title_line(frame.area().width as usize, theme),
                Line::raw("Enlarge to at least 35×12."),
                Line::raw("Esc quits; Ctrl-C cancels."),
            ]),
            frame.area(),
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .margin(1)
    .split(frame.area());
    let header = Layout::horizontal([
        Constraint::Length((TITLE.len() + VERSION_LABEL.len() + 2) as u16),
        Constraint::Min(1),
    ])
    .split(rows[0]);
    frame.render_widget(
        Paragraph::new(title_line(header[0].width as usize, theme)),
        header[0],
    );
    // Import diagnostics belong in `mass status`, not a user-facing alarm or
    // a claim that every transcript was successfully imported.
    let status = if view.searching && !view.first {
        "Searching…"
    } else if view.indexing {
        "Indexing · cached results available"
    } else {
        "Local conversation search"
    };
    frame.render_widget(
        Paragraph::new(p::clip(status, header[1].width as usize))
            .alignment(Alignment::Right)
            .style(theme.quiet),
        header[1],
    );
    let active = state.focus == 0
        && !state.reader
        && !view.first
        && !view.help
        && !view.filter
        && !view.modal
        && !view.details;
    let (visible_query, cursor_column) = super::input::viewport(
        &state.query,
        state.cursor,
        rows[2].width.saturating_sub(9) as usize,
    );
    let text = if view.finding {
        super::input::viewport(
            view.find_text,
            view.find_cursor,
            rows[2].width.saturating_sub(9) as usize,
        )
        .0
    } else if state.query.is_empty() {
        "Search your conversations…".into()
    } else {
        visible_query
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if view.finding {
                    "Find   › "
                } else if active {
                    "Search › "
                } else {
                    "Search   "
                },
                if active { theme.accent } else { theme.quiet },
            ),
            Span::styled(
                text,
                if state.query.is_empty() {
                    theme.quiet
                } else {
                    Style::default()
                },
            ),
        ])),
        rows[2],
    );
    if view.finding {
        let (_, col) = super::input::viewport(
            view.find_text,
            view.find_cursor,
            rows[2].width.saturating_sub(9) as usize,
        );
        frame.set_cursor_position((rows[2].x + 9 + col, rows[2].y));
    }
    if active {
        frame.set_cursor_position((rows[2].x + 9 + cursor_column, rows[2].y));
    }
    let filters = format!(
        "{} · {} · {}",
        view.filters.agent.as_deref().unwrap_or("All agents"),
        view.filters
            .project
            .as_deref()
            .map(|s| p::project(s, &[]))
            .unwrap_or_else(|| "All projects".into()),
        match (&view.filters.since, &view.filters.before) {
            (None, None) => "Any time".into(),
            (a, b) => format!(
                "{} → {}",
                a.as_deref().unwrap_or("Beginning"),
                b.as_deref().unwrap_or("Now")
            ),
        }
    );
    frame.render_widget(
        Paragraph::new(p::clip(&filters, rows[3].width as usize)).style(theme.quiet),
        rows[3],
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.divider),
        rows[4],
    );
    let footer = if view.first {
        "Enter Build index   Esc Cancel"
    } else if view.help || view.details {
        "↑↓ Scroll   Esc Close"
    } else if view.modal {
        "Follow the dialog controls"
    } else if view.filter {
        "Esc Cancel"
    } else if state.reader {
        "↑↓ Scroll   / Find   Ctrl-O Actions   Esc Back"
    } else if frame.area().width < 85 {
        "↑↓ Select   Enter Read   Ctrl-O Actions   Ctrl-K Help"
    } else {
        "↑↓ Select   Enter Read   Ctrl-F Filters   Ctrl-O Actions   Ctrl-K Help"
    };
    let mut hints = Vec::new();
    let mut hint_width = 0;
    for group in footer.split("   ") {
        let width = Span::raw(group).width() + 2;
        if hint_width + width > rows[7].width as usize {
            break;
        }
        if let Some((key, label)) = group.split_once(' ') {
            hints.push(Span::styled(key.to_owned(), theme.title));
            hints.push(Span::styled(format!(" {label}  "), theme.quiet));
        }
        hint_width += width;
    }
    frame.render_widget(Paragraph::new(Line::from(hints)), rows[7]);
    let feedback = if view.first {
        String::new()
    } else if let Some(error) = view.error {
        format!("Search failed: {error}")
    } else if view.indexing {
        format!("Indexing · {}", view.notice)
    } else {
        view.notice.to_owned()
    };
    frame.render_widget(
        Paragraph::new(p::clip(&feedback, rows[6].width as usize)).style(theme.quiet),
        rows[6],
    );
    let body = rows[5];
    if view.first {
        panel(
            frame,
            body,
            "Find the conversation where you worked it out",
            &format!(
                "{}\n\nBuild a private, plaintext search index.\nMASS will not launch agents or change transcripts.\n\n[ Enter: Build index ]   Esc Cancel",
                view.notice
            ),
            theme,
        );
        return;
    }
    if view.page.is_none() {
        panel(frame,body,if view.error.is_some(){"Unable to load conversations"}else if view.indexing{"Building your index"}else{"Loading conversations"},view.error.unwrap_or(if view.indexing{"You can search while indexing continues.\nCommitted conversations will appear here.\n\nCtrl-C cancels safely."}else{"Type a query to begin. Results will appear shortly."}),theme);
    } else if view.page.is_some_and(|p| p.results.is_empty()) {
        panel(
            frame,
            body,
            if state.query.is_empty() {
                "No conversations yet"
            } else {
                "No matches"
            },
            if state.query.is_empty() {
                "Check source roots in config.toml, then Ctrl-L to refresh.\nCtrl-F lets you check or clear filters."
            } else {
                "Try fewer words or another phrase.\nCtrl-F checks filters without clearing your query."
            },
            theme,
        );
    } else if let Some(page) = view.page {
        let panes = if state.reader || frame.area().width < 90 {
            vec![body]
        } else {
            Layout::horizontal([
                Constraint::Percentage(42),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(body)
            .to_vec()
        };
        if !state.reader {
            let listarea = panes[0];
            let p = page;
            let offset = view
                .filters
                .cursor
                .as_deref()
                .and_then(|s| s.rsplit(':').next())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let heading = format!(
                "{}{}–{} conversations{}",
                if state.focus == 1 { "› " } else { "" },
                offset + 1,
                offset + p.results.len(),
                if p.next_cursor.is_some() {
                    " · more ↓"
                } else {
                    ""
                }
            );
            frame.render_widget(
                Paragraph::new(p::clip(&heading, listarea.width as usize)).style(
                    if state.focus == 1 {
                        theme.accent
                    } else {
                        theme.quiet
                    },
                ),
                Rect::new(listarea.x, listarea.y, listarea.width, 1),
            );
            let peers = p
                .results
                .iter()
                .map(|h| h.session.clone())
                .collect::<Vec<_>>();
            let terms = p::terms(&state.query);
            let items = p
                .results
                .iter()
                .enumerate()
                .map(|(index, h)| {
                    let width = listarea.width.saturating_sub(3) as usize;
                    let metadata = format!(
                        "{} · {} · {}{}",
                        p::agent(&h.session),
                        p::project(&h.session.cwd, &peers),
                        p::age(&h.session.activity),
                        if h.session.time_fallback { "*" } else { "" }
                    );
                    ListItem::new(vec![
                        p::highlighted(
                            &p::clip(&h.session.title, width),
                            &terms,
                            if index == state.selected {
                                theme.title.patch(theme.selected)
                            } else {
                                theme.title
                            },
                            theme,
                        ),
                        Line::styled(p::clip(&metadata, width), theme.quiet),
                        p::highlighted(
                            &p::clip(&h.excerpt, width),
                            &terms,
                            Style::default(),
                            theme,
                        ),
                    ])
                })
                .collect::<Vec<_>>();
            view.list.select(Some(state.selected));
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("› ")
                    .highlight_style(Style::default()),
                Rect::new(
                    listarea.x,
                    listarea.y + 1,
                    listarea.width,
                    listarea.height.saturating_sub(1),
                ),
                view.list,
            );
        }
        if panes.len() == 3 {
            frame.render_widget(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(theme.divider),
                panes[1].inner(Margin::new(1, 0)),
            );
        }
        if state.reader || panes.len() == 3 {
            let area = *panes.last().unwrap();
            let area = area.inner(Margin::new(if state.reader { 0 } else { 1 }, 0));
            if let Some(reading) = view.reading {
                let title = p::clip(
                    &format!(
                        "{}{}",
                        if state.focus == 2 && !state.reader {
                            "› "
                        } else {
                            ""
                        },
                        reading.session.title
                    ),
                    area.width as usize,
                );
                let lines = vec![
                    Line::styled(title, theme.title),
                    Line::styled(
                        format!(
                            "{} · {} · {}",
                            p::agent(&reading.session),
                            p::project(&reading.session.cwd, &[]),
                            p::age(&reading.session.activity)
                        ),
                        theme.quiet,
                    ),
                ];
                frame.render_widget(
                    Paragraph::new(lines),
                    Rect::new(area.x, area.y, area.width, 2),
                );
                let content = reader_area(frame.area(), state.reader);
                let visible = view
                    .document
                    .lines
                    .iter()
                    .skip(state.scroll)
                    .take(content.height as usize)
                    .cloned()
                    .collect::<Vec<_>>();
                frame.render_widget(Paragraph::new(visible), content);
                let fraction = (state.scroll + content.height as usize)
                    .min(view.document.lines.len()) as f64
                    / view.document.lines.len().max(1) as f64;
                let progress = 100.0
                    * (reading.at as f64 + reading.messages.len() as f64 * fraction)
                    / reading.total.max(1) as f64;
                let current = view
                    .document
                    .matches
                    .iter()
                    .filter(|i| **i <= state.scroll + 2)
                    .count()
                    .min(view.document.matches.len());
                let position = format!(
                    "History {:.0}% · {} matches nearby{}",
                    progress,
                    view.document.matches.len(),
                    if current > 0 {
                        format!(" · at {current}")
                    } else {
                        String::new()
                    }
                );
                frame.render_widget(
                    Paragraph::new(p::clip(&position, area.width as usize)).style(theme.quiet),
                    Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
                );
            } else {
                frame.render_widget(
                    Paragraph::new("Loading selected passage…").style(theme.quiet),
                    area,
                );
            }
        }
    }
    if view.help {
        dialog(
            frame,
            "Keyboard help",
            "Type from any search pane to edit your query.\n\n↑↓ Select / scroll     Enter Read\nTab Change pane        Esc Back / quit\nCtrl-F Filters         Ctrl-D Session details\nCtrl-L Refresh index   Ctrl-R Review resume\nCtrl-K Help             Ctrl-O Session actions\nPageDown Next results  PageUp Previous results\nReader: / find · n/N matches · ←/→ more history\n\nNO_COLOR preserves focus and selection styling.\nImport diagnostics: run mass status for the technical report.\n\nEsc Close",
            view.dialog_scroll,
        );
    }
    if view.details
        && let Some(h) = view.page.and_then(|p| p.results.get(state.selected))
    {
        dialog(
            frame,
            "Session details",
            format!(
                "{}\n\nAgent: {}\nProject: {}\nActivity: {}{}\nSource: {}\nReference: {}\nTranscript: {}\n\nEsc Close",
                p::one_line(&h.session.title),
                p::agent(&h.session),
                h.session.cwd,
                if h.session.activity.is_empty() {
                    "Unknown"
                } else {
                    &h.session.activity
                },
                if h.session.time_fallback {
                    " (filesystem fallback)"
                } else {
                    ""
                },
                h.session.source,
                h.session.reference,
                h.session.path
            ),
            view.dialog_scroll,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn version_suffix_uses_muted_style_and_package_version() {
        for color in [true, false] {
            let theme = Theme::new(color);
            let title = title_line(80, theme);
            assert_eq!(title.to_string(), format!("{TITLE} v{}", crate::VERSION));
            assert_eq!(title.spans.last().unwrap().style, theme.quiet);
            assert!(
                !title
                    .spans
                    .last()
                    .unwrap()
                    .style
                    .add_modifier
                    .contains(Modifier::BOLD)
            );
            assert!(title_line(33, theme).to_string().ends_with(VERSION_LABEL));
        }
    }
    fn screen(first: bool, error: Option<&str>) -> String {
        let mut t = Terminal::new(TestBackend::new(55, 24)).unwrap();
        let state = State::default();
        let mut list = ListState::default();
        let mut dialog_scroll = 0;
        let filters = Filters::default();
        t.draw(|f| {
            draw(
                f,
                View {
                    state: &state,
                    page: None,
                    reading: None,
                    notice: "Pi: 3 transcript files",
                    error,
                    indexing: false,
                    first,
                    help: false,
                    filter: false,
                    modal: false,
                    filters: &filters,
                    list: &mut list,
                    details: false,
                    document: &super::super::document::Document::default(),
                    finding: false,
                    find_text: "",
                    find_cursor: 0,
                    searching: false,
                    dialog_scroll: &mut dialog_scroll,
                },
            )
        })
        .unwrap();
        t.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    #[test]
    fn startup_has_visible_action() {
        assert!(screen(true, None).contains("Enter: Build index"));
    }
    #[test]
    fn errors_are_in_the_main_panel() {
        let s = screen(false, Some("Not a Rust MASS index"));
        assert!(s.contains("Unable to load"));
        assert!(s.contains("Not a Rust MASS index"));
    }
    #[test]
    fn search_focus_is_visible() {
        assert!(screen(false, None).contains("Search ›"));
    }
}
