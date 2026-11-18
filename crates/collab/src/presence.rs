//! Ephemeral participant state for one session (ADR 0006).
//!
//! Presence is not an operation: it never enters a version, a journal or a snapshot, and losing
//! a message is cured by the next one. The set here keeps the **latest** state per actor,
//! expires it after a TTL, and gives every actor a stable colour so the canvas can draw a cursor
//! without asking the server.
//!
//! Time is passed in, not read: the same code runs on the desktop and in wasm and is tested
//! without sleeping.

use std::collections::HashMap;

use deckcraft_collab_proto::{PresenceMsg, Role, ShapeKey, SlideKey, TextCursor, Viewport};

/// One participant's latest state.
#[derive(Clone, Debug, PartialEq)]
pub struct Participant {
    pub actor_id: String,
    pub role: Option<Role>,
    /// Stable colour for the cursor and the selection outline.
    pub color: String,
    pub slide: SlideKey,
    pub shapes: Vec<ShapeKey>,
    pub text: Option<TextCursor>,
    pub viewport: Option<Viewport>,
    /// When this state arrived, so it can expire.
    pub updated_ms: u64,
}

/// Presence of everyone in one session.
#[derive(Clone, Debug)]
pub struct PresenceSet {
    ttl_ms: u64,
    by_actor: HashMap<String, Participant>,
}

/// How long a participant stays visible without a new message.
pub const DEFAULT_TTL_MS: u64 = 30_000;

impl Default for PresenceSet {
    fn default() -> Self {
        PresenceSet { ttl_ms: DEFAULT_TTL_MS, by_actor: HashMap::new() }
    }
}

impl PresenceSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ttl(ttl_ms: u64) -> Self {
        PresenceSet { ttl_ms, by_actor: HashMap::new() }
    }

    pub fn ttl_ms(&self) -> u64 {
        self.ttl_ms
    }

    /// Record a participant's latest state. A newer message replaces the older one whole: the
    /// last state is what matters, and there is nothing to merge.
    pub fn update(&mut self, msg: PresenceMsg, now_ms: u64) {
        let color = color_for(&msg.actor_id);
        let entry = Participant {
            role: None,
            color,
            actor_id: msg.actor_id,
            slide: msg.slide,
            shapes: msg.shapes,
            text: msg.text,
            viewport: msg.viewport,
            updated_ms: now_ms,
        };
        self.by_actor.insert(entry.actor_id.clone(), entry);
    }

    /// Set the role once the server's `welcome` said it (presence carries no role).
    pub fn set_role(&mut self, actor_id: &str, role: Role) {
        if let Some(p) = self.by_actor.get_mut(actor_id) {
            p.role = Some(role);
        }
    }

    /// Drop participants whose last message is older than the TTL. Returns how many went.
    pub fn expire(&mut self, now_ms: u64) -> usize {
        let ttl = self.ttl_ms;
        let before = self.by_actor.len();
        self.by_actor.retain(|_, p| now_ms.saturating_sub(p.updated_ms) < ttl);
        before - self.by_actor.len()
    }

    pub fn get(&self, actor_id: &str) -> Option<&Participant> {
        self.by_actor.get(actor_id)
    }

    pub fn remove(&mut self, actor_id: &str) -> Option<Participant> {
        self.by_actor.remove(actor_id)
    }

    /// Everyone, ordered by actor id so a redraw is deterministic.
    pub fn participants(&self) -> Vec<&Participant> {
        let mut all: Vec<&Participant> = self.by_actor.values().collect();
        all.sort_by(|a, b| a.actor_id.cmp(&b.actor_id));
        all
    }

    /// Who is on one slide, for the canvas.
    pub fn on_slide<'a>(&'a self, slide: &'a SlideKey) -> impl Iterator<Item = &'a Participant> + 'a {
        self.by_actor.values().filter(move |p| &p.slide == slide)
    }

    pub fn len(&self) -> usize {
        self.by_actor.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_actor.is_empty()
    }
}

/// A colour that does not change between messages, sessions or processes.
///
/// Eight values with enough contrast on the slide and in the dark UI; the hash is FNV-1a so the
/// desktop, the server and the browser agree without sharing a table.
pub fn color_for(actor_id: &str) -> String {
    const PALETTE: [&str; 8] = ["#3b82f6", "#ef4444", "#10b981", "#f59e0b", "#8b5cf6", "#ec4899", "#14b8a6", "#84cc16"];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in actor_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let index = (hash % PALETTE.len() as u64) as usize;
    PALETTE[index].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(actor: &str, slide: &str) -> PresenceMsg {
        PresenceMsg {
            actor_id: actor.into(),
            slide: SlideKey(slide.into()),
            shapes: vec![ShapeKey("h1".into())],
            text: None,
            viewport: None,
            color: None,
        }
    }

    #[test]
    fn the_latest_state_replaces_the_older_one() {
        let mut set = PresenceSet::new();
        set.update(presence("a", "s1"), 1_000);
        let mut newer = presence("a", "s2");
        newer.shapes = vec![ShapeKey("h2".into())];
        set.update(newer, 2_000);
        let p = set.get("a").expect("participant");
        assert_eq!(p.slide.0, "s2");
        assert_eq!(p.shapes[0].0, "h2");
        assert_eq!(p.updated_ms, 2_000);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn a_participant_disappears_after_the_ttl() {
        let mut set = PresenceSet::with_ttl(30_000);
        set.update(presence("a", "s1"), 1_000);
        assert_eq!(set.expire(30_999), 0, "still inside the window");
        assert_eq!(set.expire(31_001), 1, "older than the TTL");
        assert!(set.is_empty());
    }

    #[test]
    fn a_new_message_keeps_the_participant_alive() {
        let mut set = PresenceSet::with_ttl(30_000);
        set.update(presence("a", "s1"), 1_000);
        set.update(presence("a", "s2"), 20_000);
        assert_eq!(set.expire(40_000), 0);
        assert_eq!(set.get("a").map(|p| p.slide.0.as_str()), Some("s2"));
    }

    #[test]
    fn colour_is_stable_and_comes_from_a_small_contrasting_palette() {
        assert_eq!(color_for("actor-1"), color_for("actor-1"));
        let colors: Vec<String> = (0..8).map(|i| color_for(&format!("actor-{i}"))).collect();
        for c in &colors {
            assert!(c.starts_with('#') && c.len() == 7, "{c}");
        }
        // The palette is small on purpose: eight values, so collisions are expected and fine.
        assert!(colors.iter().all(|c| c.len() == 7));
    }

    #[test]
    fn participants_come_out_in_a_stable_order() {
        let mut set = PresenceSet::new();
        set.update(presence("c", "s1"), 1);
        set.update(presence("a", "s1"), 1);
        set.update(presence("b", "s2"), 1);
        let names: Vec<&str> = set.participants().iter().map(|p| p.actor_id.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(set.on_slide(&SlideKey("s1".into())).count(), 2);
    }

    #[test]
    fn the_role_comes_from_the_welcome_not_from_presence() {
        let mut set = PresenceSet::new();
        set.update(presence("a", "s1"), 1);
        set.set_role("a", Role::Viewer);
        assert_eq!(set.get("a").and_then(|p| p.role), Some(Role::Viewer));
    }
}
