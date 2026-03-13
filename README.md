# Hypr-Claw

Hypr-Claw is a local Linux OS assistant focused on Hyprland-style desktop control. The model decides actions from user prompts and executes tools to work like a human operator: observe screen, move cursor, type, open apps, read content, and report results.

## Team Context

This project is being built by a core team of three people:

1. Agent/runtime owner: prompt strategy, orchestration loop, memory behavior.
2. Desktop/tooling owner: screen automation, keyboard/mouse control, OCR, app actions.
3. Platform/quality owner: infrastructure, tests, packaging, release stability.

## Current Context

Current implementation direction is single-mode power agent:

- No soul switching mode.
- No dashboard mode.
- No safe-mode or trust-mode toggle path.
- Prompt-first execution with strict observe-plan-act-verify workflow.
- Dynamic tool availability from real system capabilities.
- Smart scan and memory context persistence for system understanding.

## Product Philosophy

- One powerful agent mode, not many personalities.
- Fewer hardcoded branches, more model-driven decisions.
- Fast context acquisition: summarize first, deep read only when needed.
- Practical autonomy: strong tools with explicit approval on high-impact actions.
- Keep architecture simple enough for a small team to ship quickly.

## End Product Goal

The end product is a reliable Linux OS copilot that can execute real desktop work end-to-end from natural prompts, including:

- messaging and mail workflows
- browser and research tasks
- coding and file operations
- system configuration and automation
- routine productivity actions across applications

## What Works Today

- Terminal REPL agent runtime.
- Onboarding plus system scan (standard and deep).
- Capability registry and context memory persistence.
- Queue and background task execution.
- Model switching support.
- Desktop automation tools including OCR, cursor movement, typing, key combos, window and app actions.

## Work Needed

1. Improve reliability of long multi-step desktop workflows.
2. Strengthen tool fallback logic for mixed environments.
3. Expand permission model clarity for high-impact actions.
4. Harden tests around GUI automation and recovery.
5. Reduce remaining legacy dead code paths and keep interfaces minimal.
6. Package and installation flow for user onboarding at scale.

## Architecture

Workspace layout:

- `hypr-claw-app/`: CLI entrypoint, onboarding, command loop.
- `hypr-claw-runtime/`: agent loop, tool-call orchestration, execution control.
- `hypr-claw-tools/`: concrete tool implementations and OS capability adapters.
- `hypr-claw-infra/`: permissions, audit, sessions, locking infrastructure.
- `crates/`: shared modules (`core`, `memory`, `providers`, `policy`, `tasks`, etc.).

Execution flow:

1. Startup loads config, state, memory, and capability profile.
2. Runtime exposes only tools supported by the current machine.
3. User prompt enters strict action loop.
4. Model selects tools, executes steps, verifies progress, and continues.
5. State, history, and task outcomes persist to local data files.

## How We Work

Team workflow:

1. Keep one active architecture direction and remove old paths quickly.
2. Prefer small, testable changes merged frequently.
3. Document only in this `README.md`; avoid doc sprawl.
4. Treat runtime behavior changes as product changes: update README in same PR.
5. Run `cargo check` and relevant tests before merge.

Recommended local commands:

```bash
cargo check --workspace
cargo test --workspace
```

## Requirements

- Rust 1.75+
- Linux (Hyprland target first)
- Tool dependencies for full desktop control:
  - `wtype` or `ydotool`
  - `wlrctl`
  - `grim` or `hyprshot`
  - `tesseract` and English language data
  - `swww` (optional wallpaper control)

## License

This project is licensed under MIT. See `LICENSE`.
