//! The equation editor widget in a native eframe window, with live linear and OMML readouts.
//!
//! `cargo run -p equashion-egui --example widget` (add `-- --light` for the light theme).
//! `EQUASHION_SCREENSHOT=<path.png>` saves one frame and exits (for headless checks under xvfb).

use equashion_egui::EquationEditor;

struct Demo {
    editor: EquationEditor,
    last: String,
    screenshot: Option<std::path::PathBuf>,
    frames: u32,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("equaSHion");
            let r = self.editor.show(ui);
            if r.committed {
                self.last = format!("committed: {}", self.editor.linear());
            }
            if r.cancelled {
                self.editor.revert();
                self.last = "cancelled (reverted)".into();
            }
            ui.separator();
            ui.label(egui::RichText::new("Linear").strong());
            let mut lin = self.editor.linear();
            ui.add(egui::TextEdit::singleline(&mut lin).interactive(false).desired_width(f32::INFINITY).font(egui::FontId::monospace(13.0)));
            ui.label(egui::RichText::new("OMML").strong());
            let mut omml = self.editor.omml();
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut omml).interactive(false).desired_width(f32::INFINITY).font(egui::FontId::monospace(12.0)));
            });
            if !self.last.is_empty() {
                ui.label(&self.last);
            }
        });
        if let Some(path) = &self.screenshot {
            self.frames += 1;
            if self.frames == 5 {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ui
                .ctx()
                .input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
            if let Some(img) = shot {
                let ok = write_ppm(path, &img);
                eprintln!("widget: screenshot {} -> {}", if ok { "saved" } else { "FAILED" }, path.display());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }
}

/// A binary PPM (no image crate here); `.png` paths get `.ppm` appended.
fn write_ppm(path: &std::path::Path, img: &egui::ColorImage) -> bool {
    let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
    for p in &img.pixels {
        out.extend_from_slice(&[p.r(), p.g(), p.b()]);
    }
    std::fs::write(path, out).is_ok()
}

fn main() -> eframe::Result {
    let light = std::env::args().any(|a| a == "--light");
    let screenshot = std::env::var_os("EQUASHION_SCREENSHOT").map(std::path::PathBuf::from);
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 720.0]), ..Default::default() };
    eframe::run_native(
        "equaSHion widget",
        options,
        Box::new(move |cc| {
            // The host, not the widget, decides the theme.
            cc.egui_ctx.set_theme(if light { egui::Theme::Light } else { egui::Theme::Dark });
            Ok(Box::new(Demo {
                editor: EquationEditor::with_linear(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}"),
                last: String::new(),
                screenshot,
                frames: 0,
            }))
        }),
    )
}
