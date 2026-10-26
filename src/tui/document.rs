//! Bounded, width-aware transcript rendering. No terminal escapes or executable Markdown.
use super::presentation::{self as p, Theme};
use crate::reader::Reading;
use ratatui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Default)]
pub struct Document {
    pub lines: Vec<Line<'static>>,
    pub matches: Vec<usize>,
    pub anchors: Vec<(usize, String)>,
    pub target: usize,
}
pub(super) fn wrap(text: &str, width: usize, code: bool) -> Vec<String> {
    let width = width.max(1);
    let mut remaining = text;
    let mut lines = Vec::new();
    while !remaining.is_empty() {
        let mut end = 0;
        let mut cells = 0;
        let mut space = None;
        for (i, g) in remaining.grapheme_indices(true) {
            let w = Span::raw(g).width();
            if cells + w > width {
                break;
            }
            cells += w;
            end = i + g.len();
            if g.chars().all(char::is_whitespace) && i > 0 {
                space = Some(end);
            }
        }
        if end == 0 {
            end = remaining.graphemes(true).next().unwrap().len();
        }
        if !code
            && end < remaining.len()
            && let Some(at) = space
        {
            end = at;
        }
        let line = &remaining[..end];
        lines.push(if code {
            line.to_owned()
        } else {
            line.trim_end().to_owned()
        });
        remaining = &remaining[end..];
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
fn literal_starts(text: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return vec![];
    }
    // Lowercasing can expand a Unicode character; retain original UTF-8 offsets.
    let mut offsets = Vec::new();
    for (i, ch) in text.char_indices() {
        offsets.extend(std::iter::repeat_n(
            i,
            ch.to_lowercase().map(char::len_utf8).sum(),
        ));
    }
    text.to_lowercase()
        .match_indices(&needle.to_lowercase())
        .filter_map(|(i, _)| offsets.get(i).copied())
        .collect()
}
impl Document {
    pub fn build(
        reading: &Reading,
        width: usize,
        query: &str,
        find: &str,
        excerpt: &str,
        theme: Theme,
    ) -> Self {
        let terms = if find.is_empty() {
            p::terms(query)
        } else {
            vec![find.to_lowercase()]
        };
        let highlights = if find.is_empty() {
            terms.clone()
        } else {
            p::terms(find)
        };
        let fragment = excerpt
            .split(" … ")
            .max_by_key(|s| s.len())
            .unwrap_or("")
            .trim();
        let mut doc = Self::default();
        let mut code = false;
        let mut previous_native = String::new();
        let mut previous_tail = String::new();
        let mut target = None;
        for message in &reading.messages {
            doc.anchors
                .push((doc.lines.len(), message.reference.clone()));
            let same = previous_native == message.native_id;
            if !same {
                doc.lines.push(Line::styled(
                    p::one_line(&message.role).to_uppercase(),
                    theme.accent,
                ));
                previous_native = message.native_id.clone();
                code = false;
            }
            // Remove only proven overlap, before sanitizing control characters.
            let skip = if same && message.chunk > 0 {
                (1..=512.min(message.text.len()).min(previous_tail.len()))
                    .rev()
                    .find(|&n| {
                        message.text.is_char_boundary(n)
                            && previous_tail.is_char_boundary(previous_tail.len() - n)
                            && message.text[..n] == previous_tail[previous_tail.len() - n..]
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let mut tail = message.text.len().saturating_sub(512);
            while !message.text.is_char_boundary(tail) {
                tail += 1;
            }
            previous_tail = message.text[tail..].to_owned();
            // Tabs are content, not disposable controls. Expand them for stable
            // terminal layout; export/copy retain the actual tab characters.
            let text = crate::clean_text(&message.text[skip..])
                .replace("\r\n", "\n")
                .replace('\r', "")
                .replace('\t', "    ");
            let mut byte = 0;
            let offset = literal_starts(&text, fragment).first().copied();
            let found = literal_starts(&text, find);
            for original in text.split('\n') {
                if original.trim_start().starts_with("```") {
                    code = !code;
                    doc.lines.push(Line::styled(
                        if code {
                            format!("  ┌ {}", original.trim_start().trim_start_matches('`'))
                        } else {
                            "  └".into()
                        },
                        theme.quiet,
                    ));
                    byte += original.len() + 1;
                    continue;
                }
                let heading = !code && original.starts_with('#');
                let shown = if heading {
                    original.trim_start_matches('#').trim_start()
                } else {
                    original
                };
                let style = if heading {
                    theme.title
                } else {
                    Style::default()
                };
                let target_line = offset
                    .filter(|o| *o >= byte && *o <= byte + original.len())
                    .map(|o| {
                        let mut end = o
                            .saturating_sub(byte + original.len() - shown.len())
                            .min(shown.len());
                        while !shown.is_char_boundary(end) {
                            end -= 1;
                        }
                        wrap(&shown[..end], width, code).len().saturating_sub(1)
                    });
                let mut shown_offset = 0;
                for (line_index, line) in wrap(shown, width, code).into_iter().enumerate() {
                    shown_offset += shown[shown_offset..].find(&line).unwrap_or(0);
                    let start = byte + original.len() - shown.len() + shown_offset;
                    let lower = line.to_lowercase();
                    let matched = if find.is_empty() {
                        terms.iter().any(|t| lower.contains(t))
                    } else {
                        found
                            .get(found.partition_point(|at| *at < start))
                            .is_some_and(|at| *at < start + line.len())
                    };
                    shown_offset += line.len();
                    if matched {
                        doc.matches.push(doc.lines.len());
                    }
                    if target.is_none() && target_line == Some(line_index) {
                        target = Some(doc.lines.len());
                    }
                    doc.lines
                        .push(p::highlighted(&line, &highlights, style, theme));
                }
                byte += original.len() + 1;
            }
        }
        doc.target = target
            .or_else(|| doc.matches.first().copied())
            .unwrap_or(0)
            .saturating_sub(2);
        doc
    }
    pub fn max_scroll(&self, height: usize) -> usize {
        self.lines.len().saturating_sub(height)
    }
    pub fn anchor(&self, row: usize) -> Option<(String, usize)> {
        self.anchors
            .iter()
            .rev()
            .find(|(line, _)| *line <= row)
            .map(|(line, r)| (r.clone(), row - line))
    }
    pub fn restore(&self, anchor: &(String, usize)) -> usize {
        self.anchors
            .iter()
            .find(|(_, r)| *r == anchor.0)
            .map(|(line, _)| line + anchor.1)
            .unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, Session};
    fn reading() -> Reading {
        Reading{version:1,session:Session::default(),messages:vec![Message{reference:"m".into(),native_id:"1".into(),parent:String::new(),role:"assistant".into(),text:"# Heading\n\n```rust\n\tlet x = 1;\n```\nLong context before the unique callback passage here.".into(),line:1,chunk:0,projection:false}],at:0,total:1,more:false,next:None,previous:None}
    }
    #[test]
    fn wraps_code_without_losing_indent() {
        let d = Document::build(
            &reading(),
            30,
            "callback",
            "",
            "unique callback passage",
            Theme::new(false),
        );
        assert!(d.lines.iter().any(|l| l.to_string().starts_with("    let")));
        assert!(d.lines.iter().all(|l| l.width() <= 30));
        assert!(!d.matches.is_empty());
        assert!(d.target > 0);
    }
    #[test]
    fn find_locates_phrases_across_wrapped_rows() {
        let mut reading = reading();
        reading.messages[0].text = format!(
            "{} Distant CAFÉ needle crosses rows here",
            "context ".repeat(100)
        );
        let doc = Document::build(
            &reading,
            20,
            "",
            "distant café needle",
            "distant café needle",
            Theme::new(false),
        );
        assert_eq!(doc.matches.len(), 1);
        assert!(doc.target > 20);
        assert!(
            doc.lines[doc.matches[0]]
                .to_string()
                .to_lowercase()
                .contains("distant")
        );
        assert_eq!(literal_starts("İ uppercase CAFÉ", "café"), vec![13]);
    }
    #[test]
    fn selected_excerpt_starts_near_the_actual_passage() {
        let mut reading = reading();
        reading.messages[0].text = format!(
            "{}the exact needle passage here",
            "unrelated context ".repeat(300)
        );
        let doc = Document::build(
            &reading,
            34,
            "needle",
            "",
            "the exact needle passage",
            Theme::new(false),
        );
        assert!(doc.target > 30);
        assert!(
            doc.lines
                .iter()
                .skip(doc.target)
                .take(6)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
                .contains("needle")
        );
    }
    #[test]
    fn overlap_is_verified_before_removing_controls() {
        let mut reading = reading();
        let overlap = format!("{}\n\u{1b}", "é".repeat(255));
        reading.messages[0].text = format!("Original {overlap}");
        let mut continuation = reading.messages[0].clone();
        continuation.chunk = 1;
        continuation.reference = "next".into();
        continuation.text = format!("{overlap}unique addition");
        reading.messages.push(continuation);
        reading.total = 2;
        let doc = Document::build(&reading, 80, "", "", "", Theme::new(false));
        assert!(
            doc.lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                .contains("unique addition")
        );
    }
    #[test]
    fn width_uses_graphemes() {
        let lines = wrap("你好 café 👩‍💻 and words", 8, false);
        assert!(lines.iter().all(|l| Span::raw(l).width() <= 8));
    }
}
