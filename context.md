# Hypr-Claw Context

Last updated: 2026-02-24

## Purpose

Hypr-Claw is a local Linux OS operator focused on Hyprland workflows. The user gives a prompt, and the agent should execute real desktop work end-to-end with tools, verification, and clear results.

This file is the shared context for all future model contributors. Read this first before changing code.

## Product Ideology

1. One powerful mode.
   The project intentionally uses a single power-agent workflow. Do not reintroduce multiple personalities, dashboard mode, or trust/safe mode toggles.
2. Prompt decides behavior.
   Avoid hardcoded step trees for specific tasks. The model should choose tools dynamically from runtime-allowed capabilities.
3. Observe, act, verify.
   Every GUI task should follow strict loop: observe current state, execute one decisive action, verify state change, continue.
4. Reliability over feature count.
   A smaller set of tools that actually works is better than many brittle tools.
5. Freedom with accountability.
   Keep explicit permission prompts for high-impact/destructive actions and keep auditable execution history.

## End Product Target

Build a dependable local OS assistant that can perform normal human desktop work:
- browser workflows (mail, research, forms)
- messaging and communication actions
- coding and project operations
- file and system tasks
- multi-step workflows with minimal user intervention

Success standard: user prompt -> completed outcome, not just "opened app" or "cannot proceed."

## Current State (Important)

1. Runtime
- CLI-first assistant with single power-agent profile.
- Strict workflow language is already in system prompt.
- Capability registry filters tools by detected machine capabilities.

2. Scanning and context
- Onboarding includes standard/deep scan modes.
- Registry and profile are persisted under `data/`.
- Deep scan can be expensive; current strategy should move toward summary-first and on-demand deep reads.

3. GUI tooling
- Desktop tools include window listing, active window, screenshot, OCR, keyboard, mouse, app launch, and URL open.
- New non-OCR tools exist for stronger control:
  - `desktop.cursor_position`
  - `desktop.mouse_move_and_verify`
  - `desktop.click_at_and_verify`
  - `desktop.read_screen_state`
- Runtime tool gating now differentiates keyboard vs pointer backends.

4. UX
- Transcript and compact views exist.
- Output readability still needs significant refinement (reduce clutter, stronger hierarchy, better progress signaling).

## Known Gaps Blocking Quality

1. Mouse backend reliability on Linux/Wayland is still environment-sensitive.
- Example: `ydotoold` may fail without `uinput` permissions.
- Tool availability should reflect executable reality, not only binary presence.

2. Some model responses still stop at policy-style refusals even when task is feasible via GUI automation.
- Need stronger execution policy to force full attempt before declaring blocked.

3. CLI UX is functional but visually noisy.
- Repeated panels, dense logs, and duplicated response blocks reduce readability.

4. Deep scan performance and context bloat.
- Need index/summarize-first scanning with demand-driven full reads.

## What To Build Next

### Phase 1: Reliability Foundation (highest priority)

1. Backend truth checks
- Add runtime health probes for input/screenshot/OCR backends.
- Only expose tools if the backend can actually execute.
- Surface one-line health in status: screen, keyboard, pointer, OCR.

2. Deterministic GUI action loop
- Standardize action cycle:
  - `desktop.read_screen_state`
  - decide action
  - `mouse_move_and_verify` / `click_at_and_verify` / keyboard action
  - re-read and verify goal progress

3. Strong failure handling
- Each tool failure must produce fallback suggestion and auto-fallback path (not silent dead-end).

### Phase 2: Scan and Memory Performance

1. Two-layer scan model
- Permanent context: stable system facts, configs, package summary, capability profile.
- Temporary context: recent tasks/artifacts/screen workflow traces.

2. Smart ingestion
- Build file index and metadata first.
- Read full file contents only when required by user task or conflict resolution.
- Maintain freshness hashes/timestamps to avoid repeat heavy scans.

3. Context compression
- Periodically compress temporary context into short summaries and keep links to raw artifacts.

### Phase 3: UX and Adoption

1. Default interface should be clean chat-first.
- Minimal header.
- Single progress line during execution.
- Tool logs collapsed by default, expandable on demand.

2. Shareable run artifacts.
- Export a run summary timeline that users can paste/share.

3. Task quality benchmarks.
- Maintain a top-task benchmark suite (mail triage, browser research, coding flow, file ops).
- Track success rate, median latency, recovery count.

## Architecture Snapshot

- `hypr-claw-app/`: CLI entrypoint, onboarding, command loop, capability registry, prompt assembly.
- `hypr-claw-runtime/`: execution loop and orchestration logic.
- `hypr-claw-tools/`: OS tools and desktop capability adapters.
- `hypr-claw-infra/`: permissions, audit, session/lock infrastructure.
- `crates/`: shared platform modules (`core`, `memory`, `providers`, `policy`, `tasks`, etc.).

## Contributor Protocol (How Models Should Work)

1. Read order before coding
- Read `context.md`, then `README.md`, then target files.

2. Preserve direction
- Do not add back removed systems (soul switching, dashboard, autonomy/safe mode splits).
- Keep one-mode power workflow.

3. Keep changes pragmatic
- Prefer small, testable edits over broad rewrites.
- Remove dead code paths when confidently obsolete.

4. Make tool behavior observable
- Improve logs/transcript with clear tool call, outcome, and verification state.
- Avoid hidden implicit state transitions.

5. Validate before handing off
- Run:
  - `cargo check -p hypr_claw_tools`
  - `cargo check -p hypr-claw-app`
- Run focused tests when touching execution/tool logic.

6. If blocked, report precisely
- State exact blocker (backend, permission, missing dependency, design ambiguity).
- Provide smallest next action to unblock.

## Definition of Done For New Work

A change is complete only if:
1. It improves real task completion rate or execution clarity.
2. Runtime behavior matches this ideology (observe-act-verify, dynamic tools, explicit permission for high-impact actions).
3. It passes compile checks and does not regress existing workflows.
