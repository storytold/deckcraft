//! The command line as clap derive types (RFC-0012): `--help` is these doc comments, and the
//! values reach the commands already typed.

use std::ffi::OsStr;
use std::path::PathBuf;

use clap::builder::TypedValueParser as _;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::{Map, Value};

use crate::error::{FormatError, JsonArgError, UsageError};

/// Work on DeckCraft presentations (.deckcraft, .pptx) without a window.
///
/// A presentation is edited by running commands on it:
///
///   deckcraft-cli run --cmd 'slide.new={"title":"Q3 results"}' --save q3.deckcraft
///
/// `deckcraft-cli help SUBCOMMAND`, or `deckcraft-cli SUBCOMMAND --help`, explains one
/// subcommand; `deckcraft-cli help run` explains how to write a command.
///
/// If an output reader closes early, run, render, render-transition and convert continue; their
/// exit status reflects the work, not output delivery. Stop the process to cancel them.
#[derive(Parser, Debug)]
#[command(name = "deckcraft-cli", version, propagate_version = true, arg_required_else_help = true, verbatim_doc_comment, after_help = AFTER_HELP)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

const AFTER_HELP: &str = "\
Exit status:
  0  success
  1  a command failed
  2  usage error";

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print a presentation's slides, layouts and theme as JSON.
    ///
    /// Prints FILE (.deckcraft or .pptx) as JSON: its slides (index, id, title, layout, how many
    /// shapes, notes, transition), its layouts, theme fonts, sections and slide size.
    ///
    /// The shapes of one slide, with their ids, boxes and text, come from the slide.inspect command:
    ///
    ///   deckcraft-cli run --in deck.deckcraft --print --cmd 'slide.inspect={"index":0}'
    #[command(verbatim_doc_comment)]
    Info(InfoArgs),
    /// Draw slides as PNG images.
    ///
    /// Draws slides of FILE, or of the sample presentation with --sample, as PNG images. FILE
    /// comes first; OUT is the image to write. With --all, OUT is a folder that gets one
    /// slide-01.png, slide-02.png… per slide. Prints each image's path and size.
    ///
    ///   deckcraft-cli render deck.deckcraft --slide 2 slide-3.png
    #[command(verbatim_doc_comment, override_usage = "deckcraft-cli render [OPTIONS] (FILE | --sample) OUT")]
    Render(RenderArgs),
    /// Draw frames of the transition into a slide as PNG images.
    ///
    /// Draws the transition into slide N of FILE at each time T, from 0 (the slide before) to 1
    /// (slide N), as PNG images. With one T, OUT.png is the image; with several, each frame is
    /// written next to it as OUT-t0.250.png and so on. Prints each image's path, size,
    /// transition and time.
    ///
    ///   deckcraft-cli render-transition deck.deckcraft --slide 1 --t 0.25,0.5,0.75 fade.png
    #[command(verbatim_doc_comment)]
    RenderTransition(RenderTransitionArgs),
    /// Save a presentation in the format OUT's extension names.
    ///
    /// Opens IN (.deckcraft or .pptx) and writes it to OUT, in the format OUT's extension names:
    /// .deckcraft, .pptx, .pdf, .txt or .md for the outline, .png or .jpg for the first slide,
    /// .gif for an animated GIF with one frame per slide. Other extensions are refused. Prints OUT.
    ///
    ///   deckcraft-cli convert deck.deckcraft deck.pptx
    #[command(verbatim_doc_comment)]
    Convert(ConvertArgs),
    /// Run commands on a presentation, then save or export it.
    ///
    /// Runs commands, in order, on one presentation: FILE with --in, the sample presentation with
    /// --sample, else a new one with an empty title slide. Then --save writes the presentation:
    /// .deckcraft keeps everything; .pptx, .pdf and .md (the outline) are written as convert
    /// writes them. --export writes the current slide as .png or .jpg, or the presentation as
    /// .pdf, .pptx or an animated .gif (one frame per slide). --export-params passes file.export
    /// options as one JSON object, such as
    /// '{"width": "large", "secondsPerSlide": 3, "loop": false, "slides": [0, 1, 2]}' for a GIF
    /// (`deckcraft-cli describe file.export` lists them).
    ///
    /// Each --cmd is one command: COMMAND is a command id, then `=`, then the command's
    /// parameters as one JSON object. Quote it for the shell. A command that needs no parameters
    /// is just its id.
    ///
    /// --cmd-file FILE reads the same commands from a UTF-8 text file, one per line, avoiding
    /// shell quoting and command-line length limits. Each line can be up to 16 MiB, including
    /// commands with embedded media data. Blank lines and full-line # comments are ignored, as is
    /// surrounding whitespace. CRLF line endings and an optional UTF-8 BOM at the start of the
    /// file are accepted. Repeat --cmd-file or mix it with --cmd: commands execute in the order
    /// those options appear. File errors name the path; line-reading, JSON and command errors
    /// also name the line number.
    ///
    ///   deckcraft-cli run --cmd-file commands.txt --cmd slide.next --save result.deckcraft
    ///
    ///   deckcraft-cli run \
    ///     --cmd 'slide.new={"layout":"titleOnly","title":"Q3 results"}' \
    ///     --cmd 'shape.insert={"preset":"roundRect","rect":[80,160,320,120],"text":"Revenue"}' \
    ///     --cmd 'format.size={"size":28}' \
    ///     --save q3.deckcraft
    ///
    /// `deckcraft-cli commands` lists the command ids; `deckcraft-cli describe COMMAND` explains
    /// one and its parameters.
    ///
    /// What the ids don't say:
    /// - Positions and sizes are in points on a 960×540 slide, x to the right and y down; a rect
    ///   is [x, y, width, height].
    /// - Slide indices start at 0. slide.new inserts after the current slide and makes the new
    ///   one current; slide.go {"index": N} makes slide N current.
    /// - Shape ids are looked up on the current slide only.
    /// - A command that adds a shape (shape.insert, insert.textBox, insert.table…) selects it.
    ///   format.* acts on the selected text or shapes; edit.select {"ids": [...]} selects
    ///   others. Many shape commands take the ids directly; `deckcraft-cli describe COMMAND`
    ///   says what a command acts on.
    ///
    /// If a command fails, run stops there and saves nothing. --print shows each command's
    /// answer, such as the id of an inserted shape.
    #[command(verbatim_doc_comment)]
    Run(RunArgs),
    /// List the commands run can use, one per line.
    ///
    /// Lists the commands that run --cmd, app and the MCP server take, one per line: the id, its
    /// name in the app and its parameters. FILTER keeps the commands whose id or name contains
    /// it, ignoring case.
    ///
    ///   deckcraft-cli commands format.
    ///
    /// In the parameters, `?` marks an optional one, a spaced ` | ` separates alternatives and
    /// `→` shows what the command answers. `deckcraft-cli describe COMMAND` spells one command
    /// out.
    #[command(verbatim_doc_comment)]
    Commands(CommandsArgs),
    /// Explain one command and its parameters.
    ///
    /// Explains one command: what it is in the app, each of its parameters, what it answers,
    /// and how run calls it.
    ///
    ///   deckcraft-cli describe shape.insert
    #[command(verbatim_doc_comment)]
    Describe(DescribeArgs),
    /// Run a command in the DeckCraft app started with --control.
    ///
    /// Sends one request to the DeckCraft app started with `deckcraft --control PORT` (port 7979
    /// by default) and prints its answer as JSON. COMMAND [PARAMS] runs a command, as run --cmd
    /// does; --method METHOD [PARAMS] calls a control-channel method such as ui.screenshot.
    /// PARAMS is a JSON object.
    ///
    ///   deckcraft-cli app shape.insert '{"preset":"star5","rect":[100,100,200,200]}'
    ///   deckcraft-cli app --method ui.screenshot '{"path":"/tmp/shot.png"}'
    #[command(
        verbatim_doc_comment,
        override_usage = "deckcraft-cli app [OPTIONS] COMMAND [PARAMS]\n       deckcraft-cli app [OPTIONS] --method METHOD [PARAMS]"
    )]
    App(AppArgs),
    /// Serve the commands to an agent over MCP on stdio.
    ///
    /// Runs a Model Context Protocol server on stdin and stdout. It works on its own
    /// presentation without a window, or with --connect PORT (or host:port) on the DeckCraft app
    /// started with --control PORT. --sample opens the sample presentation.
    ///
    /// --automation-read-root DIR and --automation-write-root DIR confine every file the agent's
    /// commands read (open, pictures, audio, video) and write (save, export, PDF) to those
    /// folders, links followed. Repeat either flag to grant access to more folders; a root left
    /// out grants none of its access. They run the in-process server, so they conflict with
    /// --connect (a running app is not confined by them).
    #[command(verbatim_doc_comment)]
    Mcp(McpArgs),
    /// Time opening, drawing, saving and exporting a presentation.
    ///
    /// Times how long FILE, or the sample presentation with --sample, takes to open, to draw each
    /// slide, to save as .deckcraft and .pptx, to reopen as .pptx and to export as .pdf, and the
    /// peak memory. Prints the median and slowest of N runs (5 by default, 1 to 1000) as a
    /// table, or as JSON with --json.
    ///
    ///   deckcraft-cli bench deck.pptx --runs 10
    #[command(verbatim_doc_comment)]
    Bench(BenchArgs),
    /// Print the DeckCraft community and project links.
    Links,
    /// Print the version (same as --version).
    #[command(hide = true)]
    Version,
}

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// The presentation to describe (.deckcraft or .pptx).
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
}

