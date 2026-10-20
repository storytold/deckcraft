//! Headless DeckCraft. `deckcraft-cli help` explains each subcommand (`help.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod automation_args;

mod help;

use std::process::ExitCode;

use deckcraft_engine::Session;
use serde_json::{Value, json};

macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            eprintln!("deckcraft-cli: can't write to stdout: {e}");
            std::process::exit(1);
        }
    }};
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let asks_help = rest.iter().any(|a| a == "--help" || a == "-h");
    if let Some(page) = args.first().filter(|_| asks_help).and_then(|n| help::page(n)) {
        outln!("{page}");
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => match rest.first() {
            None => {
                outln!("{}", help::overview());
                ExitCode::SUCCESS
            }
            Some(name) => match help::page(name) {
                Some(page) => {
                    outln!("{page}");
                    ExitCode::SUCCESS
                }
                None => unknown(name),
            },
        },
        Some("--version" | "-V" | "version") => {
            outln!("deckcraft-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("info") => report(info(rest)),
        Some("bench") => report(bench(rest)),
        Some("render") => report(render(rest)),
        Some("render-transition") => report(render_transition(rest)),
        Some("convert") => report(convert(rest)),
        Some("run") => report(run(rest)),
        Some("commands") => {
            let f = rest.first().map(|x| x.to_lowercase());
            for c in deckcraft_engine::command_specs()
                .iter()
                .filter(|c| f.as_ref().is_none_or(|f| c.id.to_lowercase().contains(f.as_str()) || c.label.to_lowercase().contains(f.as_str())))
            {
                match c.params.strip_prefix("{}") {
                    Some(answer) => outln!("{} — {}{answer}", c.id, c.label),
                    None => outln!("{} — {}: {}", c.id, c.label, c.params),
                }
            }
            ExitCode::SUCCESS
        }
        Some("describe") => report(describe(rest.first().map(String::as_str))),
        Some("app") => report(app(rest)),
        Some("mcp") => report(mcp(rest)),
        Some("links") => {
            use deckcraft_engine::links::*;
            outln!("Discord   {DISCORD}\nWebsite   {WEBSITE}\nApp page  {APP_PAGE}\nGitHub    {GITHUB}");
            ExitCode::SUCCESS
        }
        Some(other) => unknown(other),
        None => usage(),
    }
}

fn is_help(arg: &str) -> bool {
    matches!(arg, "--help" | "-h")
}

/// The help page of subcommand `name`, else the overview.
fn help_page(name: &str) -> String {
    help::page(name).unwrap_or_else(help::overview)
}

fn unknown(sub: &str) -> ExitCode {
    eprintln!("deckcraft-cli: unknown subcommand `{sub}`\n");
    usage()
}

fn usage() -> ExitCode {
    eprintln!("{}", help::overview());
    eprintln!("\nCommunity: {}  ·  {}", deckcraft_engine::links::DISCORD, deckcraft_engine::links::APP_PAGE);
    ExitCode::FAILURE
}

fn report(r: Result<(), String>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("deckcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn open(s: &mut Session, input: Option<&str>, sample: bool) -> Result<(), String> {
    if sample {
        return deckcraft_engine::sample::open_sample(s).map_err(|e| e.to_string());
    }
    match input {
        Some(p) => {
            let opened = s.execute("file.open", &json!({"path": p})).map_err(|e| e.to_string())?;
            warn_if_damaged(p, &opened);
            Ok(())
        }
        None => s.execute("file.new", &json!({})).map(|_| ()).map_err(|e| e.to_string()),
    }
}

/// Say on stderr what a file lost on opening, so a saved copy doesn't silently miss it (#203).
fn warn_if_damaged(path: &str, opened: &serde_json::Value) {
    let count = |key: &str| opened.get(key).and_then(|v| v.as_array()).map_or(0, |a| a.len());
    let (slides, media) = (count("lostSlides"), count("damagedMedia"));
    if slides > 0 {
        eprintln!("deckcraft-cli: warning: {path}: {slides} slide{} couldn't be read and left out", if slides == 1 { "" } else { "s" });
    }
    if media > 0 {
        eprintln!("deckcraft-cli: warning: {path}: {media} picture or media file{} couldn't be read", if media == 1 { "" } else { "s" });
    }
}

/// Median and slowest of `runs` timings of `f`, in milliseconds.
fn time_ms(runs: usize, mut f: impl FnMut() -> Result<(), String>) -> Result<(f64, f64), String> {
    let mut ms = Vec::with_capacity(runs);
    for _ in 0..runs.max(1) {
        let t = std::time::Instant::now();
        f()?;
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.sort_by(f64::total_cmp);
    let median = ms.get(ms.len() / 2).copied().unwrap_or(0.0);
    Ok((median, ms.last().copied().unwrap_or(0.0)))
}

/// Peak resident memory of this process in MiB (Linux `VmHWM`); `None` elsewhere.
fn peak_rss_mib() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = s.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024.0)
}

/// The performance baseline (docs/gaps.md "No baseline"): how long a deck takes to open, to
/// render each slide, to save and to export, and the peak memory, as JSON or a table.
fn bench(args: &[String]) -> Result<(), String> {
    use deckcraft_engine::cmd::file::{open_presentation, save_bytes};
    let mut input = None;
    let mut sample = false;
    let mut runs = 5usize;
    let mut as_json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sample" => sample = true,
            "--json" => as_json = true,
            "--runs" => {
                runs = it.next().and_then(|v| v.parse().ok()).filter(|n| (1..=1000).contains(n)).ok_or("--runs needs a number from 1 to 1000")?
            }
            f if !f.starts_with("--") && input.is_none() => input = Some(f.to_string()),
            o => return Err(format!("bench: unknown argument `{o}`")),
        }
    }
    if input.is_none() && !sample {
        return Err("bench needs a FILE or --sample".into());
    }
    let mut s = Session::new();
    let (name, bytes) = match &input {
        Some(p) => (p.clone(), std::fs::read(p).map_err(|e| format!("{p}: {e}"))?),
        None => {
            open(&mut s, None, true)?;
            let doc = &s.doc().map_err(|e| e.to_string())?.doc;
            ("sample.deckcraft".to_string(), save_bytes(doc, "deckcraft").map_err(|e| e.to_string())?)
        }
    };
    let e = |x: deckcraft_engine::EngineError| x.to_string();
    let doc = open_presentation(&name, &bytes).map_err(e)?;
    let slides = doc.slides.len();
    let open = time_ms(runs, || open_presentation(&name, &bytes).map(|_| ()).map_err(e))?;
    let opts = deckcraft_engine::render::RenderOpts { scale: 1.0, ..Default::default() };
    let mut per_slide = vec![];
    for i in 0..slides {
        per_slide.push(time_ms(runs, || {
            deckcraft_engine::render::render_slide(&doc, i, &opts);
            Ok(())
        })?);
    }
    let pptx = save_bytes(&doc, "pptx").map_err(e)?;
    let mut steps = vec![
        ("open", open),
        ("save .deckcraft", time_ms(runs, || save_bytes(&doc, "deckcraft").map(|_| ()).map_err(e))?),
        ("save .pptx", time_ms(runs, || save_bytes(&doc, "pptx").map(|_| ()).map_err(e))?),
        ("open .pptx", time_ms(runs, || open_presentation("bench.pptx", &pptx).map(|_| ()).map_err(e))?),
        ("export .pdf", time_ms(runs.min(3), || save_bytes(&doc, "pdf").map(|_| ()).map_err(e))?),
    ];
    let render_total: f64 = per_slide.iter().map(|m| m.0).sum();
    let render_max = per_slide.iter().map(|m| m.1).fold(0.0, f64::max);
    steps.insert(1, ("render all slides (1×)", (render_total, render_max)));
    let peak = peak_rss_mib();
    if as_json {
        let v = json!({
            "file": name,
            "bytes": bytes.len(),
            "slides": slides,
            "runs": runs,
            "steps": steps.iter().map(|(k, (m, x))| json!({"step": k, "medianMs": m, "maxMs": x})).collect::<Vec<_>>(),
            "renderMsPerSlide": per_slide.iter().map(|m| m.0).collect::<Vec<_>>(),
            "peakRssMiB": peak,
        });
        outln!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        outln!("{name}: {slides} slides, {} KiB, median of {runs} runs", bytes.len() / 1024);
        for (k, (m, x)) in &steps {
            outln!("  {k:<24} {m:>9.1} ms   (slowest {x:.1} ms)");
        }
        if let Some((i, m)) = per_slide.iter().enumerate().max_by(|a, b| a.1.0.total_cmp(&b.1.0)) {
            outln!("  slowest slide            {:>9.1} ms   (slide {})", m.0, i + 1);
        }
        if let Some(p) = peak {
            outln!("  peak memory              {p:>9.1} MiB");
        }
    }
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    if args.first().is_some_and(|a| is_help(a)) {
        outln!("{}", help_page("info"));
        return Ok(());
    }
    let path = args.first().ok_or("info needs a FILE")?;
    let mut s = Session::new();
    open(&mut s, Some(path), false)?;
    let v = s.execute("document.inspect", &json!({})).map_err(|e| e.to_string())?;
    outln!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

