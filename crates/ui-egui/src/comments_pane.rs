//! Stable message actions, rich drafts and bounded nested thread UI.
use std::{collections::BTreeMap, sync::Arc};

use crate::{SlideApp, ViewMode, notes, theme::Tokens};
use deckcraft_model::{
    Comment, ShapeKind, TextBody,
    comments::{self, CommentAnchor, CommentStatus, CommentTask, CommentTaskAuthor},
    edit,
};
use egui::{Ui, text::CCursorRange};
use serde_json::{Value, json};

const PAGE_SIZE: usize = 100;
const AUTHOR_PAGE_SIZE: usize = 20;

#[derive(Clone)]
struct AuthorChoice {
    author: CommentTaskAuthor,
    task_only: bool,
}
#[derive(Clone)]
struct MessageDraft {
    uid: u64,
    base_body: Option<TextBody>,
    base_text: String,
    draft: notes::Draft,
    selection: Option<CCursorRange>,
}
impl MessageDraft {
    fn new(uid: u64, comment: &Comment) -> Self {
        Self {
            uid,
            base_body: comment.body.clone(),
            base_text: comment.text.clone(),
            draft: notes::Draft::new(&comment.body.clone().unwrap_or_else(|| TextBody::from_text(&comment.text))),
            selection: None,
        }
    }
    fn current(&self, uid: u64, comment: &Comment) -> bool {
        self.uid == uid && self.base_body == comment.body && self.base_text == comment.text
    }
}
#[derive(Clone)]
struct TaskDraft {
    uid: u64,
    base: Option<CommentTask>,
    task: CommentTask,
}

fn run_for(app: &mut SlideApp, uid: u64, command: &str, params: Value) -> Result<Value, String> {
    if app.session.active().is_none_or(|state| state.uid != uid) {
        let error = "The original presentation is no longer active.".to_string();
        app.set_status(error.clone());
        return Err(error);
    }
    app.run(command, params)
}

pub(crate) fn show(app: &mut SlideApp, ui: &mut Ui) {
    if ui.button("+ New Comment").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("comment"));
    }
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let uid = st.uid;
    let revision = st.revision;
    let valid_key = egui::Id::new(("comment-validation", uid, revision));
    if !ui.data(|data| data.get_temp::<bool>(valid_key).unwrap_or(false)) {
        if let Err(error) = comments::validate(&doc, true) {
            ui.label(error.to_string());
            return;
        }
        ui.data_mut(|data| data.insert_temp(valid_key, true));
    }
    let mut rows = Vec::new();
    fn flatten<'a>(items: &'a [Comment], slide: usize, root: Option<u32>, depth: usize, out: &mut Vec<(usize, u32, usize, &'a Comment)>) {
        if depth > comments::MAX_DEPTH {
            return;
        }
        for comment in items {
            let id = comment.comment_id.unwrap_or(0);
            let root = root.unwrap_or(id);
            out.push((slide, root, depth, comment));
            flatten(&comment.replies, slide, Some(root), depth + 1, out);
        }
    }
    for (slide, owner) in doc.slides.iter().enumerate() {
        flatten(&owner.comments, slide, None, 0, &mut rows);
    }
    if rows.is_empty() {
        ui.label("No comments.");
        return;
    }
    let authors_key = egui::Id::new(("comment-authors", uid));
    let authors = ui
        .data(|data| data.get_temp::<(u64, Arc<Vec<AuthorChoice>>)>(authors_key))
        .filter(|(cached_revision, _)| *cached_revision == revision)
        .map(|(_, authors)| authors)
        .unwrap_or_else(|| {
            let mut authors = BTreeMap::new();
            for (_, _, _, comment) in &rows {
                if let Some(identity) = &comment.author_identity {
                    authors.insert(
                        identity.id,
                        AuthorChoice {
                            author: CommentTaskAuthor {
                                identity: identity.clone(),
                                name: comment.author.clone(),
                                initials: comment.initials.clone(),
                            },
                            task_only: false,
                        },
                    );
                }
            }
            for (_, _, _, comment) in &rows {
                if let Some(task) = &comment.task {
                    for author in &task.authors {
                        authors.entry(author.identity.id).or_insert_with(|| AuthorChoice { author: author.clone(), task_only: true });
                    }
                }
            }
            let authors: Arc<Vec<AuthorChoice>> = Arc::new(authors.into_values().collect());
            ui.data_mut(|data| data.insert_temp(authors_key, (revision, authors.clone())));
            authors
        });
    let page_key = egui::Id::new(("comment-page", uid));
    let focus_key = egui::Id::new(("comment-page-focus", uid));
    let mut page = ui.data(|data| data.get_temp::<usize>(page_key).unwrap_or(0));
    let pages = rows.len().div_ceil(PAGE_SIZE);
    if let Some((focus_uid, id)) = app.ui.comment_focus.filter(|(focus_uid, _)| *focus_uid == uid) {
        let marker = (focus_uid, id, revision);
        if ui.data(|data| data.get_temp::<(u64, u32, u64)>(focus_key)) != Some(marker) {
            if let Some(index) = rows.iter().position(|(_, _, _, comment)| comment.comment_id == Some(id)) {
                page = index / PAGE_SIZE;
            }
            ui.data_mut(|data| data.insert_temp(focus_key, marker));
        }
    }
    page = page.min(pages.saturating_sub(1));
    ui.horizontal(|ui| {
        if ui.add_enabled(page > 0, egui::Button::new("Previous comments")).clicked() {
            page = page.saturating_sub(1);
        }
        ui.label(format!("Comments {} / {}", page + 1, pages));
        if ui.add_enabled(page + 1 < pages, egui::Button::new("Next comments")).clicked() {
            page += 1;
        }
    });
    ui.data_mut(|data| data.insert_temp(page_key, page));
    for (slide, root, depth, comment) in rows.into_iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
        ui.push_id((uid, comment.comment_id, slide, root), |ui| {
            if depth > 0 {
                ui.label(egui::RichText::new(format!("Reply · level {depth}")).small());
            }
            card(app, ui, uid, slide, root, comment, &authors);
        });
        ui.add_space(6.0);
    }
}