#[derive(Args, Debug)]
pub struct RenderArgs {
    /// FILE then OUT; with --sample, only OUT.
    #[arg(value_name = "PATH", num_args = 1..=2, required = true)]
    pub paths: Vec<PathBuf>,
    /// Draw the sample presentation instead of a FILE.
    #[arg(long)]
    pub sample: bool,
    /// The slide at index N; indices start at 0, and 0 is the default.
    #[arg(long, value_name = "N", conflicts_with = "all")]
    pub slide: Option<usize>,
    /// Draw every slide; OUT is then a folder.
    #[arg(long)]
    pub all: bool,
    /// Pixels per point: a 960×540 slide becomes 1920×1080 at the default 2.
    #[arg(long, value_name = "S", default_value = "2", value_parser = positive_finite)]
    pub scale: f64,
    /// Draw what the editor shows: placeholder prompts and their dashed outlines.
    #[arg(long)]
    pub edit: bool,
    /// Leave the slide background out, so only what is on the slide is drawn.
    #[arg(long)]
    pub transparent: bool,
}

impl RenderArgs {
    /// The input and output paths, which clap can only count: `--sample` takes OUT alone,
    /// otherwise FILE then OUT.
    pub fn input_and_out(&self) -> Result<(Option<&PathBuf>, &PathBuf), UsageError> {
        match (self.sample, self.paths.as_slice()) {
            (true, [out]) => Ok((None, out)),
            (false, [file, out]) => Ok((Some(file), out)),
            (true, _) => Err(UsageError::RenderSamplePaths),
            (false, _) => Err(UsageError::RenderPaths),
        }
    }
}