fn render(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut sample = false;
    let mut slide: Option<usize> = None;
    let mut all = false;
    let mut scale = 2.0;
    let mut edit = false;
    let mut transparent = false;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sample" => sample = true,
            "--transparent" => transparent = true,
            "--slide" => slide = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--slide needs a number")?),
            "--all" => all = true,
            "--edit" => edit = true,
            "--scale" => scale = it.next().and_then(|v| v.parse().ok()).ok_or("--scale needs a number")?,
            "--help" | "-h" => {
                outln!("{}", help_page("render"));
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other
                if input.is_none()
                    && !sample
                    && !other.starts_with("--")
                    && out.is_none()
                    && !(other.ends_with(".png") || other.ends_with(".jpg"))
                    || (input.is_none() && !sample && std::path::Path::new(other).is_file()) =>
            {
                input = Some(other.to_string())
            }
            other => out = Some(other.to_string()),
        }
    }
    let out = out.ok_or("render needs an output path")?;
    let mut s = Session::new();
    open(&mut s, input.as_deref(), sample)?;
    let st = s.doc().map_err(|e| e.to_string())?;
    let n = st.doc.slides.len();
    let list = slide_indices(n, all, slide)?;
    if all {
        std::fs::create_dir_all(&out).map_err(|e| format!("{out}: {e}"))?;
    }
    for i in list {
        let (png, w, h) = deckcraft_engine::cmd::file::render_png_with(&st.doc, i, scale, edit, transparent);
        let path = if all { format!("{out}/slide-{:02}.png", i + 1) } else { out.clone() };
        std::fs::write(&path, png).map_err(|e| format!("{path}: {e}"))?;
        outln!("{path} ({w}×{h})");
    }
    Ok(())
}

