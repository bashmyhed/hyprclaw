# PicoClaw Pattern Import Plan (for Hypr-Claw)

## Purpose
This document captures a senior-engineering plan to import high-value architecture patterns from PicoClaw into Hypr-Claw.
It is a planning artifact only. No implementation is assumed yet.

## Core Principle
Adopt behavior and contracts, not language-specific code.
- PicoClaw is Go, Hypr-Claw is Rust.
- We import patterns, interfaces, and operational semantics.

## Current State (Hypr-Claw)
1. `hypr-claw-app`
- Strong orchestration and UX controls, but `main.rs` is monolithic.
- Command routing, supervisor, runtime orchestration, and UI concerns are tightly coupled.

2. `hypr-claw-runtime`
- Good loop/tool-call structure, retries, and timeout foundations.
- Missing first-class provider fallback chain and cooldown policy.

3. `hypr-claw-tools`
- Strong capability adapters and dispatcher.
- Tool visibility is mostly static; no hidden->promoted TTL lifecycle.

4. `hypr-claw-infra`
- Permissions, audit, locking, sessions, credentials are present.
- Session persistence is functional but not fully append-first JSONL by default.

5. Shared crates (`crates/*`)
- Useful abstractions exist.
- New/legacy overlap remains and needs disciplined convergence.

## Improvement Themes (Why + Target)

## A. Provider Fallback Chain with Cooldowns
Why:
- Provider/model instability is a major cause of failed runs.
- Retry alone is insufficient; deterministic failover is needed.

Target:
- Candidate chain: primary + fallbacks.
- Per-model cooldown windows after failures.
- Retryable vs non-retryable classification.

Primary modules:
- `hypr-claw-runtime` (`fallback_policy`).
- `hypr-claw-app` (provider/model config wiring).
- `crates/providers` (capability/error metadata).

Acceptance:
- Automatic model failover on eligible failures.
- Cooldown prevents repeated thrashing on unhealthy models.
- Metrics include fallback attempts and fallback success rate.

## B. Command Router Outcome Contract
Why:
- Command handling and free-form task execution should be explicit and decoupled.
- Reduces branching complexity in app loop.

Target:
- Unified command executor returns:
- `Handled`
- `PassthroughToLLM`

Primary modules:
- `hypr-claw-app/commands` (`router`, `outcome`).

Acceptance:
- Deterministic command path.
- Reduced complexity in orchestration loop.

## C. Tool Visibility Lifecycle (Hidden -> Promoted with TTL)
Why:
- Large static tool sets increase wrong tool-call rate.
- Context-aware visibility improves model precision.

Target:
- Core tools always visible.
- Discovered tools promoted temporarily via TTL.
- Automatic expiry and deterministic ordering.

Primary modules:
- `hypr-claw-tools` registry extensions.
- `hypr-claw-runtime` tool selection context.

Acceptance:
- Promoted tools expire without manual cleanup.
- Lower tool misuse and improved completion quality.

## D. JSONL Append-First Session Backend
Why:
- Append logs improve durability and replayability.
- Better resilience to partial writes/crashes.

Target:
- Append message events to JSONL.
- Snapshot/compact/truncate policy.
- JSON -> JSONL migration path.

Primary modules:
- `hypr-claw-infra` session store backend.
- Runtime adapters for load/replay/compact hooks.

Acceptance:
- Crash-safe partial recovery from log.
- Migration tested and reversible.

## E. Internal Message Bus
Why:
- Decouples ingress/egress transport from agent loop.
- Enables clean expansion to multi-channel gateway mode later.

Target:
- Typed inbound/outbound envelopes.
- Bounded buffers and backpressure behavior.
- Context-aware shutdown semantics.

Primary modules:
- `hypr-claw-runtime` or `hypr-claw-app` bus module.

Acceptance:
- Agent loop can consume from bus and publish responses.
- Backpressure and shutdown are deterministic.

## F. Scheduling Layer (Cron + Heartbeat)
Why:
- Recurring workflows are a practical product multiplier.
- Needs durable job state and safe execution envelope.

Target:
- Persisted jobs with schedule kinds: `at`, `every`, `cron`.
- Execution status tracking.
- Optional delivery context for output routing.

Primary modules:
- `hypr-claw-infra` scheduler state store.
- `hypr-claw-app` command surface.
- `hypr-claw-runtime` execution callback entrypoint.

Acceptance:
- Jobs survive restart.
- One-shot jobs auto-clean after execution.
- Error/status visibility per job.

## G. Light/Heavy Model Routing
Why:
- Cost/latency optimization without sacrificing quality.

Target:
- Complexity scoring from input features.
- Route simple tasks to light model, complex to heavy model.
- Log every routing decision.

Primary modules:
- `hypr-claw-runtime/routing`.
- App config for thresholds and model sets.

Acceptance:
- Cost per completed task decreases.
- Completion rate does not regress.

## Phased Delivery Plan

## Phase 0 (1 week): ADR Baseline
Deliverables:
- ADRs for A-G.
- Interface contracts and boundary decisions.
- Explicit anti-goals and "do not import" list.

Gate:
- Team signoff on contracts before implementation.

## Phase 1 (2-3 weeks): Reliability Core
Scope:
- A (fallback/cooldown), B (command outcome contract).

Why first:
- Highest reliability gain, lowest migration risk.

Gate:
- Integration tests for fallback and command routing pass.

## Phase 2 (2-3 weeks): Tool Quality + Persistence
Scope:
- C (tool TTL promotion), D (JSONL backend + migration).

Gate:
- Tool precision metrics improve.
- Persistence durability tests pass.

## Phase 3 (3-4 weeks): Bus + Scheduling Foundations
Scope:
- E (message bus), F (scheduler/heartbeat).

Gate:
- Stable bounded queues.
- Restart-safe scheduled execution.

## Phase 4 (2 weeks): Optimization
Scope:
- G (light/heavy routing), tuning, benchmark hardening.

Gate:
- Cost/perf gains with no completion regression.

## Ownership Model
1. Runtime owner
- Fallback policy, routing, bus interfaces, loop integration.

2. Tools owner
- Tool registry visibility lifecycle and discovery promotion.

3. Infra owner
- JSONL session backend, scheduler persistence, audit correlation IDs.

4. App owner
- Command router contract, CLI integration, config wiring.

## Risks and Mitigations
1. Over-migration risk
- Mitigation: phase gates and strict scope per phase.

2. Legacy/new stack confusion
- Mitigation: designate source-of-truth module per capability.

3. Regression risk from refactoring
- Mitigation: strangler pattern and interface-first migration.

4. Tool sprawl reducing precision
- Mitigation: TTL promotion and tighter default visibility.

## Success Metrics
1. Task completion rate.
2. Fallback invocation and fallback success rate.
3. Tool-call failure rate.
4. Iterations to completion.
5. Session recovery incidents.
6. Cost per completed task.

## Planning Artifacts to Produce Next
1. ADR set for themes A-G.
2. Interface specs:
- `FallbackPolicy`
- `CommandOutcome`
- `PromotableToolRegistry`
- `SessionLogStore`
- `MessageBus`
- `ScheduleStore`
- `ComplexityRouter`
3. Test matrix per phase (unit/integration/e2e).

