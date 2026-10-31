//! The subcommands: what each does with the values the command line already typed.

use std::path::Path;

use deckcraft_engine::Session;
use serde_json::{Value, json};

use crate::cli::{
    AppArgs, AppCall, BenchArgs, Command, CommandSource, CommandsArgs, ConvertArgs, DescribeArgs, InfoArgs, McpArgs, RenderArgs,
    RenderTransitionArgs, RunArgs,
};
use crate::error::{CommandError, Failure, FormatError, LineError};
use crate::io::Io;

type Result<T = (), E = crate::error::CliError> = std::result::Result<T, E>;

pub fn dispatch(command: Command, io: &mut Io<'_>) -> Result {
    match command {
        Command::Info(a) => info(a, io),
        Command::Render(a) => render(a, io),
        Command::RenderTransition(a) => render_transition(a, io),
        Command::Convert(a) => convert(a, io),
        Command::Run(a) => run(a, io),
        Command::Commands(a) => commands(a, io),
        Command::Describe(a) => describe(a, io),
        Command::App(a) => app(a, io),
        Command::Mcp(a) => mcp(a, io),
        Command::Bench(a) => bench(a, io),
        Command::Links => {
            use deckcraft_engine::links::{APP_PAGE, DISCORD, GITHUB, WEBSITE};
            io.line(format_args!("Discord   {DISCORD}\nWebsite   {WEBSITE}\nApp page  {APP_PAGE}\nGitHub    {GITHUB}"));
            Ok(())
        }
        Command::Version => {
            io.line(format_args!("deckcraft-cli {}", env!("CARGO_PKG_VERSION")));
            Ok(())
        }
    }
}

/// The engine takes paths as JSON strings.
fn utf8(path: &Path) -> Result<&str, Failure> {
    path.to_str().ok_or_else(|| Failure::NotUnicode { path: path.to_owned() })
}

fn open(s: &mut Session, input: Option<&Path>, sample: bool, io: &mut Io<'_>) -> Result<(), Failure> {
    if sample {
        return Ok(deckcraft_engine::sample::open_sample(s)?);
    }
    match input {
        Some(p) => {
            let opened = s.execute("file.open", &json!({"path": utf8(p)?}))?;
            warn_if_damaged(p, &opened, io);
            Ok(())
        }
        None => s.execute("file.new", &json!({})).map(|_| ()).map_err(Failure::from),
    }
}

/// Say on stderr what a file lost on opening, so a saved copy doesn't silently miss it (#203).
fn warn_if_damaged(path: &Path, opened: &Value, io: &mut Io<'_>) {
    let count = |key: &str| opened.get(key).and_then(|v| v.as_array()).map_or(0, |a| a.len());
    let (slides, media) = (count("lostSlides"), count("damagedMedia"));
    let path = path.display();
    if slides > 0 {
        io.warning(format_args!("{path}: {slides} slide{} couldn't be read and left out", if slides == 1 { "" } else { "s" }));
    }
    if media > 0 {
        io.warning(format_args!("{path}: {media} picture or media file{} couldn't be read", if media == 1 { "" } else { "s" }));
    }
}

/// Median and slowest of `runs` timings of `f`, in milliseconds.
fn time_ms(runs: usize, mut f: impl FnMut() -> Result<(), Failure>) -> Result<(f64, f64), Failure> {
    let mut ms = Vec::with_capacity(runs);
    for _ in 0..runs.max(1) {
        let t = std::time::Instant::now();
        f()?;
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(summarize_ms(ms))
}

/// Summarize one population of measurements; each maximum must come from that population.
pub(crate) fn summarize_ms(mut ms: Vec<f64>) -> (f64, f64) {
    ms.sort_by(f64::total_cmp);
    (ms.get(ms.len() / 2).copied().unwrap_or(0.0), ms.last().copied().unwrap_or(0.0))
}

/// Peak resident memory of this process in MiB (Linux `VmHWM`); `None` elsewhere.
fn peak_rss_mib() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = s.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024.0)
}

