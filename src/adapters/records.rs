//! Stream JSONL, discarding unneeded payloads before allocation. The retained
//! JSON budget bounds searchable text/metadata, not base64 image attachments.
use anyhow::{Result, bail};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::{
    cell::Cell,
    fmt,
    io::{self, BufRead, Read},
    sync::atomic::{AtomicBool, Ordering},
};
const LIMIT: usize = 16 * 1024 * 1024;
struct Budget {
    used: Cell<usize>,
    keeping: Cell<bool>,
    exceeded: Cell<bool>,
}
struct Line<'a> {
    input: &'a mut dyn BufRead,
    budget: &'a Budget,
    cancel: &'a AtomicBool,
    ended: bool,
    nonblank: bool,
}
impl Read for Line<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.ended || out.is_empty() {
            return Ok(0);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("Indexing cancelled"));
        }
        let buf = self.input.fill_buf()?;
        let n = buf.len().min(out.len());
        let n = buf[..n]
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(n);
        if self.budget.keeping.get() {
            let used = self.budget.used.get().saturating_add(n);
            if used > LIMIT {
                self.budget.exceeded.set(true);
                return Err(io::Error::other("Retained JSON exceeds 16 MiB"));
            }
            self.budget.used.set(used);
        }
        self.ended = n == 0 || buf[n - 1] == b'\n';
        self.nonblank |= buf[..n].iter().any(|b| !b.is_ascii_whitespace());
        out[..n].copy_from_slice(&buf[..n]);
        self.input.consume(n);
        Ok(n)
    }
}
struct Projection<'a>(&'a Budget);
impl<'de> DeserializeSeed<'de> for Projection<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        d: D,
    ) -> std::result::Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Projection<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON transcript record")
    }
    fn visit_unit<E>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_i64<E>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E>(self, v: f64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_str<E>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_string<E>(self, v: String) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = a.next_element_seed(Projection(self.0))? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if matches!(
                key.as_str(),
                "type"
                    | "id"
                    | "cwd"
                    | "parentSession"
                    | "version"
                    | "name"
                    | "message"
                    | "summary"
                    | "sessionId"
                    | "isSidechain"
                    | "customTitle"
                    | "isMeta"
                    | "agentId"
                    | "timestamp"
                    | "uuid"
                    | "parentId"
                    | "parentUuid"
                    | "payload"
                    | "runId"
                    | "childIndex"
                    | "recordType"
                    | "sourceEventType"
                    | "role"
                    | "text"
                    | "content"
                    | "parent_thread_id"
                    | "forked_from_id"
                    | "source"
            ) {
                out.insert(key, a.next_value_seed(Projection(self.0))?);
            } else {
                self.0.keeping.set(false);
                let result = a.next_value::<IgnoredAny>();
                self.0.keeping.set(true);
                result?;
            }
        }
        Ok(Value::Object(out))
    }
}
pub enum Record {
    Value(Value),
    Skipped(&'static str),
    Blank,
}
pub fn next(input: &mut dyn BufRead, cancel: &AtomicBool) -> Result<Option<Record>> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Parsing cancelled");
    }
    if input.fill_buf()?.is_empty() {
        return Ok(None);
    }
    let budget = Budget {
        used: Cell::new(0),
        keeping: Cell::new(true),
        exceeded: Cell::new(false),
    };
    let mut line = Line {
        input,
        budget: &budget,
        cancel,
        ended: false,
        nonblank: false,
    };
    let result = {
        let mut d = serde_json::Deserializer::from_reader(&mut line);
        Projection(&budget)
            .deserialize(&mut d)
            .and_then(|v| d.end().map(|_| v))
    };
    if cancel.load(Ordering::Relaxed) {
        bail!("Indexing cancelled");
    }
    budget.keeping.set(false);
    io::copy(&mut line, &mut io::sink())?;
    if !line.nonblank {
        return Ok(Some(Record::Blank));
    }
    Ok(Some(match result {
        Ok(v) => Record::Value(v),
        Err(_) if budget.exceeded.get() => Record::Skipped("retained text/metadata exceeds 16 MiB"),
        Err(e) if e.is_io() => return Err(e.into()),
        Err(_) => Record::Skipped("invalid JSON"),
    }))
}
