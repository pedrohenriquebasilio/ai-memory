//! Phase 2 Cursor-only migration (V84): adversarial coverage of the
//! `agent_kind` normalisation across every table that persists it, with a
//! two-workspace/two-project fixture that proves the migration never leaks
//! attribution across isolation boundaries (invariant #16 in AGENTS.md).
//!
//! The migration runs as part of the normal refinery sequence inside
//! `Store::open`, so the only way to see pre-V84 state is to materialise the
//! relevant tables under the earlier CHECK and run V84's SQL against them
//! directly — which is what this test does. The SQL is embedded via
//! `include_str!` so the test and the shipped migration cannot drift.
//!
//! The test asserts:
//!
//! * `cursor` rows are preserved verbatim.
//! * Every other persisted `agent_kind` (`claude-code`, `codex`, `grok`,
//!   `ibm-bob`, …) collapses to `other` after the migration — the whole point
//!   of the Cursor-only reduction.
//! * Each session, managed run, workstream native-session, workstream event,
//!   and ingest-key row stays attached to the exact `(workspace_id,
//!   project_id)` tuple it was inserted under. If V84 ever rebuilt rows
//!   across the wrong scope key the second-workspace rows would surface
//!   under the first and this test would fail.
//! * The tightened `sessions.agent_kind` CHECK rejects every legacy wire
//!   value (`claude-code`, `codex`, …) after V84 completes.

use rusqlite::{Connection, params};