/// The performance baseline (docs/gaps.md "No baseline"): how long a deck takes to open, to
/// render each slide, to save and to export, and the peak memory, as JSON or a table.
fn bench(args: BenchArgs, io: &mut Io<'_>) -> Result {
    use deckcraft_engine::cmd::file::{open_presentation, save_bytes};
    let runs = usize::from(args.runs);
    let mut s = Session::new();
    let (name, bytes) = match &args.file {
        Some(p) => (p.display().to_string(), std::fs::read(p).map_err(|source| Failure::Io { path: p.clone(), source })?),
        None => {
            open(&mut s, None, true, io)?;
            let doc = &s.doc().map_err(Failure::from)?.doc;
            ("sample.deckcraft".to_string(), save_bytes(doc, "deckcraft").map_err(Failure::from)?)
        }
    };
    let e = |x: deckcraft_engine::EngineError| Failure::from(x);
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
        ("export .pdf", time_ms(runs, || save_bytes(&doc, "pdf").map(|_| ()).map_err(e))?),
    ];
    let render_all = time_ms(runs, || {
        for i in 0..slides {
            deckcraft_engine::render::render_slide(&doc, i, &opts);
        }
        Ok(())
    })?;
    steps.insert(1, ("render all slides (1×)", render_all));
    let peak = peak_rss_mib();
    if args.json {
        let v = json!({
            "file": name,
            "bytes": bytes.len(),
            "slides": slides,
            "runs": runs,
            "steps": steps.iter().map(|(k, (m, x))| json!({"step": k, "medianMs": m, "maxMs": x})).collect::<Vec<_>>(),
            "renderMsPerSlide": per_slide.iter().map(|m| m.0).collect::<Vec<_>>(),
            "peakRssMiB": peak,
        });
        io.json(&v);
    } else {
        io.line(format_args!("{name}: {slides} slides, {} KiB, median of {runs} runs", bytes.len() / 1024));
        for (k, (m, x)) in &steps {
            io.line(format_args!("  {k:<24} {m:>9.1} ms   (slowest {x:.1} ms)"));
        }
        if let Some((i, m)) = per_slide.iter().enumerate().max_by(|a, b| a.1.0.total_cmp(&b.1.0)) {
            io.line(format_args!("  slowest slide            {:>9.1} ms   (slide {})", m.0, i + 1));
        }
        if let Some(p) = peak {
            io.line(format_args!("  peak memory              {p:>9.1} MiB"));
        }
    }
    Ok(())
}

fn info(args: InfoArgs, io: &mut Io<'_>) -> Result {
    let mut s = Session::new();
    open(&mut s, Some(&args.file), false, io)?;
    let v = s.execute("document.inspect", &json!({}))?;
    io.json(&v);
    Ok(())
}

fn render(args: RenderArgs, io: &mut Io<'_>) -> Result {
    let (input, out) = args.input_and_out()?;
    let mut s = Session::new();
    open(&mut s, input.map(|p| p.as_path()), args.sample, io)?;
    let st = s.doc().map_err(Failure::from)?;
    let list = slide_indices(st.doc.slides.len(), args.all, args.slide)?;
    if args.all {
        std::fs::create_dir_all(out).map_err(|source| Failure::Io { path: out.clone(), source })?;
    }
    for i in list {
        let (png, w, h) = deckcraft_engine::cmd::file::render_png_with(&st.doc, i, args.scale, args.edit, args.transparent);
        let path = if args.all { out.join(format!("slide-{:02}.png", i + 1)) } else { out.clone() };
        std::fs::write(&path, png).map_err(|source| Failure::Io { path: path.clone(), source })?;
        io.line(format_args!("{} ({w}×{h})", path.display()));
    }
    Ok(())
}

/// Slides `--slide` asks for. `N` is a 0-based index, the same one `slide.go` uses.
/// `--all` still names files from 1 (`slide-01.png`), which is PowerPoint's slide number.
pub(crate) fn slide_indices(n: usize, all: bool, slide: Option<usize>) -> Result<Vec<usize>, Failure> {
    if all {
        return Ok((0..n).collect());
    }
    let i = slide.unwrap_or(0);
    if i >= n {
        return Err(Failure::MissingSlide { index: i, count: n });
    }
    Ok(vec![i])
}

pub(crate) fn missing_slide(i: usize, n: usize) -> String {
    match n {
        0 => format!("no slide index {i} (this deck has no slides)"),
        1 => format!("no slide index {i} (this deck has 1 slide, index 0, which --all names slide-01.png)"),
        n => {
            let last = n.saturating_sub(1);
            format!(
                "no slide index {i} (this deck has {n} slides, numbered 0 through {last}; --all names them slide-01.png through slide-{n:02}.png)"
            )
        }
    }
}

