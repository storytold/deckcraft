//! Headless DeckCraft: `deckcraft-cli help` explains each subcommand.
//!
//! Follows RFC-0012 (command-line conventions): the command line is the clap derive types in
//! [`cli`], errors are the thiserror enums in [`error`], and `main` only wires the process to
//! [`run`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cli;
mod commands;
pub mod error;
mod io;

use std::ffi::OsString;
use std::io::Write;

use cli::Cli;
use error::CliError;
use io::Io;

/// Run the CLI with `args` (the program name first): results go to `out`, errors and progress to
/// `err`. Returns the exit status: 0 success, 1 a command failed, 2 usage error.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut io = Io::new(out, err);
    let result = match Cli::parse_args(args) {
        Ok(cli) => commands::dispatch(cli.command, &mut io),
        // help and version go to stdout with status 0; usage errors to stderr with status 2
        Err(e) if !e.use_stderr() => {
            io.text(&e.render().to_string());
            Ok(())
        }
        Err(e) => {
            io.diag_text(&e.render().to_string());
            return u8::try_from(e.exit_code()).unwrap_or(2);
        }
    };
    let mut code = 0;
    if let Err(e) = result {
        code = e.exit_code();
        report(&mut io, &e);
    }
    if let Some(e) = io.take_stdout_error() {
        report(&mut io, &CliError::from(e));
        code = 1;
    }
    code
}