/// Slides `--slide` asks for. `N` is a 0-based index, the same one `slide.go` uses.
/// `--all` still names files from 1 (`slide-01.png`), which is PowerPoint's slide number.
fn slide_indices(n: usize, all: bool, slide: Option<usize>) -> Result<Vec<usize>, String> {
    if all {
        return Ok((0..n).collect());
    }
    let i = slide.unwrap_or(0);
    if i >= n {
        return Err(missing_slide(i, n));
    }
    Ok(vec![i])
}

fn missing_slide(i: usize, n: usize) -> String {
    match n {
        0 => format!("no slide index {i} (this deck has no slides)"),
        1 => format!("no slide index {i} (this deck has 1 slide, index 0, which --all names slide-01.png)"),
        n => {
            let last = n - 1;
            format!(
                "no slide index {i} (this deck has {n} slides, numbered 0 through {last}; --all names them slide-01.png through slide-{n:02}.png)"
            )
        }
    }
}

/// Frames of slide N's transition (0-based N; from slide N-1, or black for the first slide) at
/// the given progress values. With several values, OUT gets `-t<value>` before its extension.
fn render_transition(args: &[String]) -> Result<(), String> {
    use deckcraft_engine::render;
    let mut input = None;
    let mut slide: Option<usize> = None;
    let mut ts: Vec<f64> = Vec::new();
    let mut width: Option<f64> = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--slide" => slide = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--slide needs a number")?),
            "--t" => {
                let v = it.next().ok_or("--t needs a value")?;
                for x in v.split(',') {
                    let t: f64 = x.trim().parse().map_err(|_| format!("bad --t value `{x}`"))?;
                    if !t.is_finite() {
                        return Err(format!("bad --t value `{x}`"));
                    }
                    ts.push(t);
                }
            }
            "--width" => {
                width = Some(it.next().and_then(|v| v.parse().ok()).filter(|w: &f64| w.is_finite() && *w >= 1.0).ok_or("--width needs pixels")?)
            }
            other if input.is_none() => input = Some(other.to_string()),
            other => out = Some(other.to_string()),
        }
    }
    let input = input.ok_or("render-transition needs a FILE")?;
    let out = out.ok_or("render-transition needs an output path")?;
    let i = slide.ok_or("render-transition needs --slide N")?;
    if ts.is_empty() {
        return Err("render-transition needs --t".into());
    }
    let paths = frame_paths(&out, &ts)?;
    let mut s = Session::new();
    open(&mut s, Some(&input), false)?;
    let st = s.doc().map_err(|e| e.to_string())?;
    let doc = &st.doc;
    let n = doc.slides.len();
    let Some(sl) = doc.slides.get(i) else { return Err(format!("no slide {i} (the deck has {n})")) };
    let sw = doc.slide_size.width.max(1.0);
    let w = width.unwrap_or(sw * 2.0).min(render::MAX_SIDE as f64);
    let opts = render::RenderOpts { scale: w / sw, ..Default::default() };
    let tr = sl.transition.clone().unwrap_or_default();
    // Render both slides once and composite every frame from them (morph re-renders per frame).
    let prev = i.checked_sub(1);
    let cached = (tr.kind != "morph").then(|| {
        let parts = render::uses_slide_parts(&tr.kind, &tr.option).then(|| render::SlideParts::render(doc, prev, i, &opts));
        (prev.map(|p| render::render_slide(doc, p, &opts)), render::render_slide(doc, i, &opts), parts)
    });
    let aspect = sw / doc.slide_size.height.max(1.0);
    for (t, path) in ts.iter().zip(&paths) {
        let img = match &cached {
            Some((old, new, parts)) => {
                render::composite_transition(old.as_ref(), new, parts.as_ref(), &render::transition_frame(&tr.kind, &tr.option, *t, aspect))
            }
            None => render::render_transition(doc, i, *t, &opts),
        };
        std::fs::write(path, img.to_png_at(render::dpi_for_scale(opts.scale))).map_err(|e| format!("{path}: {e}"))?;
        outln!("{path} ({}×{}) {} {} t={t}", img.width, img.height, tr.kind, tr.option);
    }
    Ok(())
}

