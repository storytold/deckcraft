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

/// How a job to a local queue or a network printer treats the page size. Full-page slides are
/// slide-shaped (13.33 x 7.5 in for 16:9), a size no paper tray lists and nobody chose: they go on the
/// printer's own paper, shrunk to fit and centred (a network printer is asked for `auto-fit`, never
/// `none`). Notes pages and handouts are laid out on the paper the person chose, which the queue must
/// have: it is demanded exactly, so a driver that lacks it is refused, not guessed at.
#[cfg(not(target_arch = "wasm32"))]
fn job_scaling(bs: &Backstage) -> crate::localprint::Scaling {
    if bs.print_layout == "slides" { crate::localprint::Scaling::ShrinkToFit } else { crate::localprint::Scaling::Exact }
}

/// The paper a job to a local queue asks for, as CUPS names it (`lp -o media=A4`): the one notes
/// pages and handouts are laid out on, so a queue whose default is another size neither clips nor
/// scales them. Slides name none (they are fitted, [`job_scaling`]). `cups`: the spooler is CUPS
/// (Linux, macOS, BSD); the Windows spooler takes the paper from the PDF's pages instead.
#[cfg(not(target_arch = "wasm32"))]
fn local_media(bs: &Backstage, cups: bool) -> Option<&'static str> {
    if !cups || bs.print_layout == "slides" {
        return None;
    }
    Some(match bs.print_paper {
        "letter" => "Letter",
        "legal" => "Legal",
        "a4" => "A4",
        "a3" => "A3",
        _ => return None,
    })
}

/// Whether the pages should come out gray: the destination's Grayscale choice (a local queue's box,
/// a network printer's Color setting). The preview and the PDF that is sent both follow it, so the
/// person sees gray exactly when gray is what prints. The PDF viewer destination has no such choice.
pub fn print_grayscale(app: &SlideApp, bs: &Backstage) -> bool {
    match &bs.print_dest {
        PrintDest::Viewer => false,
        PrintDest::Local(_) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                app.local_print.settings.grayscale
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = app;
                false
            }
        }
        PrintDest::Network => {
            #[cfg(network_print)]
            {
                app.network_grayscale()
            }
            #[cfg(not(network_print))]
            {
                let _ = app;
                false
            }
        }
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
            } else if app.local_print.printers.as_ref().is_some_and(Vec::is_empty) {
                ui.weak("No printers found");
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
                    ui.checkbox(&mut local.settings.grayscale, "Grayscale");
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
pub fn send(app: &mut SlideApp, bs: &Backstage, mut params: Value, slides: usize) -> bool {
    let _ = slides;
    // A printer may ignore a monochrome request (a driver that prints colour anyway), so the PDF
    // itself is gray when Grayscale is chosen.
    if let Value::Object(map) = &mut params {
        map.insert("grayscale".into(), Value::Bool(print_grayscale(app, bs)));
    }
    match bs.print_dest.clone() {
        PrintDest::Viewer => app.print_pdf(params),
        PrintDest::Local(printer) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let Some((title, bytes)) = app.print_bytes(&params) else { return false };
                app.local_print.send(&printer, &title, bytes, job_scaling(bs), local_media(bs, cfg!(not(windows))))
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
                app.network_send(&title, bytes, sheet_count(bs.print_layout, slides), media_paper(bs), job_scaling(bs))
            }
            #[cfg(not(network_print))]
            {
                let _ = (params, slides);
                false
            }
        }
    }
}

/// The PDF parameters of a print job: no link annotations. They are clickable, not printed, and the
/// spooler's shrink-to-fit refuses a page that has any; the links' styling is page content and stays.
/// Saving or exporting a PDF keeps its links.
#[cfg(any(network_print, not(target_arch = "wasm32")))]
pub(crate) fn print_job_params(params: &Value) -> Value {
    let mut p = params.clone();
    if let Value::Object(map) = &mut p {
        map.insert("annotations".into(), Value::Bool(false));
    }
    p
}

