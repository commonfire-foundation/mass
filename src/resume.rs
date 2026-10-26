use crate::{adapters, config::Config, models::Session};
use anyhow::{Result, bail};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{self, IsTerminal, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Plan {
    pub version: u32,
    pub reference: String,
    pub executable: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
    pub supported: bool,
    pub warnings: Vec<String>,
}
pub fn plan(session: &Session, config: &Config) -> Result<Plan> {
    plan_cancellable(session, config, &AtomicBool::new(false))
}
pub fn plan_cancellable(session: &Session, config: &Config, cancel: &AtomicBool) -> Result<Plan> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Resume review cancelled");
    }
    let mut p=Plan{version:1,reference:session.reference.clone(),executable:session.agent.clone(),argv:vec![],cwd:session.cwd.clone(),environment:BTreeMap::new(),supported:true,warnings:vec!["Experimental native reopening: compatibility depends on harness version and configuration. Launching an agent loads its settings, hooks, extensions, and permissions. Live-session status: unknown.".into()]};
    let source = config
        .sources
        .iter()
        .find(|s| s.id == session.source)
        .ok_or_else(|| anyhow::anyhow!("Source is no longer configured"))?;
    let mut block = |s: &str| {
        p.supported = false;
        p.warnings.push(s.into());
    };
    if !source.resume {
        block("Resume disabled for this source; explicitly enable resume in config.toml");
    }
    if source.agent != session.agent {
        block("Configured agent changed; refresh index");
    }
    if session.child {
        block("Child session: reopen its parent instead");
    }
    if session.read_only {
        block("Imported export or ambiguous history copy: read-only; automatic resume is unsafe");
    }
    if session.cwd.is_empty()
        || !Path::new(&session.cwd).is_absolute()
        || !Path::new(&session.cwd).is_dir()
    {
        block("Original working directory unavailable");
    }
    let path = Path::new(&session.path);
    if path.is_symlink() || !path.is_file() {
        block("Source transcript missing or symlinked");
    } else if source.resume && source.agent == session.agent && !session.child && !session.read_only
    {
        let root = source.root.canonicalize()?;
        if !path.canonicalize()?.starts_with(root) {
            block("Source path no longer belongs to the configured store");
        } else {
            let current = adapters::parse_cancellable(path, source, cancel, |_| Ok(()))?;
            if current.reference != session.reference
                || current.cwd != session.cwd
                || current.child != session.child
            {
                block("Transcript identity changed; refresh index");
            }
        }
    }
    if session.agent == "pi" {
        p.argv = vec!["--session".into(), session.path.clone()];
        p.environment.insert(
            "PI_CODING_AGENT_SESSION_DIR".into(),
            source.root.to_string_lossy().into(),
        );
        if source.root.file_name().is_some_and(|s| s == "sessions") {
            p.environment.insert(
                "PI_CODING_AGENT_DIR".into(),
                source.root.parent().unwrap().to_string_lossy().into(),
            );
        } else {
            p.supported = false;
            p.warnings
                .push("Custom Pi store home cannot be inferred safely".into());
        }
    } else {
        if session.native_id.len() != 36
            || !session
                .native_id
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-')
        {
            p.supported = false;
            p.warnings.push("Unsupported native resume ID".into());
        }
        p.argv = vec![
            if session.agent == "claude" {
                "--resume"
            } else {
                "resume"
            }
            .into(),
            session.native_id.clone(),
        ];
        let expected = if session.agent == "claude" {
            "projects"
        } else {
            "sessions"
        };
        if source.root.file_name().is_some_and(|s| s == expected) {
            p.environment.insert(
                if session.agent == "claude" {
                    "CLAUDE_CONFIG_DIR"
                } else {
                    "CODEX_HOME"
                }
                .into(),
                source.root.parent().unwrap().to_string_lossy().into(),
            );
        } else {
            p.supported = false;
            p.warnings
                .push("Archive/custom store: automatic resume is not supported".into());
        }
    }
    let found = std::env::var_os("PATH").and_then(|s| {
        std::env::split_paths(&s)
            .map(|d| d.join(&p.executable))
            .find(|f| executable(f))
            .and_then(|f| f.canonicalize().ok())
    });
    if let Some(executable) = found {
        p.executable = executable.to_string_lossy().into_owned();
    } else {
        p.supported = false;
        p.warnings
            .push(format!("{} is not executable on PATH", p.executable));
    }
    if !source.resume || session.child || session.read_only || source.agent != session.agent {
        p.warnings
            .push("Transcript identity not checked: reopening is already blocked".into());
    }
    if cancel.load(Ordering::Relaxed) {
        bail!("Resume review cancelled");
    }
    Ok(p)
}
fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
/// A command for deliberate copying, never executed through a shell by MASS.
pub fn shell_command(p: &Plan) -> Result<String> {
    if !p.supported {
        bail!("Resume command unavailable for this session");
    }
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
    let mut parts = vec![format!("cd -- {} && env", quote(&p.cwd))];
    parts.extend(
        p.environment
            .iter()
            .map(|(k, v)| quote(&format!("{k}={v}"))),
    );
    parts.push(quote(&p.executable));
    parts.extend(p.argv.iter().map(|s| quote(s)));
    Ok(parts.join(" "))
}
pub fn verify_review(reviewed: &Plan, current: &Plan) -> Result<()> {
    if !current.supported || reviewed != current {
        bail!("Resume changed since review; reopen review to inspect current guards");
    }
    Ok(())
}
pub fn launch(p: &Plan, yes: bool, revalidate: impl FnOnce() -> Result<Plan>) -> Result<i32> {
    if !p.supported {
        bail!("Resume unavailable: {}", p.warnings.join("; "));
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("Resume requires a terminal, even with --yes");
    }
    eprintln!("{}", crate::clean(&serde_json::to_string_pretty(p)?));
    if !yes {
        eprint!("Launch this agent? [y/N] ");
        io::stderr().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            return Ok(130);
        }
    }
    let current = revalidate()?;
    verify_review(p, &current)?;
    let status = std::process::Command::new(&current.executable)
        .args(&p.argv)
        .envs(&p.environment)
        .current_dir(&p.cwd)
        .status()?;
    Ok(if status.success() { 0 } else { 1 })
}