/// Output path of each frame. A single frame goes to OUT; several get `-t<value>` (three decimals)
/// before the extension. Values that round to the same name would overwrite each other, so that is an error.
fn frame_paths(out: &str, ts: &[f64]) -> Result<Vec<String>, String> {
    if ts.len() == 1 {
        return Ok(vec![out.to_string()]);
    }
    let p = std::path::Path::new(out);
    let stem = p.file_stem().and_then(|x| x.to_str()).unwrap_or("frame");
    let mut paths: Vec<String> = Vec::with_capacity(ts.len());
    for t in ts {
        let path = p.with_file_name(format!("{stem}-t{t:.3}.png")).to_string_lossy().into_owned();
        if paths.contains(&path) {
            return Err(format!("--t values round to the same three decimals: more than one frame would be written to {path}"));
        }
        paths.push(path);
    }
    Ok(paths)
}

fn convert(args: &[String]) -> Result<(), String> {
    if args.first().is_some_and(|a| is_help(a)) {
        outln!("{}", help_page("convert"));
        return Ok(());
    }
    let (Some(input), Some(out)) = (args.first(), args.get(1)) else { return Err("convert needs IN and OUT".into()) };
    let fmt = output_format(out)?;
    let mut s = Session::new();
    open(&mut s, Some(input), false)?;
    let r = if matches!(fmt, "png" | "jpeg") {
        s.execute("file.export", &json!({"path": out, "slide": 0}))
    } else if fmt == "gif" {
        s.execute("file.export", &json!({"path": out}))
    } else {
        s.execute("file.saveAs", &json!({"path": out}))
    };
    r.map_err(|e| e.to_string())?;
    outln!("{out}");
    Ok(())
}

