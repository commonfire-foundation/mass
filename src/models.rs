use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub reference: String,
    pub source: String,
    pub agent: String,
    pub native_id: String,
    #[serde(default)]
    pub identity_scope: String,
    #[serde(default)]
    pub read_only: bool,
    pub path: String,
    pub cwd: String,
    #[serde(default)]
    pub workspace: String,
    pub title: String,
    pub activity: String,
    pub time_fallback: bool,
    pub child: bool,
    pub parent: String,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub reference: String,
    pub native_id: String,
    pub parent: String,
    pub role: String,
    pub text: String,
    pub line: usize,
    pub chunk: usize,
    #[serde(default)]
    pub projection: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub session: Session,
    pub message_ref: String,
    pub excerpt: String,
    pub rank: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Coverage {
    pub last_attempt: String,
    pub last_success: String,
    pub warnings: Vec<String>,
    pub indexed: usize,
    pub unchanged: usize,
    pub removed: usize,
    #[serde(default)]
    pub duplicates: usize,
    /// Subset of duplicates skipped using validated discovery fingerprints.
    #[serde(default)]
    pub duplicates_unchanged: usize,
    #[serde(default)]
    pub ignored_empty: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub version: u32,
    pub results: Vec<Hit>,
    pub next_cursor: Option<String>,
    pub coverage: Coverage,
}
