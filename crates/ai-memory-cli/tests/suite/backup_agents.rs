//! Integration tests for `ai-memory backup-agents` and `ai-memory restore-agents`.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static CLI_TEST_LOCK: Mutex<()> = Mutex::new(());

fn cli_test_lock() -> MutexGuard<'static, ()> {
    CLI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ai-memory")
}

fn command_with_env(home: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(bin());
    let config_home = home.join(".config");
    let data_home = home.join(".local/share");
    for dir in [&config_home, &data_home] {
        fs::create_dir_all(dir).unwrap();
    }
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_DATA_HOME", data_home)
        .env("AI_MEMORY_HOME", home)
        .env("AI_MEMORY_DATA_DIR", home.join(".ai-memory-data"))
        .env("AI_MEMORY_EMBEDDING_PROVIDER", "none")
        .env_remove("AI_MEMORY_SERVER_URL")
        .env_remove("AI_MEMORY_AUTH_TOKEN")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("KIMI_CODE_HOME")
        .env_remove("KIRO_HOME");
    #[cfg(windows)]
    command
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"));
    command
}
#[test]
fn restore_agents_rejects_oversized_entries_from_the_header() {
    let _guard = cli_test_lock();
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let archive_path = project.path().join("oversized.tar.gz");
    let file = fs::File::create(&archive_path).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut header = tar::Header::new_gnu();
    header.set_path("oversized.bin").unwrap();
    header.set_size(128 * 1024 * 1024 + 1);
    header.set_mode(0o600);
    header.set_cksum();
    encoder.write_all(header.as_bytes()).unwrap();
    encoder.finish().unwrap();

    let output = command_with_env(home.path(), project.path())
        .args(["restore-agents", "-i", archive_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds"));
}

#[test]
fn agent_filters_are_exact_instead_of_substring_matches() {
    let _guard = cli_test_lock();
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let settings = home.path().join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, br#"{"mcpServers":{}}"#).unwrap();
    let archive = project.path().join("backup.tar.gz");

    let output = command_with_env(home.path(), project.path())
        .args([
            "backup-agents",
            "-o",
            archive.to_str().unwrap(),
            "--agents",
            "code",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !archive.exists(),
        "ambiguous substring must not select Claude Code"
    );
}

#[cfg(unix)]
#[test]
fn backup_agents_does_not_follow_config_or_asset_root_symlinks() {
    use std::os::unix::fs::symlink;

    let _guard = cli_test_lock();
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("settings.json");
    fs::write(&secret, br#"{"token":"must-not-leak"}"#).unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    symlink(&secret, home.path().join(".claude/settings.json")).unwrap();
    let external_plugins = outside.path().join("plugins");
    fs::create_dir_all(&external_plugins).unwrap();
    fs::write(external_plugins.join("payload"), b"outside plugin").unwrap();
    symlink(&external_plugins, home.path().join(".claude/plugins")).unwrap();
    let archive = project.path().join("backup.tar.gz");

    let output = command_with_env(home.path(), project.path())
        .args(["backup-agents", "-o", archive.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        !archive.exists(),
        "symlink-only assets must not be archived"
    );
}

#[test]
fn failed_backup_leaves_the_previous_archive_intact() {
    let _guard = cli_test_lock();
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let plugin = home.path().join(".claude/plugins/huge.bin");
    fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    fs::File::create(&plugin)
        .unwrap()
        .set_len(128 * 1024 * 1024 + 1)
        .unwrap();
    let archive = project.path().join("backup.tar.gz");
    fs::write(&archive, b"previous valid archive placeholder").unwrap();

    let output = command_with_env(home.path(), project.path())
        .args(["backup-agents", "-o", archive.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("per-asset limit"));
    assert_eq!(
        fs::read(archive).unwrap(),
        b"previous valid archive placeholder"
    );
}