#[derive(Args, Debug)]
pub struct RenderTransitionArgs {
    /// The presentation (.deckcraft or .pptx).
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
    /// The image to write (OUT.png), or the stem of the frames.
    #[arg(value_name = "OUT.png")]
    pub out: PathBuf,
    /// The slide the transition leads into; indices start at 0.
    #[arg(long, value_name = "N", required = true)]
    pub slide: usize,
    /// The times to draw, comma-separated.
    #[arg(long = "t", value_name = "T,T...", required = true, value_delimiter = ',', value_parser = finite)]
    pub times: Vec<f64>,
    /// The image width in pixels, twice the slide width in points by default.
    #[arg(long, value_name = "PX", value_parser = at_least_one_pixel)]
    pub width: Option<f64>,
}

#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// The presentation to open (.deckcraft or .pptx).
    #[arg(value_name = "IN")]
    pub input: PathBuf,
    /// The file to write; its extension names the format.
    #[arg(value_name = "OUT", value_parser = os_parser(writable_path))]
    pub out: PathBuf,
}

/// Where a `run` command comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSource {
    Inline(String),
    File(PathBuf),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Open this presentation first (.deckcraft or .pptx).
    #[arg(long = "in", value_name = "FILE", conflicts_with = "sample")]
    pub input: Option<PathBuf>,
    /// Start from the sample presentation.
    #[arg(long)]
    pub sample: bool,
    /// Run one command: COMMAND[=PARAMS]. Repeat it for more.
    #[arg(long = "cmd", value_name = "COMMAND[=PARAMS]")]
    pub cmds: Vec<String>,
    /// Run the commands in a UTF-8 text file, one per line. Repeat it for more.
    #[arg(long = "cmd-file", value_name = "FILE")]
    pub cmd_files: Vec<PathBuf>,
    /// Write the presentation to OUT (the extension names the format).
    #[arg(long, value_name = "OUT", value_parser = os_parser(writable_path))]
    pub save: Option<PathBuf>,
    /// Export the current slide or the presentation to OUT (.png, .jpg, .pdf, .pptx or .gif).
    #[arg(long, value_name = "OUT", value_parser = os_parser(writable_path))]
    pub export: Option<PathBuf>,
    /// Options for --export (file.export), as one JSON object.
    #[arg(long, value_name = "JSON", requires = "export", value_parser = json_object)]
    pub export_params: Option<Map<String, Value>>,
    /// Show each command's answer.
    #[arg(long)]
    pub print: bool,
    /// The commands in the order --cmd and --cmd-file were given (filled in by [`Cli::parse_args`]).
    #[arg(skip)]
    pub sources: Vec<CommandSource>,
}

