//! System prompt builder for tool-aware agents.

/// Build reinforced system prompt with tool usage instructions.
pub fn build_tool_prompt(base_prompt: &str, tool_names: &[String]) -> String {
    if tool_names.is_empty() {
        return base_prompt.to_string();
    }

    let tools_list = tool_names
        .iter()
        .map(|t| format!("  • {}", t))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{}\n\n\
        ═══════════════════════════════════════════════════════════════\n\
        🤖 CRITICAL: LOCAL LINUX AGENT - DIRECT OS CONTROL\n\
        ═══════════════════════════════════════════════════════════════\n\n\
        Available Tools:\n{}\n\n\
        IMPORTANT RULES:\n\
        • For greetings/simple questions (hi, hello, thanks): Just respond directly, NO TOOLS\n\
        • For actions (open, click, type, etc.): Use tools as shown below\n\n\
        CLICKING ON SCREEN (for YouTube/web/apps):\n\
        1. desktop.read_screen_state {{\"include_ocr\": true}}\n\
        2. Find target in OCR: {{\"text\": \"...\", \"x\": N, \"y\": N}}\n\
        3. desktop.mouse_move {{\"x\": N, \"y\": N}}\n\
        4. desktop.mouse_click {{\"button\": \"left\"}}\n\n\
        DO NOT use browser.action - use OCR + mouse instead.\n\n\
        After tool success, provide final confirmation and STOP.",
        base_prompt, tools_list
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tools() {
        let prompt = build_tool_prompt("Base prompt", &[]);
        assert_eq!(prompt, "Base prompt");
    }

    #[test]
    fn test_with_tools() {
        let tools = vec!["desktop.mouse_click".to_string()];
        let prompt = build_tool_prompt("Base", &tools);
        assert!(prompt.contains("desktop.mouse_click"));
        assert!(prompt.contains("YOU MUST USE TOOLS"));
    }
}
