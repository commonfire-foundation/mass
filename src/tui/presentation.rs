//! Shared terminal-native styling and width-aware presentation.
use crate::models::Session;
use ratatui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
pub struct Theme {
    pub accent: Style,
    pub divider: Style,
    pub quiet: Style,
    pub title: Style,
    pub selected: Style,
    pub matched: Style,
}
impl Theme {
    pub fn new(color: bool) -> Self {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        Self {
            accent: if color { bold.fg(Color::Cyan) } else { bold },
            // ANSI slot 1 maps to the current desktop theme's accent.
            // Avoid bold/dim so the terminal uses the palette color unchanged.
            divider: if color {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            },
            quiet: Style::default().add_modifier(Modifier::DIM),
            title: bold,
            selected: Style::default().add_modifier(Modifier::REVERSED),
            matched: if color {
                bold.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)
            } else {
                bold.add_modifier(Modifier::UNDERLINED)
            },
        }
    }
    pub fn terminal() -> Self {
        Self::new(std::env::var_os("NO_COLOR").is_none())
    }
}
pub fn one_line(text: &str) -> String {
    crate::clean(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn clip(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text = one_line(text);
    if Span::raw(&text).width() <= width {
        return text;
    }
    let mut out = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        let w = Span::raw(g).width();
        if used + w + 1 > width {
            break;
        }
        out.push_str(g);
        used += w;
    }
    out.push('…');
    out
}
pub fn age(date: &str) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(date) else {
        return "Unknown date".into();
    };
    let seconds = (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    match seconds {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86399 => format!("{}h ago", seconds / 3600),
        86400..=172799 => "Yesterday".into(),
        _ => t.format("%b %-d, %Y").to_string(),
    }
}
pub fn project(path: &str, peers: &[Session]) -> String {
    if path.is_empty() {
        return "No project".into();
    }
    let parts = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    for n in 1..=parts.len() {
        let suffix = parts[parts.len() - n..].join("/");
        if !peers
            .iter()
            .any(|s| s.cwd != path && s.cwd.trim_end_matches('/').ends_with(&format!("/{suffix}")))
        {
            return one_line(&suffix);
        }
    }
    one_line(path)
}
pub fn agent(s: &Session) -> String {
    let label = match s.agent.as_str() {
        "pi" => "Pi",
        "claude" => "Claude",
        "codex" => "Codex",
        _ => &s.agent,
    };
    format!(
        "{}{}",
        one_line(label),
        if s.child { " · child" } else { "" }
    )
}
/// Display highlighting only: does not change engine matching or ranking.
pub fn terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|s| !s.contains(':'))
        .map(|s| s.trim_matches('"').to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}
pub fn highlighted(text: &str, terms: &[String], base: Style, theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    let mut word = String::new();
    let mut word_kind = None;
    for ch in text.chars() {
        let kind = ch.is_alphanumeric() || ch == '_';
        if word_kind.is_some_and(|k| k != kind) {
            let matched = terms.iter().any(|t| word.to_lowercase().contains(t));
            spans.push(Span::styled(
                std::mem::take(&mut word),
                if matched {
                    base.patch(theme.matched)
                } else {
                    base
                },
            ));
        }
        word.push(ch);
        word_kind = Some(kind);
    }
    if !word.is_empty() {
        let matched = terms.iter().any(|t| word.to_lowercase().contains(t));
        spans.push(Span::styled(
            word,
            if matched {
                base.patch(theme.matched)
            } else {
                base
            },
        ));
    }
    Line::from(spans)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipping_uses_cells() {
        assert_eq!(clip("你好 hello", 5), "你好…");
        assert_eq!(clip("abcdef", 1), "…");
        assert_eq!(clip("a", 0), "");
    }
    #[test]
    fn project_disambiguation() {
        let peers = vec![
            Session {
                cwd: "/work/one/api".into(),
                ..Default::default()
            },
            Session {
                cwd: "/work/two/api".into(),
                ..Default::default()
            },
        ];
        assert_eq!(project(&peers[0].cwd, &peers), "one/api");
    }
    #[test]
    fn monochrome_preserves_emphasis() {
        let t = Theme::new(false);
        assert!(t.selected.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(t.accent.fg, None);
        assert_eq!(t.divider.fg, None);
        let divider = Theme::new(true).divider;
        assert_eq!(divider.fg, Some(Color::Red));
        assert!(divider.add_modifier.is_empty());
    }
}
