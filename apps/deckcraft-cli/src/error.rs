//! Errors of the CLI (RFC-0012): usage errors exit 2, failures exit 1.

use std::path::PathBuf;

use deckcraft_engine::EngineError;
use thiserror::Error;

/// An output path whose extension is not a format the CLI writes.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("{path}: unsupported output format {ext} (supported: .{supported})", path = path.display())]
    Unsupported { path: PathBuf, ext: String, supported: String },
}

/// An `--export-params` value that is not one JSON object.
#[derive(Debug, Error)]
pub enum JsonArgError {
    #[error("bad JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("must be a JSON object")]
    NotAnObject,
}

/// The command line was wrong in a way the parser could not see: exit status 2.
#[derive(Debug, Error)]
pub enum UsageError {
    #[error("render needs FILE and OUT (or --sample and OUT)")]
    RenderPaths,
    #[error("render --sample takes only OUT, not a FILE as well")]
    RenderSamplePaths,
    #[error("app needs a COMMAND or --method")]
    AppNeedsCommand,
    #[error("app --method takes only PARAMS after it")]
    AppMethodArgs,
    #[error("app PARAMS: bad JSON: {0}")]
    AppParams(#[source] serde_json::Error),
}

/// What is wrong with one `run` command.
#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{id}: bad JSON: {source}")]
    BadJson { id: String, source: serde_json::Error },
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// Why a line of a command file could not be read.
#[derive(Debug, Error)]
pub enum LineError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

/// A command that ran and failed: exit status 1.
#[derive(Debug, Error)]
pub enum Failure {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error(transparent)]
    Format(#[from] FormatError),
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{}:{line}: {source}", path.display())]
    CommandFileLine { path: PathBuf, line: usize, source: LineError },
    #[error("{}:{line}: {source}", path.display())]
    CommandFileCommand { path: PathBuf, line: usize, source: CommandError },
    #[error("{}: too many lines", path.display())]
    TooManyLines { path: PathBuf },
    #[error("{}: not valid Unicode, which the engine needs for paths", path.display())]
    NotUnicode { path: PathBuf },
    #[error("{}", crate::commands::missing_slide(*index, *count))]
    MissingSlide { index: usize, count: usize },
    #[error("no slide {index} (the deck has {count})")]
    NoTransitionSlide { index: usize, count: usize },
    #[error("--t values round to the same three decimals: more than one frame would be written to {}", path.display())]
    FrameCollision { path: PathBuf },
    #[error("no DeckCraft app on port {port} ({source}); start it with `deckcraft --control {port}`")]
    NoApp { port: String, source: std::io::Error },
    #[error("can't reach the app at {addr}: {source}")]
    NoAppAt { addr: String, source: std::io::Error },
    /// The error text of the control channel, which the `deckcraft-mcp` crate keeps as a string.
    #[error("{0}")]
    Control(String),
    /// A root the automation flags name is not usable (the engine keeps its reason as a string).
    #[error("{0}")]
    Roots(String),
    #[error(
        "--automation-read-root and --automation-write-root confine the in-process server; a running app is not confined by them, so drop --connect (or use --connect without the roots)"
    )]
    RootsWithConnect,
    #[error("can't serve MCP: {0}")]
    Serve(#[source] std::io::Error),
    #[error("can't write to stdout: {0}")]
    Stdout(#[source] std::io::Error),
}

/// Everything a subcommand can end in besides success.
#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Usage(#[from] UsageError),
    #[error(transparent)]
    Failure(#[from] Failure),
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Usage(_) => 2,
            CliError::Failure(_) => 1,
        }
    }
}

impl From<EngineError> for CliError {
    fn from(e: EngineError) -> Self {
        CliError::Failure(e.into())
    }
}

impl From<CommandError> for CliError {
    fn from(e: CommandError) -> Self {
        CliError::Failure(e.into())
    }
}
