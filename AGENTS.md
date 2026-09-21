# IPP — Agent Guide

IPP is a headless Rust runtime for interactive 3D scenes. Follow the architecture and verify the implemented capabilities before assuming a planned system exists.

## Work tracking

Use Beads for executable work, claims, dependencies and handoffs. Follow the [coordination setup](docs/development/coordination.md) and [development workflow](docs/development/workflow.md).

Review inherited tasks against current architecture before claiming them; a task never overrides the docs. Do not create a replacement roadmap or task-tracking file.

## Start every task

1. Run `git status --short` and `git worktree list`. Preserve existing edits and deletions.
2. Read [the architecture overview](docs/architecture.md), relevant topic documents, and the corresponding [implementation strategy](docs/plans/README.md). Read any more specific `AGENTS.md` in the paths you will edit.
3. Read [the development workflow](docs/development/workflow.md). Establish scope and validation from the user's request; state the immediate approach briefly, then do authorized work.
4. Follow the Beads claim workflow before implementation.

After compaction or handoff, reread the current documents and inspect the actual diff. Do not infer completion from a previous agent's summary.

## Source of truth and architectural review

**`docs/architecture.md` and `docs/architecture/` own the high-level design:** system boundaries, ownership, key invariants and direction. Plans, tasks and code must follow those decisions. Implementation details belong in source code and adjacent module/package documentation, within that design.

If an approach requires changing an architectural decision:

1. Prepare a concrete proposed architecture-doc change with its reason, affected behavior, and tradeoffs. Keep the proposal clearly identified as pending review.
2. Review that decision with the user before adopting or implementing the changed architecture. An explicit decision already approved in the conversation need not be approved again.
3. Update the architecture with the accepted decision, then align the affected strategies, implementation, and active tasks. Never leave the accepted design only in chat or a task description.

Continue independent work while a decision awaits review. Formatting, link fixes, document reorganization, and routine implementation choices within the accepted architecture do not require another architectural review.

## Where information belongs

| Location | Owns |
| --- | --- |
| `docs/architecture.md` and `docs/architecture/` | Big-picture design, subsystem boundaries, ownership and key invariants |
| `docs/plans/` | High-level implementation strategies that reference and expand the architecture |
| Source code and adjacent module/package docs | APIs, algorithms, formats, constants, recipe catalogs and usage examples |
| Beads | Executable tasks, dependencies, milestones, ownership, detailed acceptance criteria, and handoffs |
| This guide and `docs/development/` | Contributor workflow, review, and validation rules |

Keep plans directional: implementation approach, areas to investigate, and validation strategy. Do not put roadmaps, delivery phases, live status checkboxes, exhaustive API/storage sketches, or task queues in them. Add implementation-level detail with the actual implementation and its task when needed. Update relevant documents in the same change; do not create `TODO.md`, `MEMORY.md`, or a second task system.

Keep architecture concise and explain each decision once in its owning topic. Link to source for implementation details instead of copying parameter catalogs, command fields, shader equations, validation case lists or example UI behavior into architecture. Merge overlapping topics when that improves the big picture. Removing detail from architecture does not authorize changing implemented behavior.

Keep READMEs focused on purpose, intent, subsystem boundaries and stable high-level contracts. Link to architecture for design, source for APIs and algorithms, and development guides for setup and validation. Keep runnable entry points where useful; do not duplicate field/default/limit catalogs, file inventories or exhaustive test cases. Dedicated reference guides should explain information readers cannot readily obtain from types and maintained examples.

## Rust file organization

- Keep standalone production modules in `name.rs`. When a module has production submodules, use `name/mod.rs` with its children in that directory. In `src/`, do not create a directory solely for one test or test-support file.
- Keep `mod.rs` focused on module declarations, re-exports and shared boundary types. Put substantial system, service, evaluation and algorithm implementations in named files; use the established `system.rs`, `system_state.rs` and `update.rs` roles consistently within system modules.
- Name files for the domain data or operation they contain. Prefer explicit names such as `render_state.rs` for rendering settings and `component_inputs.rs` for overlay input bookkeeping. `component.rs` and `components.rs` contain one or several actual ECS component definitions beside their evaluator.
- Put substantial unit-test suites in descriptive `*_tests.rs` files beside the code they exercise. Small focused tests may remain inline. Group tests by the implementation or invariant they verify; generic World storage tests belong with World storage, not a rendering module.
- Preserve private test access when extracting or flattening files. A local `#[path = "name_tests.rs"] mod tests;` is appropriate for an adjacent test file belonging to a standalone module; apply the same approach to adjacent test support. Production module paths must follow their directories rather than importing implementation from another subsystem through `#[path]`.
- Keep crate-level `tests/` for tests using the crate's external API and source-local tests for private invariants. Shared integration-test helpers may use `tests/support/mod.rs` so Cargo does not discover them as separate test targets. Preserve test coverage, conditional compilation and subsystem ownership during moves; do not widen implementation visibility merely to relocate tests. Update affected imports, source links and module documentation in the same change.

