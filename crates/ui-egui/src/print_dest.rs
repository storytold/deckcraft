//! File > Print: where the print-ready PDF goes. Three destinations:
//!
//! * **PDF viewer** (the original path): the PDF opens in the system's viewer, which has the printers.
//! * **A printer of this computer**: the system spooler through craft-print ([`crate::localprint`]).
//! * **Network printer**: WS-Print or IPP at an address on this network, through craft-print and
//!   craft-ui's `printing::network` section ([`crate::network_glue`]). Desktop builds with the
//!   `network-print` feature only.
//!
//! The PDF is the one File > Export makes for the Print page's settings (slides, notes pages or
//! handouts, the chosen range), so the printer gets exactly what the preview shows.

use egui::Ui;
use serde_json::Value;

use crate::SlideApp;
use crate::backstage::Backstage;

/// Where Print sends the PDF.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PrintDest {
    /// Open it in the system's PDF viewer (or download it, on the web).
    #[default]
    Viewer,
    /// A queue of this computer, by name.
    Local(String),
    /// A network printer (WS-Print or IPP).
    Network,
}

/// The number of sheets the Print page's layout makes of `slides` slides.
pub fn sheet_count(layout: &str, slides: usize) -> usize {
    let per = match layout {
        "slides" | "notes" => 1,
        l => l.strip_prefix('h').and_then(|n| n.parse::<usize>().ok()).filter(|n| matches!(n, 1 | 2 | 3 | 4 | 6 | 9)).unwrap_or(1),
    };
    slides.div_ceil(per.max(1))
}

/// The paper's name for a network printer's media size: slides print on slide-shaped pages, which
/// map to no paper name, so only notes pages and handouts name one.
#[cfg(network_print)]
fn media_paper(bs: &Backstage) -> &'static str {
    if bs.print_layout == "slides" {
        return "";
    }
    match bs.print_paper {
        "letter" => "US Letter",
        "legal" => "US Legal",
        "tabloid" | "ledger" => "Tabloid",
        "a3" => "A3",
        "a4" => "A4",
        "a5" => "A5",
        _ => "",
    }
}

