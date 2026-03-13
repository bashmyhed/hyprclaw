use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackendStatus {
    pub state: String,
    pub backend: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeHealthSnapshot {
    pub hyprland: BackendStatus,
    pub screenshot: BackendStatus,
    pub ocr: BackendStatus,
    pub keyboard: BackendStatus,
    pub pointer: BackendStatus,
}

impl RuntimeHealthSnapshot {
    pub fn hyprland_ready(&self) -> bool {
        self.hyprland.state == "ready"
    }

    pub fn screenshot_ready(&self) -> bool {
        self.screenshot.state == "ready"
    }

    pub fn ocr_ready(&self) -> bool {
        self.ocr.state == "ready"
    }

    pub fn keyboard_available(&self) -> bool {
        self.keyboard.state != "missing"
    }

    pub fn pointer_available(&self) -> bool {
        self.pointer.state != "missing"
    }
}

pub fn probe_runtime_health() -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot {
        hyprland: probe_hyprland(),
        screenshot: probe_screenshot(),
        ocr: probe_ocr(),
        keyboard: probe_keyboard(),
        pointer: probe_pointer(),
    }
}

fn ready(backend: &str, detail: impl Into<String>) -> BackendStatus {
    BackendStatus {
        state: "ready".to_string(),
        backend: Some(backend.to_string()),
        detail: detail.into(),
    }
}

fn degraded(backend: &str, detail: impl Into<String>) -> BackendStatus {
    BackendStatus {
        state: "degraded".to_string(),
        backend: Some(backend.to_string()),
        detail: detail.into(),
    }
}

fn missing(detail: impl Into<String>) -> BackendStatus {
    BackendStatus {
        state: "missing".to_string(),
        backend: None,
        detail: detail.into(),
    }
}

fn binary_exists(binary: &str) -> bool {
    if binary.contains('/') {
        return PathBuf::from(binary).exists();
    }

    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .into_iter()
        .flatten()
        .map(|path| path.join(binary))
        .any(|candidate| candidate.exists())
}

fn run_probe(binary: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if !stdout.is_empty() {
            stdout
        } else if !stderr.is_empty() {
            stderr
        } else {
            format!("{binary} {:?} succeeded", args)
        };
        return Ok(detail);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {:?}", output.status.code())
    };
    Err(detail)
}

fn probe_hyprland() -> BackendStatus {
    if !binary_exists("hyprctl") {
        return missing("hyprctl not found in PATH");
    }

    match run_probe("hyprctl", &["activewindow", "-j"]) {
        Ok(_) => ready("hyprctl", "hyprctl activewindow -j succeeded"),
        Err(detail) => degraded(
            "hyprctl",
            format!("hyprctl reachable but activewindow probe failed: {detail}"),
        ),
    }
}

fn probe_screenshot() -> BackendStatus {
    if binary_exists("grim") {
        return match run_probe("grim", &["-h"]) {
            Ok(_) => ready("grim", "grim help probe succeeded"),
            Err(detail) => degraded(
                "grim",
                format!("grim present but help probe failed: {detail}"),
            ),
        };
    }

    if binary_exists("hyprshot") {
        return match run_probe("hyprshot", &["-h"]) {
            Ok(_) => ready("hyprshot", "hyprshot help probe succeeded"),
            Err(detail) => degraded(
                "hyprshot",
                format!("hyprshot present but help probe failed: {detail}"),
            ),
        };
    }

    missing("no screenshot backend found (expected grim or hyprshot)")
}

fn probe_ocr() -> BackendStatus {
    if !binary_exists("tesseract") {
        return missing("tesseract not found in PATH");
    }

    match run_probe("tesseract", &["--list-langs"]) {
        Ok(detail) => {
            let langs: Vec<&str> = detail
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .filter(|line| !line.starts_with("List of available languages"))
                .collect();
            if langs.is_empty() {
                degraded(
                    "tesseract",
                    "tesseract is installed but no OCR language packs were reported",
                )
            } else {
                let has_english = langs.iter().any(|lang| *lang == "eng");
                ready(
                    "tesseract",
                    format!(
                        "{} language pack(s) available; english={}",
                        langs.len(),
                        has_english
                    ),
                )
            }
        }
        Err(detail) => degraded(
            "tesseract",
            format!("tesseract present but language probe failed: {detail}"),
        ),
    }
}

fn probe_keyboard() -> BackendStatus {
    if binary_exists("wtype") {
        return ready("wtype", "wtype binary found in PATH");
    }

    if binary_exists("ydotool") {
        return match run_probe("ydotool", &["--help"]) {
            Ok(_) => degraded(
                "ydotool",
                "ydotool is present; runtime actions still depend on ydotoold/uinput access",
            ),
            Err(detail) => degraded(
                "ydotool",
                format!("ydotool present but help probe failed: {detail}"),
            ),
        };
    }

    missing("no keyboard backend found (expected wtype or ydotool)")
}

fn probe_pointer() -> BackendStatus {
    if binary_exists("wlrctl") {
        return match run_probe("wlrctl", &["--help"]) {
            Ok(_) => ready("wlrctl", "wlrctl help probe succeeded"),
            Err(detail) => degraded(
                "wlrctl",
                format!("wlrctl present but help probe failed: {detail}"),
            ),
        };
    }

    if binary_exists("ydotool") {
        return match run_probe("ydotool", &["--help"]) {
            Ok(_) => degraded(
                "ydotool",
                "ydotool is present; pointer actions still depend on ydotoold/uinput access",
            ),
            Err(detail) => degraded(
                "ydotool",
                format!("ydotool present but help probe failed: {detail}"),
            ),
        };
    }

    missing("no pointer backend found (expected wlrctl or ydotool)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_flags_treat_degraded_input_backends_as_usable() {
        let snapshot = RuntimeHealthSnapshot {
            hyprland: ready("hyprctl", "ok"),
            screenshot: ready("grim", "ok"),
            ocr: ready("tesseract", "ok"),
            keyboard: degraded("ydotool", "daemon required"),
            pointer: degraded("ydotool", "daemon required"),
        };

        assert!(snapshot.hyprland_ready());
        assert!(snapshot.screenshot_ready());
        assert!(snapshot.ocr_ready());
        assert!(snapshot.keyboard_available());
        assert!(snapshot.pointer_available());
    }

    #[test]
    fn missing_input_backends_are_marked_unavailable() {
        let snapshot = RuntimeHealthSnapshot {
            hyprland: missing("missing"),
            screenshot: missing("missing"),
            ocr: missing("missing"),
            keyboard: missing("missing"),
            pointer: missing("missing"),
        };

        assert!(!snapshot.hyprland_ready());
        assert!(!snapshot.screenshot_ready());
        assert!(!snapshot.ocr_ready());
        assert!(!snapshot.keyboard_available());
        assert!(!snapshot.pointer_available());
    }
}
