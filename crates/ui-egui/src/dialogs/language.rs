use super::*;

/// The selection's proofing language and whether it's marked Do Not Check Spelling: the run at
/// the caret, else the first run of the first selected shape; US English when none says.
pub fn selected_language(app: &SlideApp) -> (String, bool) {
    let Some(st) = app.session.active() else { return ("en-US".into(), false) };
    let run = |sh: &deckcraft_model::Shape, at: Option<(usize, usize)>| -> Option<deckcraft_model::RunProps> {
        let t = sh.text.as_ref()?;
        if let Some((p, c)) = at {
            let para = t.paragraphs.get(p)?;
            let mut n = 0;
            for r in &para.runs {
                n += r.char_len();
                if c <= n {
                    return Some(r.props.clone());
                }
            }
            return para.runs.last().map(|r| r.props.clone());
        }
        t.paragraphs.iter().flat_map(|p| &p.runs).next().map(|r| r.props.clone())
    };
    let props = match &st.selection.text {
        Some(t) if !t.notes && t.cell.is_none() => st.shape(t.shape).and_then(|sh| run(sh, Some(t.caret))),
        _ => st.selection.shapes.first().and_then(|id| st.shape(*id)).and_then(|sh| run(sh, None)),
    };
    let props = props.unwrap_or_default();
    (props.lang.unwrap_or_else(|| "en-US".into()), props.no_proof == Some(true))
}
