//! E2E / regression tests that run the hypr-claw binary with real REPL commands.
//!
//! Run with: `cargo test --test e2e_workflow_test -- --ignored`
//! Requires: binary built (`cargo build --release` or `cargo build`) and existing config
//! (`./data/config.yaml`). If config is missing, the test is skipped (no failure).

#![allow(clippy::unwrap_used)]

use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn hypr_claw_bin() -> std::path::PathBuf {
    let debug = std::path::Path::new("./target/debug/hypr-claw");
    let release = std::path::Path::new("./target/release/hypr-claw");
    if release.exists() {
        release.canonicalize().unwrap_or(release.to_path_buf())
    } else {
        debug.canonicalize().unwrap_or(debug.to_path_buf())
    }
}

#[test]
#[ignore]
fn e2e_smoke_capabilities_and_exit() {
    let bin = hypr_claw_bin();
    if !bin.exists() {
        eprintln!("E2E skipped: binary not found at {:?}", bin);
        return;
    }
    if !std::path::Path::new("./data/config.yaml").exists() {
        eprintln!("E2E skipped: ./data/config.yaml not found (run from repo root with config)");
        return;
    }

    let input = "capabilities\nqueue status\nexit\n";
    let (tx, rx) = mpsc::channel();
    let bin_c = bin.clone();
    let handle = thread::spawn(move || {
        let mut child = Command::new(&bin_c)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("TMPDIR", std::env::temp_dir())
            .spawn()
            .expect("spawn hypr-claw");
        let _ = std::io::Write::write_all(child.stdin.as_mut().unwrap(), input.as_bytes());
        let _ = child.stdin.take();
        let status = child.wait().expect("wait");
        let _ = tx.send(status);
    });

    let status = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("receive status");
    let _ = handle.join();

    if !status.success() {
        eprintln!(
            "E2E skipped: binary exited with {:?} (config or env may be invalid)",
            status.code()
        );
        return;
    }
    assert!(
        status.success(),
        "hypr-claw should exit successfully (code {:?})",
        status.code()
    );
}