fn position(body: &TextBody, mut offset: usize) -> edit::Pos {
    for (index, paragraph) in body.paragraphs.iter().enumerate() {
        let length = paragraph.char_len();
        if offset <= length {
            return (index, offset);
        }
        offset = offset.saturating_sub(length.saturating_add(1));
    }
    edit::end(body)
}
fn scalar_offset(body: &TextBody, pos: edit::Pos) -> Option<u32> {
    let paragraph = body.paragraphs.get(pos.0)?;
    if pos.1 > paragraph.char_len() {
        return None;
    }
    let mut offset = pos.1;
    for paragraph in body.paragraphs.iter().take(pos.0) {
        offset = offset.checked_add(paragraph.char_len())?.checked_add(1)?;
    }
    u32::try_from(offset).ok()
}
fn selected_range(app: &SlideApp) -> Result<CommentAnchor, String> {
    let error = || "Select a nonempty text range in a slide text box. Table cells, notes and masters do not support comment ranges.".to_string();
    let st = app.session.active().ok_or_else(error)?;
    if st.selection.target != deckcraft_engine::Target::Slides || app.ui.view == ViewMode::NotesPage {
        return Err(error());
    }
    let selected = st.selection.text.as_ref().filter(|text| !text.notes && text.cell.is_none() && text.is_range()).ok_or_else(error)?;
    let shape = st.shape(selected.shape).filter(|shape| !matches!(shape.kind, ShapeKind::Table(_))).ok_or_else(error)?;
    let body = shape.text.as_ref().ok_or_else(error)?;
    let (start, end) = selected.ordered();
    let start = scalar_offset(body, start).ok_or_else(error)?;
    let end = scalar_offset(body, end).ok_or_else(error)?;
    if start >= end {
        return Err(error());
    }
    Ok(CommentAnchor::TextRange { shape: selected.shape, start, end })
}
fn focus(app: &mut SlideApp, uid: u64, slide: usize, comment: &Comment) {
    if run_for(app, uid, "slide.go", json!({"index":slide})).is_err() {
        return;
    }
    if let Some(id) = comment.comment_id {
        app.ui.comment_focus = Some((uid, id));
    }
    if let Some(shape) = comment.comment_anchor().shape() {
        let _ = app.session.select(|_, selection| {
            selection.shapes = vec![shape];
            selection.text = None;
            selection.cells = None;
            selection.table_owner = None;
            selection.table_path = Default::default();
            selection.cell_block = None;
        });
        if let CommentAnchor::TextRange { start, end, .. } = comment.comment_anchor() {
            let positions = app
                .session
                .active()
                .and_then(|st| st.shape(shape))
                .and_then(|shape| shape.text.as_ref())
                .map(|body| (position(body, start as usize), position(body, end as usize)));
            if let Some((start, end)) = positions {
                let _ = run_for(app, uid, "text.edit", json!({"id":shape,"at":[start.0,start.1]}));
                let _ = run_for(app, uid, "text.select", json!({"anchor":[start.0,start.1],"caret":[end.0,end.1]}));
            }
        }
    }
}

