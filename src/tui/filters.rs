use super::{
    input,
    presentation::{self as p, Theme},
};
use crate::search::Filters;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph},
};

pub struct Sheet {
    pub fields: [String; 4],
    pub focus: usize,
    cursors: [usize; 4],
    pub projects: Vec<String>,
    pub suggestion: Option<usize>,
    pub error: Option<String>,
}
pub enum Action {
    Keep,
    Apply(Filters),
    Cancel,
}
impl Sheet {
    pub fn new(filters: &Filters) -> Self {
        let fields = [
            filters.agent.clone().unwrap_or_default(),
            filters.project.clone().unwrap_or_default(),
            filters.since.clone().unwrap_or_default(),
            filters.before.clone().unwrap_or_default(),
        ];
        let cursors = std::array::from_fn(|i| fields[i].len());
        Self {
            fields,
            focus: 0,
            cursors,
            projects: Vec::new(),
            suggestion: None,
            error: None,
        }
    }
    pub fn candidates(&self) -> Vec<&str> {
        let q = self.fields[1].to_lowercase();
        self.projects
            .iter()
            .filter(|p| p.to_lowercase().contains(&q))
            .take(6)
            .map(String::as_str)
            .collect()
    }
    pub fn paste(&mut self, s: &str) {
        if self.focus > 0 {
            input::insert(
                &mut self.fields[self.focus],
                &mut self.cursors[self.focus],
                s,
            );
            self.suggestion = None;
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.code == KeyCode::Esc {
            return Action::Cancel;
        }
        if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.fields = Default::default();
            self.cursors = [0; 4];
            self.suggestion = None;
            self.error = None;
            return Action::Keep;
        }
        match key.code {
            KeyCode::Tab => self.focus = (self.focus + 1) % 4,
            KeyCode::BackTab => self.focus = (self.focus + 3) % 4,
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if self.focus == 0 => {
                let options = ["", "pi", "claude", "codex"];
                let i = options
                    .iter()
                    .position(|s| *s == self.fields[0])
                    .unwrap_or(0);
                let i = (i + if key.code == KeyCode::Left { 3 } else { 1 }) % 4;
                self.fields[0] = options[i].into();
            }
            KeyCode::Down | KeyCode::Up if self.focus == 1 => {
                let n = self.candidates().len();
                if n > 0 {
                    self.suggestion = Some(match (self.suggestion, key.code) {
                        (Some(i), KeyCode::Down) => (i + 1) % n,
                        (Some(i), _) => (i + n - 1) % n,
                        (None, KeyCode::Up) => n - 1,
                        _ => 0,
                    });
                }
            }
            KeyCode::Down => self.focus = (self.focus + 1) % 4,
            KeyCode::Up => self.focus = (self.focus + 3) % 4,
            KeyCode::Enter => {
                if self.focus == 1
                    && let Some(i) = self.suggestion
                {
                    self.fields[1] = self.candidates()[i].to_owned();
                    self.cursors[1] = self.fields[1].len();
                    self.suggestion = None;
                }
                for field in &self.fields[2..] {
                    if !field.is_empty()
                        && chrono::NaiveDate::parse_from_str(field, "%Y-%m-%d").is_err()
                    {
                        self.error = Some("Use YYYY-MM-DD for dates, or leave them empty.".into());
                        return Action::Keep;
                    }
                }
                if !self.fields[1].is_empty() && !std::path::Path::new(&self.fields[1]).is_dir() {
                    self.error = Some(
                        "Select an existing project directory with ↑↓, or enter its full path."
                            .into(),
                    );
                    return Action::Keep;
                }
                let opt = |s: &String| if s.is_empty() { None } else { Some(s.clone()) };
                return Action::Apply(Filters {
                    agent: opt(&self.fields[0]),
                    project: opt(&self.fields[1]),
                    since: opt(&self.fields[2]),
                    before: opt(&self.fields[3]),
                    limit: 50,
                    cursor: None,
                });
            }
            _ if self.focus > 0
                && input::edit(
                    &mut self.fields[self.focus],
                    &mut self.cursors[self.focus],
                    key,
                ) == Some(true) =>
            {
                self.suggestion = None;
                self.error = None;
            }
            _ => {}
        }
        Action::Keep
    }
    pub fn draw(&self, frame: &mut Frame) {
        let theme = Theme::terminal();
        let a = frame.area();
        let w = a.width.saturating_sub(4).min(76);
        let h = a.height.saturating_sub(2).min(20);
        let area = Rect::new(a.x + (a.width - w) / 2, a.y + (a.height - h) / 2, w, h);
        frame.render_widget(Clear, area);
        frame.render_widget(Block::bordered().title(" Filters "), area);
        let inner = area.inner(Margin::new(2, 1));
        let labels = ["Agent", "Project", "Since", "Before"];
        let mut lines = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            let value = if i == 0 {
                format!(
                    "‹ {} ›",
                    if self.fields[0].is_empty() {
                        "All agents"
                    } else {
                        &self.fields[0]
                    }
                )
            } else if self.fields[i].is_empty() {
                if i == 1 {
                    "All projects · type to search".into()
                } else {
                    "YYYY-MM-DD · optional".into()
                }
            } else {
                input::viewport(
                    &self.fields[i],
                    self.cursors[i],
                    inner.width.saturating_sub(12) as usize,
                )
                .0
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} {label:<8} ", if self.focus == i { "›" } else { " " }),
                    if self.focus == i {
                        theme.accent
                    } else {
                        theme.quiet
                    },
                ),
                Span::raw(value),
            ]));
        }
        lines.push(Line::default());
        if self.focus == 1 {
            for (i, project) in self.candidates().iter().enumerate() {
                lines.push(Line::styled(
                    format!(
                        "  {} {}",
                        if self.suggestion == Some(i) {
                            "›"
                        } else {
                            " "
                        },
                        p::clip(project, inner.width.saturating_sub(5) as usize)
                    ),
                    if self.suggestion == Some(i) {
                        theme.selected
                    } else {
                        theme.quiet
                    },
                ));
            }
        }
        if let Some(error) = &self.error {
            lines.push(Line::styled(
                p::clip(error, inner.width as usize),
                theme.accent,
            ));
        }
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(
                inner.x,
                inner.y,
                inner.width,
                inner.height.saturating_sub(3),
            ),
        );
        frame.render_widget(Paragraph::new("Tab Next field · ←→ Agent · ↑↓ Projects\nEnter Apply · Ctrl-L Clear filters · Esc Cancel").style(theme.quiet),Rect::new(inner.x,inner.bottom().saturating_sub(2),inner.width,2));
        if self.focus > 0 && inner.width > 12 {
            let (_, col) = input::viewport(
                &self.fields[self.focus],
                self.cursors[self.focus],
                inner.width.saturating_sub(12) as usize,
            );
            frame.set_cursor_position((inner.x + 11 + col, inner.y + self.focus as u16));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clear_filters_does_not_require_query() {
        let mut s = Sheet::new(&Filters {
            agent: Some("pi".into()),
            ..Default::default()
        });
        s.key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
        assert!(s.fields.iter().all(String::is_empty));
    }
    #[test]
    fn invalid_date_keeps_sheet_open() {
        let mut s = Sheet::new(&Filters::default());
        s.fields[2] = "bad".into();
        assert!(matches!(s.key(KeyCode::Enter.into()), Action::Keep));
        assert!(s.error.is_some());
    }
}
