//! What Craft AI does inside DeckCraft. Craft AI is one assistant across the ArtCraft family
//! — the model, the runtime and the shared folder belong to the family — but its *work* is the
//! app's own: the system prompt tells the model which app it is inside and what that app does,
//! and every ready-made task is framed for that app's world, so the answer that comes back is
//! about slides, layouts and speaker notes, not generic advice.
//!
//! Each app of the family carries its own `tasks.rs` — WordCraft's asks about documents and
//! styles, SoundCraft's about tracks and buses, and so on — same model, same shared folder,
//! different craft. The labels and hints are catalog keys: the pane shows them translated.

/// The app Craft AI is embedded in, as the shared folder's records name it.
pub const APP: &str = "DeckCraft";

/// One ready-made task of the Assistant tab.
pub struct Task {
    /// The label the picker shows (a catalog key).
    pub label: &'static str,
    /// The input field's hint (a catalog key).
    pub hint: &'static str,
}

/// The tasks, in the picker's order.
pub const TASKS: &[Task] = &[
    Task { label: "Ask anything", hint: "Ask in any language" },
    Task { label: "Tighten slide text", hint: "Paste the slide's text, or its bullets" },
    Task { label: "Speaker notes", hint: "Which slide, and what to say aloud" },
    Task { label: "Outline into slides", hint: "Paste the outline or the notes you have" },
    Task { label: "Review the flow", hint: "The slide titles and key lines, in order" },
    Task { label: "Title suggestions", hint: "What the slide shows, for titles" },
];

/// Tell the model where it is and how to answer: briefly, practically, in the language the user
/// wrote in, about this app's own world — the words of its interface, the files it opens and
/// writes. The last line is the honesty clause: a local model can be wrong, and it is told to
/// say so rather than invent.
pub fn system_prompt() -> String {
    "You are Craft AI, the assistant built into DeckCraft, a desktop presentation app. \
     DeckCraft's world, in its own words: slides, layouts and slide masters; placeholders and \
     text boxes; themes, backgrounds and the Design Ideas pane; speaker notes; pictures, \
     shapes, SmartArt, charts and tables on slides; animations, transitions and the Animation \
     Pane; the ribbon's Home, Insert, Draw, Design, Transitions, Animations, Slide Show and \
     Review tabs; the slide sorter, outline view, reading view and Presenter View; the \
     Selection Pane and Comments; and .pptx and .pdf export. Answer in the language the user \
     wrote in, Arabic included. Be brief and practical: name the ribbon tab and the command, \
     give the steps as a short list, say what to watch out for, and when a question is not \
     about DeckCraft or presentations, answer it anyway, briefly. Never invent menu entries or \
     pane names: if you are not sure DeckCraft has a feature, say so plainly."
        .to_string()
}

/// Frame the user's input for the task they picked.
pub fn user_prompt(task: usize, input: &str) -> String {
    let input = input.trim();
    match TASKS[task.min(TASKS.len() - 1)].label {
        "Tighten slide text" => format!(
            "Tighten this slide's text for a presenter — shorter lines, kept meaning, no more \
             bullets than it has: {input}"
        ),
        "Speaker notes" => format!("Write speaker notes for this slide — what to say aloud, a few plain sentences: {input}"),
        "Outline into slides" => format!(
            "Turn this outline into a slide plan for DeckCraft — slide titles and the bullets each carries, one line per slide: {input}"
        ),
        "Review the flow" => format!("Review the flow of this presentation — where the story drags, repeats or jumps — as a short list: {input}"),
        "Title suggestions" => format!("Suggest short, fitting slide titles for this — a handful of options, no explanations: {input}"),
        _ => input.to_string(),
    }
}
