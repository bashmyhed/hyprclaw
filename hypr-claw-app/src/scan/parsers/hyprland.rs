use super::{ConfigParser, ConfigType, ParseError, ParsedConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Hyprland config parser - handles any Hyprland config structure
pub struct HyprlandParser;

impl ConfigParser for HyprlandParser {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    fn can_parse(&self, path: &Path) -> bool {
        // Check if it's hyprland.conf or in hypr directory
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == "hyprland.conf" || name.ends_with(".conf") {
                if let Some(parent) = path.parent() {
                    if let Some(parent_name) = parent.file_name().and_then(|n| n.to_str()) {
                        return parent_name == "hypr" || parent_name == "hyprland";
                    }
                }
            }
        }
        false
    }

    fn parse(&self, path: &Path) -> Result<ParsedConfig, ParseError> {
        let start = Instant::now();

        let content = fs::read_to_string(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ParseError::FileNotFound(path.to_path_buf()),
            std::io::ErrorKind::PermissionDenied => {
                ParseError::PermissionDenied(path.to_path_buf())
            }
            _ => ParseError::IoError {
                path: path.to_path_buf(),
                error: e.to_string(),
            },
        })?;

        let data = parse_hyprland_config(&content, path)?;

        Ok(ParsedConfig {
            path: path.to_path_buf(),
            config_type: ConfigType::Hyprland,
            data,
            parse_time_ms: start.elapsed().as_millis() as u64,
        })
    }
}

fn parse_hyprland_config(content: &str, base_path: &Path) -> Result<Value, ParseError> {
    let mut keybinds = Vec::new();
    let mut exec_once = Vec::new();
    let mut exec_cmds = Vec::new();
    let mut workspace_rules = Vec::new();
    let mut window_rules = Vec::new();
    let mut monitors = Vec::new();
    let mut variables = HashMap::new();
    let mut sourced_files = Vec::new();
    let mut general_settings = HashMap::new();
    let mut input_settings = HashMap::new();
    let mut gestures_settings = HashMap::new();
    let mut current_section: Option<String> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments (but not section headers)
        if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("#!")) {
            continue;
        }

        // Check for section start
        if trimmed.contains('{') {
            if let Some(section_name) = trimmed.split('{').next() {
                current_section = Some(section_name.trim().to_string());
            }
            continue;
        }

        // Check for section end
        if trimmed == "}" {
            current_section = None;
            continue;
        }

        // Parse based on current section or top-level
        if let Some(ref section) = current_section {
            parse_section_line(
                section,
                trimmed,
                &mut general_settings,
                &mut input_settings,
                &mut gestures_settings,
            );
        } else {
            parse_top_level_line(
                trimmed,
                &mut keybinds,
                &mut exec_once,
                &mut exec_cmds,
                &mut workspace_rules,
                &mut window_rules,
                &mut monitors,
                &mut variables,
                &mut sourced_files,
                base_path,
            );
        }
    }

    Ok(json!({
        "keybinds": keybinds,
        "exec_once": exec_once,
        "exec": exec_cmds,
        "workspace_rules": workspace_rules,
        "window_rules": window_rules,
        "monitors": monitors,
        "variables": variables,
        "sourced_files": sourced_files,
        "general": general_settings,
        "input": input_settings,
        "gestures": gestures_settings,
    }))
}

fn parse_top_level_line(
    line: &str,
    keybinds: &mut Vec<Value>,
    exec_once: &mut Vec<String>,
    exec_cmds: &mut Vec<String>,
    workspace_rules: &mut Vec<Value>,
    window_rules: &mut Vec<Value>,
    monitors: &mut Vec<Value>,
    variables: &mut HashMap<String, String>,
    sourced_files: &mut Vec<String>,
    base_path: &Path,
) {
    // Variables: $var = value
    if line.starts_with('$') {
        if let Some((var, value)) = line.split_once('=') {
            variables.insert(var.trim().to_string(), value.trim().to_string());
        }
        return;
    }

    // Source directives: source=path/to/file.conf
    if line.starts_with("source") {
        if let Some(path) = line.split_once('=').map(|(_, p)| p.trim()) {
            sourced_files.push(path.to_string());
        }
        return;
    }

    // Keybinds: bind*, bindd, bindle, bindl, etc.
    if line.starts_with("bind") {
        keybinds.push(parse_keybind(line));
        return;
    }

    // Exec-once
    if line.starts_with("exec-once") {
        if let Some(cmd) = line.split_once('=').map(|(_, c)| c.trim()) {
            exec_once.push(cmd.to_string());
        }
        return;
    }

    // Exec
    if line.starts_with("exec") && !line.starts_with("exec-once") {
        if let Some(cmd) = line.split_once('=').map(|(_, c)| c.trim()) {
            exec_cmds.push(cmd.to_string());
        }
        return;
    }

    // Workspace rules
    if line.starts_with("workspace") {
        workspace_rules.push(parse_workspace_rule(line));
        return;
    }

    // Window rules
    if line.starts_with("windowrule") {
        window_rules.push(parse_window_rule(line));
        return;
    }

    // Monitor config
    if line.starts_with("monitor") {
        monitors.push(parse_monitor(line));
        return;
    }
}

