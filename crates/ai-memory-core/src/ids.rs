//! Strongly-typed identifiers for the domain.
//!
//! The 3-tuple ([`WorkspaceId`], [`ProjectId`], [`PagePath`]) is the universal
//! identity coordinate for any memory. It is baked in from M0 even though v1
//! ships single-workspace, so we never inherit basic-memory's v0.20 retrofit
//! pain (issues #783, #834, #802 and friends — see
//! `docs/issues-basic-memory.md`).

use std::fmt;
use std::path::Component;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::MemoryError;

macro_rules! id_newtype {
    ($vis:vis $name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        $vis struct $name(pub Uuid);

        impl $name {
            /// Generate a fresh v7 (time-ordered) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Borrow the raw 16-byte big-endian representation.
            #[must_use]
            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            /// Reconstruct from a 16-byte slice (typically read from
            /// SQLite as a `BLOB`).
            ///
            /// # Errors
            /// Returns [`MemoryError::MalformedRecord`] if the slice is
            /// not exactly 16 bytes.
            pub fn from_slice(bytes: &[u8]) -> Result<Self, MemoryError> {
                Uuid::from_slice(bytes)
                    .map(Self)
                    .map_err(|e| MemoryError::MalformedRecord(format!("invalid uuid bytes: {e}")))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.0).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = MemoryError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s)
                    .map(Self)
                    .map_err(|e| MemoryError::MalformedRecord(format!("invalid uuid: {e}")))
            }
        }
    };
}

id_newtype!(pub WorkspaceId, "Workspace identifier (top of the 3-tuple).");
id_newtype!(pub ProjectId, "Project identifier (middle of the 3-tuple).");
id_newtype!(pub SessionId, "Identifier for a single agent run.");

impl SessionId {
    /// Resolve a harness-native session id to the `SessionId` the store keys
    /// on: a UUID native id is used as-is (canonicalized); any other string
    /// (Codex, OpenCode, ...) is hashed to a deterministic UUID v5, so hook
    /// POSTs and later lookups by native id share one key. The hook router and
    /// any offline tool that matches transcripts by native id (e.g.
    /// `ai-memory repair-backfill-timestamps`) must agree on this one rule.
    #[must_use]
    pub fn from_native(raw: &str) -> Self {
        Self::from_str(raw)
            .unwrap_or_else(|_| Self(Uuid::new_v5(&Uuid::NAMESPACE_OID, raw.as_bytes())))
    }
}
id_newtype!(pub ObservationId, "Identifier for a single observation captured during a session.");
id_newtype!(pub PageId, "Identifier for a single wiki page version.");
id_newtype!(pub EntityId, "Identifier for one project-scoped entity.");
id_newtype!(pub HandoffId, "Identifier for a cross-agent handoff record.");
id_newtype!(pub MessageId, "Identifier for one cross-project agent message (inbox/queue).");
id_newtype!(pub WorkstreamId, "Identifier for a managed cross-harness workstream.");
id_newtype!(pub ManagedRunId, "Identifier for one `ai-memory run` invocation.");
id_newtype!(pub UserId, "Identifier for a registered user (multi-user attribution; see [`crate::actor`]).");
id_newtype!(pub ApiCredentialId, "Identifier for one native `aim_` API credential.");
id_newtype!(pub AutoImproveRunId, "Identifier for one auto-improvement review run.");
id_newtype!(pub AutoImproveProposalId, "Identifier for one staged auto-improvement proposal.");
id_newtype!(pub PageFeedbackId, "Identifier for one page-feedback signal (`memory_feedback`).");

/// Relative path of a page within the wiki tree.
///
/// Always uses `/` as the separator (POSIX-style), normalised on construction.
/// Never starts with a slash; never contains `..` or `.` components. This
/// invariant lets the store treat paths as flat keys without re-validating.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PagePath(String);

