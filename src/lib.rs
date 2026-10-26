/// Project release version; distinct from JSON, adapter, and database versions.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod adapters;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod discovery;
pub mod export;
pub mod index;
pub mod models;
pub mod reader;
pub mod resume;
pub mod search;
pub mod tui;

#[derive(Debug)]
pub struct Usage(pub String);
impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Usage {}

/// Terminal labels must not contain controls that move the cursor.
pub fn clean(s: &str) -> String {
    sanitize(s, false)
}
/// Preserve code whitespace in exported/copied text, while removing terminal
/// escapes and bidi controls. JSON exports deliberately retain the original data.
pub fn clean_text(s: &str) -> String {
    sanitize(s, true)
}
fn sanitize(s: &str, whitespace: bool) -> String {
    s.chars()
        .filter(|c| {
            (!c.is_control() || *c == '\n' || (whitespace && matches!(*c, '\t' | '\r')))
                && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}
pub fn reference(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