fn parse_section_line(
    section: &str,
    line: &str,
    general: &mut HashMap<String, String>,
    input: &mut HashMap<String, String>,
    gestures: &mut HashMap<String, String>,
) {
    if let Some((key, value)) = line.split_once('=') {
        let key = key.trim().to_string();
        let value = value.trim().to_string();

        match section {
            "general" => {
                general.insert(key, value);
            }
            "input" => {
                input.insert(key, value);
            }
            "gestures" => {
                gestures.insert(key, value);
            }
            _ => {}
        }
    }
}

fn parse_keybind(line: &str) -> Value {
    // Parse: bind[type] = mods, key, action, params...
    // Example: bind = Super, Q, exec, kitty
    // Example: bindd = Super, V, Clipboard history, global, quickshell:overviewClipboardToggle

    let bind_type = line
        .split(|c: char| c == '=' || c == ',')
        .next()
        .unwrap_or("bind")
        .trim();

    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() < 2 {
        return json!({
            "raw": line,
            "type": bind_type,
        });
    }

    let params: Vec<&str> = parts[1].split(',').map(|s| s.trim()).collect();

    json!({
        "type": bind_type,
        "modifiers": params.get(0).unwrap_or(&""),
        "key": params.get(1).unwrap_or(&""),
        "action": params.get(2).unwrap_or(&""),
        "params": params.get(3..).unwrap_or(&[]),
        "raw": line,
    })
}

fn parse_workspace_rule(line: &str) -> Value {
    // Parse: workspace = id, params...
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() < 2 {
        return json!({"raw": line});
    }

    let params: Vec<&str> = parts[1].split(',').map(|s| s.trim()).collect();

    json!({
        "workspace": params.get(0).unwrap_or(&""),
        "params": params.get(1..).unwrap_or(&[]),
        "raw": line,
    })
}

fn parse_window_rule(line: &str) -> Value {
    // Parse: windowrule = rule, class
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() < 2 {
        return json!({"raw": line});
    }

    let params: Vec<&str> = parts[1].split(',').map(|s| s.trim()).collect();

    json!({
        "rule": params.get(0).unwrap_or(&""),
        "class": params.get(1).unwrap_or(&""),
        "raw": line,
    })
}

fn parse_monitor(line: &str) -> Value {
    // Parse: monitor = name, resolution, position, scale
    // Example: monitor = eDP-1,1920x1080@144,auto,1
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() < 2 {
        return json!({"raw": line});
    }

    let params: Vec<&str> = parts[1].split(',').map(|s| s.trim()).collect();

    json!({
        "name": params.get(0).unwrap_or(&""),
        "resolution": params.get(1).unwrap_or(&""),
        "position": params.get(2).unwrap_or(&""),
        "scale": params.get(3).unwrap_or(&""),
        "raw": line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_parse_hyprland_conf() {
        let parser = HyprlandParser;
        assert!(parser.can_parse(Path::new("/home/user/.config/hypr/hyprland.conf")));
        assert!(parser.can_parse(Path::new("/home/user/.config/hypr/hyprland/keybinds.conf")));
        assert!(!parser.can_parse(Path::new("/home/user/.bashrc")));
    }

    #[test]
    fn test_parse_keybind() {
        let line = "bind = Super, Q, exec, kitty";
        let result = parse_keybind(line);
        assert_eq!(result["modifiers"], "Super");
        assert_eq!(result["key"], "Q");
        assert_eq!(result["action"], "exec");
    }

    #[test]
    fn test_parse_monitor() {
        let line = "monitor = eDP-1,1920x1080@144,auto,1";
        let result = parse_monitor(line);
        assert_eq!(result["name"], "eDP-1");
        assert_eq!(result["resolution"], "1920x1080@144");
        assert_eq!(result["position"], "auto");
        assert_eq!(result["scale"], "1");
    }

    #[test]
    fn test_parse_workspace_rule() {
        let line = "workspace = 1, monitor:DP-1";
        let result = parse_workspace_rule(line);
        assert_eq!(result["workspace"], "1");
    }

    #[test]
    fn test_parse_hyprland_config_basic() {
        let content = r#"
# Comment
$var = value

bind = Super, Q, exec, kitty
exec-once = waybar
monitor = eDP-1,1920x1080,auto,1

general {
    gaps_in = 5
    gaps_out = 10
}
"#;
        let result = parse_hyprland_config(content, Path::new("/test")).unwrap();

        assert!(result["keybinds"].is_array());
        assert!(result["exec_once"].is_array());
        assert!(result["monitors"].is_array());
        assert!(result["variables"].is_object());
        assert!(result["general"].is_object());
    }
}