/// The format `convert`, `run --save` and `run --export` write to `path`, or an error naming the
/// supported extensions when `path`'s extension isn't one (rather than native bytes under that name).
fn output_format(path: &str) -> Result<&'static str, String> {
    use deckcraft_engine::cmd::file::{WRITABLE_EXTENSIONS, writable_format_for_path};
    writable_format_for_path(path).ok_or_else(|| {
        let ext = std::path::Path::new(path).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| "(none)".into());
        format!("{path}: unsupported output format {ext} (supported: .{})", WRITABLE_EXTENSIONS.join(", ."))
    })
}

fn run(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut sample = false;
    let mut cmds = vec![];
    let mut save = None;
    let mut export = None;
    let mut export_params = json!({});
    let mut print = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--in" => input = Some(it.next().cloned().ok_or("--in needs a file")?),
            "--sample" => sample = true,
            "--cmd" => cmds.push(it.next().cloned().ok_or("--cmd needs COMMAND[=PARAMS]")?),
            "--save" => save = Some(it.next().cloned().ok_or("--save needs a path")?),
            "--export" => export = Some(it.next().cloned().ok_or("--export needs a path")?),
            "--export-params" => {
                let j = it.next().ok_or("--export-params needs a JSON object")?;
                export_params = serde_json::from_str::<Value>(j).map_err(|e| format!("--export-params: bad JSON: {e}"))?;
                if !export_params.is_object() {
                    return Err("--export-params must be a JSON object".into());
                }
            }
            "--print" => print = true,
            "--help" | "-h" => {
                outln!("{}", help_page("run"));
                return Ok(());
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    for p in save.iter().chain(&export) {
        output_format(p)?;
    }
    let mut s = Session::new();
    open(&mut s, input.as_deref(), sample)?;
    for c in cmds {
        let (id, p) = match c.split_once('=') {
            Some((id, j)) => (id.to_string(), serde_json::from_str::<Value>(j).map_err(|e| format!("{id}: bad JSON: {e}"))?),
            None => (c.clone(), json!({})),
        };
        let r = s.execute(&id, &p).map_err(|e| e.to_string())?;
        if print {
            outln!("{id} → {r}");
        }
    }
    if let Some(p) = save {
        s.execute("file.saveAs", &json!({"path": p})).map_err(|e| e.to_string())?;
    }
    if let Some(p) = export {
        export_params["path"] = json!(p);
        s.execute("file.export", &export_params).map_err(|e| e.to_string())?;
    } else if export_params != json!({}) {
        return Err("--export-params needs --export OUT".into());
    }
    Ok(())
}

fn describe(id: Option<&str>) -> Result<(), String> {
    if id.is_some_and(is_help) {
        outln!("{}", help_page("describe"));
        return Ok(());
    }
    let id = id.ok_or("describe needs a command id")?;
    let c = deckcraft_engine::find_command(id).ok_or_else(|| deckcraft_engine::EngineError::UnknownCommand(id.into()).to_string())?;
    let mut head = format!("{}: {}", c.id, c.label);
    if !c.menu.is_empty() {
        head.push_str(&format!(", in {}", c.menu.join(" ▸ ")));
    }
    if let Some(k) = c.shortcut {
        head.push_str(&format!(", shortcut {k}"));
    }
    outln!("{head}.\n");
    let (args, answer) = c.params.split_once('→').unwrap_or((c.params, ""));
    let parsed = deckcraft_engine::cmd::parse_params(c.params);
    match &parsed {
        Err(_) => outln!("Parameters: {}", args.trim()),
        Ok(params) if params.is_empty() => outln!("Takes no parameters."),
        Ok(params) => print_params(params),
    }
    let params = parsed.unwrap_or_default();
    if let Some(acts) = c.acts_on() {
        outln!("{acts}");
    }
    let remark = remark(args);
    if !remark.is_empty() {
        outln!("Note: {remark}");
    }
    if !answer.trim().is_empty() {
        outln!("Answers with {}.", answer.trim());
    }
    let bare = params.is_empty() || params.iter().any(|p| params.iter().filter(|q| q.object == p.object).all(|q| q.optional));
    let call = if bare { c.id.to_string() } else { format!("'{}={{…}}'", c.id) };
    outln!("\nRun it with: deckcraft-cli run --cmd {call}");
    Ok(())
}

fn print_params(params: &[deckcraft_engine::cmd::Param]) {
    let name = |p: &deckcraft_engine::cmd::Param| {
        let or = if p.or { "or " } else { "" };
        let optional = if p.optional { ", optional" } else { "" };
        format!("{or}{}{optional}", p.name)
    };
    let width = params.iter().map(|p| name(p).chars().count()).max().unwrap_or_default();
    let sets = params.iter().map(|p| p.object).max().unwrap_or_default() > 0;
    outln!("Parameters:");
    for (k, p) in params.iter().enumerate() {
        if sets && params.get(k.wrapping_sub(1)).is_none_or(|before| before.object != p.object) {
            outln!("  {}", if p.object == 0 { "either:" } else { "or:" });
        }
        let indent = if sets { "    " } else { "  " };
        outln!("{}", format!("{indent}{:<width$}  {}", name(p), p.about).trim_end());
    }
}

fn remark(args: &str) -> &str {
    let after = args.rfind('}').and_then(|i| args.get(i + 1..)).unwrap_or_default().trim().trim_start_matches('—').trim();
    after.strip_prefix('(').and_then(|n| n.strip_suffix(')')).unwrap_or(after)
}

fn app(args: &[String]) -> Result<(), String> {
    use deckcraft_mcp::{Backend, Remote, control_addr};
    let mut port = deckcraft_mcp::DEFAULT_PORT.to_string();
    let mut method: Option<String> = None;
    let mut rest = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => port = it.next().cloned().ok_or("--port needs a value")?,
            "--method" => method = Some(it.next().cloned().ok_or("--method needs a name")?),
            "--help" | "-h" => {
                outln!("{}", help_page("app"));
                return Ok(());
            }
            other => rest.push(other.to_string()),
        }
    }
    let mut r = Remote::connect(&control_addr(&port))
        .map_err(|e| format!("no DeckCraft app on port {port} ({e}); start it with `deckcraft --control {port}`"))?;
    let parse = |s: Option<&String>| -> Result<Value, String> {
        s.map(|j| serde_json::from_str(j).map_err(|e| format!("bad JSON: {e}"))).transpose().map(|v| v.unwrap_or(json!({})))
    };
    let v = match method {
        Some(m) => r.call(&m, parse(rest.first())?)?,
        None => {
            let id = rest.first().ok_or("app needs a COMMAND or --method")?;
            r.call("engine.execute", json!({"command": id, "params": parse(rest.get(1))?}))?
        }
    };
    outln!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

