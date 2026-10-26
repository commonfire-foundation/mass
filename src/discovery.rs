//! Shared, cancellable filename discovery for indexing and first-run consent.
use crate::config::Source;
use anyhow::{Result, bail};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub struct Discovery {
    pub paths: Vec<PathBuf>,
    pub warnings: Vec<String>,
}
pub fn scan(source: &Source, exclusions: &[String], cancel: &AtomicBool) -> Result<Discovery> {
    let mut found = Discovery::default();
    let check = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            bail!("Discovery cancelled");
        }
        Ok(())
    };
    check()?;
    if !source.root.is_dir() || source.root.is_symlink() {
        found
            .warnings
            .push(format!("{}: source unavailable", source.id));
        return Ok(found);
    }
    for entry in walkdir::WalkDir::new(&source.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !cancel.load(Ordering::Relaxed)
                && !exclusions
                    .iter()
                    .any(|x| e.path().to_string_lossy().contains(x))
        })
    {
        check()?;
        match entry {
            Ok(e) if e.file_type().is_file() => {
                let p = e.path();
                let name = p.to_string_lossy();
                if name.ends_with(".jsonl") || name.ends_with(".jsonl.zst") {
                    found.paths.push(p.to_owned());
                }
            }
            Ok(_) => {}
            Err(e) => found.warnings.push(format!("{}: {e}", source.id)),
        }
    }
    check()?;
    found.paths.sort();
    // Only an eligible plain file shadows a compressed copy, not a symlink or
    // an excluded path that will never be read.
    let plain = found
        .paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .cloned()
        .collect::<HashSet<_>>();
    found.paths.retain(|p| {
        !p.to_string_lossy().ends_with(".zst") || !plain.contains(&p.with_extension(""))
    });
    check()?;
    Ok(found)
}
