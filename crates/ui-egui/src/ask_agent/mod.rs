//! Ask Agent integration: state on SlideApp, selection/document serialization, and undo grouping.

pub mod hub;
pub mod ui;

use std::sync::Arc;

use deckcraft_engine::DocState;
use egui::Rect;
use serde_json::{Value, json};

pub use hub::{AgentHub, HubError, Request, Snapshot, Status, Turn};
pub use ui::Action;

/// Editable starting points for requests to the connected agent.
const QUICK_ACTIONS: &[ui::QuickAction] = &[
    ui::QuickAction {
        label: "Shorten",
        group: "Rewrite",
        instruction: "Shorten the selected slide text while preserving its meaning and key facts.",
    },
    ui::QuickAction {
        label: "Professional",
        group: "Rewrite",
        instruction: "Rewrite the selected slide text in a clear, professional tone. Preserve its meaning and key facts.",
    },
    ui::QuickAction {
        label: "Turn into bullets",
        group: "",
        instruction: "Turn the selected slide text into concise bullet points. Preserve its key facts.",
    },
    ui::QuickAction {
        label: "Align and space",
        group: "Layout",
        instruction: "Align and evenly space the selected shapes to improve the slide layout. Preserve their content.",
    },
    ui::QuickAction {
        label: "Match styling",
        group: "Layout",
        instruction: "Make the selected shapes use consistent styling that fits the slide. Preserve their content.",
    },
    ui::QuickAction {
        label: "Speaker notes",
        group: "",
        instruction: "Draft concise speaker notes for the current slide using its existing content. Do not invent facts.",
    },
];

use crate::SlideApp;

/// The UI command.
pub const COMMAND: &str = "app.askAgent";
/// Shortcut to open Ask Agent.
pub const SHORTCUT: &str = "Cmd+Shift+O";

/// Canvas anchor kinds where the chip and bubble position themselves.
#[derive(Clone, Copy, Debug)]
pub enum Anchor {
    Shapes(Rect),
    Text { first: Rect, last: Rect },
    Caret(Rect),
}

impl Anchor {
    pub fn rect(&self) -> Rect {
        match *self {
            Anchor::Shapes(r) | Anchor::Caret(r) => r,
            Anchor::Text { first, last } => first.union(last),
        }
    }
}

/// One undo group preserving document state before any agent edits in the session.
#[derive(Clone, Debug)]
pub struct UndoGroup {
    pub uid: u64,
    pub before: Arc<deckcraft_model::Presentation>,
    pub selection: deckcraft_engine::Selection,
    pub after: Arc<deckcraft_model::Presentation>,
    pub undo_len: usize,
    /// The point type Edit Points remembered with `before`.
    pub vertex_kind: Option<(deckcraft_model::ShapeId, usize, usize, String)>,
}

/// An open Ask Agent conversation in the UI.
pub struct Open {
    pub id: u64,
    pub anchor: Rect,
    pub undo_group: Option<UndoGroup>,
    pub turns: Vec<Turn>,
    pub instruction: String,
    commented: bool,
    /// The slide comment holding this thread's answers; follow-ups reply to it.
    thread: Option<(usize, usize)>,
}

/// Ask Agent state stored on [`SlideApp`].
#[derive(Default)]
pub struct AskAgent {
    pub hub: Option<AgentHub>,
    pub open: Option<Open>,
    pub draft: String,
    pub picked: Option<String>,
    pub anchor: Option<Anchor>,
    /// Set while a command from an agent runs, so the hold does not refuse it.
    agent_turn: bool,
}

impl AskAgent {
    pub fn inspect(&self) -> Value {
        json!({
            "open": self.open.as_ref().map(|o| o.id),
            "agents": self.hub.as_ref().map(|h| h.agents()).unwrap_or_default(),
        })
    }
}

fn slide_json(st: &DocState, idx: usize) -> Value {
    let sl = st.doc.slides.get(idx);
    let id = sl.map(|s| s.id.0).unwrap_or(0);
    let title = sl.map(|s| s.title()).unwrap_or_else(|| format!("Slide {}", idx + 1));
    json!({"index": idx, "id": id, "title": title})
}

