//! Native agent histories and read-only Pi subagent exports.
mod records;
#[derive(Debug)]
pub struct EmptyTranscript;
impl std::fmt::Display for EmptyTranscript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Empty transcript")
    }
}
impl std::error::Error for EmptyTranscript {}
use crate::{
    config::Source,
    models::{Message, Session},
    reference,
};
use anyhow::{Result, bail};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
pub const VERSION: u32 = 3;
const CHUNK: usize = 16 * 1024;
fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_owned()
}
fn session_reference(s: &Session) -> String {
    if s.identity_scope.is_empty() {
        reference(&[&s.source, &s.native_id])
    } else {
        reference(&[&s.source, &s.native_id, &s.identity_scope])
    }
}
fn text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|b| {
                    matches!(
                        b["type"].as_str(),
                        Some("text" | "input_text" | "output_text")
                    )
                })
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
pub fn parse(
    path: &Path,
    source: &Source,
    emit: impl FnMut(Message) -> Result<()>,
) -> Result<Session> {
    parse_cancellable(
        path,
        source,
        &std::sync::atomic::AtomicBool::new(false),
        emit,
    )
}
pub fn parse_cancellable(
    path: &Path,
    source: &Source,
    cancel: &std::sync::atomic::AtomicBool,
    mut emit: impl FnMut(Message) -> Result<()>,
) -> Result<Session> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("Parsing cancelled");
    }
    let file = File::open(path)?;
    let stream: Box<dyn Read> = if path.extension().is_some_and(|e| e == "zst") {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(file)
    };
    let mut reader = BufReader::new(stream);
    let mut session = Session {
        source: source.id.clone(),
        agent: source.agent.clone(),
        path: path.to_string_lossy().into(),
        child: path.components().any(|p| p.as_os_str() == "subagents"),
        ..Default::default()
    };
    let mut line = 0;
    let mut nonblank = false;
    let mut skipped = std::collections::BTreeMap::<String, (usize, usize)>::new();
    let mut identity = String::new();
    let mut inherited = std::collections::HashSet::<String>::new();
    while let Some(record) = records::next(&mut reader, cancel)? {
        line += 1;
        let v = match record {
            records::Record::Blank => continue,
            records::Record::Skipped(reason) => {
                nonblank = true;
                skipped.entry(reason.into()).or_insert((0, line)).0 += 1;
                continue;
            }
            records::Record::Value(v) => v,
        };
        nonblank = true;
        let kind = v["type"].as_str().unwrap_or("");
        let mut role = String::new();
        let mut body = String::new();
        let mut projection = false;
        match source.agent.as_str() {
            "pi" if v["recordType"] == "message" && v["version"] == 1 => {
                let run = string(&v, "runId");
                let child = v["childIndex"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Export missing child index"))?;
                if run.is_empty() {
                    bail!("Export missing run identity");
                }
                session.native_id = run.clone();
                session.identity_scope = format!("export:{child}");
                session.parent = run;
                session.read_only = true;
                session.child = true;
                session.cwd = string(&v, "cwd");
                role = string(&v["message"], "role");
                if role.is_empty() {
                    role = string(&v, "role");
                }
                body = text(&v["message"]["content"]);
                if body.is_empty() {
                    body = string(&v, "text");
                }
                projection = v["sourceEventType"] == "initial_prompt";
            }
            "pi" => match kind {
                "session" => {
                    session.native_id = string(&v, "id");
                    session.cwd = string(&v, "cwd");
                    session.parent = string(&v, "parentSession");
                    if v["version"].as_u64().is_some_and(|x| !(1..=3).contains(&x)) {
                        skipped
                            .entry("unsupported Pi session version".into())
                            .or_insert((0, line))
                            .0 += 1;
                    }
                }
                "session_info" => session.title = string(&v, "name"),
                "message" => {
                    role = string(&v["message"], "role");
                    body = text(&v["message"]["content"]);
                }
                "compaction" | "branch_summary" => {
                    role = "summary".into();
                    body = string(&v, "summary");
                }
                _ => {}
            },
            "claude" => {
                for (key, target) in [
                    ("sessionId", &mut session.native_id),
                    ("cwd", &mut session.cwd),
                ] {
                    if let Some(s) = v[key].as_str() {
                        *target = s.into();
                    }
                }
                session.child |= v["isSidechain"].as_bool() == Some(true);
                if session.child {
                    let agent = string(&v, "agentId");
                    if !agent.is_empty() {
                        session.identity_scope = format!("child:{agent}");
                        session.parent = session.native_id.clone();
                    }
                }
                if kind == "custom-title" {
                    session.title = string(&v, "customTitle");
                }
                if matches!(kind, "user" | "assistant") && v["isMeta"].as_bool() != Some(true) {
                    role = kind.into();
                    body = text(&v["message"]["content"]);
                }
            }
            "codex" => {
                let p = &v["payload"];
                match kind {
                    "session_meta" => {
                        let id = string(p, "id");
                        let first = session.native_id.is_empty();
                        if !first && id != session.native_id && !inherited.contains(&id) {
                            bail!("Mixed native session identities in one transcript");
                        }
                        // Fork rollouts contain ancestor headers and history. Only
                        // explicitly linked ancestors may follow the owning header.
                        for field in ["parent_thread_id", "forked_from_id"] {
                            let parent = string(p, field);
                            if !parent.is_empty() {
                                inherited.insert(parent);
                            }
                        }
                        if first {
                            session.native_id = id;
                            session.cwd = string(p, "cwd");
                            session.parent = string(p, "parent_thread_id");
                            if session.parent.is_empty() {
                                session.parent = string(p, "forked_from_id");
                            }
                            session.child = !session.parent.is_empty() || p["source"].is_object();
                        }
                    }
                    "response_item" if p["type"] == "message" => {
                        role = string(p, "role");
                        body = text(&p["content"]);
                    }
                    "compacted" => {
                        role = "summary".into();
                        body = string(p, "message");
                    }
                    "event_msg"
                        if matches!(p["type"].as_str(), Some("user_message" | "agent_message")) =>
                    {
                        role = if p["type"] == "user_message" {
                            "user"
                        } else {
                            "assistant"
                        }
                        .into();
                        body = string(p, "message");
                        projection = true;
                    }
                    _ => {}
                }
            }
            _ => bail!("Unsupported adapter"),
        }
        let current_identity = session_reference(&session);
        if !identity.is_empty() && identity != current_identity {
            bail!("Mixed native session identities in one transcript");
        }
        if !matches!(role.as_str(), "user" | "assistant" | "summary") || body.trim().is_empty() {
            continue;
        }
        if session.native_id.is_empty() {
            bail!("Message precedes native session identity");
        }
        if identity.is_empty() {
            identity = current_identity;
        }
        if identity != session_reference(&session) {
            bail!("Mixed native session identities in one transcript");
        }
        if let Some(t) = v["timestamp"].as_str()
            && let Ok(t) = chrono::DateTime::parse_from_rfc3339(t)
        {
            let t = t.with_timezone(&chrono::Utc).to_rfc3339();
            if t > session.activity {
                session.activity = t;
            }
        }
        if session.title.is_empty() && role == "user" {
            session.title = body.chars().take(140).collect();
        }
        let native = v["id"]
            .as_str()
            .or(v["uuid"].as_str())
            .or(v["payload"]["id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("line:{line}"));
        let parent = v["parentId"]
            .as_str()
            .or(v["parentUuid"].as_str())
            .unwrap_or("")
            .to_owned();
        let mut start = 0;
        let mut chunk = 0;
        while start < body.len() {
            let mut end = (start + CHUNK).min(body.len());
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            emit(Message {
                reference: if session.identity_scope.is_empty() {
                    reference(&[&source.id, &session.native_id, &native, &chunk.to_string()])
                } else {
                    reference(&[
                        &source.id,
                        &session.native_id,
                        &session.identity_scope,
                        &native,
                        &chunk.to_string(),
                    ])
                },
                native_id: native.clone(),
                parent: parent.clone(),
                role: role.clone(),
                text: body[start..end].into(),
                line,
                chunk,
                projection,
            })?;
            if end == body.len() {
                break;
            }
            start = end - 512;
            while !body.is_char_boundary(start) {
                start += 1;
            }
            chunk += 1;
        }
    }
    if !nonblank {
        return Err(EmptyTranscript.into());
    }
    if session.native_id.is_empty() {
        bail!("Missing native session identity");
    }
    if !identity.is_empty() && identity != session_reference(&session) {
        bail!("Native identity changed after indexed messages");
    }
    session.workspace = std::fs::canonicalize(&session.cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| session.cwd.clone());
    session.reference = session_reference(&session);
    if session.title.is_empty() {
        session.title = "Untitled session".into();
    }
    for (reason, (count, first_line)) in skipped {
        session.warnings.push(format!(
            "{count} record{} skipped: {reason} (first at line {first_line})",
            if count == 1 { "" } else { "s" }
        ));
    }
    if session.activity.is_empty() {
        session.time_fallback = true;
        session.activity =
            chrono::DateTime::<chrono::Utc>::from(path.metadata()?.modified()?).to_rfc3339();
    }
    Ok(session)
}
