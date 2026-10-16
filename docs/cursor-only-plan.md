# Cursor Desktop-only reduction — scope and phasing plan

Status: **planning**. Branch: `hermes/cursor-desktop-only`. No code removed yet.

## Goal (from brief)

Transform this fork into a Cursor Desktop-only product. Remove every
other coding-harness integration (Claude Code, Codex, Gemini CLI,
OpenCode, Qoder, Kimi, Kiro, Grok, Antigravity, Copilot CLI, Command
Code, Muse, CodeBuddy, IBM Bob, Pool, Devin, OpenClaw, Zed, Claude
Desktop, Omp, Pi, Crush, Zero, Hermes, Grizzybot, Droid, Zcode). Keep
the core/store/wiki/MCP/web/admin/auth plumbing, LLM and embedding
providers (Anthropic/OpenAI/Gemini/Copilot *as LLM providers*, not
harnesses), and import/export.

The product name stays `ai-memory` for now. Cursor Cloud, Cursor CLI,
and Cursor Tab are also out of scope; the target surface is Cursor
Desktop's Agent Chat and Cmd+K with local hooks and the MCP
configuration format Cursor Desktop reads from `~/.cursor/mcp.json`.

## Measured scope

| Dimension | Value |
|---|---|
| `AgentKind` variants to drop | 25 of 26 (keep `Cursor` + `Other`) |
| `AgentKind::*` call-sites in Rust workspace | 1,453 |
| Hook bundles in `hooks/` to delete | 13 of 14 (keep `hooks/cursor/`) |
| `install_hooks.rs` size | ~16 kLOC, single dispatcher |
| `install_mcp.rs` `McpClient` variants | 20+ (keep Cursor only) |
| `ManagedHarness` variants in `ai-memory-workstream` | 14 (zero are Cursor) |
| `ai-memory-hooks/` crate | ~44 kLOC, ~525 `AgentKind` refs |
| Store migration V26 CHECK constraint | enumerates all 27 kinds |
| Marketplace plugins to remove | `plugins/claude-code`, `plugins/codex` |
| Companions potentially affected | 7 (`ai-memory-macos` definitely goes; others are harness-neutral but ship multi-harness assumptions) |

Call-site density (number of `AgentKind::*` refs in file):
- `crates/ai-memory-hooks/src/capture_policy.rs` — 260
- `crates/ai-memory-hooks/src/workstream.rs` — 88
- `crates/ai-memory-hooks/src/payload.rs` — 70
- `crates/ai-memory-hooks/src/router.rs` — 63
- `crates/ai-memory-workstream/src/transcript.rs` — 63
- `crates/ai-memory-cli/src/commands/backup_agents.rs` — 58

## Why four PRs, not one

`AGENTS.md` codifies rules this change must obey:

- "Small, scoped, behavior-preserving changes. No adjacent feature work, no opportunistic refactors, no speculative abstractions or new public surface without a shipped caller, persisted data, or an explicit requirement."
- "No dead code or half-built public surface."
- Multi-session isolation (invariant #16) requires adversarial tests on every scope-resolution and permission edit; a single mega-PR cannot carry those credibly across all touched boundaries.
- `CHANGELOG.md` merge-gate, humanizer pass on new prose, cross-doc sync (`comparison.md`, `competitive-parity.md`, `research-2026-landscape.md`, `security-boundaries.md`), TypeScript SDK schema snapshot update, plugin render drift test, retrieval benchmark re-run on the release candidate, Homebrew tap bump — all mandatory for a release-grade change and each nontrivial on its own.
- `cargo tf` (full tier) + full-matrix CI on macOS and Windows + Nix leg must pass before tagging.

A single autonomous session cannot satisfy that bar across ~300 files and 1,453 cascade points.

## Phased plan

### PR 1 — Pure deletions (safe, no `AgentKind` semantic change)

Target branch: `hermes/cursor-desktop-only-phase1-deletions` from `main`.

**Delete**:
- Hook bundles: `hooks/{antigravity-cli,claude-code,codex,command-code,copilot-cli,devin,gemini-cli,grok,kimi-code,kiro-cli,opencode,pool}` (keep `cursor/`, `_lib.sh`, `lib/`).
- Marketplace plugins: `plugins/claude-code/`, `plugins/codex/`, `plugins/README.md`, `.claude-plugin/marketplace.json`, `.agents/plugins/marketplace.json`.
- Plugin templates: `crates/ai-memory-cli/templates/plugin/*`.
- Single-harness CLI command files that only service non-Cursor harnesses: `openclaw_plugin.rs`, `codex_hook_trust.rs`, `opencode_dialect.rs`, `render_plugins.rs`, `agent_plugins.rs`, `run_autowire.rs`, `ai-memory-hooks/src/antigravity.rs`.
- Harness-specific tests/fixtures: `opencode_dialect.rs`, `plugins.rs`, Kiro v2/v3 fixtures, `claude_plugin_smoke.py`, `codex_plugin_smoke.py`, `test_grok_ps.ps1`, `test_plugin_launcher.sh`.
- Desktop docs for other agents: `docs/desktop/{antigravity,chatgpt-desktop,claude-desktop,codex-desktop,grok-desktop,zed,desktop-landscape,installer-and-coexistence,plan,verification-2026-10,extensions,capture-and-dedup}.md`.
- Harness-matrix docs: `docs/support-matrix.md`, `docs/managed-harness-contributions.md`, `docs/mcp-install.md`.

**Edit** (to drop references, not reshape behavior):
- `Cargo.toml` workspace members (remove deleted crates when they are single-purpose, e.g., `ai-memory-workstream` is NOT dropped here; it stays until PR 2 reshapes it).
- `cli.rs`: delete `Command::{RenderPlugins, OpencodeDialect, CodexHookTrust, OpenclawPlugin, RunAutowire}` entries and their subargs.

**Keep** untouched in PR 1:
- `AgentKind` enum (still 27 variants).
- `install_hooks.rs`, `install_mcp.rs`, `setup.rs`, `doctor.rs`, `run.rs` (large dispatchers — PR 2/3 reshapes them).
- `ai-memory-workstream` crate.
- Store migrations.

**Tests**: existing full suite must stay green. Add one RED test asserting `plugins/claude-code/` and `plugins/codex/` do not exist, flip to GREEN by the delete.

**Risk**: low. These are pure deletions of material only exercised by non-Cursor flows that we will not ship.

### PR 2 — `AgentKind` enum reduction

Target branch: `hermes/cursor-desktop-only-phase2-agentkind` from PR 1's merged state.

- Reduce `AgentKind` to `{Cursor, Other}`.
- `cargo check` iteratively; fix each cascade site. Many functions that dispatched by variant collapse to a single branch; some lose their `kind` parameter entirely (keep for schema stability if persisted).
- Add store migration V27 that normalizes legacy `agent_kind` strings in `sessions`, `observations`, `handoffs`, `pages`, `workstream_*` tables to `cursor`/`other`, and relax the V26 CHECK constraint.
- Delete `ai-memory-workstream` as a workspace member if Cursor is not a managed harness (research needed — Cursor Desktop's lifecycle may not match `ai-memory run`'s cross-harness launcher model).
- Update MCP admin `parse_agent_kind` constraints.
- Update TypeScript SDK schema snapshot (`companions/ai-memory-ts/npm run schema:update`).
- Update `/web` session filter UI.

**Tests**: RED-GREEN per boundary. Specifically add adversarial multi-session tests (invariant #16) that verify a legacy session with an unknown `agent_kind` string either migrates to `other` or is rejected with a clear error, never silently cross-attributed.

**Risk**: high. 1,453 call-sites. Expect compilation storm; budget 3–5 focused days.

### PR 3 — Setup/init/doctor/status Cursor-only

Target branch: `hermes/cursor-desktop-only-phase3-setup` from PR 2's merged state.

- Reduce `setup`, `init`, `install-hooks`, `install-mcp`, `install-instructions`, `install-skills`, `doctor`, `status`, `uninstall`, `backup-agents`, `restore-agents`, `backfill`, `recovery`, `finalize-session` to Cursor Desktop only.
- `install_mcp.rs` keeps only the `~/.cursor/mcp.json` writer.
- `install_hooks.rs` keeps only the `~/.cursor/hooks.json` writer (verify Cursor Desktop's actual hook contract; the shell/ps1 scripts in `hooks/cursor/` already exist).
- `install_instructions.rs` writes to `.cursor/rules/ai-memory.mdc` (verify exact path — Cursor's rules file surface).
- Remove `ai-memory run` subcommand (`run.rs`, ~10 kLOC) unless Cursor Desktop has a launch-and-attach model (does not, per current evidence).
- Reshape `AgentChoice`, `McpClient`, `RunHarnessChoice`, `InstallSkillsAgent`, `BridgeAgent` enums.
- Rewrite `doctor` capture-coverage table.

**Tests**: RED for every new Cursor-only path; delete non-Cursor install-flow tests; adversarial tests that `--agent` or `--client` flags accepting other harness names fail closed with a clear error (not silent noop).

**Risk**: moderate. Mostly mechanical, bounded by `install_hooks.rs` and `install_mcp.rs` complexity.

### PR 4 — Docs, CHANGELOG, humanizer, benchmarks, release prep

Target branch: `hermes/cursor-desktop-only-phase4-docs` from PR 3's merged state.

- `README.md`: rewrite Supported harnesses, Install, Quickstart sections for Cursor Desktop only.
- `docs/install.md`: reduce from ~3.7 kLOC multi-harness guide to Cursor Desktop only.
- `docs/comparison.md`, `docs/competitive-parity.md`, `docs/research-2026-landscape.md`: update camp table and per-competitor claims; the audits remain honest.
- `docs/security-boundaries.md`: update any row whose "enforcing code" column referenced deleted paths.
- `docs/cookbook.md`, `docs/managed-workstreams.md`, `docs/external-lifecycle.md`, `docs/lifecycle-ops.md`, `docs/agent-messaging.md`, `docs/auto-improvement-loop.md`, `docs/users.md`: Cursor-only language.
- `CHANGELOG.md`: single `## [Unreleased]` block with `### Removed` (every deleted harness), `### Changed` (Cursor-only setup/install surface), per Keep-a-Changelog, past tense, PR back-references.
- Run humanizer skill on new prose.
- `AGENTS.md`, `CLAUDE.md`, `INSTALL_FOR_AGENTS.md`: Cursor Desktop only.
- Retrieval benchmark re-run on final release candidate per AGENTS.md's maintenance rules; publish report in `docs/benchmarks/`.
- Homebrew tap bump after release.

**Tests**: `scripts/check-changelog-sections.sh`, `scripts/check-changelog-frozen.sh`, link-check, humanizer idempotence.

**Risk**: low on correctness, high on diligence (easy to miss a stale reference). A programmatic grep sweep after each edit is mandatory; remaining residual refs must be justified in the PR body (e.g., a historical CHANGELOG section that is frozen).

## Open design questions that must be answered before PR 2

1. **Does Cursor Desktop expose a stable hooks contract this project can target?** `hooks/cursor/*` already contains session-start, pre/post-tool-use, user-prompt-submit, stop, pre-compact, session-end shell and PowerShell variants. Confirm Cursor Desktop fires each event and the payload shape before PR 3 relies on them.
2. **Does Cursor Desktop read `~/.cursor/mcp.json` from the same path as Cursor CLI?** PR 3's `install-mcp` writer depends on this.
3. **Does Cursor Desktop support local rules via `.cursor/rules/*.mdc` project files, and is a user-level rules file supported?** `install_instructions` has to pick the right destination.
4. **Is `ai-memory run` worth salvaging?** Cursor Desktop is launched by the user, not by `ai-memory run <harness>`. If no launch wrapper is needed, the entire `ai-memory-workstream` crate and `run.rs` command go away in PR 2/3.
5. **Companions scope.** Which of `ai-memory-macos`, `ai-memory-desktop`, `ai-memory-importer`, `ai-memory-relay`, `ai-memory-client`, `ai-memory-ts`, `ai-memory-wikisync` ship with the Cursor-only product? None are Cursor-specific; some (`ai-memory-ts`, `ai-memory-wikisync`) are harness-neutral and worth keeping.

## Residual references that will be justified in-PR

- `CHANGELOG.md` released sections: frozen per `scripts/check-changelog-frozen.sh`; cannot be rewritten. Historical mentions of other harnesses stay.
- `LICENSE`, `NOTICE`: third-party license text, kept as-is.
- `docs/design-decisions.md` historical rationale: kept, with a note that the product scope narrowed in version 3.2.