/// `error: <message>` on stderr; a usage error also points at `--help`, as clap's own do.
fn report(io: &mut Io<'_>, e: &CliError) {
    io.diag(format_args!("error: {e}"));
    if matches!(e, CliError::Usage(_)) {
        io.diag(format_args!("\nFor more information, try '--help'."));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn run_args(args: &[&str]) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(std::iter::once("deckcraft-cli").chain(args.iter().copied()), &mut out, &mut err);
        (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn help_goes_to_stdout_and_usage_errors_to_stderr() {
        let (code, out, err) = run_args(&["--help"]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.contains("render-transition") && out.contains("Exit status"), "{out}");
        let (code, out, err) = run_args(&["frobnicate"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.starts_with("error:"), "{err}");
        let (code, _, err) = run_args(&["run", "--cmd"]);
        assert_eq!(code, 2, "{err}");
        let (code, out, _) = run_args(&[]);
        assert_eq!((code, out.as_str()), (2, ""));
    }

    #[test]
    fn version_goes_to_stdout() {
        for args in [&["--version"][..], &["-V"], &["version"]] {
            let (code, out, err) = run_args(args);
            assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
            assert_eq!(out, format!("deckcraft-cli {}\n", env!("CARGO_PKG_VERSION")), "{args:?}");
        }
    }

    #[test]
    fn help_and_version_report_a_failing_stdout() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::StorageFull.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for args in [&["--help"][..], &["--version"], &["links"]] {
            let mut err = Vec::new();
            let code = run(std::iter::once("deckcraft-cli").chain(args.iter().copied()), &mut Full, &mut err);
            assert_eq!(code, 1, "{args:?}");
            assert!(String::from_utf8_lossy(&err).contains("can't write to stdout"), "{args:?}");
        }
    }

    #[test]
    fn a_closed_stdout_is_not_an_error() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut err = Vec::new();
        assert_eq!(run(["deckcraft-cli", "links"], &mut Closed, &mut err), 0);
        assert!(err.is_empty());
    }

    #[test]
    fn a_bad_option_writes_nothing() {
        let dir = temp_dir("unknown-option");
        let out = dir.join("out.png");
        for flag in ["--bogus", "-x"] {
            let (code, _, err) = run_args(&["render", "--sample", flag, &out.to_string_lossy()]);
            assert_eq!(code, 2, "{flag}: {err}");
            assert!(err.contains(flag), "{err}");
            assert!(!out.exists(), "{flag}: rendered anyway");
        }
        assert!(!std::path::Path::new("--bogus").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn render_paths_are_checked_before_any_work() {
        let (code, out, err) = run_args(&["render", "--sample", "deck.pptx", "out.png"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.starts_with("error: render --sample takes only OUT") && err.contains("try '--help'"), "{err}");
        let (code, _, err) = run_args(&["render", "out.png"]);
        assert_eq!(code, 2, "{err}");
    }

    #[test]
    fn a_missing_slide_is_a_failure_with_the_numbering() {
        let dir = temp_dir("missing-slide");
        let (code, _, err) = run_args(&["render", "--sample", "--slide", "99", &dir.join("o.png").to_string_lossy()]);
        assert_eq!(code, 1, "{err}");
        assert!(err.starts_with("error: no slide index 99") && err.contains("slide-01.png"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `render --transparent` leaves the slide background out (#784): the corner is transparent.
    #[test]
    fn render_transparent_leaves_the_background_out() {
        let dir = temp_dir("transparent");
        let alpha = |path: &std::path::Path| deckcraft_engine::render::decode_image(&std::fs::read(path).unwrap()).unwrap().get_pixel(1, 1).0[3];
        let opaque = dir.join("opaque.png");
        assert_eq!(run_args(&["render", "--sample", "--scale", "0.25", &opaque.to_string_lossy()]).0, 0);
        assert_eq!(alpha(&opaque), 255);
        let clear = dir.join("clear.png");
        assert_eq!(run_args(&["render", "--sample", "--transparent", "--scale", "0.25", &clear.to_string_lossy()]).0, 0);
        assert_eq!(alpha(&clear), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `run --export OUT.gif --export-params JSON` and `convert IN OUT.gif` write animated GIFs.
    #[test]
    fn exports_an_animated_gif() {
        let dir = temp_dir("gif");
        let gif = dir.join("sample.gif").to_string_lossy().to_string();
        let deck = dir.join("sample.deckcraft").to_string_lossy().to_string();
        let params = r#"{"width": "small", "secondsPerSlide": 2, "loop": false, "slides": [0, 1]}"#;
        let (code, _, err) = run_args(&["run", "--sample", "--save", &deck, "--export", &gif, "--export-params", params]);
        assert_eq!(code, 0, "{err}");
        let bytes = std::fs::read(&gif).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 640, "small = 640 px wide");
        assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "play once: no loop block");

        let gif2 = dir.join("converted.gif").to_string_lossy().to_string();
        assert_eq!(run_args(&["convert", &deck, &gif2]).0, 0);
        let bytes = std::fs::read(&gif2).unwrap();
        assert!(bytes.starts_with(b"GIF89a") && bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "loops forever by default");

        // Bad options are errors, not panics.
        let bad = dir.join("bad.gif").to_string_lossy().to_string();
        assert_eq!(run_args(&["run", "--sample", "--export", &bad, "--export-params", r#"{"width": 1e9}"#]).0, 1);
        assert_eq!(run_args(&["run", "--sample", "--export", &bad, "--export-params", "[1]"]).0, 2);
        assert_eq!(run_args(&["run", "--sample", "--export-params", "{}"]).0, 2);
        assert_eq!(run_args(&["run", "--sample", "--export-params", r#"{"width": 64}"#]).0, 2);
        assert!(!std::path::Path::new(&bad).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod slide_index_tests {
    use crate::commands::{missing_slide, slide_indices};

    #[test]
    fn slide_numbers_start_at_zero_and_files_at_one() {
        assert_eq!(slide_indices(8, false, None).ok().as_deref(), Some(&[0][..]));
        assert_eq!(slide_indices(8, false, Some(0)).ok().as_deref(), Some(&[0][..]));
        assert_eq!(slide_indices(8, false, Some(7)).ok().as_deref(), Some(&[7][..]));
        assert_eq!(slide_indices(8, true, None).ok().as_deref(), Some(&[0, 1, 2, 3, 4, 5, 6, 7][..]));
        let err = slide_indices(8, false, Some(8)).err().map(|e| e.to_string()).unwrap_or_default();
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
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use crate::commands::frame_paths;

    #[test]
    fn single_frame_uses_out_as_is() {
        assert_eq!(frame_paths(Path::new("a/b.png"), &[0.5]).unwrap(), [PathBuf::from("a/b.png")]);
    }

    #[test]
    fn frames_get_distinct_names() {
        let paths = frame_paths(Path::new("f.png"), &[0.0, 0.25, 1.0]).unwrap();
        assert_eq!(paths, [PathBuf::from("f-t0.000.png"), "f-t0.250.png".into(), "f-t1.000.png".into()]);
    }

    #[test]
    fn values_rounding_to_one_name_are_rejected() {
        assert!(frame_paths(Path::new("f.png"), &[0.1001, 0.1002]).is_err());
        assert!(frame_paths(Path::new("f.png"), &[0.5, 0.5]).is_err());
    }
}

#[cfg(test)]
mod timing_tests {
    use crate::commands::summarize_ms;

    #[test]
    fn timing_summary_keeps_median_and_max_from_one_population() {
        let slide_medians = [8.99, 16.565];
        let whole_deck = summarize_ms(vec![17.0, 18.0, 16.0]);
        assert_eq!(whole_deck, (17.0, 18.0));
        assert!(whole_deck.1 >= whole_deck.0);
        assert!(slide_medians.iter().sum::<f64>() > whole_deck.1, "fixture must expose summing independent slide medians");
        assert_eq!(summarize_ms(vec![]), (0.0, 0.0));
    }
}
