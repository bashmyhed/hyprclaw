# Hypr-Claw Context

Last updated: 2026-03-13

This is the current handoff document for the repo. New agents should read this first, then read the referenced files before making changes.

## What This Project Is

Hypr-Claw is trying to become a real local OS assistant for Linux/Hyprland:
- user gives one prompt
- model understands the task
- model gathers context automatically when possible
- model asks the user only when missing context is important or the action is high-impact
- model uses tools to operate the desktop/browser/files/processes
- model verifies progress after each action and continues until done

The target is not “chat about actions”. The target is “complete the work”.

## Product Direction

Keep these constraints:

1. Single power workflow.
- Do not reintroduce multiple personalities, “safe mode”, or old dashboard/soul systems.

2. Tool-driven execution.
- The model should act through tools, not produce explanation-only responses for feasible tasks.

3. Observe -> plan -> execute -> verify.
- This loop is the core behavior for GUI/browser work.

4. Explicit approvals for serious actions.
- Sending messages, typing into unknown forms, destructive file/process actions, and system-impacting operations should pause for confirmation.

5. Context should come from both machine and owner.
- Machine profile: configs, Hyprland, capabilities, runtime health.
- Owner profile: preferred browser/editor/terminal/apps, sensitive apps/paths, approval notes.

## Current Architecture

- [hypr-claw-app/src/main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)
  Main CLI, onboarding, prompt assembly, runtime orchestration, UI rendering.
- [hypr-claw-app/src/tool_setup.rs](/home/rick/hyprclaw/hypr-claw-app/src/tool_setup.rs)
  Tool registry construction.
- [hypr-claw-app/src/debug_timeline.rs](/home/rick/hyprclaw/hypr-claw-app/src/debug_timeline.rs)
  New live debug/timeline observer added in this pass.
- [hypr-claw-runtime/src/agent_loop.rs](/home/rick/hyprclaw/hypr-claw-runtime/src/agent_loop.rs)
  Main LLM + tool execution loop.
- [hypr-claw-tools/src/os_tools.rs](/home/rick/hyprclaw/hypr-claw-tools/src/os_tools.rs)
  Desktop tools and new Pinchtab-backed browser tools.
- [hypr-claw-infra/src/infra/permission_adapter.rs](/home/rick/hyprclaw/hypr-claw-infra/src/infra/permission_adapter.rs)
  Approval/risk adaptation layer.

## Important Recent Changes

### 1. Browser tool family exists now

Added Pinchtab-backed:
- `browser.health`
- `browser.navigate`
- `browser.snapshot`
- `browser.action`
- `browser.evaluate`
- `browser.screenshot`

Main files:
- [os_tools.rs](/home/rick/hyprclaw/hypr-claw-tools/src/os_tools.rs)
- [tool_setup.rs](/home/rick/hyprclaw/hypr-claw-app/src/tool_setup.rs)

Notes:
- These use HTTP to a Pinchtab service.
- They expect `PINCHTAB_URL` and optionally `PINCHTAB_TOKEN`.
- Default base URL is `http://127.0.0.1:9867`.

### 2. Onboarding is now much lighter

First-run setup no longer asks for a long interactive scan/profile session.

Current behavior:
- runs a basic system scan automatically
- infers owner defaults from environment and installed binaries
- marks onboarding complete
- defers deep scan to `scan`
- defers personal tuning to `owner edit`

This was done to get the user into the agent loop faster.

### 3. Owner profile / manual context exists now

Added owner-profile onboarding and editing:
- preferred browser
- preferred terminal
- preferred editor
- messaging apps
- daily apps
- sensitive apps
- sensitive paths
- approval notes
- general notes

Commands:
- `owner`
- `owner edit`

Main file:
- [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)

### 4. Common app opening is more direct now

Added:
- `desktop.open_workspace_app`

This tool resolves common targets in one step instead of forcing the model to invent URLs/app names each time.

Current built-in targets include:
- Gmail
- WhatsApp
- Telegram
- Codex / ChatGPT
- YouTube
- GitHub
- Google Calendar
- Google Drive
- Google Docs / Sheets / Slides
- Spotify
- a few native apps like Firefox, terminal, code, file manager

It also accepts `query` and `prefer_native` and falls back to web search for unknown app names.

Files:
- [os_tools.rs](/home/rick/hyprclaw/hypr-claw-tools/src/os_tools.rs)
- [tool_setup.rs](/home/rick/hyprclaw/hypr-claw-app/src/tool_setup.rs)
- [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)

### 5. Prompt/runtime now uses owner + browser context

`augment_system_prompt_for_turn(...)` now includes:
- owner preferences
- owner approval notes
- browser tool availability
- stronger routing hints for browser tasks like Gmail, WhatsApp Web, Telegram Web, Codex

This means the runtime should prefer browser tools before pixel-only automation for browser-heavy tasks.

### 6. Interactive mode now has a single-screen operator home

The interactive session no longer just accumulates prints forever between prompts.

Current behavior:
- clears to a single operator screen between runs
- shows model/provider/health/tools/thread
- shows last task result or failure summary
- shows recent live trace tail
- uses a minimal `›` prompt

This is still ANSI-rendered terminal UI, not a full ratatui/crossterm dashboard, but it is much closer to a real operator console than the old scrollback-heavy loop.

Key file:
- [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)

### 7. CLI is simpler and more agent-like now

Added a live operator timeline observer:
- stages like `task`, `think`, `tool`, `state`, `done`, `fail`, `error`
- streamed directly during runs
- integrated with the existing debug event system

Files:
- [debug_timeline.rs](/home/rick/hyprclaw/hypr-claw-app/src/debug_timeline.rs)
- [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)

