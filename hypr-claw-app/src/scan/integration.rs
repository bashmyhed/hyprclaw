use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Notify;

use crate::scan::parsers::{
    partition_results, GitParser, HyprlandParser, ParserRegistry, ShellParser,
};
use crate::scan::*;

const MAX_CONFIG_PARSE_CANDIDATES: usize = 24;
const PACKAGE_SAMPLE_LIMIT: usize = 80;

/// Run integrated system scan with new scan engine
pub async fn run_integrated_scan(
    user_id: &str,
    deep_scan: bool,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Start with basic system profile
    let mut profile = collect_basic_system_profile(user_id).await;

    if deep_scan {
        // Discover home structure
        let user_dirs = UserDirectories::discover();
        let discovered = discover_home_structure(&user_dirs.home);

        println!("\n🏠 Discovered {} directories in home", discovered.len());

        // Build policy interactively
        let policy = ScanPolicy::build_interactively(&discovered)?;
        println!(
            "⚡ Smart scan mode: depth={}, max_files={}, max_dirs={}, per_dir_entries={}",
            policy.standard_depth,
            policy.max_files_total,
            policy.max_dirs_total,
            policy.max_entries_per_dir
        );

        // Calibrate resources
        let monitor = ResourceMonitor::auto_calibrate();
        monitor.print_calibration();

        // Run scan
        println!("\n🔎 Starting deep scan...");
        let interrupt = Arc::new(Notify::new());

        let mut scan_results = Vec::new();
        for path in &policy.included_paths {
            if !path.exists() {
                continue;
            }
            let result = scan_directory(path, &policy, &monitor, interrupt.clone()).await?;
            scan_results.push(result);
        }

        // Aggregate results
        let total_files: usize = scan_results.iter().map(|r| r.stats.files_scanned).sum();
        let total_dirs: usize = scan_results.iter().map(|r| r.stats.dirs_scanned).sum();
        let total_bytes: u64 = scan_results.iter().map(|r| r.stats.bytes_processed).sum();

        println!("\n✅ Scan complete!");
        println!("  Files: {}", total_files);
        println!("  Directories: {}", total_dirs);
        println!("  Size: {} MB", total_bytes / 1024 / 1024);

        // Build deep scan data with config parsing
        let deep_data = build_deep_scan_data(&scan_results, &user_dirs).await;
        if let Some(obj) = profile.as_object_mut() {
            obj.insert("deep_scan".to_string(), deep_data);
        }
    }

    Ok(profile)
}

async fn collect_basic_system_profile(user_id: &str) -> Value {
    let os_release = tokio::fs::read_to_string("/etc/os-release")
        .await
        .unwrap_or_default();
    let distro_name = parse_os_release_value(&os_release, "PRETTY_NAME")
        .or_else(|| parse_os_release_value(&os_release, "NAME"))
        .unwrap_or_else(|| "Unknown Linux".to_string());
    let distro_id =
        parse_os_release_value(&os_release, "ID").unwrap_or_else(|| "unknown".to_string());
    let distro_version = parse_os_release_value(&os_release, "VERSION_ID").unwrap_or_default();

    let kernel = command_output("uname", &["-r"])
        .await
        .unwrap_or_else(|| "unknown".to_string());
    let hostname = command_output("hostname", &[])
        .await
        .unwrap_or_else(|| "unknown".to_string());

    let hyprland_available = command_exists("hyprctl").await;
    let active_workspace = if hyprland_available {
        command_output("hyprctl", &["activeworkspace", "-j"])
            .await
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|json| json.get("id").and_then(|v| v.as_u64()))
            .unwrap_or(0)
    } else {
        0
    };

    let home = std::env::var("HOME").unwrap_or_default();

    json!({
        "scanned_at": chrono::Utc::now().timestamp(),
        "platform": {
            "distro_name": distro_name,
            "distro_id": distro_id,
            "distro_version": distro_version,
            "kernel": kernel,
            "arch": std::env::consts::ARCH
        },
        "user": {
            "id": user_id,
            "name": std::env::var("USER").unwrap_or_else(|_| user_id.to_string()),
            "home": home.clone(),
            "shell": std::env::var("SHELL").unwrap_or_default(),
            "hostname": hostname
        },
        "desktop": {
            "session": std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
            "desktop_env": std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            "hyprland_available": hyprland_available,
            "active_workspace": active_workspace,
        },
        "paths": {
            "home": home,
        }
    })
}

