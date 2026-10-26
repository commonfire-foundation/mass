use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::Span;
use unicode_segmentation::UnicodeSegmentation;

fn previous(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}
fn next(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(i, _)| cursor + i)
        .unwrap_or(text.len())
}
pub fn insert(text: &mut String, cursor: &mut usize, value: &str) {
    let value = crate::clean(&value.replace(['\r', '\t'], " ")).replace('\n', " ");
    text.insert_str(*cursor, &value);
    *cursor += value.len();
}
/// None means this is not an editing key; Some says whether text changed.
pub fn edit(text: &mut String, cursor: &mut usize, key: KeyEvent) -> Option<bool> {
    *cursor = (*cursor).min(text.len());
    if !text.is_char_boundary(*cursor) {
        *cursor = text.len();
    }
    let old = text.clone();
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Home | KeyCode::Char('a') if key.code == KeyCode::Home || ctrl => *cursor = 0,
        KeyCode::End | KeyCode::Char('e') if key.code == KeyCode::End || ctrl => {
            *cursor = text.len()
        }
        KeyCode::Left => *cursor = previous(text, *cursor),
        KeyCode::Right => *cursor = next(text, *cursor),
        KeyCode::Backspace if *cursor > 0 => {
            let start = previous(text, *cursor);
            text.replace_range(start..*cursor, "");
            *cursor = start;
        }
        KeyCode::Backspace => {}
        KeyCode::Delete if *cursor < text.len() => {
            let end = next(text, *cursor);
            text.replace_range(*cursor..end, "");
        }
        KeyCode::Delete => {}
        KeyCode::Char('u') if ctrl => {
            text.replace_range(..*cursor, "");
            *cursor = 0;
        }
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            text.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        _ => return None,
    }
    Some(*text != old)
}
/// A grapheme-safe horizontal viewport and cursor cell offset.
pub fn viewport(text: &str, cursor: usize, width: usize) -> (String, u16) {
    if width == 0 {
        return (String::new(), 0);
    }
    let cursor = cursor.min(text.len());
    let mut start = 0;
    for (i, g) in text[..cursor].grapheme_indices(true) {
        if Span::raw(&text[start..cursor]).width() < width {
            break;
        }
        start = i + g.len();
    }
    let col = Span::raw(&text[start..cursor]).width();
    let mut shown = String::new();
    let mut cells = 0;
    for g in text[start..].graphemes(true) {
        let w = Span::raw(g).width();
        if cells + w > width {
            break;
        }
        shown.push_str(g);
        cells += w;
    }
    (shown, col.min(width - 1) as u16)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_in_middle_and_graphemes() {
        let mut s = "a👩‍💻éz".to_owned();
        let mut c = s.len();
        edit(&mut s, &mut c, KeyCode::Left.into());
        edit(&mut s, &mut c, KeyCode::Backspace.into());
        assert_eq!(s, "a👩‍💻z");
        edit(&mut s, &mut c, KeyCode::Backspace.into());
        assert_eq!(s, "az");
        insert(&mut s, &mut c, "中\n文");
        assert_eq!(s, "a中 文z");
    }
    #[test]
    fn viewport_keeps_cursor_visible() {
        let s = "long query with 你好 ending";
        let (v, c) = viewport(s, s.len(), 12);
        assert!(v.ends_with("ending"));
        assert!(c < 12);
        assert!(Span::raw(v).width() <= 12);
    }
}