/// Serialize the current selection for the agent.
pub fn selection(st: &DocState) -> Value {
    let sel = &st.selection;
    let mut v = json!({"slide": slide_json(st, sel.slide)});
    if let Some(ts) = &sel.text {
        let (a, b) = ts.ordered();
        let text = deckcraft_engine::cmd::text::body_of(st, ts).map(|body| deckcraft_model::edit::text_range(body, a, b)).unwrap_or_default();
        v["kind"] = json!("text");
        v["text"] = json!({
            "shape": (!ts.notes).then_some(ts.shape),
            "anchor": [ts.anchor.0, ts.anchor.1],
            "caret": [ts.caret.0, ts.caret.1],
            "cell": ts.cell.map(|(r, c)| [r, c]),
            "notes": ts.notes,
            "text": text,
        });
    } else if !sel.shapes.is_empty() {
        let ctx = st.current_slide().and_then(|sl| deckcraft_model::resolve::Ctx::for_slide(&st.doc, sl));
        let shapes: Vec<Value> =
            sel.shapes.iter().filter_map(|id| st.shape(*id)).map(|sh| deckcraft_engine::cmd::inspect::shape_summary(ctx.as_ref(), sh, 0)).collect();
        v["kind"] = json!("shapes");
        v["shapes"] = json!(shapes);
    } else {
        v["kind"] = json!("slide");
        if sel.slides.len() > 1 {
            let slides: Vec<Value> = sel.slides.iter().filter_map(|id| st.doc.slide_index(*id)).map(|i| slide_json(st, i)).collect();
            v["slides"] = json!(slides);
        }
    }
    v
}

/// Summary of the active document for the agent.
pub fn document(st: &DocState) -> Value {
    let title =
        st.path.as_deref().and_then(|p| std::path::Path::new(p).file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| st.title());
    let lang = st.shapes().iter().find_map(|s| s.text.as_ref()?.paragraphs.iter().find_map(|p| p.runs.iter().find_map(|r| r.props.lang.clone())));
    json!({
        "id": st.uid,
        "title": title,
        "saved": st.path.is_some(),
        "slides": st.doc.slides.len(),
        "language": lang,
    })
}

/// Handle `app.askAgent` command invocation.
pub fn command(app: &mut SlideApp, p: &Value) -> Result<Value, String> {
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        handle_action(app, Action::Close);
        return Ok(json!({"open": false}));
    }
    let anchor =
        app.ask_agent.anchor.map(|a| a.rect()).unwrap_or_else(|| egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(340.0, 200.0)));
    if let Some(instr) = p.get("instruction").and_then(Value::as_str) {
        handle_action(app, Action::Close);
        let (sel, doc) = app.session.active().map(|st| (selection(st), document(st))).unwrap_or_default();
        let id = app.ask_agent.hub.as_ref().ok_or("no agent hub")?.ask(Request {
            id: 0,
            instruction: instr.to_string(),
            history: vec![],
            selection: sel,
            document: doc,
            agent: app.ask_agent.picked.clone(),
        });
        app.ask_agent.open =
            Some(Open { id, anchor, undo_group: None, turns: vec![], instruction: instr.to_string(), commented: false, thread: None });
        return Ok(json!({"id": id}));
    }
    if app.ask_agent.hub.is_none() {
        return Err("Ask Agent needs an agent: start DeckCraft with --control PORT and run `deckcraft-cli mcp --connect PORT`".into());
    }
    handle_action(app, Action::Close);
    app.ask_agent.open = Some(Open { id: 0, anchor, undo_group: None, turns: vec![], instruction: String::new(), commented: false, thread: None });
    Ok(json!({}))
}

/// Reconcile user typing outside the bubble.
pub fn typed_on_slide(app: &mut SlideApp, events: &[egui::Event]) {
    if app.ask_agent.open.is_none() {
        return;
    }
    if events.iter().any(|e| matches!(e, egui::Event::Text(_) | egui::Event::Paste(_))) {
        handle_action(app, Action::Close);
    }
}

