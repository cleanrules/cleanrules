//! `cleanrules`, the command line of the cleanrules engine (`spec/cli.md`).
//!
//! It reads documents from files and calls the core: every report, diagram
//! and error it prints is the core's, and it never evaluates, audits or draws
//! on its own (`CLAUDE.md`, invariant 2). Its exit codes are for CI: `0`
//! clean, `1` findings at or above `--fail-on`, `2` usage errors, `3` errors
//! that stop the command.

mod args;
mod commands;
mod failure;
mod read;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

use args::{Cli, Command};
use failure::Exit;

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().collect();
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                // Help and version go to standard output, and exit with 0.
                return if error.print().is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(Exit::Error.code())
                };
            }
            let failure = failure::usage(&error, &arguments);
            return report(&failure, wants_json(&arguments));
        }
    };
    let (json_errors, outcome) = match &cli.command {
        Command::Validate(args) => (args.format.is_json(), commands::validate(args)),
        Command::Audit(args) => (args.format.is_json(), commands::audit(args)),
        Command::Draw(args) => (false, commands::draw(args)),
    };
    match outcome {
        Ok(exit) => ExitCode::from(exit.code()),
        Err(failure) => report(&failure, json_errors),
    }
}

/// Write a failure to standard error, and its exit code.
fn report(failure: &failure::Failure, json: bool) -> ExitCode {
    let mut stderr = std::io::stderr().lock();
    // Nothing is left to report a failure to write an error to.
    let _ = stderr.write_all(failure.render(json).as_bytes());
    ExitCode::from(failure.exit().code())
}

/// Whether the arguments ask for JSON (`--format json` or `--format=json`),
/// even when they do not parse.
fn wants_json(arguments: &[OsString]) -> bool {
    arguments
        .windows(2)
        .any(|pair| pair[0] == "--format" && pair[1] == "json")
        || arguments.iter().any(|a| a == "--format=json")
}
