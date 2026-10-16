-- Phase 2 of the Cursor-only reduction: collapse `agent_kind` to the two
-- values the Rust `AgentKind` enum now carries — `cursor` and `other`.
--
-- Every table that persists `agent_kind` is normalized in place first, so
-- that even a database that was running under an earlier release and
-- captured sessions from non-Cursor harnesses continues to open: legacy
-- harness kinds (`claude-code`, `codex`, …) all become `other`. The
-- `other` fallback is the enum's explicit "unknown / legacy" bucket, so
-- this is semantically lossless — stored rows become indistinguishable
-- from a fresh import of an unknown agent, which is the correct state in
-- a Cursor-only server.
--
-- After the UPDATEs, the `sessions` table is rebuilt (needed to tighten
-- the CHECK constraint; SQLite cannot ALTER a CHECK) with all triggers
-- and indexes recreated.  The `managed_runs`, `workstream_native_sessions`,
-- `workstream_events`, and `ingest_keys` tables carry `agent_kind` as
-- plain TEXT without a CHECK, so a normalising UPDATE is enough there.
--
-- Foreign keys are already disabled by the migration runner, so `DROP
-- TABLE sessions` is safe (child rows are not cascade-deleted).

-- Normalise every stored agent_kind value.
UPDATE sessions
   SET agent_kind = 'other'
 WHERE agent_kind NOT IN ('cursor', 'other');

UPDATE managed_runs
   SET agent_kind = 'other'
 WHERE agent_kind NOT IN ('cursor', 'other');

UPDATE managed_runs
   SET previous_agent_kind = 'other'
 WHERE previous_agent_kind IS NOT NULL
   AND previous_agent_kind NOT IN ('cursor', 'other');

UPDATE workstream_native_sessions
   SET agent_kind = 'other'
 WHERE agent_kind NOT IN ('cursor', 'other');

UPDATE workstream_events
   SET agent_kind = 'other'
 WHERE agent_kind NOT IN ('cursor', 'other');

UPDATE ingest_keys
   SET agent_kind = 'other'
 WHERE agent_kind IS NOT NULL
   AND agent_kind NOT IN ('cursor', 'other');

-- The FTS mirror for workstream_events keeps the UNINDEXED `agent_kind`
-- column; rebuild it so searches do not surface the stale value.
INSERT INTO workstream_events_fts(workstream_events_fts) VALUES ('rebuild');

-- Rebuild `sessions` with the tightened CHECK.
DROP TRIGGER IF EXISTS auto_improve_scheduler_claims_session_pairing_ai;
DROP TRIGGER IF EXISTS session_consolidation_jobs_session_pairing_ai;

CREATE TABLE sessions_new (
    id                       BLOB PRIMARY KEY NOT NULL,
    workspace_id             BLOB NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id               BLOB NOT NULL REFERENCES projects(id)   ON DELETE CASCADE,
    agent_kind               TEXT NOT NULL CHECK (agent_kind IN ('cursor','other')),
    cwd                      TEXT,
    started_at               INTEGER NOT NULL,
    ended_at                 INTEGER,
    summary_page_id          BLOB REFERENCES pages(id) ON DELETE SET NULL,
    ended_observation_count  INTEGER NOT NULL DEFAULT 0 CHECK (ended_observation_count >= 0),
    actor_user               TEXT
);

INSERT INTO sessions_new (
    id, workspace_id, project_id, agent_kind, cwd, started_at, ended_at,
    summary_page_id, ended_observation_count, actor_user
)
SELECT
    id, workspace_id, project_id, agent_kind, cwd, started_at, ended_at,
    summary_page_id, ended_observation_count, actor_user
FROM sessions;

DROP TABLE sessions;

ALTER TABLE sessions_new RENAME TO sessions;

CREATE INDEX idx_sessions_recent ON sessions(workspace_id, project_id, started_at DESC);
CREATE INDEX idx_sessions_project ON sessions(project_id);
CREATE INDEX idx_sessions_started_at ON sessions(started_at DESC);
CREATE INDEX idx_sessions_scope_ended
    ON sessions(workspace_id, project_id, ended_at ASC)
    WHERE ended_at IS NOT NULL;
CREATE INDEX idx_sessions_open_owner
    ON sessions(workspace_id, project_id, agent_kind, actor_user)
    WHERE ended_at IS NULL;

CREATE TRIGGER sessions_ws_proj_pairing_ai
BEFORE INSERT ON sessions
FOR EACH ROW
WHEN NEW.workspace_id IS NOT (SELECT workspace_id FROM projects WHERE id = NEW.project_id)
BEGIN
    SELECT RAISE(ABORT, 'sessions.workspace_id does not match the project''s workspace');
END;

CREATE TRIGGER auto_improve_scheduler_claims_session_pairing_ai
BEFORE INSERT ON auto_improve_scheduler_claims
FOR EACH ROW
WHEN NEW.workspace_id IS NOT (SELECT workspace_id FROM sessions WHERE id = NEW.session_id)
  OR NEW.project_id IS NOT (SELECT project_id FROM sessions WHERE id = NEW.session_id)
BEGIN
    SELECT RAISE(ABORT, 'auto_improve_scheduler_claims scope does not match the session scope');
END;

CREATE TRIGGER session_consolidation_jobs_session_pairing_ai
BEFORE INSERT ON session_consolidation_jobs
FOR EACH ROW
WHEN NEW.workspace_id IS NOT (SELECT workspace_id FROM sessions WHERE id = NEW.session_id)
  OR NEW.project_id IS NOT (SELECT project_id FROM sessions WHERE id = NEW.session_id)
BEGIN
    SELECT RAISE(ABORT, 'session_consolidation_jobs scope does not match the session scope');
END;