fn message_editor(app: &mut SlideApp, ui: &mut Ui, uid: u64, id: u32, comment: &Comment) {
    let key = egui::Id::new(("comment-rich-draft", uid, id));
    let mut draft = ui.data(|data| data.get_temp::<MessageDraft>(key)).unwrap_or_else(|| MessageDraft::new(uid, comment));
    let current = draft.current(uid, comment);
    if !current {
        ui.label("This message changed while you were editing. Cancel edit to reload it.");
    }
    let selected = draft.selection.filter(|range| !range.is_empty());
    ui.horizontal(|ui| {
        for label in ["Bold", "Italic", "Underline"] {
            if ui.add_enabled(current && selected.is_some(), egui::Button::new(label)).clicked()
                && let Some(range) = selected
            {
                let [a, b] = range.sorted_cursors();
                let start = position(&draft.draft.body, a.index.0);
                let end = position(&draft.draft.body, b.index.0);
                // props_at follows the run before a caret at a boundary;
                // inspect the first selected character when toggling a range.
                let props = edit::props_at(&draft.draft.body, position(&draft.draft.body, a.index.0.saturating_add(1)));
                match label {
                    "Bold" => edit::format(&mut draft.draft.body, start, end, &|props_out| props_out.bold = Some(props.bold != Some(true))),
                    "Italic" => edit::format(&mut draft.draft.body, start, end, &|props_out| props_out.italic = Some(props.italic != Some(true))),
                    _ => edit::format(&mut draft.draft.body, start, end, &|props_out| {
                        props_out.underline =
                            Some(if props.underline.as_deref().is_some_and(|value| value != "none") { "none" } else { "sng" }.into())
                    }),
                }
                draft.draft.edited = true;
            }
        }
    });
    ui.label(egui::RichText::new("Select text in the message to change its formatting.").small());
    let editor_id = egui::Id::new(("comment-rich-editor", uid, id));
    let label = ui.label("Message text");
    let output = notes::editor(ui, &mut draft.draft, &notes::Look { size: 13.0, color: None }, |editor| {
        editor.id(editor_id).desired_rows(3).desired_width(f32::INFINITY)
    });
    output.response.response.labelled_by(label.id);
    if let Some(selection) = output.cursor_range {
        draft.selection = Some(selection);
    }
    let mut reset = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(current, egui::Button::new("Save changes")).clicked()
            && run_for(app, uid, "comment.edit", json!({"commentId":id,"body":draft.draft.body})).is_ok()
        {
            reset = true;
        }
        if ui.button("Cancel edit").clicked() {
            reset = true;
        }
    });
    if reset {
        ui.data_mut(|data| data.remove::<MessageDraft>(key));
    } else {
        ui.data_mut(|data| data.insert_temp(key, draft));
    }
}