/// Handle Escape key to dismiss open bubble.
pub fn keyboard(app: &mut SlideApp, ctx: &egui::Context) {
    if app.ask_agent.open.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        handle_action(app, Action::Close);
    }
}

pub fn remember_focus(_app: &mut SlideApp, _ctx: &egui::Context) {}

/// Draw the chip or bubble and dispatch actions.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(hub) = app.ask_agent.hub.clone() else { return };
    let Some(open) = &app.ask_agent.open else {
        if let Some(anchor) = app.ask_agent.anchor.map(|a| a.rect())
            && !hub.agents().is_empty()
            && ui::chip(ctx, anchor)
        {
            app.ask_agent.open =
                Some(Open { id: 0, anchor, undo_group: None, turns: vec![], instruction: String::new(), commented: false, thread: None });
        }
        return;
    };
    let (anchor, snapshot) = (open.anchor, hub.snapshot(open.id));
    if let Some(s) = snapshot.as_ref().filter(|s| s.status == Status::Answered) {
        comment_once(app, s);
    }
    let agents = hub.agents();
    let (history, instruction) = app.ask_agent.open.as_ref().map(|open| (open.turns.clone(), open.instruction.clone())).unwrap_or_default();
    let view = ui::View { snapshot: snapshot.as_ref(), agents: &agents, actions: QUICK_ACTIONS, history: &history, instruction: &instruction };
    if let Some(action) = ui::bubble(ctx, anchor, &view, &mut app.ask_agent.draft, &mut app.ask_agent.picked) {
        handle_action(app, action);
    }
}

/// The agent's answer, once, as a comment on what it was asked about, authored like its edits.
fn comment_once(app: &mut SlideApp, s: &Snapshot) {
    let Some(open) = app.ask_agent.open.as_mut().filter(|o| !o.commented) else { return };
    open.commented = true;
    let (Some(answer), id, thread) = (s.answer.clone(), open.id, open.thread) else { return };
    let answer = format!("You: {}\n\nAgent: {answer}", open.instruction);
    // The first answer starts a comment thread; follow-ups reply in it.
    if let Some((slide, index)) = thread {
        // Navigation is not a document edit: target the original comment while
        // preserving the person's current selection, including on failure.
        let Some(st) = app.session.active_mut() else { return };
        let selection = st.selection.clone();
        st.selection.slide = slide;
        let result = run_for(app, id, "comment.reply", json!({"index": index, "text": answer}));
        if let Some(st) = app.session.active_mut() {
            st.selection = selection;
        }
        if let Err(error) = result {
            app.set_status(format!("Could not save the agent's reply: {error}"));
        }
    } else if let Ok(result) = run_for(app, id, "comment.add", json!({"text": answer})) {
        let thread = app
            .session
            .active()
            .and_then(|st| result["index"].as_u64().and_then(|index| usize::try_from(index).ok()).map(|index| (st.selection.slide, index)));
        if let Some(open) = app.ask_agent.open.as_mut() {
            open.thread = thread;
        }
    }
}

/// The shapes agents are working on, with each agent's colour.
pub fn held_shapes(app: &SlideApp) -> Vec<(u64, [u8; 3])> {
    let Some(hub) = &app.ask_agent.hub else { return vec![] };
    let ids = |sel: &Value| -> Vec<u64> {
        sel["shapes"].as_array().into_iter().flatten().filter_map(|s| s["id"].as_u64()).chain(sel["text"]["shape"].as_u64()).collect()
    };
    hub.held().into_iter().flat_map(|(sel, name)| ids(&sel).into_iter().map(move |id| (id, hub::colour(&name)))).collect()
}