#[derive(Args, Debug)]
pub struct CommandsArgs {
    /// Keep the commands whose id or name contains this, ignoring case.
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,
}

#[derive(Args, Debug)]
pub struct DescribeArgs {
    /// The command id, such as shape.insert.
    #[arg(value_name = "COMMAND")]
    pub command: String,
}

#[derive(Args, Debug)]
pub struct AppArgs {
    /// The app's control port, or host:port.
    #[arg(long, value_name = "PORT", default_value = "7979")]
    pub port: String,
    /// Call this control-channel method instead of running a command.
    #[arg(long, value_name = "METHOD")]
    pub method: Option<String>,
    /// COMMAND then its PARAMS (a JSON object); with --method, only PARAMS.
    #[arg(value_name = "ARG", num_args = 0..=2)]
    pub args: Vec<String>,
}

/// What `app` sends: a command with parameters, or a control-channel method with parameters.
#[derive(Debug)]
pub enum AppCall<'a> {
    Command { id: &'a str, params: Value },
    Method { name: &'a str, params: Value },
}

impl AppArgs {
    /// The request, with its JSON checked before anything is sent.
    pub fn call(&self) -> Result<AppCall<'_>, UsageError> {
        let params = |json: Option<&String>| match json {
            Some(j) => serde_json::from_str(j).map_err(UsageError::AppParams),
            None => Ok(Value::Object(Map::new())),
        };
        match (&self.method, self.args.as_slice()) {
            (Some(name), [] | [_]) => Ok(AppCall::Method { name, params: params(self.args.first())? }),
            (Some(_), _) => Err(UsageError::AppMethodArgs),
            (None, [id] | [id, _]) => Ok(AppCall::Command { id, params: params(self.args.get(1))? }),
            (None, _) => Err(UsageError::AppNeedsCommand),
        }
    }
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Drive the app started with --control PORT (PORT or host:port) instead of a headless server.
    #[arg(long, value_name = "PORT", conflicts_with_all = ["automation_read_root", "automation_write_root"])]
    pub connect: Option<String>,
    /// Open the sample presentation.
    #[arg(long)]
    pub sample: bool,
    /// Confine the files the agent reads to this folder. Repeat it for more folders.
    #[arg(long, value_name = "DIR", value_parser = os_parser(non_empty_path))]
    pub automation_read_root: Vec<PathBuf>,
    /// Confine the files the agent writes to this folder. Repeat it for more folders.
    #[arg(long, value_name = "DIR", value_parser = os_parser(non_empty_path))]
    pub automation_write_root: Vec<PathBuf>,
}