/// Frames of slide N's transition (0-based N; from slide N-1, or black for the first slide) at
/// the given progress values. With several values, OUT gets `-t<value>` before its extension.
fn render_transition(args: RenderTransitionArgs, io: &mut Io<'_>) -> Result {
    use deckcraft_engine::render;
    let paths = frame_paths(&args.out, &args.times)?;
    let mut s = Session::new();
    open(&mut s, Some(&args.file), false, io)?;
    let st = s.doc().map_err(Failure::from)?;
    let doc = &st.doc;
    let (i, n) = (args.slide, doc.slides.len());
    let Some(sl) = doc.slides.get(i) else { return Err(Failure::NoTransitionSlide { index: i, count: n }.into()) };
    let sw = doc.slide_size.width.max(1.0);
    let w = args.width.unwrap_or(sw * 2.0).min(render::MAX_SIDE as f64);
    let opts = render::RenderOpts { scale: w / sw, ..Default::default() };
    let tr = sl.transition.clone().unwrap_or_default();
    // Render both slides once and composite every frame from them (morph re-renders per frame).
    let prev = i.checked_sub(1);
    let cached = (tr.kind != "morph").then(|| {
        let parts = render::uses_slide_parts(&tr.kind, &tr.option).then(|| render::SlideParts::render(doc, prev, i, &opts));
        (prev.map(|p| render::render_slide(doc, p, &opts)), render::render_slide(doc, i, &opts), parts)
    });
    let aspect = sw / doc.slide_size.height.max(1.0);
    for (t, path) in args.times.iter().zip(&paths) {
        let img = match &cached {
            Some((old, new, parts)) => {
                render::composite_transition(old.as_ref(), new, parts.as_ref(), &render::transition_frame(&tr.kind, &tr.option, *t, aspect))
            }
            None => render::render_transition(doc, i, *t, &opts),
        };
        std::fs::write(path, img.to_png_at(render::dpi_for_scale(opts.scale))).map_err(|source| Failure::Io { path: path.clone(), source })?;
        io.line(format_args!("{} ({}×{}) {} {} t={t}", path.display(), img.width, img.height, tr.kind, tr.option));
    }
    Ok(())
}

/// Output path of each frame. A single frame goes to OUT; several get `-t<value>` (three decimals)
/// before the extension. Values that round to the same name would overwrite each other, so that is an error.
pub(crate) fn frame_paths(out: &Path, ts: &[f64]) -> Result<Vec<std::path::PathBuf>, Failure> {
    if ts.len() == 1 {
        return Ok(vec![out.to_owned()]);
    }
    let stem = out.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("frame");
    let mut paths: Vec<std::path::PathBuf> = Vec::with_capacity(ts.len());
    for t in ts {
        let path = out.with_file_name(format!("{stem}-t{t:.3}.png"));
        if paths.contains(&path) {
            return Err(Failure::FrameCollision { path });
        }
        paths.push(path);
    }
    Ok(paths)
}

fn convert(args: ConvertArgs, io: &mut Io<'_>) -> Result {
    let fmt = output_format(&args.out).map_err(Failure::from)?;
    let out = utf8(&args.out)?;
    let mut s = Session::new();
    open(&mut s, Some(&args.input), false, io)?;
    let r = if matches!(fmt, "png" | "jpeg") {
        s.execute("file.export", &json!({"path": out, "slide": 0}))
    } else if fmt == "gif" {
        s.execute("file.export", &json!({"path": out}))
    } else {
        s.execute("file.saveAs", &json!({"path": out}))
    };
    r.map_err(Failure::from)?;
    io.line(format_args!("{out}"));
    Ok(())
}

/// The format `convert`, `run --save` and `run --export` write to `path`, or an error naming the
/// supported extensions when `path`'s extension isn't one (rather than native bytes under that name).
pub fn output_format(path: &Path) -> Result<&'static str, FormatError> {
    use deckcraft_engine::cmd::file::{WRITABLE_EXTENSIONS, writable_format_for_path};
    let unsupported = || {
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| "(none)".into());
        FormatError::Unsupported { path: path.to_owned(), ext, supported: WRITABLE_EXTENSIONS.join(", .") }
    };
    writable_format_for_path(path.to_str().ok_or_else(unsupported)?).ok_or_else(unsupported)
}

const MAX_COMMAND_FILE_LINE_BYTES: usize = 16 * 1024 * 1024;

