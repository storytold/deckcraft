//! `deckcraft-cli`: the process wiring only; see the library for the commands.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::io::Write;
use std::process::ExitCode;

/// The process's stdout without Rust's `Stdout`, which silently drops writes to a descriptor that
/// is not writable (EBADF); here such an error reaches the caller and fails the command.
fn stdout() -> Box<dyn Write> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd as _;
        if let Ok(fd) = std::io::stdout().as_fd().try_clone_to_owned() {
            return Box::new(std::io::LineWriter::new(std::fs::File::from(fd)));
        }
    }
    Box::new(std::io::stdout())
}

fn main() -> ExitCode {
    // `std::env::args` panics on an argument that is not Unicode (a Linux file name in Latin-1)
    let code = deckcraft_cli::run(std::env::args_os(), &mut stdout(), &mut std::io::stderr());
    ExitCode::from(code)
}