impl PagePath {
    /// Construct from a raw string. Rejects empty, absolute, Windows-prefix,
    /// backslash-separated, and dot-segment paths.
    ///
    /// # Errors
    /// Returns [`MemoryError::InvalidPagePath`] when the input is empty or
    /// contains a path component that would escape or alias the wiki root.
    pub fn new(raw: impl Into<String>) -> Result<Self, MemoryError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(MemoryError::InvalidPagePath("empty path".into()));
        }
        if raw.starts_with('/') {
            return Err(MemoryError::InvalidPagePath(format!(
                "leading slash: {raw}"
            )));
        }
        if raw.len() >= 3 {
            let bytes = raw.as_bytes();
            if bytes[1] == b':' && bytes[2] == b'/' && bytes[0].is_ascii_alphabetic() {
                return Err(MemoryError::InvalidPagePath(format!(
                    "windows drive prefix in {raw}"
                )));
            }
        }
        if raw.contains('\\') {
            return Err(MemoryError::InvalidPagePath(format!(
                "backslash separator in {raw}"
            )));
        }
        for component in std::path::Path::new(&raw).components() {
            match component {
                Component::Normal(_) => {}
                Component::CurDir | Component::ParentDir => {
                    return Err(MemoryError::InvalidPagePath(format!(
                        "dot segment in {raw}"
                    )));
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(MemoryError::InvalidPagePath(format!(
                        "absolute path in {raw}"
                    )));
                }
            }
        }
        for segment in raw.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(MemoryError::InvalidPagePath(format!(
                    "invalid segment in {raw}"
                )));
            }
        }
        Ok(Self(raw))
    }

    /// Reject a path that cannot be materialised and checkpointed on every
    /// supported platform.
    ///
    /// Deliberately **not** part of [`PagePath::new`]. Persisted rows are
    /// reconstructed through that constructor on every read
    /// (`reader.rs` does so in the recency, search, vector and graph
    /// queries), so tightening it would make any already-stored
    /// non-portable page unreadable — and because those are list queries,
    /// one such page would break a whole listing rather than just itself.
    /// The rule therefore applies where a *new* path enters the system.
    ///
    /// The rule is the same on every platform on purpose. A wiki authored
    /// on Linux is expected to be usable on Windows by the same release;
    /// making the check platform-conditional would let a Linux session
    /// create pages a Windows session cannot read, which is the defect
    /// being fixed rather than a fix for it (#462).
    ///
    /// # Errors
    /// Returns [`MemoryError::InvalidPagePath`] naming the offending
    /// component and the reason.
    pub fn ensure_portable(&self) -> Result<(), MemoryError> {
        for component in self.as_str().split('/') {
            ensure_portable_component(component, self.as_str())?;
        }
        Ok(())
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PagePath").field(&self.0).finish()
    }
}

impl fmt::Display for PagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The key a case-folding or Unicode-normalizing filesystem effectively
/// stores a page path under: two paths with the same key are one file on
/// macOS (APFS) and Windows (NTFS), whatever they look like here.
///
/// Lowercase first, then compose: `İ` lowercases to `i` + U+0307, which only
/// composes back to a single scalar after the fold.
#[must_use]
pub fn portable_page_key(path: &str) -> String {
    let lowered = path.to_lowercase();
    let nfc = icu_normalizer::ComposingNormalizer::new_nfc();
    if nfc.is_normalized(&lowered) {
        lowered
    } else {
        nfc.normalize(&lowered).into_owned()
    }
}

/// Discriminator for the agent CLI that captured an observation or handoff.
///
/// After the Cursor-only reduction (Phase 2) this fork keeps only two
/// variants: [`AgentKind::Cursor`], the one advertised harness, and
/// [`AgentKind::Other`], the conservative fallback used for manual
/// captures, legacy rows left over from upstream releases, and anything
/// else the server does not actively support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    /// Cursor Desktop (Agent Chat / Cmd+K), the only advertised harness.
    Cursor,
    /// Anything else: manual capture, legacy rows migrated from an
    /// earlier multi-harness release, and unknown wire values. Not an
    /// advertised supported harness.
    Other,
}

impl AgentKind {
    /// Every variant, for tests that must cover the full agent surface —
    /// e.g. the store-level check that the persisted `sessions.agent_kind`
    /// CHECK constraint accepts every kind. Extend together with the enum.
    pub const ALL: [Self; 2] = [Self::Cursor, Self::Other];