#[derive(Args, Debug)]
pub struct BenchArgs {
    /// The presentation to time (.deckcraft or .pptx).
    #[arg(value_name = "FILE", required_unless_present = "sample", conflicts_with = "sample")]
    pub file: Option<PathBuf>,
    /// Time the sample presentation.
    #[arg(long)]
    pub sample: bool,
    /// How many times to time each step, 1 to 1000.
    #[arg(long, value_name = "N", default_value = "5", value_parser = clap::value_parser!(u16).range(1..=1000))]
    pub runs: u16,
    /// Print JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

impl Cli {
    /// Parse `args` (the program name first). The commands of `run` keep the order their
    /// `--cmd` and `--cmd-file` options had, which a derived struct cannot see.
    pub fn parse_args<I, T>(args: I) -> Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let mut matches = Cli::command().try_get_matches_from(args)?;
        let order = matches.subcommand_matches("run").map(command_order);
        let mut cli = Cli::from_arg_matches_mut(&mut matches)?;
        if let (Command::Run(run), Some(order)) = (&mut cli.command, order) {
            run.sources = order
                .into_iter()
                .filter_map(|(_, from_file, k)| {
                    if from_file {
                        run.cmd_files.get(k).cloned().map(CommandSource::File)
                    } else {
                        run.cmds.get(k).cloned().map(CommandSource::Inline)
                    }
                })
                .collect();
        }
        Ok(cli)
    }
}

/// `(position on the command line, from --cmd-file, index among its own kind)` for every command.
fn command_order(run: &clap::ArgMatches) -> Vec<(usize, bool, usize)> {
    let at = |id: &str| run.indices_of(id).into_iter().flatten().enumerate().collect::<Vec<_>>();
    let mut order: Vec<_> =
        at("cmds").into_iter().map(|(k, i)| (i, false, k)).chain(at("cmd_files").into_iter().map(|(k, i)| (i, true, k))).collect();
    order.sort_unstable();
    order
}

fn finite(s: &str) -> Result<f64, String> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("`{s}` is not a finite number"))
}

fn positive_finite(s: &str) -> Result<f64, String> {
    finite(s).and_then(|v| if v > 0.0 { Ok(v) } else { Err(format!("`{s}` is not above 0")) })
}

fn at_least_one_pixel(s: &str) -> Result<f64, String> {
    finite(s).and_then(|v| if v >= 1.0 { Ok(v) } else { Err(format!("`{s}` is under 1 pixel")) })
}

fn json_object(s: &str) -> Result<Map<String, Value>, JsonArgError> {
    match serde_json::from_str::<Value>(s)? {
        Value::Object(map) => Ok(map),
        _ => Err(JsonArgError::NotAnObject),
    }
}

/// A parser of the raw argument, so a path need not be Unicode.
fn os_parser<T, E>(f: fn(&OsStr) -> Result<T, E>) -> impl clap::builder::TypedValueParser<Value = T>
where
    T: Clone + Send + Sync + 'static,
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>> + 'static,
{
    clap::builder::OsStringValueParser::new().try_map(move |s| f(&s))
}

fn non_empty_path(s: &OsStr) -> Result<PathBuf, &'static str> {
    if s.is_empty() { Err("a folder is needed, not an empty value") } else { Ok(PathBuf::from(s)) }
}