fn optional_field(ui: &mut Ui, label: &str, slot: &mut Option<String>) {
    let mut value = slot.clone().unwrap_or_default();
    let label = ui.label(label);
    if ui.add(egui::TextEdit::singleline(&mut value).char_limit(comments::MAX_SCALAR)).labelled_by(label.id).changed() {
        *slot = (!value.is_empty()).then_some(value);
    }
}
fn assignees(ui: &mut Ui, task: &mut CommentTask, authors: &[AuthorChoice]) {
    ui.label(format!("Assignees ({} selected)", task.assigned_to.len()));
    let search_key = ui.id().with("assignee-search");
    let page_key = ui.id().with("assignee-page");
    let mut search = ui.data(|data| data.get_temp::<String>(search_key)).unwrap_or_default();
    let mut page = ui.data(|data| data.get_temp::<usize>(page_key)).unwrap_or(0);
    let label = ui.label("Find assignee");
    if ui.add(egui::TextEdit::singleline(&mut search).char_limit(256)).labelled_by(label.id).changed() {
        page = 0;
    }
    let query = search.to_lowercase();
    let filtered: Vec<_> = authors
        .iter()
        .filter(|choice| {
            query.is_empty() || choice.author.name.to_lowercase().contains(&query) || choice.author.identity.id.to_string().contains(&query)
        })
        .collect();
    let pages = filtered.len().div_ceil(AUTHOR_PAGE_SIZE).max(1);
    page = page.min(pages.saturating_sub(1));
    ui.horizontal(|ui| {
        if ui.add_enabled(page > 0, egui::Button::new("Previous assignees")).clicked() {
            page = page.saturating_sub(1);
        }
        ui.label(format!("Assignees {} / {}", page + 1, pages));
        if ui.add_enabled(page + 1 < pages, egui::Button::new("Next assignees")).clicked() {
            page += 1;
        }
    });
    for choice in filtered.into_iter().skip(page * AUTHOR_PAGE_SIZE).take(AUTHOR_PAGE_SIZE) {
        let author = &choice.author;
        let mut selected = task.assigned_to.contains(&author.identity.id);
        let can_select = selected
            || (task.assigned_to.len() < comments::MAX_ATTACHMENTS
                && (!choice.task_only
                    || task.authors.iter().any(|author| author.identity.id == choice.author.identity.id)
                    || task.authors.len() < comments::MAX_ATTACHMENTS));
        if ui.add_enabled(can_select, egui::Checkbox::new(&mut selected, format!("{} · #{}", author.name, author.identity.id))).changed() {
            if selected {
                task.assigned_to.push(author.identity.id);
                if choice.task_only && !task.authors.iter().any(|item| item.identity.id == author.identity.id) {
                    task.authors.push(author.clone());
                }
            } else {
                task.assigned_to.retain(|id| *id != author.identity.id);
            }
        }
    }
    if authors.is_empty() {
        ui.label("No known author identities.");
    }
    if task.assigned_to.len() >= comments::MAX_ATTACHMENTS || task.authors.len() >= comments::MAX_ATTACHMENTS {
        ui.label("Tasks support up to 64 assigned and 64 retained participants.");
    }
    ui.data_mut(|data| {
        data.insert_temp(search_key, search);
        data.insert_temp(page_key, page);
    });
}
fn task_editor(app: &mut SlideApp, ui: &mut Ui, uid: u64, id: u32, comment: &Comment, authors: &[AuthorChoice]) {
    let key = egui::Id::new(("comment-task-draft", uid, id));
    let mut draft = ui.data(|data| data.get_temp::<TaskDraft>(key)).unwrap_or_else(|| TaskDraft {
        uid,
        base: comment.task.clone(),
        task: comment.task.clone().unwrap_or_default(),
    });
    let current = draft.uid == uid && draft.base == comment.task;
    if !current {
        ui.label("This task changed while you were editing. Cancel task changes to reload it.");
    }
    optional_field(ui, "Title", &mut draft.task.title);
    optional_field(ui, "Start (ISO date/time)", &mut draft.task.start_date);
    optional_field(ui, "Due (ISO date/time)", &mut draft.task.due_date);
    optional_field(ui, "Completion (0–100%)", &mut draft.task.complete);
    assignees(ui, &mut draft.task, authors);
    let mut reset = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(current, egui::Button::new("Save task")).clicked()
            && run_for(app, uid, "comment.task.set", json!({"commentId":id,"task":draft.task})).is_ok()
        {
            reset = true;
        }
        if ui.add_enabled(current, egui::Button::new("Remove task")).clicked()
            && run_for(app, uid, "comment.task.set", json!({"commentId":id,"task":null})).is_ok()
        {
            reset = true;
        }
        if ui.button("Cancel task changes").clicked() {
            reset = true;
        }
    });
    if reset {
        ui.data_mut(|data| data.remove::<TaskDraft>(key));
    } else {
        ui.data_mut(|data| data.insert_temp(key, draft));
    }
}

