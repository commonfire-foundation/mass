use std::{
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn version_matches_project_manifest() {
    let output = Command::new(env!("CARGO_BIN_EXE_mass"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("mass {}", mass::VERSION)
    );
}
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/cli-tests")
            .join(format!(
                "{}-{} space",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        let config = format!(
            "[[sources]]\nid='synthetic'\nagent='pi'\nroot={:?}\n",
            root.join("sessions").to_str().unwrap()
        );
        std::fs::write(root.join("config.toml"), config).unwrap();
        let lines = [
            serde_json::json!({"type":"session","id":"cli-session","cwd":root}),
            serde_json::json!({"type":"message","id":"entry","message":{"role":"user","content":"quoted phrase café callback"}}),
        ];
        std::fs::write(
            root.join("sessions/test.jsonl"),
            lines
                .iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        Self { root }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mass"))
            .args([
                "--config",
                self.root.join("config.toml").to_str().unwrap(),
                "--db",
                self.root.join("index.sqlite3").to_str().unwrap(),
            ])
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn redirected_bare_and_missing_index() {
    let f = Fixture::new();
    assert_eq!(f.run(&[]).status.code(), Some(2));
    let o = f.run(&["search", "callback", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).contains("No index yet"));
    assert_eq!(
        f.run(&["search", "a", "--limit", "0"]).status.code(),
        Some(2)
    );
}
#[test]
fn explicit_export_preserves_existing_destinations() {
    let f = Fixture::new();
    assert!(f.run(&["index"]).status.success());
    let page: serde_json::Value =
        serde_json::from_slice(&f.run(&["search", "callback", "--json"]).stdout).unwrap();
    let hit = &page["results"][0];
    let reference = hit["session"]["reference"].as_str().unwrap();
    let destination = f.root.join("conversation export.json");
    let args = [
        "export",
        reference,
        "--format",
        "json",
        "--message",
        hit["message_ref"].as_str().unwrap(),
        "--output",
        destination.to_str().unwrap(),
    ];
    assert!(f.run(&args).status.success());
    let bytes = std::fs::read(&destination).unwrap();
    let exported: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["messages"][0]["text"],
        "quoted phrase café callback"
    );
    assert!(exported["session"].get("source_path").is_none());
    assert!(!f.run(&args).status.success());
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_eq!(f.run(&["export", reference]).status.code(), Some(2));
}
#[test]
fn json_search_show_resume_and_spaces() {
    let f = Fixture::new();
    assert!(f.run(&["index", "--json"]).status.success());
    assert_eq!(f.run(&["search", "unknown:value"]).status.code(), Some(2));
    assert_eq!(f.run(&["search", "\"unterminated"]).status.code(), Some(2));
    let o = f.run(&[
        "search",
        "\"quoted phrase\" café",
        "--project",
        f.root.to_str().unwrap(),
        "--json",
    ]);
    assert!(o.status.success(), "{:?}", o);
    assert!(!o.stdout.contains(&27));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["version"], 1);
    let hit = &v["results"][0];
    let r = hit["session"]["reference"].as_str().unwrap();
    let at = hit["message_ref"].as_str().unwrap();
    let o = f.run(&["show", r, "--at", at, "--json"]);
    assert!(o.status.success());
    let shown: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(shown["messages"][0]["reference"], at);
    let o = f.run(&["resume", r, "--dry-run"]);
    assert!(o.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()["supported"],
        false
    );
    assert_eq!(f.run(&["resume", r, "--yes"]).status.code(), Some(1));
    let o = f.run(&["search", "notfoundxyz", "--json"]);
    assert!(o.status.success());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
