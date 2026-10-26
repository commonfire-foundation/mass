use super::{
    input,
    presentation::{self, Theme},
    render,
};
use crate::{clipboard, export, models::Session};
use anyhow::{Result, bail};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph},
};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Target {
    pub session: Session,
    pub message: Option<String>,
    pub excerpt: String,
}
#[derive(Debug)]
pub enum Job {
    Export {
        reference: String,
        message: Option<String>,
        format: export::Format,
        paths: bool,
        destination: PathBuf,
    },
    Copy {
        helper: clipboard::Helper,
        text: Option<String>,
        reference: String,
        message: Option<String>,
    },
}
impl Job {
    pub fn run(self, db: &rusqlite::Connection, scratch: &Path) -> Result<String> {
        match self {
            Self::Export {
                reference,
                message,
                format,
                paths,
                destination,
            } => {
                let destination = std::path::absolute(destination)?;
                export::to_file(
                    db,
                    &reference,
                    message.as_deref(),
                    format,
                    paths,
                    &destination,
                )?;
                Ok(format!(
                    "Export saved\n{}\n\nIndexed text, not a native backup. Review for secrets before sharing.",
                    crate::clean(&destination.to_string_lossy())
                ))
            }
            Self::Copy {
                helper,
                text,
                reference,
                message,
            } => {
                let text = if let Some(text) = text {
                    text
                } else {
                    let Some(message) = message else {
                        bail!("No message selected");
                    };
                    let snapshot = db.unchecked_transaction()?;
                    let mut text = String::new();
                    export::messages(&snapshot, &reference, Some(&message), |m| {
                        if !text.is_empty() {
                            text.push_str("\n\n");
                        }
                        text.push_str(&m.text);
                        Ok(())
                    })?;
                    text
                };
                clipboard::copy(&helper, &crate::clean_text(&text), scratch)?;
                Ok(
                    "Copied to clipboard\nClipboard contents may be visible to other applications."
                        .into(),
                )
            }
        }
    }
}
pub enum Outcome {
    Keep,
    Close,
    Run(Job),
    Resume(Box<Session>),
}
enum Mode {
    Menu(usize),
    Export {
        format: export::Format,
        message: bool,
        path: String,
        cursor: usize,
        paths: bool,
    },
    Info(String),
}
pub struct Sheet {
    target: Target,
    helper: Option<clipboard::Helper>,
    mode: Mode,
    pub busy: bool,
    error: Option<String>,
    scroll: usize,
}
const ITEMS: [&str; 10] = [
    "Export conversation · Markdown",
    "Export conversation · JSON",
    "Export current message · Markdown",
    "Export current message · JSON",
    "Copy matching excerpt",
    "Copy current message",
    "Copy session reference",
    "Copy source path",
    "Show source location",
    "Review / reopen in original harness",
];
impl Sheet {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            helper: clipboard::detect(),
            mode: Mode::Menu(0),
            busy: false,
            error: None,
            scroll: 0,
        }
    }
    pub fn finish(&mut self, result: Result<String, String>) {
        self.busy = false;
        match result {
            Ok(s) => {
                self.mode = Mode::Info(s);
                self.error = None;
                self.scroll = 0;
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub fn paste(&mut self, text: &str) {
        if !self.busy
            && let Mode::Export { path, cursor, .. } = &mut self.mode
        {
            input::insert(path, cursor, text);
        }
    }
    fn unavailable(&self, index: usize) -> Option<&'static str> {
        if matches!(index, 2..=5) && self.target.message.is_none() {
            return Some("No message selected");
        }
        if (4..=7).contains(&index) && self.helper.is_none() {
            return Some("Clipboard unavailable: install/configure wl-copy or xclip yourself");
        }
        if index == 4 && self.target.excerpt.is_empty() {
            return Some("No matching excerpt; copy the current message instead");
        }
        None
    }
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if self.busy {
            return Outcome::Keep;
        }
        if key.code == KeyCode::Esc {
            return match self.mode {
                Mode::Export { .. } => {
                    self.mode = Mode::Menu(0);
                    self.error = None;
                    Outcome::Keep
                }
                _ => Outcome::Close,
            };
        }
        match &mut self.mode {
            Mode::Info(_) => match key.code {
                KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Down => self.scroll += 1,
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
                KeyCode::PageDown => self.scroll += 8,
                _ => {}
            },
            Mode::Export {
                format,
                message,
                path,
                cursor,
                paths,
            } => {
                if key.code == KeyCode::Tab {
                    *paths = !*paths;
                } else if key.code == KeyCode::Enter {
                    if path.trim().is_empty() {
                        self.error = Some("Enter an explicit destination filename".into());
                        return Outcome::Keep;
                    }
                    self.busy = true;
                    self.error = None;
                    return Outcome::Run(Job::Export {
                        reference: self.target.session.reference.clone(),
                        message: if *message {
                            self.target.message.clone()
                        } else {
                            None
                        },
                        format: *format,
                        paths: *paths,
                        destination: PathBuf::from(&*path),
                    });
                } else {
                    input::edit(path, cursor, key);
                }
            }
            Mode::Menu(selected) => {
                match key.code {
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => *selected = (*selected + 1).min(ITEMS.len() - 1),
                    KeyCode::Home => *selected = 0,
                    KeyCode::End => *selected = ITEMS.len() - 1,
                    _ => {}
                }
                let selected = *selected;
                if key.code != KeyCode::Enter {
                    return Outcome::Keep;
                }
                if let Some(reason) = self.unavailable(selected) {
                    self.error = Some(reason.into());
                    return Outcome::Keep;
                }
                self.error = None;
                match selected {
                    0..=3 => {
                        self.mode = Mode::Export {
                            format: if selected % 2 == 0 {
                                export::Format::Markdown
                            } else {
                                export::Format::Json
                            },
                            message: selected >= 2,
                            path: String::new(),
                            cursor: 0,
                            paths: false,
                        }
                    }
                    4..=7 => {
                        self.busy = true;
                        let text = match selected {
                            4 => Some(self.target.excerpt.clone()),
                            5 => None,
                            6 => Some(self.target.session.reference.clone()),
                            _ => Some(self.target.session.path.clone()),
                        };
                        return Outcome::Run(Job::Copy {
                            helper: self.helper.clone().unwrap(),
                            text,
                            reference: self.target.session.reference.clone(),
                            message: self.target.message.clone(),
                        });
                    }
                    8 => {
                        self.mode = Mode::Info(format!(
                            "Transcript\n{}\n\nContaining directory\n{}\n\nOriginal files are not modified.",
                            crate::clean(&self.target.session.path),
                            crate::clean(
                                &Path::new(&self.target.session.path)
                                    .parent()
                                    .unwrap_or(Path::new("."))
                                    .to_string_lossy()
                            )
                        ))
                    }
                    _ => return Outcome::Resume(Box::new(self.target.session.clone())),
                }
            }
        }
        Outcome::Keep
    }
    pub fn draw(&mut self, frame: &mut Frame, theme: Theme) {
        if let Mode::Info(text) = &self.mode {
            render::dialog(
                frame,
                "Session actions",
                format!("{text}\n\nEsc Close"),
                &mut self.scroll,
            );
            return;
        }
        let full = frame.area();
        let width = full.width.saturating_sub(4).min(76);
        let height = full.height.saturating_sub(4).min(20);
        let area = Rect::new(
            full.x + (full.width - width) / 2,
            full.y + (full.height - height) / 2,
            width,
            height,
        );
        let inner = area.inner(Margin::new(1, 1));
        frame.render_widget(Clear, area);
        frame.render_widget(Block::bordered().title(" Session actions "), area);
        let clip = |s: &str| presentation::clip(s, inner.width as usize);
        let mut lines = Vec::new();
        let mut cursor = None;
        match &self.mode {
            Mode::Menu(selected) => {
                lines.push(Line::styled(clip(&self.target.session.title), theme.title));
                let count = (inner.height as usize)
                    .saturating_sub(5)
                    .max(1)
                    .min(ITEMS.len());
                let start = selected.saturating_sub(count - 1);
                for (i, item) in ITEMS.iter().enumerate().skip(start).take(count) {
                    let text = format!(
                        "{}{}{}",
                        if i == *selected { "› " } else { "  " },
                        item,
                        if self.unavailable(i).is_some() {
                            " (unavailable)"
                        } else {
                            ""
                        }
                    );
                    lines.push(Line::styled(
                        clip(&text),
                        if i == *selected {
                            theme.selected
                        } else {
                            theme.quiet
                        },
                    ));
                }
                lines.push(Line::raw(clip(&format!(
                    "↑↓ Select · Enter Choose · Esc Close · {}/{}",
                    selected + 1,
                    ITEMS.len()
                ))));
                lines.push(Line::styled(
                    clip("Exports / clipboard may contain secrets."),
                    theme.quiet,
                ));
                let hint = self
                    .error
                    .as_deref()
                    .or_else(|| self.unavailable(*selected));
                if let Some(hint) = hint {
                    lines.push(Line::raw(clip(hint)));
                }
            }
            Mode::Export {
                format,
                message,
                path,
                cursor: at,
                paths,
            } => {
                lines.push(Line::styled(
                    format!(
                        "Export {} · {:?}",
                        if *message {
                            "current message"
                        } else {
                            "conversation"
                        },
                        format
                    ),
                    theme.title,
                ));
                lines.push(Line::raw("Destination filename:"));
                let (shown, col) = input::viewport(path, *at, inner.width as usize);
                lines.push(Line::styled(shown, theme.accent));
                cursor = Some((inner.x + col, inner.y + 2));
                lines.push(Line::raw(clip(&format!(
                    "Metadata paths: {} · Tab toggles",
                    if *paths { "included" } else { "omitted" }
                ))));
                lines.push(Line::raw(clip("Text may contain secrets and local paths.")));
                lines.push(Line::raw(clip("Indexed text only; not a native backup.")));
                lines.push(Line::raw(clip("Enter Save (no overwrite) · Esc Back")));
                if let Some(error) = &self.error {
                    lines.push(Line::raw(clip(error)));
                }
            }
            Mode::Info(_) => unreachable!(),
        }
        if self.busy {
            lines.push(Line::raw("Working…"));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        if !self.busy
            && let Some((x, y)) = cursor
            && y < inner.bottom()
        {
            frame.set_cursor_position((x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sheet() -> Sheet {
        let mut sheet = Sheet::new(Target {
            session: Session::default(),
            message: Some("message".into()),
            excerpt: "excerpt".into(),
        });
        sheet.helper = None;
        sheet
    }
    #[test]
    fn missing_clipboard_stays_in_menu_without_a_job() {
        let mut sheet = sheet();
        sheet.mode = Mode::Menu(6);
        assert!(matches!(sheet.key(KeyCode::Enter.into()), Outcome::Keep));
        assert!(
            sheet
                .error
                .as_deref()
                .unwrap()
                .contains("Clipboard unavailable")
        );
        assert!(!sheet.busy);
    }
    #[test]
    fn export_requires_destination_and_cancel_creates_no_job() {
        let mut sheet = sheet();
        assert!(matches!(sheet.key(KeyCode::Enter.into()), Outcome::Keep));
        assert!(matches!(sheet.key(KeyCode::Enter.into()), Outcome::Keep));
        assert!(!sheet.busy);
        assert!(matches!(sheet.key(KeyCode::Esc.into()), Outcome::Keep));
        assert!(matches!(sheet.key(KeyCode::Esc.into()), Outcome::Close));
    }
}
