//! Desktop operations - launching apps, browser ops, and GUI automation.

use super::{OsError, OsResult};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::time::{sleep, Duration, Instant};

fn validate_app_name(app: &str) -> OsResult<()> {
    if app.trim().is_empty() {
        return Err(OsError::InvalidArgument("app cannot be empty".to_string()));
    }
    if !app
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ' '))
    {
        return Err(OsError::InvalidArgument(
            "app contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_arg(arg: &str) -> OsResult<()> {
    if arg.contains('\0') || arg.contains('\n') {
        return Err(OsError::InvalidArgument(
            "argument contains invalid control characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_text(text: &str) -> OsResult<()> {
    if text.contains('\0') {
        return Err(OsError::InvalidArgument(
            "text contains null byte".to_string(),
        ));
    }
    Ok(())
}

fn validate_key_token(key: &str) -> OsResult<()> {
    if key.trim().is_empty() {
        return Err(OsError::InvalidArgument("key cannot be empty".to_string()));
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+' | ' '))
    {
        return Err(OsError::InvalidArgument(
            "key contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_coordinate(value: i32, label: &str) -> OsResult<()> {
    if value < 0 {
        return Err(OsError::InvalidArgument(format!(
            "{label} must be >= 0, got {value}"
        )));
    }
    Ok(())
}

fn validate_url(url: &str) -> OsResult<()> {
    if url.trim().is_empty() {
        return Err(OsError::InvalidArgument("url cannot be empty".to_string()));
    }
    let lower = url.to_lowercase();
    if !(lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("file://"))
    {
        return Err(OsError::InvalidArgument(
            "url must start with http://, https://, mailto:, or file://".to_string(),
        ));
    }
    Ok(())
}

fn encode_query(query: &str) -> String {
    query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn command_exists(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn run_checked(command: &str, args: &[&str]) -> OsResult<()> {
    let output = Command::new(command).args(args).output().await?;
    if output.status.success() {
        return Ok(());
    }
    Err(OsError::OperationFailed(
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

async fn run_output(command: &str, args: &[&str]) -> OsResult<String> {
    let output = Command::new(command).args(args).output().await?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    Err(OsError::OperationFailed(
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

fn parse_cursor_position(raw: &str) -> Option<(i32, i32)> {
    if let Ok(json) = serde_json::from_str::<Value>(raw) {
        if let Some(obj) = json.as_object() {
            if let (Some(x), Some(y)) = (obj.get("x"), obj.get("y")) {
                if let (Some(x), Some(y)) = (x.as_f64(), y.as_f64()) {
                    return Some((x.round() as i32, y.round() as i32));
                }
            }
        }
    }

    let mut nums = Vec::<i32>::new();
    let mut token = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_digit() || (token.is_empty() && ch == '-') {
            token.push(ch);
            continue;
        }
        if !token.is_empty() {
            if let Ok(v) = token.parse::<i32>() {
                nums.push(v);
            }
            token.clear();
        }
    }
    if !token.is_empty() {
        if let Ok(v) = token.parse::<i32>() {
            nums.push(v);
        }
    }
    if nums.len() >= 2 {
        Some((nums[0], nums[1]))
    } else {
        None
    }
}

#[derive(Clone, Debug)]
struct DesktopEntryHint {
    desktop_id: String,
    name: String,
    exec_program: Option<String>,
    flatpak_app_id: Option<String>,
}

#[derive(Clone, Debug)]
enum LaunchTarget {
    Command { program: String, args: Vec<String> },
    Flatpak { app_id: String, args: Vec<String> },
    GtkLaunch { desktop_id: String },
}

impl LaunchTarget {
    fn key(&self) -> String {
        match self {
            Self::Command { program, args } => format!("cmd:{program}:{}", args.join("\x1f")),
            Self::Flatpak { app_id, args } => format!("flatpak:{app_id}:{}", args.join("\x1f")),
            Self::GtkLaunch { desktop_id } => format!("gtk:{desktop_id}"),
        }
    }
}

fn canonical_app_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_likely_flatpak_id(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.contains('.') && !trimmed.contains('/') && !trimmed.ends_with(".desktop")
}

fn parse_exec_tokens(exec: &str) -> Vec<String> {
    exec.split_whitespace()
        .map(|part| part.trim_matches('"').trim_matches('\'').to_string())
        .filter(|part| !part.is_empty())
        .filter(|part| !part.starts_with('%'))
        .collect()
}

fn parse_exec_program_and_flatpak(exec: &str) -> (Option<String>, Option<String>) {
    let tokens = parse_exec_tokens(exec);
    if tokens.is_empty() {
        return (None, None);
    }

    let mut idx = 0usize;
    if tokens[idx] == "env" {
        idx += 1;
        while idx < tokens.len() && tokens[idx].contains('=') {
            idx += 1;
        }
    }
    if idx >= tokens.len() {
        return (None, None);
    }

    let program = Some(tokens[idx].clone());
    let flatpak_id = if tokens.get(idx).map(String::as_str) == Some("flatpak")
        && tokens.get(idx + 1).map(String::as_str) == Some("run")
    {
        tokens.get(idx + 2).cloned()
    } else {
        None
    };
    (program, flatpak_id)
}

fn desktop_entry_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".local/share/applications"));
        dirs.push(PathBuf::from(&home).join(".local/share/flatpak/exports/share/applications"));
    }
    dirs
}

fn parse_desktop_entry(file: &Path) -> Option<DesktopEntryHint> {
    let content = std::fs::read_to_string(file).ok()?;
    let mut name = None::<String>;
    let mut exec = None::<String>;
    for line in content.lines().map(str::trim) {
        if line.starts_with("Name=") && name.is_none() {
            name = Some(line.trim_start_matches("Name=").to_string());
            continue;
        }
        if line.starts_with("Exec=") && exec.is_none() {
            exec = Some(line.trim_start_matches("Exec=").to_string());
            continue;
        }
        if line == "NoDisplay=true" {
            return None;
        }
        if name.is_some() && exec.is_some() {
            break;
        }
    }

    let desktop_id = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string();
    if desktop_id.is_empty() {
        return None;
    }

    let exec = exec?;
    let (exec_program, flatpak_app_id) = parse_exec_program_and_flatpak(&exec);
    Some(DesktopEntryHint {
        desktop_id,
        name: name.unwrap_or_default(),
        exec_program,
        flatpak_app_id,
    })
}

fn load_desktop_entry_hints(limit: usize) -> Vec<DesktopEntryHint> {
    let mut files = Vec::<PathBuf>::new();
    for dir in desktop_entry_dirs() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) == Some("desktop") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files.truncate(limit);

    let mut hints = files
        .into_iter()
        .filter_map(|file| parse_desktop_entry(&file))
        .collect::<Vec<_>>();
    hints.sort_by(|a, b| a.name.cmp(&b.name));
    hints
}

fn desktop_entry_hints() -> &'static [DesktopEntryHint] {
    static CACHE: OnceLock<Vec<DesktopEntryHint>> = OnceLock::new();
    CACHE
        .get_or_init(|| load_desktop_entry_hints(900))
        .as_slice()
}

fn match_desktop_hints(app: &str, limit: usize) -> Vec<DesktopEntryHint> {
    let query = canonical_app_key(app);
    if query.is_empty() {
        return Vec::new();
    }

    let mut scored = Vec::<(usize, DesktopEntryHint)>::new();
    for hint in desktop_entry_hints().iter().cloned() {
        let id = canonical_app_key(&hint.desktop_id);
        let name = canonical_app_key(&hint.name);
        let exec = hint
            .exec_program
            .as_deref()
            .map(canonical_app_key)
            .unwrap_or_default();

        let score = if id == query || name == query || exec == query {
            4
        } else if id.contains(&query) || name.contains(&query) || exec.contains(&query) {
            3
        } else if query.contains(&id) || query.contains(&name) || query.contains(&exec) {
            2
        } else {
            0
        };
        if score > 0 {
            scored.push((score, hint));
        }
    }
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.name.cmp(&b.name)));
    scored
        .into_iter()
        .map(|(_, hint)| hint)
        .take(limit)
        .collect()
}

fn alias_candidates(app: &str) -> Vec<String> {
    let mut out = Vec::new();
    let key = canonical_app_key(app);

    let mut push = |value: &str| {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        if !out.iter().any(|existing| existing == value) {
            out.push(value.to_string());
        }
    };

    push(app);
    match key.as_str() {
        "vscode" | "visualstudiocode" | "code" | "codeoss" | "codium" | "vscodium" => {
            for c in [
                "code",
                "code-insiders",
                "code-oss",
                "codium",
                "vscodium",
                "com.visualstudio.code",
                "com.vscodium.codium",
            ] {
                push(c);
            }
        }
        "telegram" | "telegramdesktop" => {
            for c in ["telegram-desktop", "telegram", "org.telegram.desktop"] {
                push(c);
            }
        }
        "firefox" => push("firefox"),
        "chrome" | "googlechrome" => {
            for c in ["google-chrome-stable", "google-chrome", "chromium"] {
                push(c);
            }
        }
        "brave" | "bravebrowser" => push("brave-browser"),
        "kitty" => push("kitty"),
        "alacritty" => push("alacritty"),
        "wezterm" => push("wezterm"),
        _ => {}
    }

    out
}

fn push_target_unique(
    targets: &mut Vec<LaunchTarget>,
    seen: &mut HashSet<String>,
    target: LaunchTarget,
) {
    if seen.insert(target.key()) {
        targets.push(target);
    }
}

fn build_launch_targets(app: &str, args: &[String]) -> Vec<LaunchTarget> {
    let mut targets = Vec::<LaunchTarget>::new();
    let mut seen = HashSet::<String>::new();

    for candidate in alias_candidates(app) {
        if candidate.ends_with(".desktop") {
            let id = candidate.trim_end_matches(".desktop").to_string();
            push_target_unique(
                &mut targets,
                &mut seen,
                LaunchTarget::GtkLaunch { desktop_id: id },
            );
        }
        if is_likely_flatpak_id(&candidate) {
            push_target_unique(
                &mut targets,
                &mut seen,
                LaunchTarget::Flatpak {
                    app_id: candidate.clone(),
                    args: args.to_vec(),
                },
            );
        }
        push_target_unique(
            &mut targets,
            &mut seen,
            LaunchTarget::Command {
                program: candidate,
                args: args.to_vec(),
            },
        );
    }

    for hint in match_desktop_hints(app, 24) {
        push_target_unique(
            &mut targets,
            &mut seen,
            LaunchTarget::GtkLaunch {
                desktop_id: hint.desktop_id.clone(),
            },
        );
        if let Some(program) = hint.exec_program {
            push_target_unique(
                &mut targets,
                &mut seen,
                LaunchTarget::Command {
                    program,
                    args: args.to_vec(),
                },
            );
        }
        if let Some(app_id) = hint.flatpak_app_id {
            push_target_unique(
                &mut targets,
                &mut seen,
                LaunchTarget::Flatpak {
                    app_id,
                    args: args.to_vec(),
                },
            );
        }
    }

    targets
}

/// Launch an app directly.
pub async fn launch_app(app: &str, args: &[String]) -> OsResult<u32> {
    validate_app_name(app)?;
    for arg in args {
        validate_arg(arg)?;
    }
    let targets = build_launch_targets(app.trim(), args);
    if targets.is_empty() {
        return Err(OsError::NotFound(format!(
            "no launch candidates generated for app '{}'",
            app
        )));
    }

    let mut attempted = Vec::<String>::new();
    for target in targets {
        match target {
            LaunchTarget::Command { program, args } => {
                let display = if args.is_empty() {
                    program.clone()
                } else {
                    format!("{} {}", program, args.join(" "))
                };
                if !command_exists(&program).await {
                    attempted.push(format!("{display} (missing in PATH)"));
                    continue;
                }
                match Command::new(&program)
                    .args(args.iter().map(String::as_str))
                    .spawn()
                {
                    Ok(child) => return Ok(child.id().unwrap_or_default()),
                    Err(e) => {
                        attempted.push(format!("{display} ({e})"));
                    }
                }
            }
            LaunchTarget::Flatpak { app_id, args } => {
                if !command_exists("flatpak").await {
                    attempted.push(format!("flatpak run {} (flatpak missing)", app_id));
                    continue;
                }
                match Command::new("flatpak")
                    .arg("run")
                    .arg(&app_id)
                    .args(args.iter().map(String::as_str))
                    .spawn()
                {
                    Ok(child) => return Ok(child.id().unwrap_or_default()),
                    Err(e) => attempted.push(format!("flatpak run {} ({})", app_id, e)),
                }
            }
            LaunchTarget::GtkLaunch { desktop_id } => {
                if !command_exists("gtk-launch").await {
                    attempted.push(format!("gtk-launch {} (gtk-launch missing)", desktop_id));
                    continue;
                }
                match Command::new("gtk-launch").arg(&desktop_id).spawn() {
                    Ok(child) => return Ok(child.id().unwrap_or_default()),
                    Err(e) => attempted.push(format!("gtk-launch {} ({})", desktop_id, e)),
                }
            }
        }
    }

    Err(OsError::OperationFailed(format!(
        "Unable to launch '{}'. Tried: {}",
        app,
        attempted.join(" | ")
    )))
}

/// Open a URL with xdg-open.
pub async fn open_url(url: &str) -> OsResult<()> {
    validate_url(url)?;
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(OsError::Io)?;
    Ok(())
}

/// Search the web with a selected search engine.
pub async fn search_web(query: &str, engine: Option<&str>) -> OsResult<String> {
    let encoded = encode_query(query);
    let target = match engine.unwrap_or("duckduckgo").to_lowercase().as_str() {
        "google" => format!("https://www.google.com/search?q={encoded}"),
        "bing" => format!("https://www.bing.com/search?q={encoded}"),
        _ => format!("https://duckduckgo.com/?q={encoded}"),
    };
    open_url(&target).await?;
    Ok(target)
}

/// Open Gmail in default browser.
pub async fn open_gmail() -> OsResult<()> {
    open_url("https://mail.google.com").await
}

/// Type text into the currently focused window.
pub async fn type_text(text: &str) -> OsResult<()> {
    validate_text(text)?;
    if command_exists("wtype").await {
        return run_checked("wtype", &[text]).await;
    }
    if command_exists("ydotool").await {
        return run_checked("ydotool", &["type", text]).await;
    }
    Err(OsError::OperationFailed(
        "No text input backend found (install 'wtype' or 'ydotool')".to_string(),
    ))
}

/// Press a single key in the focused window.
pub async fn key_press(key: &str) -> OsResult<()> {
    validate_key_token(key)?;
    if !command_exists("wtype").await {
        return Err(OsError::OperationFailed(
            "wtype not found for key presses".to_string(),
        ));
    }
    run_checked("wtype", &["-k", key]).await
}

/// Press a key combination (modifiers + key), e.g. ctrl+l.
pub async fn key_combo(keys: &[String]) -> OsResult<()> {
    if keys.len() < 2 {
        return Err(OsError::InvalidArgument(
            "key_combo requires at least one modifier and one key".to_string(),
        ));
    }
    if !command_exists("wtype").await {
        return Err(OsError::OperationFailed(
            "wtype not found for key combos".to_string(),
        ));
    }

    for key in keys {
        validate_key_token(key)?;
    }

    let main_key = keys.last().cloned().unwrap_or_default();
    let modifiers = &keys[..keys.len() - 1];

    let mut args: Vec<String> = Vec::new();
    for modifier in modifiers {
        args.push("-M".to_string());
        args.push(modifier.to_string());
    }
    args.push("-k".to_string());
    args.push(main_key);
    for modifier in modifiers.iter().rev() {
        args.push("-m".to_string());
        args.push(modifier.to_string());
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_checked("wtype", &arg_refs).await
}

fn parse_mouse_button(button: &str) -> OsResult<&'static str> {
    match button.to_lowercase().as_str() {
        "left" => Ok("1"),
        "middle" => Ok("2"),
        "right" => Ok("3"),
        other => Err(OsError::InvalidArgument(format!(
            "unsupported mouse button: {other}"
        ))),
    }
}

/// Click mouse button in current cursor position.
pub async fn mouse_click(button: &str) -> OsResult<()> {
    let code = parse_mouse_button(button)?;
    if command_exists("ydotool").await {
        return run_checked("ydotool", &["click", code]).await;
    }
    if command_exists("wlrctl").await {
        return run_checked("wlrctl", &["pointer", "click", button]).await;
    }
    Err(OsError::OperationFailed(
        "No click backend found (install 'ydotool' or 'wlrctl')".to_string(),
    ))
}

/// Move cursor to absolute coordinate.
pub async fn mouse_move_absolute(x: i32, y: i32) -> OsResult<()> {
    validate_coordinate(x, "x")?;
    validate_coordinate(y, "y")?;

    let xs = x.to_string();
    let ys = y.to_string();

    if command_exists("wlrctl").await {
        return run_checked("wlrctl", &["pointer", "move", &xs, &ys]).await;
    }
    if command_exists("ydotool").await {
        // ydotool mousemove supports absolute mode on newer versions.
        return run_checked("ydotool", &["mousemove", "--absolute", &xs, &ys]).await;
    }
    Err(OsError::OperationFailed(
        "No mouse move backend found (install 'wlrctl' or 'ydotool')".to_string(),
    ))
}

/// Click at absolute coordinate.
pub async fn click_at(x: i32, y: i32, button: &str) -> OsResult<()> {
    mouse_move_absolute(x, y).await?;
    // Small delay lets compositor settle cursor move before click.
    sleep(Duration::from_millis(30)).await;
    mouse_click(button).await
}

/// Read cursor position using Hyprland.
pub async fn cursor_position() -> OsResult<(i32, i32)> {
    let output_json = Command::new("hyprctl")
        .args(["cursorpos", "-j"])
        .output()
        .await?;
    if output_json.status.success() {
        let raw = String::from_utf8_lossy(&output_json.stdout).to_string();
        if let Some(pos) = parse_cursor_position(&raw) {
            return Ok(pos);
        }
    }

    let output_plain = Command::new("hyprctl").args(["cursorpos"]).output().await?;
    if output_plain.status.success() {
        let raw = String::from_utf8_lossy(&output_plain.stdout).to_string();
        if let Some(pos) = parse_cursor_position(&raw) {
            return Ok(pos);
        }
    }

    Err(OsError::OperationFailed(
        "Unable to read cursor position from hyprctl".to_string(),
    ))
}

/// Move cursor and verify it reached target.
pub async fn mouse_move_and_verify(
    x: i32,
    y: i32,
    tolerance: i32,
    timeout_ms: u64,
) -> OsResult<(i32, i32)> {
    mouse_move_absolute(x, y).await?;
    let tol = tolerance.max(0);
    let timeout = Duration::from_millis(timeout_ms.max(120));
    let started = Instant::now();
    let mut last_seen: Option<(i32, i32)> = None;
    let mut last_error: Option<String> = None;

    loop {
        match cursor_position().await {
            Ok((cx, cy)) => {
                last_seen = Some((cx, cy));
                if (cx - x).abs() <= tol && (cy - y).abs() <= tol {
                    return Ok((cx, cy));
                }
            }
            Err(err) => last_error = Some(err.to_string()),
        }

        if started.elapsed() >= timeout {
            return Err(OsError::OperationFailed(format!(
                "cursor verify timeout target=({}, {}) tol={} last_seen={:?}{}",
                x,
                y,
                tol,
                last_seen,
                last_error
                    .as_deref()
                    .map(|e| format!(" last_error={e}"))
                    .unwrap_or_default()
            )));
        }
        sleep(Duration::from_millis(35)).await;
    }
}

/// Click at coordinate and verify cursor state after click.
pub async fn click_at_and_verify(
    x: i32,
    y: i32,
    button: &str,
    tolerance: i32,
    timeout_ms: u64,
) -> OsResult<Value> {
    let before = cursor_position().await.ok();
    let moved = mouse_move_and_verify(x, y, tolerance, timeout_ms).await?;
    click_at(x, y, button).await?;
    sleep(Duration::from_millis(80)).await;
    let after = cursor_position().await.ok();

    Ok(json!({
        "target": {"x": x, "y": y},
        "button": button,
        "before": before.map(|(x, y)| json!({"x": x, "y": y})).unwrap_or(json!(null)),
        "moved": {"x": moved.0, "y": moved.1},
        "after": after.map(|(x, y)| json!({"x": x, "y": y})).unwrap_or(json!(null))
    }))
}

/// Combined screen read state for agent observation (window/cursor/screenshot/OCR).
pub async fn read_screen_state(
    include_ocr: bool,
    include_windows: bool,
    include_cursor: bool,
    include_screenshot: bool,
    lang: Option<&str>,
    window_limit: usize,
    max_ocr_matches: usize,
) -> OsResult<Value> {
    let mut warnings = Vec::<String>::new();
    let mut screenshot_path: Option<String> = None;
    let mut active_window_json = Value::Null;
    let mut windows_json = Vec::<Value>::new();
    let mut cursor_json = Value::Null;
    let mut ocr_text = String::new();
    let mut ocr_matches = Vec::<OcrMatch>::new();

    if include_windows {
        match active_window().await {
            Ok(v) => active_window_json = v,
            Err(err) => warnings.push(format!("active_window: {}", err)),
        }
        match list_windows(window_limit.max(1)).await {
            Ok(v) => windows_json = v,
            Err(err) => warnings.push(format!("list_windows: {}", err)),
        }
    }

    if include_cursor {
        match cursor_position().await {
            Ok((x, y)) => cursor_json = json!({"x": x, "y": y}),
            Err(err) => warnings.push(format!("cursor_position: {}", err)),
        }
    }

    if include_screenshot || include_ocr {
        match capture_screen(None).await {
            Ok(path) => screenshot_path = Some(path),
            Err(err) => warnings.push(format!("capture_screen: {}", err)),
        }
    }

    if include_ocr {
        let ocr_result = if let Some(path) = screenshot_path.as_deref() {
            ocr_screen(Some(path), lang).await
        } else {
            ocr_screen(None, lang).await
        };
        match ocr_result {
            Ok((text, mut matches)) => {
                ocr_text = text;
                if max_ocr_matches > 0 && matches.len() > max_ocr_matches {
                    matches.truncate(max_ocr_matches);
                }
                ocr_matches = matches;
            }
            Err(err) => warnings.push(format!("ocr_screen: {}", err)),
        }
    }

    if screenshot_path.is_none()
        && active_window_json.is_null()
        && windows_json.is_empty()
        && cursor_json.is_null()
        && ocr_text.is_empty()
    {
        return Err(OsError::OperationFailed(format!(
            "unable to read screen state{}",
            if warnings.is_empty() {
                String::new()
            } else {
                format!(" ({})", warnings.join(" | "))
            }
        )));
    }

    Ok(json!({
        "screenshot_path": screenshot_path,
        "active_window": active_window_json,
        "windows": windows_json,
        "cursor": cursor_json,
        "ocr_text": ocr_text,
        "ocr_matches": ocr_matches,
        "warnings": warnings
    }))
}

/// Capture current screen to file and return saved path.
pub async fn capture_screen(path: Option<&str>) -> OsResult<String> {
    let target = path
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("/tmp/hypr-claw-shot-{}.png", chrono::Utc::now().timestamp_millis()));

    if let Some(parent) = Path::new(&target).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    if command_exists("grim").await {
        run_checked("grim", &[&target]).await?;
        return Ok(target);
    }
    if command_exists("hyprshot").await {
        // hyprshot -m output prints path, but we pass explicit output path.
        run_checked("hyprshot", &["-m", "output", "-o", &target]).await?;
        return Ok(target);
    }
    Err(OsError::OperationFailed(
        "No screenshot backend found (install 'grim' or 'hyprshot')".to_string(),
    ))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OcrMatch {
    pub text: String,
    pub confidence: f32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub center_x: i32,
    pub center_y: i32,
}

fn parse_tesseract_tsv(tsv: &str) -> Vec<OcrMatch> {
    let mut matches = Vec::new();
    for (idx, line) in tsv.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        let raw_text = cols[11].trim();
        if raw_text.is_empty() {
            continue;
        }
        let conf = cols[10].parse::<f32>().unwrap_or(-1.0);
        if conf < 0.0 {
            continue;
        }
        let x = cols[6].parse::<i32>().unwrap_or(-1);
        let y = cols[7].parse::<i32>().unwrap_or(-1);
        let width = cols[8].parse::<i32>().unwrap_or(0);
        let height = cols[9].parse::<i32>().unwrap_or(0);
        if x < 0 || y < 0 || width <= 0 || height <= 0 {
            continue;
        }
        matches.push(OcrMatch {
            text: raw_text.to_string(),
            confidence: conf,
            x,
            y,
            width,
            height,
            center_x: x + width / 2,
            center_y: y + height / 2,
        });
    }
    matches
}

fn normalize_for_match(input: &str, case_sensitive: bool) -> String {
    let mut out = String::new();
    let mut last_was_space = true;
    for ch in input.chars() {
        let mapped = if case_sensitive {
            ch
        } else {
            ch.to_ascii_lowercase()
        };
        if mapped.is_ascii_alphanumeric() {
            out.push(mapped);
            last_was_space = false;
            continue;
        }
        if !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim().to_string()
}

fn build_phrase_matches(
    query: &str,
    words: &[OcrMatch],
    case_sensitive: bool,
    min_confidence: f32,
    limit: usize,
) -> Vec<OcrMatch> {
    let query_norm = normalize_for_match(query, case_sensitive);
    if query_norm.is_empty() {
        return Vec::new();
    }

    let mut kept_words = Vec::<OcrMatch>::new();
    let mut normalized_words = Vec::<String>::new();
    for word in words {
        if word.confidence < min_confidence {
            continue;
        }
        let normalized = normalize_for_match(&word.text, case_sensitive);
        if normalized.is_empty() {
            continue;
        }
        kept_words.push(word.clone());
        normalized_words.push(normalized);
    }
    if kept_words.is_empty() {
        return Vec::new();
    }

    let mut full = String::new();
    let mut spans = Vec::<(usize, usize)>::new();
    for token in &normalized_words {
        if !full.is_empty() {
            full.push(' ');
        }
        let start = full.len();
        full.push_str(token);
        let end = full.len();
        spans.push((start, end));
    }

    let mut results = Vec::<OcrMatch>::new();
    let mut search_from = 0usize;
    while let Some(rel) = full.get(search_from..).and_then(|s| s.find(&query_norm)) {
        let abs = search_from + rel;
        let abs_end = abs + query_norm.len();

        let start_idx = spans
            .iter()
            .position(|(_, end)| *end > abs)
            .unwrap_or(0);
        let end_idx = spans
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (start, _))| *start < abs_end)
            .map(|(idx, _)| idx)
            .unwrap_or(start_idx);

        let slice = &kept_words[start_idx..=end_idx];
        let text = slice
            .iter()
            .map(|w| w.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let confidence = slice.iter().map(|w| w.confidence).sum::<f32>() / slice.len() as f32;
        let x = slice.iter().map(|w| w.x).min().unwrap_or(0);
        let y = slice.iter().map(|w| w.y).min().unwrap_or(0);
        let max_x = slice.iter().map(|w| w.x + w.width).max().unwrap_or(x);
        let max_y = slice.iter().map(|w| w.y + w.height).max().unwrap_or(y);
        let width = (max_x - x).max(1);
        let height = (max_y - y).max(1);

        results.push(OcrMatch {
            text,
            confidence,
            x,
            y,
            width,
            height,
            center_x: x + width / 2,
            center_y: y + height / 2,
        });
        if limit > 0 && results.len() >= limit {
            break;
        }
        search_from = abs.saturating_add(1);
    }

    if !results.is_empty() {
        return results;
    }

    kept_words
        .into_iter()
        .filter(|word| normalize_for_match(&word.text, case_sensitive).contains(&query_norm))
        .take(limit.max(1))
        .collect()
}

async fn ocr_screen_with_retries(
    lang: Option<&str>,
    max_attempts: usize,
    retry_delay_ms: u64,
) -> OsResult<(String, Vec<OcrMatch>)> {
    let attempts = max_attempts.max(1);
    let mut last_error: Option<String> = None;
    for attempt in 0..attempts {
        match ocr_screen(None, lang).await {
            Ok((text, words)) if !words.is_empty() => return Ok((text, words)),
            Ok((_text, _words)) => {
                last_error = Some("OCR returned empty text".to_string());
            }
            Err(err) => last_error = Some(err.to_string()),
        }
        if attempt + 1 < attempts {
            sleep(Duration::from_millis(retry_delay_ms.max(80))).await;
        }
    }
    Err(OsError::OperationFailed(last_error.unwrap_or_else(|| {
        "OCR failed without detailed error".to_string()
    })))
}

/// OCR current screen and return recognized words with boxes.
pub async fn ocr_screen(
    path: Option<&str>,
    lang: Option<&str>,
) -> OsResult<(String, Vec<OcrMatch>)> {
    let image_path = if let Some(path) = path {
        path.to_string()
    } else {
        capture_screen(None).await?
    };

    if !command_exists("tesseract").await {
        return Err(OsError::OperationFailed(
            "tesseract not found (install 'tesseract' package)".to_string(),
        ));
    }

    let mut args = vec![image_path.as_str(), "stdout", "tsv"];
    if let Some(lang) = lang {
        args.push("-l");
        args.push(lang);
    }
    let tsv = run_output("tesseract", &args).await?;
    let words = parse_tesseract_tsv(&tsv);
    let full_text = words
        .iter()
        .map(|m| m.text.clone())
        .collect::<Vec<String>>()
        .join(" ");
    Ok((full_text, words))
}

/// Find text matches from OCR output.
pub async fn find_text(
    query: &str,
    case_sensitive: bool,
    limit: usize,
    lang: Option<&str>,
) -> OsResult<Vec<OcrMatch>> {
    if query.trim().is_empty() {
        return Err(OsError::InvalidArgument(
            "query cannot be empty".to_string(),
        ));
    }

    let (_, words) = ocr_screen_with_retries(lang, 2, 120).await?;
    Ok(build_phrase_matches(
        query,
        &words,
        case_sensitive,
        25.0,
        if limit == 0 { usize::MAX } else { limit },
    ))
}

/// Find text on screen and click its center.
pub async fn click_text(
    query: &str,
    occurrence: usize,
    button: &str,
    case_sensitive: bool,
    lang: Option<&str>,
) -> OsResult<OcrMatch> {
    let mut tries = 0usize;
    loop {
        let matches = find_text(query, case_sensitive, occurrence + 1, lang).await?;
        if matches.len() > occurrence {
            let target = matches[occurrence].clone();
            click_at(target.center_x, target.center_y, button).await?;
            return Ok(target);
        }

        tries += 1;
        if tries >= 3 {
            return Err(OsError::NotFound(format!(
                "text '{}' not found on screen",
                query
            )));
        }
        sleep(Duration::from_millis(150)).await;
    }
}

/// Wait until text appears on screen and return first match.
pub async fn wait_for_text(
    query: &str,
    case_sensitive: bool,
    timeout_ms: u64,
    poll_interval_ms: u64,
    lang: Option<&str>,
) -> OsResult<OcrMatch> {
    if query.trim().is_empty() {
        return Err(OsError::InvalidArgument(
            "query cannot be empty".to_string(),
        ));
    }

    let timeout = Duration::from_millis(timeout_ms.max(250));
    let poll = Duration::from_millis(poll_interval_ms.max(100));
    let started = Instant::now();

    let mut last_error: Option<String> = None;
    loop {
        match find_text(query, case_sensitive, 1, lang).await {
            Ok(matches) => {
                if let Some(first) = matches.into_iter().next() {
                    return Ok(first);
                }
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
        if started.elapsed() >= timeout {
            return Err(OsError::NotFound(format!(
                "text '{}' not found within {}ms{}",
                query,
                timeout.as_millis(),
                last_error
                    .as_deref()
                    .map(|e| format!(" (last error: {e})"))
                    .unwrap_or_default()
            )));
        }
        sleep(poll).await;
    }
}

/// Return current active window metadata from Hyprland.
pub async fn active_window() -> OsResult<Value> {
    let output = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .await?;
    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let json: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| OsError::OperationFailed(e.to_string()))?;
    Ok(json)
}

/// List Hyprland windows (clients) metadata.
pub async fn list_windows(limit: usize) -> OsResult<Vec<Value>> {
    let output = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .await?;
    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let mut json: Vec<Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| OsError::OperationFailed(e.to_string()))?;
    if limit > 0 && json.len() > limit {
        json.truncate(limit);
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_app_key_normalizes_common_forms() {
        assert_eq!(canonical_app_key("Visual Studio Code"), "visualstudiocode");
        assert_eq!(
            canonical_app_key("org.telegram.desktop"),
            "orgtelegramdesktop"
        );
    }

    #[test]
    fn parse_exec_program_supports_env_wrappers() {
        let exec =
            "env BAMF_DESKTOP_FILE_HINT=/var/lib/app.desktop /usr/bin/code --unity-launch %F";
        let (program, flatpak) = parse_exec_program_and_flatpak(exec);
        assert_eq!(program.as_deref(), Some("/usr/bin/code"));
        assert!(flatpak.is_none());
    }

    #[test]
    fn parse_exec_program_detects_flatpak_app_id() {
        let exec = "flatpak run org.telegram.desktop -- %U";
        let (program, flatpak) = parse_exec_program_and_flatpak(exec);
        assert_eq!(program.as_deref(), Some("flatpak"));
        assert_eq!(flatpak.as_deref(), Some("org.telegram.desktop"));
    }

    #[test]
    fn alias_candidates_cover_vscode_family() {
        let aliases = alias_candidates("vscode");
        assert!(aliases.iter().any(|v| v == "code"));
        assert!(aliases.iter().any(|v| v == "code-oss"));
        assert!(aliases.iter().any(|v| v == "com.visualstudio.code"));
    }
}
