//! Portable indexed-text exports, not native harness backups.
use crate::{models::Message, reader};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Format {
    Markdown,
    Json,
}
#[derive(Debug, Serialize)]
pub struct ExportMessage {
    pub reference: String,
    pub native_id: String,
    pub parent: String,
    pub role: String,
    pub text: String,
    pub source_line: usize,
    pub continuation: bool,
}
/// Emit complete logical messages. A selected chunk selects its whole original message.
pub fn messages(
    db: &Connection,
    reference: &str,
    selected: Option<&str>,
    mut emit: impl FnMut(ExportMessage) -> Result<()>,
) -> Result<()> {
    let snapshot = reader::snapshot(db)?;
    let db = snapshot.as_deref().unwrap_or(db);
    let session = reader::resolve(db, reference)?;
    let selected: Option<(String, usize)> = selected.map(|at| db.query_row(
        "SELECT m.native_id,m.line FROM messages m JOIN files f ON f.key=m.file WHERE f.ref=?1 AND m.ref=?2",
        params![session.reference,at], |r| Ok((r.get(0)?,r.get(1)?))
    ).context("Message does not belong to this session")).transpose()?;
    let mut statement = db.prepare("SELECT m.data FROM messages m JOIN files f ON f.key=m.file WHERE f.ref=?1 AND (?2 IS NULL OR (m.native_id=?2 AND m.line=?3)) ORDER BY m.id")?;
    let rows = statement.query_map(
        params![
            session.reference,
            selected.as_ref().map(|s| &s.0),
            selected.as_ref().map(|s| s.1)
        ],
        |r| r.get::<_, String>(0),
    )?;
    let mut pending: Option<ExportMessage> = None;
    let mut previous_chunk = 0;
    for row in rows {
        let message: Message = serde_json::from_str(&row?)?;
        if message.role == "metadata" && message.text.is_empty() {
            continue;
        }
        let adjacent = pending.as_ref().is_some_and(|p| {
            p.native_id == message.native_id
                && p.role == message.role
                && p.source_line == message.line
                && message.chunk == previous_chunk + 1
        });
        if adjacent {
            let p = pending.as_mut().unwrap();
            let skip = (1..=512.min(message.text.len()).min(p.text.len()))
                .rev()
                .find(|&n| {
                    message.text.is_char_boundary(n)
                        && p.text.is_char_boundary(p.text.len() - n)
                        && message.text[..n] == p.text[p.text.len() - n..]
                })
                .unwrap_or(0);
            p.text.push_str(&message.text[skip..]);
        } else {
            if let Some(p) = pending.take() {
                emit(p)?;
            }
            pending = Some(ExportMessage {
                reference: message.reference,
                native_id: message.native_id,
                parent: message.parent,
                role: message.role,
                text: message.text,
                source_line: message.line,
                continuation: message.chunk != 0,
            });
        }
        previous_chunk = message.chunk;
    }
    if let Some(p) = pending {
        emit(p)?;
    }
    Ok(())
}
fn label(text: &str) -> String {
    crate::clean(text)
        .replace(['\n', '\r'], " ")
        .chars()
        .flat_map(|c| {
            if "\\`*_{}[]<>#!".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
pub fn write(
    db: &Connection,
    reference: &str,
    selected: Option<&str>,
    format: Format,
    include_paths: bool,
    out: &mut impl Write,
) -> Result<()> {
    let snapshot = db.unchecked_transaction()?;
    let session = reader::resolve(&snapshot, reference)?;
    let project = Path::new(&session.cwd)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut metadata = serde_json::json!({"reference":session.reference,"native_id":session.native_id,"agent":session.agent,"title":session.title,"project":project,"activity":session.activity,"activity_is_filesystem_time":session.time_fallback,"child":session.child,"parent":session.parent,"record_diagnostic_count":session.warnings.len()});
    if include_paths {
        metadata["source_path"] = session.path.clone().into();
        metadata["cwd"] = session.cwd.clone().into();
    }
    match format {
        Format::Json => {
            write!(
                out,
                "{{\"format\":\"mass.conversation\",\"version\":1,\"indexed_snapshot\":true,\"native_backup\":false,\"selection\":{},\"session\":{},\"messages\":[",
                serde_json::to_string(&selected)?,
                metadata
            )?;
            let mut first = true;
            messages(&snapshot, &session.reference, selected, |m| {
                if !first {
                    write!(out, ",")?;
                }
                first = false;
                serde_json::to_writer(&mut *out, &m)?;
                Ok(())
            })?;
            writeln!(out, "]}}")?;
        }
        Format::Markdown => {
            writeln!(
                out,
                "# {}\n\n- Agent: {}\n- Project: {}\n- Activity: {}{}\n- Session reference: {}\n- Record diagnostics: {}\n",
                label(&session.title),
                label(&session.agent),
                label(&project),
                label(&session.activity),
                if session.time_fallback {
                    " (filesystem time)"
                } else {
                    ""
                },
                session.reference,
                session.warnings.len()
            )?;
            if include_paths {
                writeln!(
                    out,
                    "- Source: {}\n- Working directory: {}\n",
                    label(&session.path),
                    label(&session.cwd)
                )?;
            }
            writeln!(
                out,
                "> Indexed conversation text, not a native session backup. Text may contain secrets and paths; omitted tool output, reasoning, images, and unsupported records are not restored by this export.\n"
            )?;
            if let Some(at) = selected {
                writeln!(out, "Selected message containing chunk: {}\n", label(at))?;
            }
            messages(&snapshot, &session.reference, selected, |m| {
                writeln!(
                    out,
                    "## {}{}\n\nReference: {} · source line {}\n\n{}\n",
                    label(&m.role),
                    if m.continuation {
                        " (continuation)"
                    } else {
                        ""
                    },
                    m.reference,
                    m.source_line,
                    crate::clean_text(&m.text)
                )?;
                Ok(())
            })?;
        }
    }
    Ok(())
}
pub fn to_file(
    db: &Connection,
    reference: &str,
    selected: Option<&str>,
    format: Format,
    include_paths: bool,
    destination: &Path,
) -> Result<()> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .context("Export directory must already exist and be writable")?;
    {
        let mut out = std::io::BufWriter::new(temporary.as_file_mut());
        write(db, reference, selected, format, include_paths, &mut out)?;
        out.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(destination).map_err(|e| {
        anyhow::anyhow!(
            "Cannot save export without overwriting an existing file: {}",
            e.error
        )
    })?;
    Ok(())
}