/// Draw the destination list and the settings of the chosen destination. Returns whether the
/// destination can take a job now (the Print button is enabled only then).
pub fn destination(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, sheets_ok: bool) -> bool {
    let desktop = app.services.pick_save.is_some();
    let shown = match &bs.print_dest {
        PrintDest::Viewer => if desktop { "PDF viewer (choose the printer there)" } else { "PDF download" }.to_owned(),
        PrintDest::Local(name) => name.clone(),
        PrintDest::Network => "Network printer (WS-Print / IPP)".to_owned(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    app.local_print.set_ctx(ui.ctx());
    let popup = egui::ComboBox::from_id_salt("backstage_print_dest").width(240.0).selected_text(shown).show_ui(ui, |ui| {
        let viewer_label = if desktop { "PDF viewer (choose the printer there)" } else { "PDF download" };
        if ui.selectable_label(bs.print_dest == PrintDest::Viewer, viewer_label).clicked() {
            bs.print_dest = PrintDest::Viewer;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if desktop {
            // The list is read the first time the destinations are opened, off this thread.
            app.local_print.load();
            if app.local_print.loading() {
                ui.weak("Looking for printers…");
            }
            for name in app.local_print.printers.clone().unwrap_or_default() {
                if ui.selectable_label(bs.print_dest == PrintDest::Local(name.clone()), &name).clicked() {
                    bs.print_dest = PrintDest::Local(name);
                }
            }
        }
        #[cfg(network_print)]
        if ui.selectable_label(bs.print_dest == PrintDest::Network, "Network printer (WS-Print / IPP)").clicked() {
            bs.print_dest = PrintDest::Network;
        }
    });
    let _ = popup;
    ui.add_space(6.0);
    match bs.print_dest.clone() {
        PrintDest::Viewer => true,
        PrintDest::Local(_) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let local = &mut app.local_print;
                let busy = local.running();
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        let l = ui.label("Copies:");
                        ui.add(egui::DragValue::new(&mut local.settings.copies).range(1..=999)).labelled_by(l.id);
                    });
                    let sides = match local.settings.duplex {
                        None => "Printer's default",
                        Some(crate::localprint::Duplex::Off) => "One-sided",
                        Some(crate::localprint::Duplex::LongEdge) => "Two-sided, flip on long edge",
                        Some(crate::localprint::Duplex::ShortEdge) => "Two-sided, flip on short edge",
                    };
                    egui::ComboBox::from_id_salt("backstage_print_sides").width(240.0).selected_text(sides).show_ui(ui, |ui| {
                        for (value, label) in [
                            (None, "Printer's default"),
                            (Some(crate::localprint::Duplex::Off), "One-sided"),
                            (Some(crate::localprint::Duplex::LongEdge), "Two-sided, flip on long edge"),
                            (Some(crate::localprint::Duplex::ShortEdge), "Two-sided, flip on short edge"),
                        ] {
                            if ui.selectable_label(local.settings.duplex == value, label).clicked() {
                                local.settings.duplex = value;
                            }
                        }
                    });
                    ui.checkbox(&mut local.settings.grayscale, "Black and white");
                });
                if busy {
                    let text = match local.progress {
                        Some((sent, total)) => format!("Sending to the printer: sheet {sent} of {total}"),
                        None => "Sending to the printer…".to_owned(),
                    };
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text);
                    });
                }
                sheets_ok && !busy
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = sheets_ok;
                false
            }
        }
        PrintDest::Network => {
            #[cfg(network_print)]
            {
                app.net_print.show(ui, sheets_ok).block.is_none()
            }
            #[cfg(not(network_print))]
            {
                false
            }
        }
    }
}

/// Print (the Print button): build the PDF for `params` and send it to the chosen destination.
/// `slides` is how many slides the settings select (the network job's page count comes from it).
pub fn send(app: &mut SlideApp, bs: &Backstage, params: Value, slides: usize) -> bool {
    let _ = slides;
    match bs.print_dest.clone() {
        PrintDest::Viewer => app.print_pdf(params),
        PrintDest::Local(printer) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let Some((title, bytes)) = app.print_bytes(&params) else { return false };
                app.local_print.send(&printer, &title, bytes)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (printer, params, slides);
                false
            }
        }
        PrintDest::Network => {
            #[cfg(network_print)]
            {
                let Some((title, bytes)) = app.print_bytes(&params) else { return false };
                app.network_send(&title, bytes, sheet_count(bs.print_layout, slides), media_paper(bs))
            }
            #[cfg(not(network_print))]
            {
                let _ = (params, slides);
                false
            }
        }
    }
}

impl SlideApp {
    /// The print-ready PDF for the Print page's settings, and the presentation's title; the failure
    /// is shown as a notice.
    #[cfg(any(network_print, not(target_arch = "wasm32")))]
    pub(crate) fn print_bytes(&mut self, params: &Value) -> Option<(String, Vec<u8>)> {
        let (title, doc) = self.session.active().map(|d| (d.title(), d.doc.clone()))?;
        match deckcraft_engine::cmd::file::pdf_bytes(&doc, params) {
            Ok(bytes) => Some((title, bytes)),
            Err(e) => {
                self.notify(format!("Couldn't prepare the presentation for printing: {e}"));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handouts_and_slides_count_their_sheets() {
        assert_eq!(sheet_count("slides", 7), 7);
        assert_eq!(sheet_count("notes", 7), 7);
        assert_eq!(sheet_count("h3", 7), 3);
        assert_eq!(sheet_count("h9", 9), 1);
        assert_eq!(sheet_count("h6", 0), 0);
        assert_eq!(sheet_count("h5", 4), 4, "an unknown handout layout is one slide a page");
    }
}
