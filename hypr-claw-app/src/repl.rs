//! REPL-based agent runtime - persistent session loop

use std::io::{self, Write};

pub struct ReplAgent<S, L, D, R, Sum> {
    agent_loop: hypr_claw_runtime::AgentLoop<S, L, D, R, Sum>,
    session_key: String,
    agent_id: String,
    system_prompt: String,
}

impl<S, L, D, R, Sum> ReplAgent<S, L, D, R, Sum>
where
    S: hypr_claw_runtime::SessionStore,
    L: hypr_claw_runtime::LockManager,
    D: hypr_claw_runtime::ToolDispatcher,
    R: hypr_claw_runtime::ToolRegistry,
    Sum: hypr_claw_runtime::Summarizer,
{
    pub fn new(
        agent_loop: hypr_claw_runtime::AgentLoop<S, L, D, R, Sum>,
        session_key: String,
        agent_id: String,
        system_prompt: String,
    ) -> Self {
        Self {
            agent_loop,
            session_key,
            agent_id,
            system_prompt,
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("╔══════════════════════════════════════════════════════════════════╗");
        println!("║              Hypr-Claw Agent REPL                                ║");
        println!("║  Commands: exit, status, tasks, clear, help                      ║");
        println!("╚══════════════════════════════════════════════════════════════════╝");
        println!();

        loop {
            print!("hypr> ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            if input.is_empty() {
                continue;
            }

            match input {
                "exit" | "quit" => {
                    println!("👋 Goodbye!");
                    break;
                }
                "help" => {
                    println!("\n📖 Available Commands:");
                    println!("  exit, quit  - Exit the agent");
                    println!("  help        - Show this help message");
                    println!("  status      - Show agent status");
                    println!("  tasks       - List background tasks");
                    println!("  clear       - Clear screen");
                    println!("\n💡 Enter any natural language command to execute\n");
                    continue;
                }
                "status" => {
                    println!("\n📊 Agent Status:");
                    println!("  Session: {}", self.session_key);
                    println!("  Agent ID: {}", self.agent_id);
                    println!("  Status: Active");
                    println!();
                    continue;
                }
                "tasks" => {
                    println!("\n📋 Background Tasks:");
                    println!("  (Task manager not yet implemented)");
                    println!();
                    continue;
                }
                "clear" => {
                    print!("\x1B[2J\x1B[1;1H");
                    continue;
                }
                _ => {}
            }

            // Show thinking indicator
            eprint!("🤔 Thinking...");
            std::io::Write::flush(&mut std::io::stderr()).ok();

            match self.agent_loop.run(
                &self.session_key,
                &self.agent_id,
                &self.system_prompt,
                input,
            ).await {
                Ok(response) => {
                    eprint!("\r\x1B[K"); // Clear the thinking line
                    println!("\n{}\n", response);
                }
                Err(e) => {
                    eprint!("\r\x1B[K"); // Clear the thinking line
                    eprintln!("❌ Error: {}\n", e);
                }
            }
        }

        Ok(())
    }
}
