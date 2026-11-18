//! The cabinet's host-bridge contract for a collaboration session (`INTEGRATION.md` §6).
//!
//! The messages themselves are `craft-web-host`'s (`session`, `session-state`, `presence`, `commit`,
//! `committed`, ADR-0016 §3); this module is DeckCraft's side of them, kept free of `web_sys` so
//! the handling is tested headlessly. Without a `Session` the app stays single-user and makes no
//! network call (TASK.md §1).

use std::fmt;

use deckcraft_collab::Participant;
use deckcraft_collab_proto::Role;

/// The session the host bridge opened. A credential-bearing value, so it has a redacted `Debug`
/// and no `Serialize`: it must never reach preferences, a log line or a URL.
#[derive(Clone, PartialEq, Eq)]
pub struct HostSession {
    session_id: String,
    ws_url: String,
    token: String,
    role: String,
}

impl HostSession {
    pub fn new(session_id: impl Into<String>, ws_url: impl Into<String>, token: impl Into<String>, role: impl Into<String>) -> HostSession {
        HostSession { session_id: session_id.into(), ws_url: ws_url.into(), token: token.into(), role: role.into() }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The sync server's WebSocket URL. The token is not part of it.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    /// The JWT. It travels in the first frame and in the media requests' `Authorization` header,
    /// never in a URL, a log line or a DOM attribute.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Replace the JWT: the cabinet pushes a fresh one before the old expires.
    pub fn set_token(&mut self, token: impl Into<String>) {
        self.token = token.into();
    }
}

impl fmt::Debug for HostSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostSession")
            .field("session_id", &self.session_id)
            .field("ws_url", &self.ws_url)
            .field("token", &"<redacted>")
            .field("role", &self.role)
            .finish()
    }
}

/// What the host bridge delivered to the app.
#[derive(Clone, Debug)]
pub enum HostEvent {
    /// `deckcraft:session` (host → app): open a collaboration session.
    Session(HostSession),
    /// `deckcraft:committed` (host → app): the cabinet stored the commit.
    Committed { version: u64, etag: String },
}

/// What the app must do after one host event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Dial the session; the host opened it, so the app owns no network call before this point.
    Connect(HostSession),
    /// The cabinet stored the commit; show its version and etag.
    Committed { version: u64, etag: String },
}

/// The host-side collaboration state the shell keeps between frames.
#[derive(Default, Debug)]
pub struct HostState {
    /// The session the host opened, if any.
    pub session: Option<HostSession>,
    /// The last commit the cabinet confirmed.
    pub committed: Option<(u64, String)>,
    /// Whether `session-state` has been posted at least once.
    pub connected: bool,
}

impl HostState {
    /// Apply one host message. Anything else leaves the app single-user.
    pub fn on_event(&mut self, event: HostEvent) -> HostAction {
        match event {
            HostEvent::Session(session) => {
                self.connected = false;
                self.session = Some(session.clone());
                HostAction::Connect(session)
            }
            HostEvent::Committed { version, etag } => {
                self.committed = Some((version, etag.clone()));
                HostAction::Committed { version, etag }
            }
        }
    }

    pub fn mark_connected(&mut self) {
        self.connected = true;
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }
}

/// The contract's word for a session role (`INTEGRATION.md` §6: `owner`, `editor`, `commenter`,
/// `viewer`). DeckCraft's protocol knows the first, second and last.
pub fn role_word(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
    }
}

/// One participant in the shape the host bridge sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostParticipant {
    /// The sync server's actor id (`alice@example.com`): stable for the session.
    pub client_id: String,
    /// The session role: `owner`, `editor`, `commenter` or `viewer`.
    pub role: String,
    /// The CSS colour (`#rrggbb`) the app draws this participant's caret in.
    pub color: Option<String>,
}

/// The participant list the cabinet shows, from the presence set's rows.
pub fn host_participants(participants: &[&Participant]) -> Vec<HostParticipant> {
    participants
        .iter()
        .map(|participant| HostParticipant {
            client_id: participant.actor_id.clone(),
            // A peer that has not sent presence yet has no role here; the contract's word for
            // "may only read" is the safe one.
            role: participant.role.map(role_word).unwrap_or("viewer").to_string(),
            color: Some(participant.color.clone()).filter(|color| !color.is_empty()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_message_starts_a_connection_and_keeps_the_token_out_of_debug() {
        let mut state = HostState::default();
        assert!(!state.has_session());
        let session = HostSession::new("s1", "wss://collab.example/v1/sessions/s1/ws", "jwt-secret", "editor");
        assert_eq!(state.on_event(HostEvent::Session(session.clone())), HostAction::Connect(session));
        assert!(state.has_session());
        let debug = format!("{:?}", state.session.as_ref().expect("the session is stored"));
        assert!(debug.contains("s1"), "{debug}");
        assert!(!debug.contains("jwt-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }

    #[test]
    fn a_committed_message_is_recorded_for_the_ui() {
        let mut state = HostState::default();
        let action = state.on_event(HostEvent::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(action, HostAction::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(state.committed, Some((7, "\"e7\"".to_string())));
        assert!(!state.has_session());
    }

    #[test]
    fn without_a_session_the_app_stays_single_user() {
        let state = HostState::default();
        assert!(!state.has_session());
        assert!(state.committed.is_none());
    }

    #[test]
    fn status_words_follow_the_host_contract() {
        assert_eq!(role_word(Role::Owner), "owner");
        assert_eq!(role_word(Role::Editor), "editor");
        assert_eq!(role_word(Role::Viewer), "viewer");
    }

    #[test]
    fn participants_keep_the_actor_id_role_and_colour() {
        use deckcraft_collab_proto::{Role, SlideKey};
        let people = [
            Participant {
                actor_id: "alice@example.com".into(),
                role: Some(Role::Owner),
                color: "#112233".into(),
                slide: SlideKey("s1".into()),
                shapes: Vec::new(),
                text: None,
                viewport: None,
                updated_ms: 0,
            },
            Participant {
                actor_id: "bob@example.com".into(),
                role: None,
                color: String::new(),
                slide: SlideKey("s1".into()),
                shapes: Vec::new(),
                text: None,
                viewport: None,
                updated_ms: 0,
            },
        ];
        let rows: Vec<&Participant> = people.iter().collect();
        let host = host_participants(&rows);
        assert_eq!(host[0].client_id, "alice@example.com");
        assert_eq!(host[0].role, "owner");
        assert_eq!(host[0].color.as_deref(), Some("#112233"));
        // An unknown role is not invented: the contract's word for "may only read" is `viewer`.
        assert_eq!(host[1].role, "viewer");
        assert_eq!(host[1].color, None);
    }
}
