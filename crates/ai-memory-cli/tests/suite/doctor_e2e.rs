//! Use-case end-to-end test for `ai-memory doctor` (capture-coverage check).
//!
//! Doctor exists to make one silent failure visible: a harness that ran in this
//! project but has no ai-memory hook installed still writes its own local
//! session transcripts, yet nothing reaches the server. The operator believes
//! every harness feeds one memory and is quietly wrong (audit finding F1). For
//! the current project doctor enumerates each known harness's local native
//! sessions, asks the server how many it actually captured per agent
//! (`GET /admin/sessions/by-agent`), and warns — with the exact
//! `install-hooks --agent <agent> --apply` remediation — about any harness that
//! ran here recently but captured zero.
//!
//! The pure verdict logic (`build_rows`) and the local detector (`scan_local`)
//! are unit-tested in the command module. The UNTESTED half is the whole live
//! flow against a real server: does the shipped `doctor` subcommand, driven
//! exactly as an operator would, actually reconcile the local side against the
//! server's captured counts and print the right verdict? This drives that flow:
//!
//! 1. **Gap detected.** A planted Claude transcript for this cwd with an empty
//!    store (server captured zero) → doctor flags `claude-code` as uncaptured
//!    and prints its `install-hooks --agent claude-code --apply` fix.
//! 2. **Healthy control.** After `backfill` imports that same transcript through
//!    `/hook`, the server has captured sessions for `claude-code` → doctor no
//!    longer flags it, and reports every harness as captured. This control is
//!    what stops a blanket "everything is uncaptured" warning from passing #1
//!    for the wrong reason.
//!
//! Unix-gated: the fixture uses the POSIX-encoded `~/.claude/projects/<enc-cwd>/`
//! native layout (as the sibling `backfill_e2e` test and the `scan_local` unit
//! test do); cross-platform native-store discovery is owned by
//! `ai-memory-workstream`.

#![cfg(unix)]

/// Spawns a server and several subprocesses: seconds, not milliseconds, so this
/// lives in the slow tier (`cargo tf` / CI), not the everyday loop.
mod slow {
    use std::fs;
    use std::time::Duration;

    use crate::e2e_support::hermetic;

    const BIN: &str = env!("CARGO_BIN_EXE_ai-memory");
    const WORKSPACE: &str = "doctor-e2e-ws";
    const PROJECT: &str = "doctor-e2e-proj";
    #[tokio::test]
    async fn invalid_home_routes_refuse_before_any_server_request_but_explicit_pair_bypasses() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let data_dir = tempfile::tempdir().expect("data dir");
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project cwd");
        fs::write(
            home.path().join(".ai-memory.toml"),
            "workspace=\"fallback\"\nproject=\"fallback\"\n[routes.path.\"~/..\"]\nroute_workspace=\"bad\"\nroute_project=\"bad\"\n",
        )
        .unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let app = axum::Router::new().fallback(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "unexpected request",
                )
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let first_data_dir = data_dir.path().to_path_buf();
        let first_home = home.path().to_path_buf();
        let first_cwd = project.path().to_path_buf();
        let first_base = base.clone();
        let output = tokio::task::spawn_blocking(move || {
            hermetic(BIN)
                .args(["doctor", "--json"])
                .env("AI_MEMORY_DATA_DIR", first_data_dir)
                .env("AI_MEMORY_HOME", first_home)
                .env("AI_MEMORY_SERVER_URL", first_base)
                .env("AI_MEMORY_EMBEDDING_PROVIDER", "none")
                .current_dir(first_cwd)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid home route map"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(requests.load(Ordering::SeqCst), 0);

        let second_data_dir = data_dir.path().to_path_buf();
        let second_home = home.path().to_path_buf();
        let second_cwd = project.path().to_path_buf();
        let explicit = tokio::task::spawn_blocking(move || {
            hermetic(BIN)
                .args([
                    "doctor",
                    "--workspace",
                    WORKSPACE,
                    "--project",
                    PROJECT,
                    "--json",
                ])
                .env("AI_MEMORY_DATA_DIR", second_data_dir)
                .env("AI_MEMORY_HOME", second_home)
                .env("AI_MEMORY_SERVER_URL", base)
                .env("AI_MEMORY_EMBEDDING_PROVIDER", "none")
                .current_dir(second_cwd)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            requests.load(Ordering::SeqCst) > 0,
            "explicit scope must reach the server"
        );
        assert!(
            !explicit.status.success(),
            "the deliberately failing server remains observable"
        );
        server.abort();
    }
}
