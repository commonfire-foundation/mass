use crate::{
    config, index, reader, resume,
    search::{self, Filters},
};
use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::atomic::AtomicBool,
};
#[derive(Parser)]
#[command(version,about="Find, read, and reopen local agent conversations", color=clap::ColorChoice::Never)]
pub struct Cli {
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    Tui {
        #[arg(default_value = "")]
        query: String,
    },
    Search {
        query: String,
        #[command(flatten)]
        filters: Filters,
        #[arg(long)]
        json: bool,
    },
    Show {
        reference: String,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Export indexed conversation text (not a native harness backup).
    Export {
        reference: String,
        #[arg(long, value_enum, default_value = "markdown")]
        format: crate::export::Format,
        #[arg(long)]
        output: PathBuf,
        /// Export the complete message containing this chunk reference.
        #[arg(long)]
        message: Option<String>,
        /// Include source/cwd paths. Conversation text itself is never path-redacted.
        #[arg(long)]
        include_paths: bool,
    },
    Resume {
        reference: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    Index {
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
}
fn output(value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(io::stdout().lock(), value)?;
    println!();
    Ok(())
}
pub fn run(cli: Cli) -> Result<i32> {
    if cli.command.is_none() && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        Cli::command().print_help()?;
        println!();
        return Ok(2);
    }
    let dbpath = cli
        .db
        .unwrap_or_else(|| config::xdg("XDG_CACHE_HOME", ".cache").join("index.sqlite3"));
    let config = config::load(cli.config.as_deref())?;
    match cli.command.unwrap_or(Command::Tui {
        query: String::new(),
    }) {
        Command::Index { json } => {
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let signal = cancel.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            let c = index::refresh(&dbpath, &config, &cancel, |s| {
                eprintln!("{}", crate::clean(&s))
            })?;
            if json {
                output(&serde_json::json!({"version":1,"coverage":c}))?;
            } else {
                println!(
                    "{} indexed · {} unchanged · {} removed",
                    c.indexed, c.unchanged, c.removed
                );
                for w in &c.warnings {
                    eprintln!("{}", crate::clean(w));
                }
            }
            return Ok(if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                130
            } else if c.warnings.is_empty() {
                0
            } else {
                1
            });
        }
        Command::Status { json } => {
            let coverage = if dbpath.is_file() {
                Some(index::coverage(&index::open(&dbpath, false)?)?)
            } else {
                None
            };
            let sources=config.sources.iter().map(|s|serde_json::json!({"id":s.id,"agent":s.agent,"root":s.root,"available":s.root.is_dir(),"resume":s.resume})).collect::<Vec<_>>();
            let v = serde_json::json!({"version":1,"index":dbpath,"coverage":coverage,"sources":sources,"privacy":"Disposable sensitive plaintext cache; secure deletion is not guaranteed"});
            if json {
                output(&v)?;
            } else {
                println!("{}", crate::clean(&serde_json::to_string_pretty(&v)?));
            }
        }
        Command::Search {
            query,
            filters,
            json,
        } => {
            let page = search::query(&index::open(&dbpath, false)?, &query, &filters)?;
            if json {
                output(&page)?;
            } else {
                println!(
                    "{} sessions · last successful index: {}",
                    page.results.len(),
                    page.coverage.last_success
                );
                for hit in page.results {
                    println!(
                        "\n{}  {}\n    {} · {} · {}{}\n    {}",
                        hit.session.reference,
                        crate::clean(&hit.session.title).replace('\n', " "),
                        hit.session.agent,
                        crate::clean(&hit.session.cwd),
                        hit.session.activity,
                        if hit.session.time_fallback {
                            " (filesystem time)"
                        } else {
                            ""
                        },
                        crate::clean(&hit.excerpt).replace('\n', " ")
                    );
                }
                if let Some(next) = page.next_cursor {
                    println!("\nMore results: --cursor {next}");
                }
                for w in page.coverage.warnings {
                    eprintln!("Coverage: {}", crate::clean(&w));
                }
            }
        }
        Command::Show {
            reference,
            at,
            json,
        } => {
            let connection = index::open(&dbpath, false)?;
            let db = connection.unchecked_transaction()?;
            let session = reader::resolve(&db, &reference)?;
            if let Some(at) = at.as_deref() {
                let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM messages m JOIN files f ON f.key=m.file WHERE f.ref=?1 AND m.ref=?2)",[&session.reference,at],|r|r.get(0))?;
                if !valid {
                    bail!("Message does not belong to this session");
                }
            }
            let mut stdout = io::stdout().lock();
            if json {
                write!(
                    stdout,
                    "{{\"version\":1,\"session\":{},\"messages\":[",
                    serde_json::to_string(&session)?
                )?;
                let mut first = true;
                reader::stream(&db, &reference, at.as_deref(), |m| {
                    if !first {
                        write!(stdout, ",")?;
                    }
                    first = false;
                    serde_json::to_writer(&mut stdout, &m)?;
                    Ok(())
                })?;
                writeln!(stdout, "]}}")?;
            } else {
                writeln!(
                    stdout,
                    "{}\n{}",
                    crate::clean(&session.title),
                    crate::clean(&session.cwd)
                )?;
                reader::stream(&db, &reference, at.as_deref(), |m| {
                    writeln!(
                        stdout,
                        "\n{} · {} · line {} / chunk {}\n{}",
                        m.role,
                        m.reference,
                        m.line,
                        m.chunk,
                        crate::clean_text(&m.text)
                    )?;
                    Ok(())
                })?;
            }
        }
        Command::Export {
            reference,
            format,
            output,
            message,
            include_paths,
        } => {
            let db = index::open(&dbpath, false)?;
            crate::export::to_file(
                &db,
                &reference,
                message.as_deref(),
                format,
                include_paths,
                &output,
            )?;
            eprintln!(
                "Exported indexed text to {}. Review it for secrets before sharing; this is not a native session backup.",
                crate::clean(&output.to_string_lossy())
            );
        }
        Command::Resume {
            reference,
            dry_run,
            yes,
        } => {
            let db = index::open(&dbpath, false)?;
            let s = reader::resolve(&db, &reference)?;
            let p = resume::plan(&s, &config)?;
            if dry_run {
                output(&p)?;
            } else {
                return resume::launch(&p, yes, || {
                    let config = config::load(cli.config.as_deref())?;
                    let current = reader::resolve(&db, &reference)?;
                    resume::plan(&current, &config)
                });
            }
        }
        Command::Tui { query } => {
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                bail!("TUI requires a terminal");
            }
            return crate::tui::run(dbpath, config, query, cli.config);
        }
    }
    Ok(0)
}