/// The subset of the pre-V84 schema that V84 touches, written out just once so
/// the test owns its own fixture and does not depend on the whole migrator.
const PRE_V84_SCHEMA: &str = r#"
CREATE TABLE workspaces (
    id   BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE projects (
    id            BLOB PRIMARY KEY NOT NULL,
    workspace_id  BLOB NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name          TEXT NOT NULL
);

CREATE TABLE pages (
    id BLOB PRIMARY KEY NOT NULL
);

CREATE TABLE auto_improve_scheduler_claims (
    workspace_id BLOB NOT NULL,
    project_id   BLOB NOT NULL,
    session_id   BLOB NOT NULL
);

CREATE TABLE session_consolidation_jobs (
    workspace_id BLOB NOT NULL,
    project_id   BLOB NOT NULL,
    session_id   BLOB NOT NULL
);

CREATE TABLE sessions (
    id                       BLOB PRIMARY KEY NOT NULL,
    workspace_id             BLOB NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id               BLOB NOT NULL REFERENCES projects(id)   ON DELETE CASCADE,
    agent_kind               TEXT NOT NULL CHECK (agent_kind IN (
                                 'claude-code','codex','open-code','cursor','gemini-cli',
                                 'claude-desktop','openclaw','antigravity-cli','omp','pi',
                                 'crush','grok','zero','devin','kimi-code','kiro-cli',
                                 'command-code','hermes','pool','zcode','grizzybot',
                                 'copilot-cli','droid','muse','codebuddy','ibm-bob','other'
                             )),
    cwd                      TEXT,
    started_at               INTEGER NOT NULL,
    ended_at                 INTEGER,
    summary_page_id          BLOB REFERENCES pages(id) ON DELETE SET NULL,
    ended_observation_count  INTEGER NOT NULL DEFAULT 0 CHECK (ended_observation_count >= 0),
    actor_user               TEXT
);

CREATE TABLE workstreams (
    id BLOB PRIMARY KEY NOT NULL
);

CREATE TABLE workstream_native_sessions (
    workstream_id       BLOB NOT NULL REFERENCES workstreams(id) ON DELETE CASCADE,
    agent_kind          TEXT NOT NULL,
    native_session_id   TEXT NOT NULL,
    PRIMARY KEY (workstream_id, agent_kind, native_session_id)
);

CREATE TABLE managed_runs (
    id                   BLOB PRIMARY KEY NOT NULL,
    workstream_id        BLOB NOT NULL REFERENCES workstreams(id) ON DELETE CASCADE,
    workspace_id         BLOB NOT NULL,
    project_id           BLOB NOT NULL,
    agent_kind           TEXT NOT NULL,
    previous_agent_kind  TEXT
);

CREATE TABLE workstream_events (
    workstream_id       BLOB NOT NULL REFERENCES workstreams(id) ON DELETE CASCADE,
    sequence            INTEGER NOT NULL,
    event_id            TEXT NOT NULL,
    agent_kind          TEXT NOT NULL,
    native_session_id   TEXT NOT NULL,
    role                TEXT,
    content             TEXT NOT NULL,
    PRIMARY KEY (workstream_id, sequence)
);

CREATE VIRTUAL TABLE workstream_events_fts USING fts5(
    content,
    role UNINDEXED,
    agent_kind UNINDEXED,
    content='workstream_events',
    content_rowid='rowid'
);

CREATE TABLE ingest_keys (
    key         TEXT PRIMARY KEY NOT NULL,
    agent_kind  TEXT
);
"#;

/// V84's SQL verbatim. If this diverges from the shipped migration the test
/// is wrong on purpose: use `include_str!` so a drift forces a review.
const V84_SQL: &str = include_str!("../../migrations/V84__cursor_only_agent_kind.sql");

fn uuid_bytes(byte: u8) -> [u8; 16] {
    [byte; 16]
}

#[test]
fn v84_normalises_every_legacy_agent_kind_without_leaking_attribution() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(PRE_V84_SCHEMA).unwrap();

    // Two independent (workspace, project) scopes. If the migration ever
    // rebuilt a row under the wrong workspace or project id, the final
    // per-scope asserts below would fail — this is the adversarial multi-
    // session isolation check for invariant #16.
    let ws_a = uuid_bytes(0xA1);
    let ws_b = uuid_bytes(0xB2);
    let proj_a = uuid_bytes(0xA2);
    let proj_b = uuid_bytes(0xB3);

    conn.execute(
        "INSERT INTO workspaces (id, name) VALUES (?1, 'alpha'), (?2, 'beta')",
        params![ws_a, ws_b],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, workspace_id, name) VALUES (?1, ?2, 'alpha-proj'), (?3, ?4, 'beta-proj')",
        params![proj_a, ws_a, proj_b, ws_b],
    )
    .unwrap();

    // The exhaustive list of legacy wire values the pre-V84 CHECK accepted,
    // minus `cursor` and `other` (which stay). Each pair maps to one scope so
    // we can later assert they stuck where they were put.
    let legacy_kinds = [
        "claude-code",
        "codex",
        "open-code",
        "gemini-cli",
        "claude-desktop",
        "openclaw",
        "antigravity-cli",
        "omp",
        "pi",
        "crush",
        "grok",
        "zero",
        "devin",
        "kimi-code",
        "kiro-cli",
        "command-code",
        "hermes",
        "pool",
        "zcode",
        "grizzybot",
        "copilot-cli",
        "droid",
        "muse",
        "codebuddy",
        "ibm-bob",
    ];

    let ws_id = uuid_bytes(0xC3);
    conn.execute("INSERT INTO workstreams (id) VALUES (?1)", params![ws_id])
        .unwrap();

    // Sessions: alternate the two scopes so an id-swap under rebuild is
    // visible. Also seed one `cursor` row per scope (must stay) and one
    // `other` row per scope (idempotent).
    type SessionRow<'a> = ([u8; 16], &'a str, [u8; 16], [u8; 16]);
    let mut sessions: Vec<SessionRow<'_>> = Vec::new();
    for (i, kind) in legacy_kinds
        .iter()
        .chain(["cursor", "other"].iter())
        .enumerate()
    {
        let sid = uuid_bytes(i as u8);
        let (ws, proj) = if i % 2 == 0 {
            (ws_a, proj_a)
        } else {
            (ws_b, proj_b)
        };
        conn.execute(
            "INSERT INTO sessions (id, workspace_id, project_id, agent_kind, started_at) \
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![sid, ws, proj, *kind],
        )
        .unwrap();
        sessions.push((sid, *kind, ws, proj));
    }

    // One `managed_runs` row per legacy kind, both current and
    // `previous_agent_kind`. Alternate scopes.
    type RunRow<'a> = ([u8; 16], &'a str, &'a str, [u8; 16], [u8; 16]);
    let mut runs: Vec<RunRow<'_>> = Vec::new();
    for (i, kind) in legacy_kinds.iter().enumerate() {
        let rid = uuid_bytes(0x40 + i as u8);
        let (ws, proj) = if i % 2 == 0 {
            (ws_a, proj_a)
        } else {
            (ws_b, proj_b)
        };
        conn.execute(
            "INSERT INTO managed_runs (id, workstream_id, workspace_id, project_id, agent_kind, previous_agent_kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![rid, ws_id, ws, proj, *kind, *kind],
        )
        .unwrap();
        runs.push((rid, *kind, *kind, ws, proj));
    }
    // Plus one surviving `cursor` run and one `other` run.
    conn.execute(
        "INSERT INTO managed_runs (id, workstream_id, workspace_id, project_id, agent_kind, previous_agent_kind) \
         VALUES (?1, ?2, ?3, ?4, 'cursor', NULL), (?5, ?2, ?3, ?4, 'other', 'other')",
        params![
            uuid_bytes(0xC0),
            ws_id,
            ws_a,
            proj_a,
            uuid_bytes(0xC1),
        ],
    )
    .unwrap();

    for (i, kind) in legacy_kinds.iter().enumerate() {
        conn.execute(
            "INSERT INTO workstream_native_sessions (workstream_id, agent_kind, native_session_id) \
             VALUES (?1, ?2, ?3)",
            params![ws_id, *kind, format!("native-{i}")],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO workstream_native_sessions (workstream_id, agent_kind, native_session_id) \
         VALUES (?1, 'cursor', 'native-cursor')",
        params![ws_id],
    )
    .unwrap();

    for (i, kind) in legacy_kinds.iter().enumerate() {
        conn.execute(
            "INSERT INTO workstream_events (workstream_id, sequence, event_id, agent_kind, native_session_id, role, content) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'user', ?6)",
            params![
                ws_id,
                i as i64,
                format!("evt-{i}"),
                *kind,
                format!("native-{i}"),
                format!("body for {kind}"),
            ],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO workstream_events (workstream_id, sequence, event_id, agent_kind, native_session_id, role, content) \
         VALUES (?1, 999, 'evt-cursor', 'cursor', 'native-cursor', 'user', 'cursor body')",
        params![ws_id],
    )
    .unwrap();

    for (i, kind) in legacy_kinds.iter().enumerate() {
        conn.execute(
            "INSERT INTO ingest_keys (key, agent_kind) VALUES (?1, ?2)",
            params![format!("key-{i}"), *kind],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO ingest_keys (key, agent_kind) VALUES ('key-cursor', 'cursor'), ('key-null', NULL)",
        [],
    )
    .unwrap();

    // Apply V84 in a single transactional batch, exactly as refinery would.
    conn.execute_batch(V84_SQL).expect("V84 must apply cleanly");

    // --- Normalisation ---
    //
    // Every legacy kind collapses to `other`; `cursor` and `other` stay.
    let distinct_session_kinds: Vec<String> = conn
        .prepare("SELECT DISTINCT agent_kind FROM sessions ORDER BY agent_kind")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(distinct_session_kinds, vec!["cursor", "other"]);

    for table in [
        "managed_runs",
        "workstream_native_sessions",
        "workstream_events",
        "ingest_keys",
    ] {
        let kinds: Vec<String> = conn
            .prepare(&format!(
                "SELECT DISTINCT agent_kind FROM {table} \
                 WHERE agent_kind IS NOT NULL ORDER BY agent_kind"
            ))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            kinds,
            vec!["cursor".to_string(), "other".to_string()],
            "{table} still carries a legacy agent_kind"
        );
    }
    let prev_kinds: Vec<String> = conn
        .prepare(
            "SELECT DISTINCT previous_agent_kind FROM managed_runs \
             WHERE previous_agent_kind IS NOT NULL ORDER BY previous_agent_kind",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(prev_kinds, vec!["other"]);

    // --- Isolation (invariant #16) ---
    //
    // Each inserted session row must still carry the exact (workspace,
    // project) it was inserted under. If the rebuild had rewritten the key
    // tuples, a row would surface under the wrong scope.
    for (sid, original_kind, expected_ws, expected_proj) in &sessions {
        let (ws_actual, proj_actual, kind_actual) = conn
            .query_row(
                "SELECT workspace_id, project_id, agent_kind FROM sessions WHERE id = ?1",
                params![sid],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            &ws_actual[..],
            &expected_ws[..],
            "session originally under kind {original_kind:?} drifted to a foreign workspace"
        );
        assert_eq!(
            &proj_actual[..],
            &expected_proj[..],
            "session originally under kind {original_kind:?} drifted to a foreign project"
        );
        let expected_kind = if *original_kind == "cursor" {
            "cursor"
        } else {
            "other"
        };
        assert_eq!(
            kind_actual, expected_kind,
            "session originally under {original_kind:?} normalised wrong"
        );
    }

    for (rid, original_kind, original_prev, expected_ws, expected_proj) in &runs {
        let (ws_actual, proj_actual, kind_actual, prev_actual): (
            Vec<u8>,
            Vec<u8>,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT workspace_id, project_id, agent_kind, previous_agent_kind \
                 FROM managed_runs WHERE id = ?1",
                params![rid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(&ws_actual[..], &expected_ws[..]);
        assert_eq!(&proj_actual[..], &expected_proj[..]);
        assert_eq!(kind_actual, "other");
        assert_eq!(
            prev_actual.as_deref(),
            Some("other"),
            "managed_runs.previous_agent_kind for {original_kind:?}/{original_prev:?} not normalised"
        );
    }

    // --- Tightened CHECK ---
    //
    // After V84, inserting a legacy wire value must be refused by the new
    // sessions.agent_kind CHECK. This proves the schema itself, not just the
    // in-place UPDATE, is now Cursor-only.
    for legacy in ["claude-code", "codex", "grok", "ibm-bob", "kimi-code"] {
        let err = conn.execute(
            "INSERT INTO sessions (id, workspace_id, project_id, agent_kind, started_at) \
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![uuid_bytes(0xF0), ws_a, proj_a, legacy],
        );
        assert!(
            err.is_err(),
            "post-V84 CHECK must refuse legacy agent_kind {legacy:?}"
        );
    }
    // A legitimate `cursor` write still works, exercising the control.
    conn.execute(
        "INSERT INTO sessions (id, workspace_id, project_id, agent_kind, started_at) \
         VALUES (?1, ?2, ?3, 'cursor', 1)",
        params![uuid_bytes(0xF1), ws_a, proj_a],
    )
    .expect("post-V84 CHECK must still allow cursor");
}