/// Why the person may not run `command` now: it would change shapes an agent is working on.
pub fn held(app: &SlideApp, command: &str) -> Option<String> {
    let hub = app.ask_agent.hub.as_ref().filter(|_| !app.ask_agent.agent_turn)?;
    let spec = deckcraft_engine::cmd::find_command(command)?;
    let st = app.session.active()?;
    if !spec.undoable || command.starts_with("edit.select") {
        return None;
    }
    let mine: Vec<u64> = st.selection.shapes.iter().map(|s| u64::from(s.0)).chain(st.selection.text.as_ref().map(|t| u64::from(t.shape.0))).collect();
    hub.held().into_iter().find_map(|(sel, name)| {
        let ids = sel["shapes"].as_array().into_iter().flatten().filter_map(|s| s["id"].as_u64()).chain(sel["text"]["shape"].as_u64());
        let slide_held = sel["kind"] == "slide" && sel["slide"]["index"].as_u64() == Some(st.selection.slide as u64);
        (slide_held || ids.into_iter().any(|id| mine.contains(&id))).then(|| format!("Held by Agent ({name})"))
    })
}

fn handle_action(app: &mut SlideApp, action: Action) {
    let hub = app.ask_agent.hub.clone();
    match action {
        Action::Send(text) => {
            let Some(hub) = hub else { return };
            let (sel, doc) = app.session.active().map(|st| (selection(st), document(st))).unwrap_or_default();
            let agent = app.ask_agent.picked.clone();
            let Some(open) = &mut app.ask_agent.open else { return };
            // After an answer, this is a follow-up: the thread so far goes with it.
            if let Some(answer) = hub.snapshot(open.id).and_then(|s| s.answer) {
                open.turns.push(Turn { instruction: std::mem::take(&mut open.instruction), answer });
                hub.close(open.id);
            }
            open.instruction = text.clone();
            open.commented = false;
            open.id = hub.ask(Request { id: 0, instruction: text, history: open.turns.clone(), selection: sel, document: doc, agent });
        }
        Action::Undo => {
            let Some(open) = app.ask_agent.open.take() else { return };
            if let Some(hub) = &hub {
                hub.close(open.id);
            }
            if let Some(ug) = open.undo_group
                && let Some(st) = app.session.active_mut()
            {
                if st.uid == ug.uid && Arc::ptr_eq(&ug.after, &st.doc) {
                    st.doc = ug.before;
                    st.selection = ug.selection;
                    st.history.undo.truncate(ug.undo_len);
                    st.history.redo.clear();
                    st.revision += 1;
                } else {
                    app.set_status("The presentation changed since the agent's edits; use Undo in the Edit menu");
                }
            }
        }
        Action::Keep | Action::Close => {
            if let Some(open) = app.ask_agent.open.take()
                && let Some(hub) = &hub
            {
                hub.close(open.id);
            }
        }
    }
}