/// An output path whose extension names a format the CLI writes.
fn writable_path(s: &OsStr) -> Result<PathBuf, FormatError> {
    let path = PathBuf::from(s);
    crate::commands::output_format(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use clap::error::ErrorKind;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::parse_args(std::iter::once("deckcraft-cli").chain(args.iter().copied()))
    }

    fn kind(args: &[&str]) -> ErrorKind {
        parse(args).unwrap_err().kind()
    }

    fn run_args(args: &[&str]) -> RunArgs {
        match parse(args).unwrap().command {
            Command::Run(r) => r,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn unknown_subcommands_and_flags_are_usage_errors() {
        assert_eq!(kind(&["frobnicate"]), ErrorKind::InvalidSubcommand);
        assert_eq!(kind(&["render", "--sample", "--scael", "2", "out.png"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["render", "--sample", "-x", "out.png"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["run", "--bogus"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["mcp", "--bogus"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["--info", "--help"]), ErrorKind::UnknownArgument);
    }

    #[test]
    fn no_subcommand_shows_the_help_as_an_error() {
        assert_eq!(kind(&[]), ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand);
    }

    #[test]
    fn repeated_single_value_flags_are_rejected() {
        for args in [
            &["render", "--sample", "--scale", "1", "--scale", "2", "o.png"][..],
            &["render", "--sample", "--slide", "1", "--slide", "2", "o.png"],
            &["render", "--sample", "--edit", "--edit", "o.png"],
            &["run", "--save", "a.pptx", "--save", "b.pptx"],
            &["run", "--sample", "--sample"],
            &["run", "--print", "--print"],
            &["run", "--sample", "--in", "a.pptx", "--in", "b.pptx"],
            &["bench", "--sample", "--runs", "1", "--runs", "2"],
            &["app", "--port", "1", "--port", "2", "x"],
            &["mcp", "--connect", "1", "--connect", "2"],
            &["mcp", "--sample", "--sample"],
        ] {
            assert_eq!(kind(args), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }

    #[test]
    fn repeatable_flags_repeat() {
        let r = run_args(&["run", "--cmd", "a", "--cmd", "b", "--cmd-file", "f", "--cmd-file", "g"]);
        assert_eq!((r.cmds.len(), r.cmd_files.len()), (2, 2));
        let Command::Mcp(m) =
            parse(&["mcp", "--automation-read-root", "a b", "--automation-read-root=c", "--automation-write-root=d"]).unwrap().command
        else {
            panic!("not mcp")
        };
        assert_eq!(m.automation_read_root, [PathBuf::from("a b"), PathBuf::from("c")]);
        assert_eq!(m.automation_write_root, [PathBuf::from("d")]);
    }

    #[test]
    fn commands_keep_the_order_of_their_options() {
        let r = run_args(&["run", "--cmd", "one", "--cmd-file", "first.txt", "--cmd", "two", "--cmd-file", "second.txt", "--cmd", "three"]);
        let file = |p: &str| CommandSource::File(p.into());
        let inline = |c: &str| CommandSource::Inline(c.into());
        assert_eq!(r.sources, [inline("one"), file("first.txt"), inline("two"), file("second.txt"), inline("three")]);
        assert!(run_args(&["run", "--sample"]).sources.is_empty());
    }

    #[test]
    fn a_value_flag_never_takes_a_flag_as_its_value() {
        for args in [
            &["run", "--cmd", "--print"][..],
            &["run", "--cmd-file", "--print"],
            &["run", "--save", "--sample"],
            &["run", "--in", "--sample"],
            &["render", "--sample", "--slide", "--all", "o.png"],
            &["mcp", "--connect", "--sample"],
            &["mcp", "--automation-read-root", "--version"],
            &["mcp", "--automation-write-root", "--connect"],
            &["app", "--method", "--port"],
        ] {
            assert_eq!(kind(args), ErrorKind::InvalidValue, "{args:?}");
        }
        // missing value
        assert_eq!(kind(&["run", "--cmd-file"]), ErrorKind::InvalidValue);
        // a value that looks like a short flag is that flag, not a value
        assert_eq!(kind(&["mcp", "--automation-read-root", "-folder"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["render", "--sample", "--slide", "-1", "o.png"]), ErrorKind::UnknownArgument);
        assert_eq!(kind(&["mcp", "--automation-read-root"]), ErrorKind::InvalidValue);
    }

    #[test]
    fn a_dash_value_is_spelled_with_equals_or_a_path() {
        let Command::Mcp(m) = parse(&["mcp", "--automation-read-root=-folder", "--automation-write-root", "./-folder"]).unwrap().command else {
            panic!("not mcp")
        };
        assert_eq!(m.automation_read_root, [PathBuf::from("-folder")]);
        assert_eq!(m.automation_write_root, [PathBuf::from("./-folder")]);
        assert_eq!(run_args(&["run", "--save=-out.pptx"]).save, Some("-out.pptx".into()));
    }

    #[test]
    fn empty_automation_roots_are_rejected() {
        for flag in ["--automation-read-root", "--automation-write-root"] {
            assert_eq!(kind(&["mcp", &format!("{flag}=")]), ErrorKind::ValueValidation, "{flag}");
            assert_eq!(kind(&["mcp", flag, ""]), ErrorKind::ValueValidation, "{flag}");
        }
    }

    #[test]
    fn roots_conflict_with_connect() {
        assert_eq!(kind(&["mcp", "--connect", "7990", "--automation-read-root", "r"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind(&["mcp", "--automation-write-root=w", "--connect=7990"]), ErrorKind::ArgumentConflict);
        assert!(parse(&["mcp", "--connect", "7990", "--sample"]).is_ok());
    }

    #[test]
    fn bad_numbers_are_usage_errors() {
        for args in [
            &["render", "--sample", "--slide", "x", "o.png"][..],
            &["render", "--sample", "--scale", "nan", "o.png"],
            &["render", "--sample", "--scale", "inf", "o.png"],
            &["render", "--sample", "--scale", "0", "o.png"],
            &["render", "--sample", "--scale=-2", "o.png"],
            &["render-transition", "d.pptx", "o.png", "--slide", "1", "--t", "0.5,x"],
            &["render-transition", "d.pptx", "o.png", "--slide", "1", "--t", "nan"],
            &["render-transition", "d.pptx", "o.png", "--slide", "1", "--t", "0.5", "--width", "0"],
            &["bench", "--sample", "--runs", "0"],
            &["bench", "--sample", "--runs", "1001"],
            &["bench", "--sample", "--runs", "many"],
        ] {
            assert_eq!(kind(args), ErrorKind::ValueValidation, "{args:?}");
        }
        assert!(parse(&["render-transition", "d.pptx", "o.png", "--slide", "1", "--t", "0, 0.5 ,1", "--width", "1.5"]).is_ok());
        assert!(parse(&["bench", "--sample", "--runs", "1000"]).is_ok());
    }

    #[test]
    fn required_options_and_operands() {
        assert_eq!(kind(&["render-transition", "d.pptx", "o.png", "--t", "0.5"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["render-transition", "d.pptx", "o.png", "--slide", "1"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["info"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["convert", "in.pptx"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["describe"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["bench"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["render"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(kind(&["info", "a.pptx", "b.pptx"]), ErrorKind::UnknownArgument);
    }

    #[test]
    fn conflicting_flags_are_usage_errors() {
        assert_eq!(kind(&["run", "--sample", "--in", "d.pptx"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind(&["render", "--sample", "--all", "--slide", "1", "out"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind(&["bench", "d.pptx", "--sample"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind(&["run", "--export-params", "{}"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn render_paths_follow_sample() {
        let Command::Render(r) = parse(&["render", "deck.pptx", "out.png"]).unwrap().command else { panic!("not render") };
        assert_eq!(r.input_and_out().unwrap(), (Some(&PathBuf::from("deck.pptx")), &PathBuf::from("out.png")));
        let Command::Render(r) = parse(&["render", "--sample", "out.png"]).unwrap().command else { panic!("not render") };
        assert_eq!(r.input_and_out().unwrap(), (None, &PathBuf::from("out.png")));
        let Command::Render(r) = parse(&["render", "--sample", "deck.pptx", "out.png"]).unwrap().command else { panic!("not render") };
        assert!(matches!(r.input_and_out(), Err(UsageError::RenderSamplePaths)));
        let Command::Render(r) = parse(&["render", "out.png"]).unwrap().command else { panic!("not render") };
        assert!(matches!(r.input_and_out(), Err(UsageError::RenderPaths)));
        assert_eq!(kind(&["render", "a", "b", "c"]), ErrorKind::TooManyValues);
    }

    #[test]
    fn output_paths_need_a_writable_extension() {
        for args in
            [&["run", "--save", "deck.odp"][..], &["run", "--export=deck.odp"], &["convert", "in.pptx", "out"], &["convert", "in.pptx", "out.svg"]]
        {
            assert_eq!(kind(args), ErrorKind::ValueValidation, "{args:?}");
        }
        assert!(parse(&["convert", "in.pptx", "out.PDF"]).is_ok());
        assert!(parse(&["run", "--save", "deck.deckcraft", "--export", "s.gif"]).is_ok());
    }

    #[test]
    fn export_params_must_be_a_json_object() {
        assert_eq!(kind(&["run", "--export", "a.gif", "--export-params", "[1]"]), ErrorKind::ValueValidation);
        assert_eq!(kind(&["run", "--export", "a.gif", "--export-params", "{"]), ErrorKind::ValueValidation);
        let r = run_args(&["run", "--export", "a.gif", "--export-params", r#"{"width":"small"}"#]);
        assert_eq!(r.export_params.unwrap()["width"], "small");
    }

    #[test]
    fn app_calls_are_checked_before_anything_is_sent() {
        let call = |args: &[&str]| match parse(args).unwrap().command {
            Command::App(a) => a.call().map(|c| format!("{c:?}")),
            other => panic!("{other:?}"),
        };
        assert!(call(&["app", "slide.next"]).unwrap().starts_with("Command { id: \"slide.next\""));
        assert!(call(&["app", "shape.insert", r#"{"preset":"star5"}"#]).is_ok());
        assert!(call(&["app", "--method", "ui.screenshot", "{}"]).unwrap().starts_with("Method"));
        assert!(matches!(call(&["app"]), Err(UsageError::AppNeedsCommand)));
        assert!(matches!(call(&["app", "--method", "m", "{}", "{}"]), Err(UsageError::AppMethodArgs)));
        assert!(matches!(call(&["app", "shape.insert", "{"]), Err(UsageError::AppParams(_))));
        assert_eq!(kind(&["app", "a", "{}", "extra"]), ErrorKind::TooManyValues);
    }

    #[test]
    fn version_and_help_work_everywhere() {
        for args in [&["--version"][..], &["-V"], &["version"], &["run", "--version"], &["mcp", "-V"]] {
            let e = parse(args).map(|c| format!("{c:?}"));
            if args.contains(&"version") {
                assert_eq!(e.unwrap(), "Cli { command: Version }");
            } else {
                assert_eq!(e.unwrap_err().kind(), ErrorKind::DisplayVersion, "{args:?}");
            }
        }
        for sub in ["info", "render", "render-transition", "convert", "run", "commands", "describe", "app", "mcp", "bench", "links"] {
            assert_eq!(kind(&[sub, "--help"]), ErrorKind::DisplayHelp, "{sub}");
            assert_eq!(kind(&[sub, "-h", "out.png"]), ErrorKind::DisplayHelp, "{sub}");
            assert_eq!(kind(&["help", sub]), ErrorKind::DisplayHelp, "{sub}");
        }
    }

    #[test]
    fn double_dash_ends_the_options() {
        let Command::Describe(d) = parse(&["describe", "--", "-x"]).unwrap().command else { panic!("not describe") };
        assert_eq!(d.command, "-x");
        assert_eq!(kind(&["describe", "-x"]), ErrorKind::UnknownArgument);
    }
}