impl SlideApp {
    /// The print-ready PDF for the Print page's settings, and the presentation's title; the failure
    /// is shown as a notice.
    #[cfg(any(network_print, not(target_arch = "wasm32")))]
    pub(crate) fn print_bytes(&mut self, params: &Value) -> Option<(String, Vec<u8>)> {
        let (title, doc) = self.session.active().map(|d| (d.title(), d.doc.clone()))?;
        match deckcraft_engine::cmd::file::pdf_bytes(&doc, &print_job_params(params)) {
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn slides_are_fitted_and_a_chosen_paper_is_exact() {
        use crate::localprint::Scaling;
        let mut bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
        bs.print_layout = "slides";
        assert_eq!(job_scaling(&bs), Scaling::ShrinkToFit);
        for layout in ["notes", "h1", "h6"] {
            bs.print_layout = layout;
            assert_eq!(job_scaling(&bs), Scaling::Exact, "{layout} is on the paper the person chose");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_cups_job_names_the_chosen_paper_for_notes_and_handouts_only() {
        let mut bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
        bs.print_layout = "slides";
        assert_eq!(local_media(&bs, true), None, "slides are fitted to the queue's own paper");
        for (paper, media) in [("letter", "Letter"), ("a4", "A4"), ("legal", "Legal"), ("a3", "A3")] {
            bs.print_paper = paper;
            for layout in ["notes", "h1", "h6"] {
                bs.print_layout = layout;
                assert_eq!(local_media(&bs, true), Some(media), "{layout} on {paper}");
                assert_eq!(local_media(&bs, false), None, "the Windows spooler takes the paper from the PDF");
            }
        }
    }

    #[cfg(network_print)]
    #[test]
    fn a_network_job_names_no_paper_for_slides() {
        let mut bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
        bs.print_layout = "slides";
        bs.print_paper = "a4";
        assert_eq!(media_paper(&bs), "", "the slide size is never sent as media");
        assert_eq!(crate::network_glue::media_for_paper(media_paper(&bs)), None);
        bs.print_layout = "notes";
        assert_eq!(crate::network_glue::media_for_paper(media_paper(&bs)), Some("iso_a4_210x297mm"));
    }

    fn linked_app() -> SlideApp {
        use deckcraft_model::text::{Action, Hyperlink};
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        let st = app.session.doc_mut().unwrap();
        let slide = std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut st.doc).slides[0]);
        let link = |url: &str| Hyperlink {
            action: Action::Url { url: url.into() },
            tooltip: "Link".into(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        };
        let shape = slide.shapes.first_mut().expect("title shape");
        shape.text = Some(deckcraft_model::text::TextBody::from_text("A linked title"));
        shape.click = Some(link("https://example.com/shape"));
        shape.text.as_mut().unwrap().paragraphs[0].runs[0].props.link = Some(link("https://example.com/text"));
        app
    }

    fn link_markup(bytes: &[u8]) -> bool {
        [b"/Annots".as_slice(), b"/Link", b"/URI"].iter().any(|n| bytes.windows(n.len()).any(|w| w == *n))
    }

    /// Every print job (the local queue, and the network printer, which prints the same bytes) is a
    /// PDF without link annotations; exporting the same deck keeps them, and the pages are the same.
    #[cfg(any(network_print, not(target_arch = "wasm32")))]
    #[test]
    fn a_print_job_has_no_link_annotations_but_an_export_keeps_them() {
        let mut app = linked_app();
        let bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
        for layout in ["slides", "notes", "h6"] {
            let mut bs = bs.clone();
            bs.print_layout = layout;
            let params = crate::backstage::print_params(&bs, &[0]);
            let (_, printed) = app.print_bytes(&params).expect("the print PDF");
            let exported = deckcraft_engine::cmd::file::pdf_bytes(&app.session.active().unwrap().doc, &params).unwrap();
            assert!(!link_markup(&printed), "{layout}: the print PDF has no /Annots, /Link or /URI");
            assert!(link_markup(&exported), "{layout}: Save/Export PDF keeps the links");
        }
        let mut gray = crate::backstage::print_params(&bs, &[0]);
        gray["grayscale"] = true.into();
        let (_, printed) = app.print_bytes(&gray).unwrap();
        assert!(!link_markup(&printed), "the grayscale print PDF has none either");
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod linked {
        use std::sync::Mutex;

        use super::*;

        static GOT: Mutex<Option<Vec<u8>>> = Mutex::new(None);

        fn list() -> Vec<String> {
            vec!["Office".into()]
        }

        fn submit(pdf: &[u8], _: &craft_print::Job, _: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
            *GOT.lock().unwrap() = Some(pdf.to_vec());
            Ok(String::new())
        }

        #[test]
        fn the_pdf_the_local_spooler_is_handed_has_no_links() {
            let mut app = linked_app();
            app.local_print = crate::localprint::Local::with(list, submit);
            let mut bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
            bs.print_dest = PrintDest::Local("Office".into());
            let params = crate::backstage::print_params(&bs, &[0]);
            assert!(send(&mut app, &bs, params, 1));
            for _ in 0..400 {
                if app.local_print.poll().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let pdf = GOT.lock().unwrap().take().expect("the fake spooler was handed a job");
            assert!(pdf.starts_with(b"%PDF"));
            assert!(!link_markup(&pdf), "no /Annots, /Link or /URI reach the spooler");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod grayscale {
        use std::sync::Mutex;

        use super::*;
        use deckcraft_engine::Session;

        /// What the fake spooler was handed: the PDF and whether the job asked for gray.
        static GOT: Mutex<Option<(Vec<u8>, bool)>> = Mutex::new(None);

        fn list() -> Vec<String> {
            vec!["Office".into()]
        }

        fn submit(pdf: &[u8], job: &craft_print::Job, _: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
            *GOT.lock().unwrap() = Some((pdf.to_vec(), job.grayscale));
            Ok(String::new())
        }

        fn red_app() -> SlideApp {
            let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
            let st = app.session.doc_mut().unwrap();
            let slide = std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut st.doc).slides[0]);
            slide.background = Some(deckcraft_model::Background::Fill {
                fill: deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(220, 30, 40))),
            });
            app.local_print = crate::localprint::Local::with(list, submit);
            app
        }

        fn bs() -> Backstage {
            let mut bs = Backstage::new(crate::backstage::Page::Print, (1, Some(0)));
            bs.print_dest = PrintDest::Local("Office".into());
            bs
        }

        fn run(app: &mut SlideApp, bs: &Backstage) -> (Vec<u8>, bool) {
            *GOT.lock().unwrap() = None;
            let params = crate::backstage::print_params(bs, &[0]);
            assert!(send(app, bs, params, 1));
            for _ in 0..400 {
                if app.local_print.poll().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            GOT.lock().unwrap().take().expect("the fake spooler was handed a job")
        }

        #[test]
        fn grayscale_makes_a_gray_pdf_and_asks_the_driver_for_mono_too() {
            // One test: the fake spooler's capture is shared.
            let mut app = red_app();
            let bs = bs();
            let doc = app.session.active().unwrap().doc.clone();
            let params = |gray: bool| {
                let mut p = crate::backstage::print_params(&bs, &[0]);
                p["grayscale"] = gray.into();
                p
            };
            let (colour_expected, gray_expected) = (
                deckcraft_engine::cmd::file::pdf_bytes(&doc, &params(false)).unwrap(),
                deckcraft_engine::cmd::file::pdf_bytes(&doc, &params(true)).unwrap(),
            );
            assert_ne!(colour_expected, gray_expected, "the grayscale option changes the PDF");

            assert!(!print_grayscale(&app, &bs));
            let (pdf, mono) = run(&mut app, &bs);
            assert!(!mono);
            assert_eq!(pdf, colour_expected, "colour is the default");

            app.local_print.settings.grayscale = true;
            assert!(print_grayscale(&app, &bs));
            let (pdf, mono) = run(&mut app, &bs);
            assert!(mono, "the queue is asked for monochrome as well (Job::grayscale, which craft-print sends as the colour hint)");
            assert_eq!(pdf, gray_expected, "and the PDF handed over is gray itself, for a driver that ignores the hint");

            let mut viewer = bs.clone();
            viewer.print_dest = PrintDest::Viewer;
            assert!(!print_grayscale(&app, &viewer), "the PDF viewer destination has no grayscale choice");
        }
    }
}
