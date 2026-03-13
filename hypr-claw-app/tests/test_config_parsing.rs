#[cfg(test)]
mod config_parsing_tests {
    use hypr_claw_app::scan::parsers::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_parser_registry_with_all_parsers() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(HyprlandParser));
        registry.register(Box::new(ShellParser));
        registry.register(Box::new(GitParser));

        // Test Hyprland config
        let hypr_path = PathBuf::from("/test/.config/hypr/hyprland.conf");
        assert!(HyprlandParser.can_parse(&hypr_path));

        // Test shell configs
        let bash_path = PathBuf::from("/test/.bashrc");
        assert!(ShellParser.can_parse(&bash_path));

        // Test git config
        let git_path = PathBuf::from("/test/.gitconfig");
        assert!(GitParser.can_parse(&git_path));
    }

    #[test]
    fn test_parse_real_hyprland_config() {
        let temp_dir = std::env::temp_dir().join("test_hypr_parse");
        fs::create_dir_all(&temp_dir).ok();

        let config_content = r#"
# Hyprland config
$mod = SUPER

bind = $mod, Q, exec, kitty
bind = $mod, C, killactive
bind = $mod, M, exit

exec-once = waybar
exec-once = swww init

monitor = eDP-1,1920x1080@144,auto,1

general {
    gaps_in = 5
    gaps_out = 10
    border_size = 2
}

workspace = 1, monitor:eDP-1
"#;
        let config_path = temp_dir.join("hyprland.conf");
        fs::write(&config_path, config_content).unwrap();

        let parser = HyprlandParser;
        let result = parser.parse(&config_path).unwrap();

        println!("\n📝 Parsed Hyprland config:");
        println!("{}", serde_json::to_string_pretty(&result.data).unwrap());

        assert!(result.data["keybinds"].is_array());
        assert!(result.data["exec_once"].is_array());
        assert!(result.data["monitors"].is_array());
        assert!(result.data["general"].is_object());

        let keybinds = result.data["keybinds"].as_array().unwrap();
        assert!(keybinds.len() >= 3);

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_parse_real_shell_config() {
        let temp_dir = std::env::temp_dir().join("test_shell_parse");
        fs::create_dir_all(&temp_dir).ok();

        let bashrc_content = r#"
# Bash config
alias ll='ls -la'
alias gs='git status'
alias dc='docker-compose'

export EDITOR=nvim
export BROWSER=firefox
export PATH="$HOME/.local/bin:$PATH"

function update_system() {
    sudo pacman -Syu
}

function backup_home() {
    rsync -av $HOME /backup/
}
"#;
        let bashrc_path = temp_dir.join(".bashrc");
        fs::write(&bashrc_path, bashrc_content).unwrap();

        let parser = ShellParser;
        let result = parser.parse(&bashrc_path).unwrap();

        println!("\n📝 Parsed shell config:");
        println!("{}", serde_json::to_string_pretty(&result.data).unwrap());

        assert_eq!(result.data["aliases"]["ll"], "ls -la");
        assert_eq!(result.data["aliases"]["gs"], "git status");
        assert_eq!(result.data["exports"]["EDITOR"], "nvim");
        assert!(result.data["functions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("update_system")));
        assert!(result.data["path_additions"].as_array().unwrap().len() > 0);

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_parse_real_git_config() {
        let temp_dir = std::env::temp_dir().join("test_git_parse");
        fs::create_dir_all(&temp_dir).ok();

        let gitconfig_content = r#"
[user]
    name = Test User
    email = test@example.com

[core]
    editor = nvim
    autocrlf = input
    pager = delta

[alias]
    st = status
    co = checkout
    br = branch
    ci = commit
    unstage = reset HEAD --

[remote "origin"]
    url = git@github.com:user/repo.git
    fetch = +refs/heads/*:refs/remotes/origin/*

[remote "upstream"]
    url = git@github.com:upstream/repo.git
    fetch = +refs/heads/*:refs/remotes/upstream/*
"#;
        let gitconfig_path = temp_dir.join(".gitconfig");
        fs::write(&gitconfig_path, gitconfig_content).unwrap();

        let parser = GitParser;
        let result = parser.parse(&gitconfig_path).unwrap();

        println!("\n📝 Parsed git config:");
        println!("{}", serde_json::to_string_pretty(&result.data).unwrap());

        assert_eq!(result.data["user"]["name"], "Test User");
        assert_eq!(result.data["user"]["email"], "test@example.com");
        assert_eq!(result.data["core"]["editor"], "nvim");
        assert_eq!(result.data["aliases"]["st"], "status");
        assert_eq!(result.data["remotes"][0]["name"], "origin");
        assert_eq!(result.data["remotes"][1]["name"], "upstream");

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_parse_multiple_configs() {
        let temp_dir = std::env::temp_dir().join("test_multi_parse");
        fs::create_dir_all(&temp_dir).ok();

        // Create multiple config files
        fs::write(temp_dir.join(".bashrc"), "alias ll='ls -la'").unwrap();
        fs::write(temp_dir.join(".gitconfig"), "[user]\n    name = Test").unwrap();
        fs::create_dir_all(temp_dir.join(".config/hypr")).unwrap();
        fs::write(
            temp_dir.join(".config/hypr/hyprland.conf"),
            "bind = SUPER, Q, exec, kitty",
        )
        .unwrap();

        let mut registry = ParserRegistry::new();
        registry.register(Box::new(HyprlandParser));
        registry.register(Box::new(ShellParser));
        registry.register(Box::new(GitParser));

        let paths = vec![
            temp_dir.join(".bashrc"),
            temp_dir.join(".gitconfig"),
            temp_dir.join(".config/hypr/hyprland.conf"),
        ];

        let results = registry.parse_all(&paths);
        let (parsed, failed) = partition_results(results);

        println!("\n📊 Parse results:");
        println!("  Parsed: {}", parsed.len());
        println!("  Failed: {}", failed.len());

        assert_eq!(parsed.len(), 3);
        assert_eq!(failed.len(), 0);

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_graceful_error_handling() {
        let parser = HyprlandParser;

        // Test non-existent file
        let result = parser.parse(&PathBuf::from("/nonexistent/file.conf"));
        assert!(result.is_err());
        if let Err(e) = result {
            println!("\n⚠️  Expected error: {}", e.user_message());
            assert!(e.user_message().contains("not found"));
        }
    }
}
