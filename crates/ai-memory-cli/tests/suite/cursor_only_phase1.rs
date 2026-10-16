//! Cursor Desktop-only reduction, Phase 1 guard.
//!
//! Phase 1 of the Cursor-only reduction (`docs/cursor-only-plan.md`) deletes
//! the shipped artefacts that service non-Cursor coding harnesses: the
//! non-Cursor hook bundles under `hooks/`, the Claude Code and Codex
//! marketplace plugins and their catalog files, the embedded plugin
//! templates, the hidden `render-plugins` CLI entry point and its drift
//! test, the Kiro v3 lifecycle fixture, the harness-matrix docs
//! (`docs/support-matrix.md`, `docs/managed-harness-contributions.md`,
//! `docs/mcp-install.md`), the per-harness desktop research pages, and the
//! shell/PowerShell/Python plugin smoke tests. Each item is enumerated here
//! so a regression that re-introduces one of them trips a loud, named
//! failure instead of a silent reappearance.
//!
//! The checks are intentionally not folder-presence tautologies: they open
//! shipped source files (`cli.rs`, `lib.rs`, `commands/mod.rs`, the hooks
//! `tests/suite/main.rs`) and assert the Cursor-only invariants that the
//! `render-plugins` subcommand, the `render_plugins` module declaration, the
//! dispatcher arms and the Kiro v3 fixture test have all been removed.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repository root")
}

fn exists(path: &str) -> bool {
    repo_root().join(path).exists()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|err| panic!("read {path}: {err}"))
}

#[test]
fn non_cursor_hook_bundles_are_removed() {
    for harness in [
        "antigravity-cli",
        "claude-code",
        "codex",
        "command-code",
        "copilot-cli",
        "devin",
        "gemini-cli",
        "grok",
        "kimi-code",
        "kiro-cli",
        "opencode",
        "pool",
    ] {
        let path = format!("hooks/{harness}");
        assert!(
            !exists(&path),
            "{path} still ships; Cursor-only reduction phase 1 deletes every non-Cursor hook bundle"
        );
    }
    assert!(
        exists("hooks/cursor"),
        "hooks/cursor must stay as the one supported harness bundle"
    );
}

#[test]
fn marketplace_plugins_and_catalogs_are_removed() {
    for path in [
        "plugins/claude-code",
        "plugins/codex",
        "plugins/README.md",
        ".claude-plugin/marketplace.json",
        ".agents/plugins/marketplace.json",
    ] {
        assert!(
            !exists(path),
            "{path} still exists; Cursor-only reduction phase 1 removes the Claude Code and Codex marketplace plugins"
        );
    }
}

#[test]
fn plugin_templates_and_renderer_are_removed() {
    assert!(
        !exists("crates/ai-memory-cli/templates/plugin"),
        "plugin templates are only consumed by render-plugins; both go together"
    );
    assert!(
        !exists("crates/ai-memory-cli/src/commands/render_plugins.rs"),
        "render_plugins command module must be removed"
    );
    assert!(
        !exists("crates/ai-memory-cli/tests/suite/plugins.rs"),
        "render-plugins drift test must be removed with the renderer"
    );

    let cli = read("crates/ai-memory-cli/src/cli.rs");
    assert!(
        !cli.contains("RenderPlugins"),
        "cli.rs still references RenderPlugins; the subcommand must be removed"
    );
    assert!(
        !cli.contains("RenderPluginsArgs"),
        "cli.rs still declares RenderPluginsArgs; the subargs struct must be removed"
    );

    let lib = read("crates/ai-memory-cli/src/lib.rs");
    assert!(
        !lib.contains("render_plugins"),
        "lib.rs still dispatches render_plugins; the arms must be removed"
    );

    let mod_rs = read("crates/ai-memory-cli/src/commands/mod.rs");
    assert!(
        !mod_rs.contains("render_plugins"),
        "commands/mod.rs still declares the render_plugins module"
    );

    let suite_main = read("crates/ai-memory-cli/tests/suite/main.rs");
    assert!(
        !suite_main.contains("mod plugins"),
        "tests/suite/main.rs still declares the plugins drift test"
    );
}

#[test]
fn kiro_v3_fixture_and_test_are_removed() {
    assert!(
        !exists("crates/ai-memory-cli/tests/fixtures/kiro-v3-hook-payloads.json"),
        "Kiro v3 fixture must be removed"
    );
    let hook = read("crates/ai-memory-cli/src/commands/hook.rs");
    assert!(
        !hook.contains("kiro_v3_live_lifecycle_fixtures"),
        "hook.rs still carries the kiro_v3_live_lifecycle_fixtures test"
    );
    assert!(
        !hook.contains("kiro-v3-hook-payloads.json"),
        "hook.rs still include_str!s the Kiro v3 fixture"
    );
}

#[test]
fn harness_matrix_and_non_cursor_desktop_docs_are_removed() {
    for doc in [
        "docs/support-matrix.md",
        "docs/managed-harness-contributions.md",
        "docs/mcp-install.md",
        "docs/desktop/antigravity.md",
        "docs/desktop/chatgpt-desktop.md",
        "docs/desktop/claude-desktop.md",
        "docs/desktop/codex-desktop.md",
        "docs/desktop/grok-desktop.md",
        "docs/desktop/zed.md",
        "docs/desktop/desktop-landscape.md",
        "docs/desktop/installer-and-coexistence.md",
        "docs/desktop/plan.md",
        "docs/desktop/verification-2026-10.md",
        "docs/desktop/extensions.md",
        "docs/desktop/capture-and-dedup.md",
    ] {
        assert!(
            !exists(doc),
            "{doc} still exists; Cursor-only reduction deletes it"
        );
    }
    assert!(
        exists("docs/desktop/cursor.md"),
        "docs/desktop/cursor.md must stay"
    );
}

#[test]
fn harness_specific_smoke_tests_are_removed() {
    for path in [
        "tests/e2e/claude_plugin_smoke.py",
        "tests/e2e/codex_plugin_smoke.py",
        "tests/hooks/test_plugin_launcher.sh",
        "tests/hooks/test_grok_ps.ps1",
    ] {
        assert!(
            !exists(path),
            "{path} is a non-Cursor smoke test; Cursor-only reduction removes it"
        );
    }
}