/// Execute an engine edit on behalf of a request, coalescing changes into one undo group.
pub fn run_for(app: &mut SlideApp, request: u64, id: &str, params: Value) -> Result<Value, String> {
    let spec = deckcraft_engine::cmd::find_command(id).ok_or("Ask Agent edits must use engine commands")?;
    if id.starts_with("file.") || matches!(id, "edit.undo" | "edit.redo" | "text.compose" | "text.commit") {
        return Err("file, history and composition commands cannot run for an Ask Agent request".into());
    }
    let st = app.session.active().ok_or("open the request's presentation first")?;
    if st.interaction.is_some() {
        return Err("finish the pointer gesture before the agent edits".into());
    }
    app.session.end_composition();
    let st = app.session.active().ok_or("open the request's presentation first")?;

    if let Some(open) = &app.ask_agent.open
        && open.id == request
        && let Some(ug) = &open.undo_group
        && (ug.uid != st.uid || !Arc::ptr_eq(&ug.after, &st.doc))
    {
        return Err("the presentation changed after the agent's edit; finish this request and ask again".into());
    }

    let (uid, before, selection, undo_len, limit) = (st.uid, st.doc.clone(), st.selection.clone(), st.history.undo.len(), st.history.limit);
    let vertex_kind = app.session.tool.vertex_kind.clone();

    if let Some(st) = app.session.active_mut() {
        st.history.limit = limit.max(1).saturating_add(1);
    }
    let name = app.ask_agent.hub.as_ref().and_then(|h| h.snapshot(request)).and_then(|s| s.agent).unwrap_or_default();
    let author = std::mem::replace(&mut app.session.prefs.author, format!("Agent ({name})"));
    let was_agent = std::mem::replace(&mut app.ask_agent.agent_turn, true);
    let result = app.run(id, params);
    app.ask_agent.agent_turn = was_agent;
    app.session.prefs.author = author;
    if let Some(st) = app.session.active_mut() {
        st.history.limit = limit;
    }
    let result = result?;
    app.session.end_composition();
    let st = app.session.active_mut().ok_or("presentation closed")?;

    if !Arc::ptr_eq(&before, &st.doc) {
        if !spec.undoable {
            st.doc = before;
            st.selection = selection;
            st.revision += 1;
            return Err("this command changes the document without undo support".into());
        }
        let (ug_before, ug_sel, ug_undo_len, ug_vertex_kind) = if let Some(open) = &app.ask_agent.open
            && open.id == request
            && let Some(ug) = &open.undo_group
        {
            (ug.before.clone(), ug.selection.clone(), ug.undo_len, ug.vertex_kind.clone())
        } else {
            (before, selection, undo_len, vertex_kind)
        };

        st.history.undo.truncate(ug_undo_len);
        st.history.undo.push(deckcraft_engine::HistoryEntry {
            label: "Ask Agent".into(),
            doc: ug_before.clone(),
            selection: ug_sel.clone(),
            vertex_kind: ug_vertex_kind.clone(),
        });
        st.history.redo.clear();

        if let Some(open) = &mut app.ask_agent.open
            && open.id == request
        {
            open.undo_group = Some(UndoGroup {
                uid,
                before: ug_before,
                selection: ug_sel,
                after: st.doc.clone(),
                undo_len: ug_undo_len,
                vertex_kind: ug_vertex_kind,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use deckcraft_engine::Session;
    use std::time::Duration;

    #[test]
    fn test_bubble_flow_headless() {
        let hub = AgentHub::default();
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default()).with_agent_hub(hub.clone());

        let res = command(&mut app, &json!({"instruction": "make title bigger"})).unwrap();
        let req_id = res["id"].as_u64().unwrap();
        assert!(app.ask_agent.open.is_some());
        assert_eq!(app.ask_agent.open.as_ref().unwrap().id, req_id);

        let acquired = hub.listen(1, Some("Codex"), Duration::from_millis(50), || true).unwrap();
        assert_eq!(acquired.id, req_id);
        assert_eq!(acquired.instruction, "make title bigger");

        hub.reply(1, req_id, "Processing…", false, None).unwrap();
        let snap = hub.snapshot(req_id).unwrap();
        assert_eq!(snap.notes, vec!["Processing…"]);

        let doc_before = app.session.active().unwrap().doc.clone();
        run_for(&mut app, req_id, "shape.insert", json!({"preset": "rect", "rect": [10.0, 10.0, 100.0, 50.0]})).unwrap();
        assert!(!Arc::ptr_eq(&doc_before, &app.session.active().unwrap().doc));

        assert!(held(&app, "shape.insert").is_some_and(|why| why.contains("Codex")), "the slide is held for the agent");
        hub.reply(1, req_id, "Done inserting shape", true, None).unwrap();
        comment_once(&mut app, &hub.snapshot(req_id).unwrap());
        let st = app.session.active().unwrap();
        assert_eq!(st.doc.slides[st.selection.slide].comments.last().unwrap().author, "Agent (Codex)");
        assert!(held(&app, "shape.insert").is_none(), "answered: the slide is free");

        handle_action(&mut app, Action::Send("now make it red".into()));
        let follow_id = app.ask_agent.open.as_ref().unwrap().id;
        assert_ne!(follow_id, req_id);
        assert_eq!(app.ask_agent.open.as_ref().unwrap().turns.len(), 1);

        let acquired2 = hub.listen(1, Some("Codex"), Duration::from_millis(50), || true).unwrap();
        assert_eq!(acquired2.id, follow_id);
        assert_eq!(acquired2.history.len(), 1);

        hub.reply(1, follow_id, "Done with the follow-up", true, None).unwrap();
        comment_once(&mut app, &hub.snapshot(follow_id).unwrap());
        // Repainting a completed answer must not append a second reply.
        comment_once(&mut app, &hub.snapshot(follow_id).unwrap());
        let st = app.session.active().unwrap();
        let comments = &st.doc.slides[st.selection.slide].comments;
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "You: make title bigger\n\nAgent: Done inserting shape");
        assert_eq!(comments[0].replies.len(), 1);
        assert_eq!(comments[0].replies[0].text, "You: now make it red\n\nAgent: Done with the follow-up");
        assert_eq!(comments[0].replies[0].author, "Agent (Codex)");
        assert_eq!(st.history.undo.len(), 1, "edits, answer and reply share one undo group");

        handle_action(&mut app, Action::Undo);
        assert!(Arc::ptr_eq(&doc_before, &app.session.active().unwrap().doc));
        assert!(app.ask_agent.open.is_none());
    }

    #[test]
    fn follow_up_stays_on_original_slide_after_navigation() {
        let hub = AgentHub::default();
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default()).with_agent_hub(hub.clone());
        app.run("slide.new", json!({})).unwrap();
        app.session.active_mut().unwrap().selection.slide = 0;
        let before = app.session.active().unwrap().doc.clone();
        let undo_len = app.session.active().unwrap().history.undo.len();
        let id = command(&mut app, &json!({"instruction": "review this slide"})).unwrap()["id"].as_u64().unwrap();
        hub.listen(1, Some("Codex"), Duration::from_millis(50), || true).unwrap();
        hub.reply(1, id, "First answer", true, None).unwrap();
        comment_once(&mut app, &hub.snapshot(id).unwrap());

        app.session.active_mut().unwrap().selection.slide = 1;
        handle_action(&mut app, Action::Send("explain further".into()));
        let follow_id = app.ask_agent.open.as_ref().unwrap().id;
        hub.listen(1, Some("Codex"), Duration::from_millis(50), || true).unwrap();
        hub.reply(1, follow_id, "More detail", true, None).unwrap();
        comment_once(&mut app, &hub.snapshot(follow_id).unwrap());
        let st = app.session.active().unwrap();
        assert_eq!(st.selection.slide, 1, "person's navigation is preserved");
        assert_eq!(st.doc.slides[0].comments.len(), 1);
        assert_eq!(st.doc.slides[0].comments[0].replies[0].text, "You: explain further\n\nAgent: More detail");
        assert_eq!(st.doc.slides[0].comments[0].replies[0].author, "Agent (Codex)");
        assert!(st.doc.slides[1].comments.is_empty());
        assert_eq!(st.history.undo.len(), undo_len + 1);
        handle_action(&mut app, Action::Undo);
        assert!(Arc::ptr_eq(&before, &app.session.active().unwrap().doc));
    }

    #[test]
    fn test_command_without_hub_returns_error() {
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        let err = command(&mut app, &json!({})).unwrap_err();
        assert!(err.contains("Ask Agent needs an agent"));
        assert!(app.ask_agent.open.is_none());
    }

    #[test]
    fn test_dismiss_closes_request_in_hub() {
        let hub = AgentHub::default();
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default()).with_agent_hub(hub.clone());

        let res = command(&mut app, &json!({"instruction": "draft slide"})).unwrap();
        let req_id = res["id"].as_u64().unwrap();
        assert!(hub.snapshot(req_id).is_some());

        typed_on_slide(&mut app, &[egui::Event::Text("a".into())]);
        assert!(app.ask_agent.open.is_none());
        assert!(hub.snapshot(req_id).is_none(), "typing dismisses and closes request in hub");
    }
}
