//! Commit e momento della compilazione per `GET /versione`. Senza git: «sconosciuto».

use std::process::Command;

fn main() {
    let commit = std::env::var("XBOT_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    println!(
        "cargo:rustc-env=XBOT_COMMIT={}",
        commit.unwrap_or_else(|| "sconosciuto".into())
    );
    println!("cargo:rustc-env=XBOT_COMPILATO_MS={ms}");
    println!("cargo:rerun-if-env-changed=XBOT_COMMIT");
    println!("cargo:rerun-if-changed=src");
    // Un commit nuovo cambia logs/HEAD nella cartella git (anche in un worktree).
    if let Some(dir) = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
    {
        let dir = dir.trim();
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/logs/HEAD");
    }
}