async fn build_deep_scan_data(scan_results: &[ScanResult], user_dirs: &UserDirectories) -> Value {
    let mut config_files = Vec::new();
    let mut script_files = Vec::new();
    let mut source_files = Vec::new();
    let mut project_dirs = Vec::new();

    // Collect files by type
    for result in scan_results {
        for file in &result.scanned_files {
            match &file.file_class {
                FileClass::Config { .. } => {
                    config_files.push(file.path.clone());
                }
                FileClass::Script { language } => {
                    script_files.push(json!({
                        "path": file.path.to_string_lossy(),
                        "language": language,
                        "size": file.size
                    }));
                }
                FileClass::Source { language } => {
                    source_files.push(json!({
                        "path": file.path.to_string_lossy(),
                        "language": language,
                        "size": file.size
                    }));
                }
                _ => {}
            }
        }
    }

    // Find project directories
    let discovered = discover_home_structure(&user_dirs.home);
    for dir in discovered {
        if dir.category == DirectoryCategory::Projects {
            project_dirs.push(dir.path.to_string_lossy().to_string());
        }
    }

    // Parse configs
    let parse_candidates = select_parse_candidates(&config_files);
    println!(
        "\n📝 Parsing prioritized configs: {} selected out of {} discovered...",
        parse_candidates.len(),
        config_files.len()
    );
    let mut registry = ParserRegistry::new();
    registry.register(Box::new(HyprlandParser));
    registry.register(Box::new(ShellParser));
    registry.register(Box::new(GitParser));

    let parse_results = registry.parse_all(&parse_candidates);
    let (parsed, failed) = partition_results(parse_results);
    let packages = collect_package_inventory().await;

    // Show parse results
    if !parsed.is_empty() {
        println!("  ✅ Parsed: {} configs", parsed.len());
    }
    if !failed.is_empty() {
        println!("  ⚠️  Failed: {} configs", failed.len());
        for (_, error) in failed.iter().take(3) {
            println!("    - {}", error.user_message());
        }
        if failed.len() > 3 {
            println!("    ... and {} more", failed.len() - 3);
        }
    }

    // Serialize parsed configs
    let parsed_configs: Vec<Value> = parsed
        .iter()
        .map(|p| {
            json!({
                "path": p.path.to_string_lossy(),
                "type": format!("{:?}", p.config_type),
                "data": p.data,
                "parse_time_ms": p.parse_time_ms,
            })
        })
        .collect();

    json!({
        "scanned_at": chrono::Utc::now().timestamp(),
        "config_files_found": config_files.len(),
        "script_files": script_files.into_iter().take(50).collect::<Vec<_>>(),
        "source_files": source_files.into_iter().take(100).collect::<Vec<_>>(),
        "project_roots": project_dirs.into_iter().take(50).collect::<Vec<_>>(),
        "packages": packages,
        "parsed_configs": parsed_configs,
        "parse_stats": {
            "total": parse_candidates.len(),
            "discovered_total": config_files.len(),
            "parsed": parsed.len(),
            "failed": failed.len(),
        }
    })
}