fn mcp(args: &[String]) -> Result<(), String> {
    use deckcraft_engine::file_access::{self, AutomationRoots};
    use deckcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let mut connect: Option<String> = None;
    let mut sample = false;
    let mut roots_args = automation_args::AutomationArgs::new(None, None);
    let mut it = args.iter().cloned();
    while let Some(a) = it.next() {
        if roots_args.consume(&a, &mut it)? {
            continue;
        }
        match a.as_str() {
            "--connect" => connect = Some(it.next().ok_or("--connect needs a port or host:port")?),
            "--sample" => sample = true,
            "--help" | "-h" => {
                outln!("{}", help_page("mcp"));
                return Ok(());
            }
            other => return Err(format!("unknown mcp option `{other}`")),
        }
    }
    let roots = AutomationRoots::new(roots_args.read_root.as_deref(), roots_args.write_root.as_deref())?;
    if roots.is_some() && connect.is_some() {
        return Err("--automation-read-root and --automation-write-root confine the in-process server; a running app is not confined by them, \
so drop --connect (or use --connect without the roots)"
            .into());
    }
    let backend: Box<dyn Backend> = match connect {
        Some(c) => Box::new(Remote::connect(&control_addr(&c)).map_err(|e| format!("can't reach the app at {c}: {e}"))?),
        None => {
            let mut h = Headless::new();
            // `--sample` is the operator's own choice, made before the roots come into force.
            if sample {
                deckcraft_engine::sample::open_sample(&mut h.session).map_err(|e| e.to_string())?;
            }
            if let Some(roots) = roots {
                // Every thread of this process, not only the server's: nothing reaches past the roots.
                file_access::confine_process(roots.clone());
                let show = |root: Option<&std::path::Path>| root.map_or_else(|| "none".to_string(), |r| r.display().to_string());
                eprintln!("deckcraft-cli: files confined: read root {}, write root {}", show(roots.read_root()), show(roots.write_root()));
                h = h.with_automation_roots(Some(roots));
            }
            Box::new(h)
        }
    };
    let mut server = Server::new(backend);
    let stdin = std::io::stdin();
    server.serve(stdin.lock(), std::io::stdout()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn render_help_prints_usage_and_writes_nothing() {
        assert!(render(&args(&["--help"])).is_ok());
        assert!(render(&args(&["-h"])).is_ok());
        assert!(!std::path::Path::new("--help").exists());
    }

    #[test]
    fn render_rejects_unknown_flag() {
        let err = render(&args(&["--sample", "--scael", "2", "out.png"])).unwrap_err();
        assert_eq!(err, "unknown option `--scael`");
        assert!(!std::path::Path::new("--scael").exists());
    }

    #[test]
    fn other_subcommands_accept_help() {
        assert!(info(&args(&["--help"])).is_ok());
        assert!(convert(&args(&["-h"])).is_ok());
        assert!(run(&args(&["--help"])).is_ok());
        assert!(describe(Some("--help")).is_ok());
        assert!(mcp(&args(&["-h"])).is_ok());
    }

    #[test]
    fn render_refuses_an_unknown_option_instead_of_writing_it_as_a_file() {
        let out = std::env::temp_dir().join("deckcraft-cli-unknown-option.png");
        let _ = std::fs::remove_file(&out);
        for flag in ["--bogus", "-x"] {
            let args: Vec<String> = ["--sample", flag, &out.to_string_lossy()].iter().map(|a| a.to_string()).collect();
            let e = render(&args).unwrap_err();
            assert!(e.contains(&format!("unknown option `{flag}`")), "{flag}: {e}");
            assert!(!out.exists(), "{flag}: rendered anyway");
        }
    }

    /// `render --transparent` leaves the slide background out (#784): the corner is transparent.
    #[test]
    fn render_transparent_leaves_the_background_out() {
        let dir = std::env::temp_dir().join(format!("deckcraft-cli-transparent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alpha = |path: &std::path::Path| deckcraft_engine::render::decode_image(&std::fs::read(path).unwrap()).unwrap().get_pixel(1, 1).0[3];
        let opaque = dir.join("opaque.png");
        render(&args(&["--sample", "--scale", "0.25", &opaque.to_string_lossy()])).unwrap();
        assert_eq!(alpha(&opaque), 255);
        let clear = dir.join("clear.png");
        render(&args(&["--sample", "--transparent", "--scale", "0.25", &clear.to_string_lossy()])).unwrap();
        assert_eq!(alpha(&clear), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `run --export OUT.gif --export-params JSON` and `convert IN OUT.gif` write animated GIFs.
    #[test]
    fn exports_an_animated_gif() {
        let dir = std::env::temp_dir().join(format!("deckcraft-cli-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gif = dir.join("sample.gif").to_string_lossy().to_string();
        let deck = dir.join("sample.deckcraft").to_string_lossy().to_string();
        let params = r#"{"width": "small", "secondsPerSlide": 2, "loop": false, "slides": [0, 1]}"#;
        run(&args(&["--sample", "--save", &deck, "--export", &gif, "--export-params", params])).unwrap();
        let bytes = std::fs::read(&gif).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 640, "small = 640 px wide");
        assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "play once: no loop block");

        let gif2 = dir.join("converted.gif").to_string_lossy().to_string();
        convert(&args(&[&deck, &gif2])).unwrap();
        let bytes = std::fs::read(&gif2).unwrap();
        assert!(bytes.starts_with(b"GIF89a") && bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "loops forever by default");

        // Bad options are errors, not panics.
        let bad = dir.join("bad.gif").to_string_lossy().to_string();
        assert!(run(&args(&["--sample", "--export", &bad, "--export-params", r#"{"width": 1e9}"#])).is_err());
        assert!(run(&args(&["--sample", "--export", &bad, "--export-params", "[1]"])).is_err());
        assert!(run(&args(&["--sample", "--export-params", "{}"])).is_ok());
        assert!(run(&args(&["--sample", "--export-params", r#"{"width": 64}"#])).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod slide_index_tests {
    use super::{missing_slide, slide_indices};

    #[test]
    fn slide_numbers_start_at_zero_and_files_at_one() {
        assert_eq!(slide_indices(8, false, None).ok().as_deref(), Some(&[0][..]));
        assert_eq!(slide_indices(8, false, Some(0)).ok().as_deref(), Some(&[0][..]));
        assert_eq!(slide_indices(8, false, Some(7)).ok().as_deref(), Some(&[7][..]));
        assert_eq!(slide_indices(8, true, None).ok().as_deref(), Some(&[0, 1, 2, 3, 4, 5, 6, 7][..]));
        let err = slide_indices(8, false, Some(8)).err().unwrap_or_default();
        assert!(!err.is_empty(), "index 8 was accepted");
        assert!(err.contains("numbered 0 through 7"), "{err}");
        assert!(err.contains("slide-01.png"), "{err}");
        assert!(err.contains("slide-08.png"), "{err}");
        assert!(missing_slide(0, 0).contains("no slides"));
        assert!(missing_slide(1, 1).contains("index 0"));
    }
}

#[cfg(test)]
mod frame_path_tests {
    use super::frame_paths;

    #[test]
    fn single_frame_uses_out_as_is() {
        assert_eq!(frame_paths("a/b.png", &[0.5]), Ok(vec!["a/b.png".to_string()]));
    }

    #[test]
    fn frames_get_distinct_names() {
        let paths = frame_paths("f.png", &[0.0, 0.25, 1.0]).unwrap_or_default();
        assert_eq!(paths, ["f-t0.000.png", "f-t0.250.png", "f-t1.000.png"]);
    }

    #[test]
    fn values_rounding_to_one_name_are_rejected() {
        assert!(frame_paths("f.png", &[0.1001, 0.1002]).is_err());
        assert!(frame_paths("f.png", &[0.5, 0.5]).is_err());
    }
}