fn read_command_file_line(reader: &mut impl std::io::BufRead, line: &mut Vec<u8>) -> std::io::Result<bool> {
    line.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(available.len());
        line.len().checked_add(count).filter(|length| *length <= MAX_COMMAND_FILE_LINE_BYTES).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("command line exceeds {MAX_COMMAND_FILE_LINE_BYTES} byte limit"))
        })?;
        line.extend_from_slice(available.get(..count).unwrap_or_default());
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(true);
        }
    }
}

/// The same command syntax and execution path for --cmd and each command-file line.
fn run_command(s: &mut Session, command: &str, print: bool, io: &mut Io<'_>) -> Result<(), CommandError> {
    let (id, params) = match command.split_once('=') {
        Some((id, j)) => (id, serde_json::from_str::<Value>(j).map_err(|source| CommandError::BadJson { id: id.to_owned(), source })?),
        None => (command, json!({})),
    };
    let r = s.execute(id, &params)?;
    if print {
        io.line(format_args!("{id} → {r}"));
    }
    Ok(())
}

fn run_command_file(s: &mut Session, path: &Path, print: bool, io: &mut Io<'_>) -> Result<(), Failure> {
    let file = std::fs::File::open(path).map_err(|source| Failure::Io { path: path.to_owned(), source })?;
    let mut reader = std::io::BufReader::new(file);
    let mut bytes = Vec::new();
    let mut line_number = 0usize;
    loop {
        line_number = line_number.checked_add(1).ok_or_else(|| Failure::TooManyLines { path: path.to_owned() })?;
        let more = read_command_file_line(&mut reader, &mut bytes).map_err(|e| Failure::CommandFileLine {
            path: path.to_owned(),
            line: line_number,
            source: LineError::Io(e),
        })?;
        if !more {
            break;
        }
        let line = std::str::from_utf8(&bytes).map_err(|e| Failure::CommandFileLine {
            path: path.to_owned(),
            line: line_number,
            source: LineError::Utf8(e),
        })?;
        let line = if line_number == 1 { line.trim_start_matches('\u{feff}') } else { line };
        let command = line.trim();
        if command.is_empty() || command.starts_with('#') {
            continue;
        }
        run_command(s, command, print, io).map_err(|source| Failure::CommandFileCommand { path: path.to_owned(), line: line_number, source })?;
    }
    Ok(())
}

fn run(args: RunArgs, io: &mut Io<'_>) -> Result {
    let mut s = Session::new();
    open(&mut s, args.input.as_deref(), args.sample, io)?;
    for source in &args.sources {
        match source {
            CommandSource::Inline(command) => run_command(&mut s, command, args.print, io)?,
            CommandSource::File(path) => run_command_file(&mut s, path, args.print, io)?,
        }
    }
    if let Some(p) = &args.save {
        s.execute("file.saveAs", &json!({"path": utf8(p)?}))?;
    }
    if let Some(p) = &args.export {
        let mut params = args.export_params.unwrap_or_default();
        params.insert("path".into(), json!(utf8(p)?));
        s.execute("file.export", &Value::Object(params))?;
    }
    Ok(())
}

fn commands(args: CommandsArgs, io: &mut Io<'_>) -> Result {
    let f = args.filter.map(|x| x.to_lowercase());
    for c in deckcraft_engine::command_specs()
        .iter()
        .filter(|c| f.as_ref().is_none_or(|f| c.id.to_lowercase().contains(f.as_str()) || c.label.to_lowercase().contains(f.as_str())))
    {
        match c.params.strip_prefix("{}") {
            Some(answer) => io.line(format_args!("{} — {}{answer}", c.id, c.label)),
            None => io.line(format_args!("{} — {}: {}", c.id, c.label, c.params)),
        }
    }
    Ok(())
}