fn select_parse_candidates(config_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut ranked = config_files
        .iter()
        .filter_map(|path| {
            let priority = parse_priority(path)?;
            if !seen.insert(path.clone()) {
                return None;
            }
            Some((priority, path.clone()))
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    ranked
        .into_iter()
        .take(MAX_CONFIG_PARSE_CANDIDATES)
        .map(|(_, path)| path)
        .collect()
}

fn parse_priority(path: &Path) -> Option<u8> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    let path_str = path.to_string_lossy();

    // Highest-value desktop and shell configs.
    if name == "hyprland.conf" && path_str.contains("/.config/hypr/") {
        return Some(0);
    }
    if matches!(
        name,
        ".bashrc"
            | ".bash_profile"
            | ".bash_logout"
            | ".zshrc"
            | ".zshenv"
            | ".zprofile"
            | ".profile"
            | ".gitconfig"
            | "config.fish"
    ) {
        return Some(1);
    }
    if name == "config" && path_str.contains("/.config/git/") {
        return Some(1);
    }

    // Additional Hyprland fragments.
    if name.ends_with(".conf") && path_str.contains("/.config/hypr/") {
        return Some(2);
    }

    // Skip parsing generic config files in onboarding deep scan.
    None
}

fn parse_os_release_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(&format!("{}=", key)) {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

async fn collect_package_inventory() -> Value {
    let mut sources = Vec::new();
    let mut total_count = 0usize;
    let mut pacman_explicit_count = 0usize;
    let mut aur_count = 0usize;

    if command_exists("pacman").await {
        let (count, sample) = command_output_lines("pacman", &["-Qq"], PACKAGE_SAMPLE_LIMIT).await;
        if count > 0 || !sample.is_empty() {
            total_count += count;
            pacman_explicit_count = count;
            sources.push(json!({
                "manager": "pacman",
                "count": count,
                "sample": sample,
            }));
        }

        let (aur_pkg_count, aur_sample) =
            command_output_lines("pacman", &["-Qm"], PACKAGE_SAMPLE_LIMIT / 2).await;
        if aur_pkg_count > 0 || !aur_sample.is_empty() {
            aur_count = aur_pkg_count;
            sources.push(json!({
                "manager": "aur",
                "count": aur_pkg_count,
                "sample": aur_sample,
            }));
        }
    }

    if command_exists("flatpak").await {
        let (count, sample) = command_output_lines(
            "flatpak",
            &["list", "--app", "--columns=application"],
            PACKAGE_SAMPLE_LIMIT / 2,
        )
        .await;
        if count > 0 || !sample.is_empty() {
            total_count += count;
            sources.push(json!({
                "manager": "flatpak",
                "count": count,
                "sample": sample,
            }));
        }
    }

    if command_exists("dpkg-query").await {
        let (count, sample) = command_output_lines(
            "dpkg-query",
            &["-W", "-f=${binary:Package}\n"],
            PACKAGE_SAMPLE_LIMIT,
        )
        .await;
        if count > 0 || !sample.is_empty() {
            total_count += count;
            sources.push(json!({
                "manager": "dpkg",
                "count": count,
                "sample": sample,
            }));
        }
    } else if command_exists("rpm").await {
        let (count, sample) = command_output_lines("rpm", &["-qa"], PACKAGE_SAMPLE_LIMIT).await;
        if count > 0 || !sample.is_empty() {
            total_count += count;
            sources.push(json!({
                "manager": "rpm",
                "count": count,
                "sample": sample,
            }));
        }
    }

    json!({
        "total_count": total_count,
        "pacman_explicit_count": pacman_explicit_count,
        "aur_count": aur_count,
        "sources": sources,
    })
}

async fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new(command)
        .args(args)
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

async fn command_output_lines(
    command: &str,
    args: &[&str],
    sample_limit: usize,
) -> (usize, Vec<String>) {
    let output = match tokio::process::Command::new(command)
        .args(args)
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        _ => return (0, Vec::new()),
    };

    let rows = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    let count = rows.len();
    let sample = rows.into_iter().take(sample_limit).collect::<Vec<_>>();
    (count, sample)
}

async fn command_exists(command: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(command)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