fn card(app: &mut SlideApp, ui: &mut Ui, uid: u64, slide: usize, root: u32, c: &Comment, authors: &[AuthorChoice]) {
    let t = Tokens::get(ui.ctx());
    let Some(id) = c.comment_id else {
        ui.label("This imported comment needs local identity migration before editing.");
        return;
    };
    let focused = app.ui.comment_focus == Some((uid, id));
    let response = egui::Frame::NONE
        .fill(if focused { t.selected } else { t.chrome })
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(format!("Slide {} · {}", slide + 1, c.author)).strong());
            let status = match c.comment_status() {
                CommentStatus::Active => "Active",
                CommentStatus::Resolved => "Resolved",
                CommentStatus::Closed => "Closed",
            };
            ui.label(egui::RichText::new(format!("{} · {status}", c.display_date.as_deref().unwrap_or(&c.date))).small());
            if ui.selectable_label(focused, &c.text).clicked() {
                focus(app, uid, slide, c);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Go to anchor").clicked() {
                    focus(app, uid, slide, c);
                }
                for (label, command) in [("Resolve", "comment.resolve"), ("Reopen", "comment.reopen"), ("Close", "comment.close")] {
                    if ui.small_button(label).clicked() {
                        let _ = run_for(app, uid, command, json!({"commentId":root}));
                    }
                }
                if ui.small_button("Delete").clicked() {
                    let _ = run_for(app, uid, "comment.delete", json!({"commentId":id}));
                    if app.ui.comment_focus == Some((uid, id)) {
                        app.ui.comment_focus = None;
                    }
                }
            });
            ui.collapsing("Edit message", |ui| message_editor(app, ui, uid, id, c));
            let reply_key = egui::Id::new(("comment-reply-draft", uid, id));
            let mut reply = ui.data(|data| data.get_temp::<String>(reply_key)).unwrap_or_default();
            let reply_label = ui.label("Reply text");
            ui.add(
                egui::TextEdit::multiline(&mut reply).desired_rows(2).char_limit(comments::MAX_TEXT).hint_text("Reply…").desired_width(f32::INFINITY),
            )
            .labelled_by(reply_label.id);
            let closed = app
                .session
                .active()
                .and_then(|st| st.doc.slides.get(slide))
                .and_then(|slide| slide.comments.iter().find(|comment| comment.comment_id == Some(root)))
                .is_some_and(|root| root.comment_status() != CommentStatus::Active);
            if ui.add_enabled(!reply.trim().is_empty(), egui::Button::new(if closed { "Reopen and reply" } else { "Post reply" })).clicked()
                && run_for(app, uid, "comment.reply", json!({"commentId":root,"replyTo":id,"text":reply,"reopen":true})).is_ok()
            {
                reply.clear();
            }
            ui.data_mut(|data| data.insert_temp(reply_key, reply));
            ui.collapsing("Anchor and attachments", |ui| {
                if ui.button("Attach to selected object").clicked() {
                    if let Some(shape) = app.session.active().and_then(|st| st.selection.shapes.first().copied()) {
                        let _ = run_for(app, uid, "comment.attach", json!({"commentId":id,"shape":shape}));
                    } else {
                        app.set_status("Select an object on the comment's slide first.");
                    }
                }
                if ui.button("Attach to selected text range").clicked() {
                    match selected_range(app) {
                        Ok(anchor) => {
                            let _ = run_for(app, uid, "comment.attach", json!({"commentId":id,"anchor":anchor}));
                        }
                        Err(error) => app.set_status(error),
                    }
                }
                if ui.button("Detach from object").clicked() {
                    let _ = run_for(app, uid, "comment.detach", json!({"commentId":id}));
                }
                for attachment in &c.attachments {
                    ui.push_id(attachment.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(&attachment.name);
                            if ui.small_button("Save file…").clicked() {
                                download(app, uid, id, attachment.id);
                            }
                            if ui.small_button("Remove").clicked() {
                                let _ = run_for(app, uid, "comment.attachment.remove", json!({"commentId":id,"attachmentId":attachment.id}));
                            }
                        });
                    });
                }
                if ui.button("Attach file…").clicked() && app.session.active().is_some_and(|state| state.uid == uid) {
                    app.pick_and_run("commentAttachment", "comment.attachment.import", &json!({"commentId":id}));
                }
            });
            ui.collapsing("Task", |ui| task_editor(app, ui, uid, id, c, authors));
        });
    if focused {
        let key = egui::Id::new("comment-focus-scrolled");
        if ui.data(|data| data.get_temp::<(u64, u32)>(key)) != Some((uid, id)) {
            ui.scroll_to_rect(response.response.rect, Some(egui::Align::Center));
            ui.data_mut(|data| data.insert_temp(key, (uid, id)));
        }
    }
}
fn download(app: &mut SlideApp, uid: u64, id: u32, attachment: u32) {
    let Ok(value) = run_for(app, uid, "comment.attachment.get", json!({"commentId":id,"attachmentId":attachment})) else { return };
    let Some(bytes) = value["data"].as_str().and_then(deckcraft_engine::cmd::base64_decode) else { return };
    let name = value["name"].as_str().unwrap_or("attachment");
    if let Some(pick) = app.services.pick_save.as_mut() {
        if let Some(path) = pick(name)
            && let Some(write) = app.services.write.as_mut()
            && let Err(error) = write(&path, &bytes)
        {
            app.set_status(error);
        }
    } else if let Some(download) = app.services.download.as_mut() {
        download(name, &bytes);
    }
}