fn describe(args: DescribeArgs, io: &mut Io<'_>) -> Result {
    let id = args.command.as_str();
    let c = deckcraft_engine::find_command(id).ok_or_else(|| Failure::from(deckcraft_engine::EngineError::UnknownCommand(id.into())))?;
    let mut head = format!("{}: {}", c.id, c.label);
    if !c.menu.is_empty() {
        head.push_str(&format!(", in {}", c.menu.join(" ▸ ")));
    }
    if let Some(k) = c.shortcut {
        head.push_str(&format!(", shortcut {k}"));
    }
    io.line(format_args!("{head}.\n"));
    let (args, answer) = c.params.split_once('→').unwrap_or((c.params, ""));
    let parsed = deckcraft_engine::cmd::parse_params(c.params);
    match &parsed {
        Err(_) => io.line(format_args!("Parameters: {}", args.trim())),
        Ok(params) if params.is_empty() => io.line(format_args!("Takes no parameters.")),
        Ok(params) => print_params(params, io),
    }
    let params = parsed.unwrap_or_default();
    if let Some(acts) = c.acts_on() {
        io.line(format_args!("{acts}"));
    }
    let remark = remark(args);
    if !remark.is_empty() {
        io.line(format_args!("Note: {remark}"));
    }
    if !answer.trim().is_empty() {
        io.line(format_args!("Answers with {}.", answer.trim()));
    }
    let bare = params.is_empty() || params.iter().any(|p| params.iter().filter(|q| q.object == p.object).all(|q| q.optional));
    let call = if bare { c.id.to_string() } else { format!("'{}={{…}}'", c.id) };
    io.line(format_args!("\nRun it with: deckcraft-cli run --cmd {call}"));
    Ok(())
}

fn print_params(params: &[deckcraft_engine::cmd::Param], io: &mut Io<'_>) {
    let name = |p: &deckcraft_engine::cmd::Param| {
        let or = if p.or { "or " } else { "" };
        let optional = if p.optional { ", optional" } else { "" };
        format!("{or}{}{optional}", p.name)
    };
    let width = params.iter().map(|p| name(p).chars().count()).max().unwrap_or_default();
    let sets = params.iter().map(|p| p.object).max().unwrap_or_default() > 0;
    io.line(format_args!("Parameters:"));
    for (k, p) in params.iter().enumerate() {
        if sets && params.get(k.wrapping_sub(1)).is_none_or(|before| before.object != p.object) {
            io.line(format_args!("  {}", if p.object == 0 { "either:" } else { "or:" }));
        }
        let indent = if sets { "    " } else { "  " };
        io.line(format_args!("{}", format!("{indent}{:<width$}  {}", name(p), p.about).trim_end()));
    }
}

fn remark(args: &str) -> &str {
    let after = args.rfind('}').and_then(|i| args.get(i + 1..)).unwrap_or_default().trim().trim_start_matches('—').trim();
    after.strip_prefix('(').and_then(|n| n.strip_suffix(')')).unwrap_or(after)
}

fn app(args: AppArgs, io: &mut Io<'_>) -> Result {
    use deckcraft_mcp::{Backend, Remote, control_addr};
    let call = args.call()?;
    let port = &args.port;
    let mut r = Remote::connect(&control_addr(port)).map_err(|source| Failure::NoApp { port: port.clone(), source })?;
    let v = match call {
        AppCall::Method { name, params } => r.call(name, params),
        AppCall::Command { id, params } => r.call("engine.execute", json!({"command": id, "params": params})),
    }
    .map_err(Failure::Control)?;
    io.json(&v);
    Ok(())
}

fn mcp(args: McpArgs, io: &mut Io<'_>) -> Result {
    use deckcraft_engine::file_access::{self, AutomationRoots};
    use deckcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let roots = AutomationRoots::new(&args.automation_read_root, &args.automation_write_root).map_err(Failure::Roots)?;
    if roots.is_some() && args.connect.is_some() {
        return Err(Failure::RootsWithConnect.into());
    }
    let backend: Box<dyn Backend> = match args.connect {
        Some(c) => Box::new(Remote::connect(&control_addr(&c)).map_err(|source| Failure::NoAppAt { addr: c, source })?),
        None => {
            let mut h = Headless::new();
            // `--sample` is the operator's own choice, made before the roots come into force.
            if args.sample {
                deckcraft_engine::sample::open_sample(&mut h.session).map_err(Failure::from)?;
            }
            if let Some(roots) = roots {
                // Every thread of this process, not only the server's: nothing reaches past the roots.
                file_access::confine_process(roots.clone());
                let show = |roots: Vec<String>| if roots.is_empty() { "none".to_string() } else { roots.join(", ") };
                io.diag(format_args!(
                    "files confined: read roots {}, write roots {}",
                    show(roots.read_roots().map(|r| r.display().to_string()).collect()),
                    show(roots.write_roots().map(|r| r.display().to_string()).collect())
                ));
                h = h.with_automation_roots(Some(roots));
            }
            Box::new(h)
        }
    };
    let mut server = Server::new(backend);
    let stdin = std::io::stdin();
    server.serve(stdin.lock(), io.raw_stdout()).map_err(Failure::Serve)?;
    Ok(())
}