The current console is intentionally much lighter:
- short boot summary
- short task header
- live trace lines
- compact success/failure ending
- single-screen home between tasks

There is still no full-screen TUI. This is a simplified operator CLI, not a real terminal dashboard.

### 8. Approval UX is improved

Approvals are no longer only “Approve action [y/N]”.

The permission adapter now classifies risk for actions such as:
- `browser.action`
- `desktop.type_text`
- risky key presses / key combos
- sensitive `fs.write`
- `proc.kill`
- `hypr.exec`
- shutdown/reboot/system-impacting operations

The prompt now shows:
- risk
- tool
- reason
- summarized input
- timeout

File:
- [permission_adapter.rs](/home/rick/hyprclaw/hypr-claw-infra/src/infra/permission_adapter.rs)

## How The Runtime Currently Works

High-level path:

1. CLI input reaches [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)
2. Runtime builds allowed tool set from:
- capability registry
- live runtime health
3. `focused_tools_for_input(...)` narrows tool set for some task classes and now strongly prefers `desktop.open_workspace_app` for common app targets
4. `augment_system_prompt_for_turn(...)` injects:
- machine context
- owner context
- capability info
- workflow rules
5. [agent_loop.rs](/home/rick/hyprclaw/hypr-claw-runtime/src/agent_loop.rs) runs the LLM/tool loop
6. Tool calls go through `RuntimeDispatcherAdapter` in [main.rs](/home/rick/hyprclaw/hypr-claw-app/src/main.rs)
7. Permission checks go through infra adapter
8. Debug events now feed the live timeline observer

## Verified State

These checks passed after the current changes:

```bash
cargo fmt --all
cargo check -p hypr-claw-app
cargo test -p hypr_claw permission_adapter -- --nocapture
cargo check -p hypr_claw_tools -p hypr-claw-app
```

Known warnings still exist in scan parser code and are not from the recent changes:
- [mod.rs](/home/rick/hyprclaw/hypr-claw-app/src/scan/parsers/mod.rs#L3)
- [hyprland.rs](/home/rick/hyprclaw/hypr-claw-app/src/scan/parsers/hyprland.rs#L5)

## Current Weak Points

These are still open:

1. No full app-specific adapters yet.
- `desktop.open_workspace_app` helps a lot, but there are still no dedicated `gmail.*`, `whatsapp.*`, `telegram.*`, `codex.*` workflow tools.

2. No self-tool-generation workflow.
- The project still cannot safely detect a missing tool, propose it, get approval, create it, verify it, then use it.

3. No true hackathon-grade TUI/web console yet.
- The CLI is better, but still not a split-pane operator dashboard with previews and approval cards.

4. Approval persistence is missing.
- Approvals are per-action CLI prompts. There is no reusable grant model.

5. Browser tools depend on Pinchtab actually running.
- If Pinchtab is unavailable, the system falls back to desktop/browser pixel flows.

6. “Perfect OS assistant” behavior is not done.
- The foundations are stronger, but not every prompt can yet be completed end-to-end reliably.

## Best Next Work

If another agent continues, do these in this order:

1. Build a real demo UI.
- Split-pane terminal UI or local web UI.
- Show live task, current tool, approvals, recent timeline, transcript, and maybe screenshot/snapshot preview.

2. Add app adapters on top of browser tools and `desktop.open_workspace_app`.
- `gmail.open`, `gmail.read_threads`, `gmail.compose`
- `whatsapp.open_chat`, `whatsapp.read_recent`, `whatsapp.send_message`
- `telegram.open_chat`, `telegram.read_recent`, `telegram.send_message`
- `codex.open`, `codex.prompt`, `codex.capture_response`

3. Upgrade approval model.
- Persist approvals.
- Add scoped grants.
- Separate “observe allowed” from “act requires confirmation”.

4. Add missing-capability flow.
- If model needs a tool that does not exist, it should surface a proposal instead of failing vaguely.

5. Improve runtime latency.
- Current loop still does extra work and the UI is event-based but not optimized for minimal delay.

## Practical Notes For Future Agents

### Repo state

There are already local modifications in progress across:
- `hypr-claw-app`
- `hypr-claw-tools`
- `hypr-claw-infra`

Do not revert unrelated changes blindly.

There are also local vendored directories:
- `pinchtab/`
- `picoclaw/`

They are useful reference implementations. The user explicitly wants this project to borrow ideas/capabilities from them.

### Package names

Useful workspace package names:
- `hypr-claw-app`
- `hypr_claw`
- `hypr_claw_tools`
- `hypr-claw-runtime`

### Commands worth using

```bash
cargo check -p hypr-claw-app
cargo check -p hypr_claw
cargo check -p hypr_claw_tools
cargo check -p hypr_claw_tools -p hypr-claw-app
cargo test -p hypr_claw permission_adapter -- --nocapture
```

### Search starting points

Use these when resuming:
- `augment_system_prompt_for_turn`
- `focused_tools_for_input`
- `desktop.open_workspace_app`
- `RuntimeDispatcherAdapter`
- `derive_runtime_allowed_tools`
- `BrowserActionTool`
- `OwnerProfile`
- `TimelineDebugObserver`
- `classify_permission_request`

## Do Not Lose This Direction

The user wants:
- one prompt interface
- broad OS control
- browser + desktop + app operation
- context gathered automatically first, manually when needed
- strong tool use
- approval for serious actions
- a polished hackathon-grade presentation

The current codebase is now closer to that, but not finished. Continue from the runtime/UI/approval/browser direction rather than adding unrelated abstractions.