    /// Kebab-case wire string matching the serde representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Other => "other",
        }
    }

    /// Parse a known wire string. Unknown values (including every
    /// legacy multi-harness tag) map to [`AgentKind::Other`], which is
    /// the conservative fallback for anything the Cursor-only server
    /// does not recognise natively.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s {
            "cursor" => Self::Cursor,
            _ => Self::Other,
        }
    }

    /// Whether this agent injects the native `session-start` hook's stdout
    /// into the resuming session as context.
    ///
    /// In this Cursor-only fork Cursor Desktop injects, and the conservative
    /// `Other` fallback does not — a destructive handoff fetch must only run
    /// against an agent whose stdout semantics we know, and `Other` is by
    /// definition unknown.
    #[must_use]
    pub fn session_start_injects_handoff(self) -> bool {
        matches!(self, Self::Cursor)
    }

    /// Whether this agent injects the native `user-prompt` (UserPromptSubmit)
    /// hook's stdout into the upcoming turn as context. Cursor Desktop uses
    /// the `SessionStart` path for the handoff rather than
    /// `UserPromptSubmit`, so no supported harness in this fork injects on
    /// that event — the method stays for the hook router's dispatch tables.
    #[must_use]
    pub fn user_prompt_injects_handoff(self) -> bool {
        false
    }

    /// Whether this agent adds a `UserPromptSubmit` hook's
    /// `hookSpecificOutput.additionalContext` to the turn it belongs to.
    /// Cursor Desktop does not; neither does the `Other` fallback.
    #[must_use]
    pub fn user_prompt_injects_context(self) -> bool {
        false
    }

    /// Whether `PostToolUse` stdout is model-visible context. No harness in
    /// this fork uses this channel.
    #[must_use]
    pub fn post_tool_injects_handoff(self) -> bool {
        false
    }

    /// Whether this agent reuses one session id across a `SessionEnd` and a
    /// later restart of the same conversation. Cursor Desktop does not, and
    /// the conservative `Other` fallback stays safe.
    #[must_use]
    pub fn reuses_session_id_after_end(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_path_accepts_simple() {
        assert_eq!(PagePath::new("foo/bar.md").unwrap().as_str(), "foo/bar.md");
    }

    #[test]
    fn page_path_rejects_empty() {
        assert!(PagePath::new("").is_err());
    }

    #[test]
    fn page_path_rejects_leading_slash() {
        assert!(PagePath::new("/foo").is_err());
    }

    #[test]
    fn page_path_rejects_dot_segments() {
        assert!(PagePath::new("a/./b").is_err());
        assert!(PagePath::new("a/../b").is_err());
    }

    /// A UUID native id round-trips as-is; a non-UUID native id (Codex,
    /// OpenCode) hashes to a deterministic UUID v5, so hook capture and any
    /// offline tool matching transcripts by native id agree on one key.
    #[test]
    fn session_id_from_native_hashes_non_uuid_ids_and_passes_uuids_through() {
        let uuid_native = "11111111-2222-3333-4444-555555555555";
        assert_eq!(SessionId::from_native(uuid_native).to_string(), uuid_native);

        let non_uuid_native = "codex-native-id-123";
        let expected = SessionId(Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            non_uuid_native.as_bytes(),
        ));
        assert_eq!(SessionId::from_native(non_uuid_native), expected);
        // Deterministic: repeated calls (and hence repeated hook deliveries,
        // or a repeated repair run) must resolve to the same id.
        assert_eq!(
            SessionId::from_native(non_uuid_native),
            SessionId::from_native(non_uuid_native)
        );
    }

    #[test]
    fn agent_kind_cursor_round_trips_and_injects_session_start_handoff() {
        assert_eq!(AgentKind::Cursor.as_str(), "cursor");
        assert_eq!(AgentKind::from_wire("cursor"), AgentKind::Cursor);
        assert_eq!(
            serde_json::to_string(&AgentKind::Cursor).unwrap(),
            "\"cursor\""
        );
        assert_eq!(
            serde_json::from_str::<AgentKind>("\"cursor\"").unwrap(),
            AgentKind::Cursor
        );
        // Cursor Desktop fetches the handoff on SessionStart; it uses no
        // other injection channel.
        assert!(AgentKind::Cursor.session_start_injects_handoff());
        assert!(!AgentKind::Cursor.user_prompt_injects_handoff());
        assert!(!AgentKind::Cursor.user_prompt_injects_context());
        assert!(!AgentKind::Cursor.post_tool_injects_handoff());
        assert!(!AgentKind::Cursor.reuses_session_id_after_end());
    }

    #[test]
    fn agent_kind_other_is_the_conservative_fallback() {
        assert_eq!(AgentKind::Other.as_str(), "other");
        assert_eq!(
            serde_json::to_string(&AgentKind::Other).unwrap(),
            "\"other\""
        );
        assert_eq!(
            serde_json::from_str::<AgentKind>("\"other\"").unwrap(),
            AgentKind::Other
        );
        // Every legacy multi-harness tag from the upstream enum must now
        // degrade to Other — the whole point of the Cursor-only reduction.
        for legacy in [
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
            "copilot-cli",
            "grizzybot",
            "droid",
            "muse",
            "codebuddy",
            "ibm-bob",
            "bob",
            "something-made-up",
        ] {
            assert_eq!(
                AgentKind::from_wire(legacy),
                AgentKind::Other,
                "{legacy:?} must degrade to Other in a Cursor-only server"
            );
        }
        // Other is conservative: every behavioural switch returns false.
        assert!(!AgentKind::Other.session_start_injects_handoff());
        assert!(!AgentKind::Other.user_prompt_injects_handoff());
        assert!(!AgentKind::Other.user_prompt_injects_context());
        assert!(!AgentKind::Other.post_tool_injects_handoff());
        assert!(!AgentKind::Other.reuses_session_id_after_end());
    }

    #[test]
    fn agent_kind_all_covers_only_cursor_and_other() {
        assert_eq!(AgentKind::ALL, [AgentKind::Cursor, AgentKind::Other]);
    }

    #[test]
    fn page_path_rejects_backslashes_and_windows_prefixes() {
        assert!(PagePath::new(r"notes\secret.md").is_err());
        assert!(PagePath::new(r"C:\Users\me\secret.md").is_err());
        assert!(PagePath::new("C:/Users/me/secret.md").is_err());
    }

    #[test]
    fn ids_are_unique() {
        let a = WorkspaceId::new();
        let b = WorkspaceId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn id_round_trips_through_string() {
        let id = SessionId::new();
        let s = id.to_string();
        let parsed: SessionId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn id_round_trips_through_bytes() {
        let id = ProjectId::new();
        assert_eq!(ProjectId::from_slice(id.as_bytes()).unwrap(), id);
    }

    #[test]
    fn id_rejects_malformed_byte_lengths() {
        assert!(PageId::from_slice(&[0_u8; 15]).is_err());
        assert!(PageId::from_slice(&[0_u8; 17]).is_err());
    }

    #[test]
    fn session_id_from_native_keeps_uuids_and_hashes_other_ids() {
        let uuid = SessionId::new();
        assert_eq!(SessionId::from_native(&uuid.to_string()), uuid);
        let hashed = SessionId::from_native("agy-session-1");
        assert_eq!(
            hashed,
            SessionId::from_native("agy-session-1"),
            "deterministic"
        );
        assert_eq!(
            hashed.0,
            Uuid::new_v5(&Uuid::NAMESPACE_OID, b"agy-session-1"),
            "the key hook POSTs have always used"
        );
    }
}

/// Names Windows reserves regardless of extension: `CON.md` is still the
/// console device.
const DOS_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether `stem` (a file name without its extension) is a DOS device name
/// Windows resolves to a device instead of a file, compared case-insensitively.
#[must_use]
pub fn is_dos_device_name(stem: &str) -> bool {
    DOS_DEVICE_NAMES.contains(&stem.to_ascii_lowercase().as_str())
}

/// Characters Windows refuses in a filename. `/` is the separator and is
/// handled by the caller; `\\` and a drive prefix are already rejected by
/// [`PagePath::new`].
const WINDOWS_RESERVED_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// Returns true if a path component is reserved by Git (`.git` case-insensitively,
/// or an 8.3 short-name alias like `git~1`..`git~4`).
#[must_use]
pub fn is_git_reserved_component(component: &str) -> bool {
    // Compare on bytes: a `str` slice at a fixed byte index panics on a
    // multibyte component (a 5-byte UTF-8 name like "abcé" has no char
    // boundary at 4), and this runs on untrusted write input.
    let b = component.as_bytes();
    component.eq_ignore_ascii_case(".git")
        || (b.len() == 5 && b[..4].eq_ignore_ascii_case(b"git~") && (b'1'..=b'4').contains(&b[4]))
}

fn ensure_portable_component(component: &str, full: &str) -> Result<(), MemoryError> {
    let invalid = |reason: &str| {
        Err(MemoryError::InvalidPagePath(format!(
            "page path {full:?}: component {component:?} {reason}"
        )))
    };

    if let Some(bad) = component
        .chars()
        .find(|c| WINDOWS_RESERVED_CHARS.contains(c))
    {
        return invalid(&format!(
            "contains {bad:?}, which Windows refuses in a filename"
        ));
    }
    if let Some(bad) = component.chars().find(|c| (*c as u32) < 0x20) {
        return invalid(&format!(
            "contains control character U+{:04X}, which is not a legal filename byte",
            bad as u32
        ));
    }
    // A trailing dot or space is silently stripped by the Win32 layer, so the
    // file lands under a different name than the one recorded in the index.
    if component.ends_with('.') || component.ends_with(' ') {
        return invalid("ends with a dot or space, which Windows strips on create");
    }
    // Device names match on the stem, so `CON`, `CON.md` and `con.markdown`
    // are all the console.
    let stem = component.split('.').next().unwrap_or(component);
    if is_dos_device_name(stem) {
        return invalid(&format!(
            "uses the reserved DOS device name {stem:?}; Windows resolves it to a device, not a file"
        ));
    }
    // Git reserves `.git` for repository metadata. Any tree entry named `.git`
    // or an 8.3 alias is refused by libgit2 with `GIT_EINVALIDPATH` and cannot
    // be checkpointed.
    if is_git_reserved_component(component) {
        return invalid("is reserved by Git for repository metadata and refused in tree entries");
    }
    Ok(())
}

#[cfg(test)]
mod portable_page_path_tests {
    use super::{PagePath, portable_page_key};

    /// Every shape #462 reproduced on native Windows. Each either fails late
    /// with a 500, or writes but cannot be checkpointed by libgit2 and cannot
    /// be deleted through the normal API — silent partial state, which is
    /// worse than a clean rejection.
    #[test]
    fn windows_hostile_paths_are_rejected() {
        for raw in [
            "CON.md",
            "notes/aux.md",
            "notes/NUL.md",
            "notes/com1.md",
            "notes/LPT9.txt",
            "notes/trailing./x.md",
            "notes/trailing /x.md",
            "notes/end.md.",
            "notes/end.md ",
            "notes/a<b.md",
            "notes/a>b.md",
            "notes/a\"b.md",
            "notes/a|b.md",
            "notes/a?b.md",
            "notes/a*b.md",
            "notes/a:b.md",
            "notes/a\u{1}b.md",
        ] {
            let path = PagePath::new(raw).expect("still constructible: reads must keep working");
            assert!(
                path.ensure_portable().is_err(),
                "{raw:?} is not portable and must be refused at write time"
            );
        }
    }

    #[test]
    fn git_reserved_paths_are_rejected() {
        for raw in [
            ".git",
            ".git/config",
            ".git/HEAD",
            "notes/.git",
            "notes/.git/sub.md",
            "notes/.GIT/sub.md",
            "notes/.Git/sub.md",
            "notes/git~1",
            "notes/git~1/foo.md",
            "notes/GIT~2/bar.md",
            "notes/git~4/config",
            "a/b/c/.git/deep.md",
        ] {
            let path = PagePath::new(raw).expect("still constructible: reads must keep working");
            assert!(
                path.ensure_portable().is_err(),
                "{raw:?} contains a git-reserved component and must be refused at write time"
            );
        }
    }

    /// The rule must not reject ordinary pages. `con` is only reserved as a
    /// whole component, so `concepts/` and `icon.md` are fine.
    #[test]
    fn ordinary_paths_stay_writable() {
        for raw in [
            "notes/portable.md",
            "concepts/no-impl-without-test.md",
            "sessions/2026-08-22.md",
            "notes/icon.md",
            "notes/console.md",
            "prn-notes/aux-iliary.md",
            "a/b/c/deep.md",
            "notes/dot.in.middle.md",
            "notes/UPPER.MD",
            "notes/.git.md",
            "notes/.gitignore",
            "notes/.gitattributes",
            "notes/github.md",
            "git-notes/index.md",
        ] {
            let path = PagePath::new(raw).expect("valid path");
            assert!(
                path.ensure_portable().is_ok(),
                "{raw:?} is portable and must stay writable"
            );
        }
    }

    /// A multibyte component whose byte length is 5 must not be mistaken for
    /// a `git~N` alias, and must not panic: `is_git_reserved_component` once
    /// sliced the string at byte index 4, which is not a char boundary in a
    /// name like "abcé" (5 bytes) or "a😀" (5 bytes). Since this runs on the
    /// write funnel for untrusted input, the panic was a crashable defect.
    #[test]
    fn multibyte_components_are_not_git_reserved_and_do_not_panic() {
        for component in ["abcé", "ab€", "a😀", "éé", "🦀🦀"] {
            assert!(
                !super::is_git_reserved_component(component),
                "{component:?} is an ordinary name, not a git-reserved alias"
            );
        }
        // The full write funnel (construct + ensure_portable) must accept a
        // page path with a 5-byte multibyte component without panicking.
        for raw in ["notes/abcé.md", "notes/a😀.md", "notes/ab€.md"] {
            let path = PagePath::new(raw).expect("valid non-ASCII path");
            assert!(
                path.ensure_portable().is_ok(),
                "{raw:?} is portable and must stay writable"
            );
        }
    }

    /// Reads must keep working for pages already stored under a
    /// non-portable name: `PagePath::new` stays tolerant so a listing does
    /// not break on one bad row.
    #[test]
    fn existing_non_portable_pages_remain_constructible() {
        for raw in [
            "CON.md",
            "notes/a|b.md",
            "notes/trailing./x.md",
            ".git/config",
            "notes/.git/sub.md",
            "notes/git~1/foo.md",
        ] {
            assert!(
                PagePath::new(raw).is_ok(),
                "{raw:?} must still construct so persisted rows stay readable"
            );
        }
    }

    #[test]
    fn portable_key_collapses_case_and_normalization() {
        for (a, b) in [
            ("concepts/alpha.md", "concepts/Alpha.md"),
            ("Concepts/alpha.md", "concepts/ALPHA.md"),
            ("concepts/caf\u{00e9}.md", "concepts/cafe\u{0301}.md"),
            ("concepts/CAF\u{00c9}.md", "concepts/cafe\u{0301}.md"),
        ] {
            assert_eq!(
                portable_page_key(a),
                portable_page_key(b),
                "{a:?} and {b:?} are one file on macOS/Windows"
            );
        }
    }

    #[test]
    fn portable_key_keeps_distinct_paths_distinct() {
        for (a, b) in [
            ("concepts/alpha.md", "concepts/alphabet.md"),
            ("concepts/alpha.md", "decisions/alpha.md"),
            ("concepts/nested/a.md", "concepts/a.md"),
            ("concepts/cafe.md", "concepts/caf\u{00e9}.md"),
        ] {
            assert_ne!(portable_page_key(a), portable_page_key(b));
        }
    }
}