## Working rules

- Before stabilization, backward compatibility is not a requirement. Change APIs, contracts and formats directly; regenerate clients and fixtures. Do not add compatibility shims, legacy paths or migrations unless explicitly requested.

- Prefer `rg` for discovery; inspect the relevant files before editing. Preserve unrelated user changes and deletions.
- Format TypeScript/JavaScript with Biome, Python with Ruff, and Rust with rustfmt using the repository-pinned tools. For source changes, run `npm run format` to apply formatting and `npm run format:check` before handoff; see [setup and per-language commands](docs/development/building.md#formatting). For documentation-only changes, format and check the changed Markdown files with pinned Prettier. Format maintained templates at their source; regenerate target-specific output through the build pipeline.
- Format architecture Markdown with pinned Prettier (`python tools/ipp.py format --language md`). Keep prose paragraphs and list items on single source lines without column-width wrapping; preserve intentional paragraph breaks and code-block layout.
- Rust readability requires one blank line between functions, methods, types and impl blocks, and between distinct logical steps within a function. Keep documentation and attributes attached to their item, and keep related statements together. Write macro bodies with the same spacing and multiline layout as ordinary Rust. Stable rustfmt preserves these blank lines but does not insert missing ones; review source readability as well as formatter output.
- Keep reusable client and integration code independent of examples and tests. Examples own their application assets; tests may consume real application fixtures while retaining independent expected-result calculations. Colocate test helpers with their actual suite or established shared harness. `tools/check_repo.py` checks literal source imports and Rust module layout; computed imports and responsibility cohesion still need review.
- Keep work bounded and use separate source worktrees for concurrent implementation. Create manual task worktrees only under the ignored `.worktrees/<beads-task-id>/<worker-or-purpose>/` path in the primary checkout; do not place them directly under the checkout root or in ad-hoc parent directories. Shared interfaces need an explicit integration owner. Never overwrite another owner's work or assume uncommitted files follow a new worktree.
- After work is merged into `main`, the integration owner must remove every implementation and supporting worktree used for that task, including subagent worktrees, before final handoff. Stop task-owned processes, preserve any remaining unmerged or uncommitted work in a verified recovery archive, then run `git worktree remove <path>` from the primary checkout. Always keep the primary checkout. Verify cleanup with `git worktree list` and record it in Beads; do not leave completed worktrees behind.
- Hosts own runtime clocks and event loops. Client transports enqueue asynchronous work and observe outcomes/events; never expose simulation time advancement or individual evaluation steps through the production client protocol.
- Follow the [diagnostic logging policy](docs/architecture/runtime.md#diagnostic-logging). Use selectable levels for lifecycle and command boundaries, filter before formatting, and keep frame/draw/evaluation hot paths quiet. Logs must describe committed effects accurately and remain separate from semantic outcomes/events.
- Use the [runtime naming conventions](docs/architecture/runtime.md#runtime-terminology): Host, Service, World, System and StateOverlay. Domain type names must identify their subsystem and role even outside their module; organize implementations under `services/<service>` and `systems/<system>`.
- Keep `ipp-core` headless, apply ordered batches without rollback and preserve fixed frame order, and keep occupied storage stable with invalidation before reuse. Read the architecture for the full contracts and exceptions.
- Keep ordinary component values in one stable typed store. Retain hidden producer values only for controlled properties and animation restoration values only for bound targets; do not introduce a universal component mirror. Preserve explicit owned/bound entity semantics and Bound/Owned/Auto component semantics; lifecycle policy stays in the core.
- Generate target-correct contracts; never substitute host layouts for WASM or hand-edit generated output. Preserve asset ownership and replacement-session isolation.
- Follow the [Rust workspace/dependency policy](docs/architecture/rust-workspace.md). Keep optional capabilities and their registrations/shaders out of lean builds, separate host compilation tools from runtime dependencies, and justify production dependencies with their transitive and artifact costs. Share GL rendering through compile-time WebGL/GLES devices; keep context libraries and optional shims in the relevant host distribution.
- Prefer safe Rust. Every `unsafe` block needs a `// SAFETY:` explanation covering lifetime, invalidation, and aliasing as applicable. Unordered iteration must not determine order-sensitive behavior.
- Complete authorized reversible work without repeated permission requests. Architectural changes still require the review above; tooling instructions do not expand authorization for publishing or destructive actions.
- Write commit messages and PR titles in plain language that captures the high-level change. Explain why when it adds useful context; do not force a rationale for self-explanatory changes. Do not use type prefixes such as `chore:`, `feat:`, or `fix:`.
- When authorized to commit, stage explicit paths and inspect the staged diff. Never include AI attribution, co-author trailers, or telemetry in commits or PRs.

## Iteration loop

- Read the relevant code, finish a coherent change, then run the smallest check that answers the next correctness question. An early compiler check or focused test is useful for a concrete uncertainty; do not interrupt every small edit with a routine build/test chain.
- Keep formatting, Clippy, repository checks and broader caller validation at meaningful checkpoints and handoff. Do not default to `edit → format → check → test → Clippy` after each edit. Complete the required validation below and reuse passing evidence while its inputs remain unchanged.
- Stabilize shared interfaces with their integration owner before dependent agents implement against them. Exchange coherent snapshots on a known base; avoid repeatedly copying partially migrated files between worktrees and using compiler failures to discover interface changes. Continue independent work while a dependency is unavailable.
- Establish a working browser/GLES environment before expensive integration runs and reuse that setup across agents and retries. Inspect failure logs first, rebuild affected prerequisites, and rerun the smallest affected maintained scenario or suite. Keep regression retries under the prescribed regression entry point.
- Batch independent searches and reads. Discover paths and symbols first, then request bounded excerpts; narrow a query when output would be truncated. Within an uninterrupted task, reread unchanged material only for a specific question. Preserve the required startup and post-compaction review.
- While a long check runs, do useful independent work without changing that check's inputs. Poll at a cadence suited to its expected duration, normally tens of seconds, and inspect compact status or failure excerpts. Retain full logs as artifacts instead of repeatedly printing them.
- Keep required claims and handoffs, but coalesce routine Beads updates and inter-agent messages at meaningful boundaries. Communicate changed contracts, blockers, actionable findings and completed handoffs promptly; avoid repeating unchanged status or full validation inventories after individual tool calls.

## Integration testing policy

- Follow the [integration testing policy](docs/development/integration-testing.md). Feature delivery includes a reusable harness exercising the real participating runtime, generated client, protocol/transport, assets, and rendering. Extensive mocking is not a substitute for integration evidence.
- Every implementation plan must describe a realistic harness: environment, fixtures, observable assertions, and how it extends to new protocols or process arrangements. Keep scenarios independent of process launch and wire layout; add small drivers/runners as needed instead of disposable stage-specific harnesses.
- Start with the smallest real environment that proves the feature. Support a future process boundary without building every deployment arrangement now. Direct-core tests do not count as real transport or rendering coverage.
- Rendering coverage includes actual frame capture and meaningful image assertions, supported by state/events. Await readiness and frame completion, collect failure artifacts, and clean up all owned processes, workers, and connections.
- Focused unit tests are useful for local invariants and algorithms; they supplement real integration tests. Update the maintained harness, relevant scenarios, and CI evidence with behavior changes.

## Validate and hand off

Scope validation to the work being done. Run the relevant tests, package checks, and real integration scenarios for changed behavior and its affected callers. Use named suites such as `python tools/ipp.py test cameras` and targeted Cargo packages/tests during ordinary development; do not run the full regression or workspace/feature matrix without the trigger below.

Run a full regression pass only when the user instructs you to merge into `main` or push. A plain commit request, a handoff, or working directly on `main` does not trigger it. One integration owner runs that pass for the combined changes; subagents report their focused evidence instead of repeating it.

For regression testing, agents must use only `python tools/ipp.py regression` ([script](tools/ipp.py), [usage](docs/development/building.md#regression-entry-point)). This includes regression retries: use `--retry <summary.json>` for unfinished work and add affected checks with repeatable `--only` or `--suite` options instead of assembling Cargo/npm/Python command chains, invoking the full test runner separately, or slicing command arrays. Inspect `--list` or `--plan` when needed. Configure native GLES through `IPP_EGL_LIBRARY_DIR` where applicable. Keep the script running through independent failures and use its logs and `summary.json` for the handoff. Reuse passing results while their relevant source, build configuration, and environment remain unchanged; after fixes, run affected selections through the same script and report their partial scope rather than restarting unrelated suites. Extend the shared suite/build registries or regression check catalog when coverage changes.

Run `python3 tools/check_repo.py` and `git diff --check`. For Rust changes, use [the build guide](docs/development/building.md) and applicable implementation checks in [the validation workflow](docs/development/workflow.md#validation). Distinguish successful scaffold builds from implemented runtime or integration behavior.

Review the diff for scope, correctness, and architectural consistency. Report actual validation, remaining risks, and the next action. Record the handoff in Beads using the [handoff template](docs/development/handoff-template.md); use `in_review` while integration remains and close only after acceptance, checks, and integration or explicit user acceptance.

`CLAUDE.md` is a symlink to this file. Edit `AGENTS.md` to keep both agent entry points consistent.
