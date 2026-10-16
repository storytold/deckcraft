//! Cached gallery previews; equation editing itself belongs to the session.
#[derive(Default)]
pub struct EquationHost {
    pub preview: Option<equashion_egui::EquationEditor>,
}
