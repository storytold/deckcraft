//! Native, offscreen file-provider Recent preview; no desktop window or network service.
//!
//! cargo run -p deckcraft-ui-egui --example file_providers_preview -- --provider-preview --out target/provider-recents.png
//! Without --provider-preview no provider is registered. The example writes its own tiny native
//! local presentation beside the PNG; names and account identity are synthetic preview data.

use std::io::Write;
use std::path::PathBuf;

use deckcraft_engine::Session;
use deckcraft_ui_egui::backstage::Page;
use deckcraft_ui_egui::file_providers::{Host, Resource};
use deckcraft_ui_egui::{Services, SlideApp};

fn render() -> Result<(), String> {
    let mut provider = false;
    let mut output = PathBuf::from("target/provider-recents.png");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--provider-preview" => provider = true,
            "--out" => output = args.next().map(PathBuf::from).ok_or("--out needs a path")?,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let local = output.with_file_name("provider-preview-local.deckcraft");
    let session = Session::with_new();
    let document = session.active().ok_or("preview presentation was not created")?;
    let bytes = deckcraft_engine::cmd::file::save_bytes(&document.doc, "deckcraft").map_err(|error| error.to_string())?;
    match std::fs::OpenOptions::new().create_new(true).write(true).open(&local) {
        Ok(mut file) => file.write_all(&bytes).map_err(|error| error.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    let host = Host::default();
    let file_providers = if provider {
        host.set_session("preview0123456789abcdef".into())?;
        let connection = host.connect("preview".into(), "Preview storage".into(), "preview-account".into())?;
        host.set_resources(
            &connection,
            vec![Resource { id: "provider-presentation".into(), name: "Provider presentation.deckcraft".into(), updated_at: String::new() }],
        )?;
        Some(host)
    } else {
        None
    };
    let mut app = SlideApp::new(session, Services { file_providers, ..Default::default() });
    app.ui.recent = vec![local.to_string_lossy().into_owned()];
    app.open_backstage(Page::Open);
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_pixels_per_point(1.0).wgpu().build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(6);
    harness.render().map_err(|error| error.to_string())?.save(&output).map_err(|error| error.to_string())?;
    println!("wrote {} (provider preview: {provider})", output.display());
    Ok(())
}

fn main() {
    if let Err(error) = render() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
