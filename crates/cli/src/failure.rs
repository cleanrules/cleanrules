//! Exit codes and errors (`spec/cli.md`, *Exit codes*, *Errors*).

use std::fmt::Write as _;

use cleanrules::format::write_error;
use cleanrules::json::{Json, to_string};

/// An exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// Done, nothing found at or above `--fail-on`.
    Clean,
    /// Findings at or above `--fail-on`.
    Findings,
    /// A usage error.
    Usage,
    /// The command could not do its work.
    Error,
}

impl Exit {
    pub(crate) fn code(self) -> u8 {
        match self {
            Exit::Clean => 0,
            Exit::Findings => 1,
            Exit::Usage => 2,
            Exit::Error => 3,
        }
    }
}

/// What stopped a command.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The arguments are wrong (`usage_error`).
    Usage { message: String, hint: String },
    /// A file could not be read or written (`io_error`).
    Io {
        file: String,
        message: String,
        hint: Option<String>,
    },
    /// The core's error, about a file when it concerns one.
    Core {
        file: Option<String>,
        error: cleanrules::Error,
    },
}

impl Failure {
    pub(crate) fn exit(&self) -> Exit {
        match self {
            Failure::Usage { .. } => Exit::Usage,
            Failure::Io { .. } | Failure::Core { .. } => Exit::Error,
        }
    }

    /// The error as it is written to standard error, ending with a line break.
    pub(crate) fn render(&self, json: bool) -> String {
        if json {
            let mut members = vec![("error".to_owned(), self.json())];
            if let Some(file) = self.file() {
                members.push(("file".to_owned(), Json::String(file.to_owned())));
            }
            return format!("{}\n", to_string(&Json::Object(members)));
        }
        let mut text = String::new();
        match self {
            Failure::Usage { message, hint } => {
                let _ = writeln!(text, "error[usage_error]: {message}");
                let _ = writeln!(text, "  hint: {hint}");
            }
            Failure::Io {
                file,
                message,
                hint,
            } => {
                let _ = writeln!(text, "error[io_error]: {message}");
                let _ = writeln!(text, "  --> {}", shown(file));
                if let Some(hint) = hint {
                    let _ = writeln!(text, "  hint: {hint}");
                }
            }
            Failure::Core { file, error } => {
                let _ = writeln!(text, "error[{}]: {}", error.code(), error.message());
                let context = error.context();
                let place = match (&context.line, &context.column, &context.pointer) {
                    (Some(line), Some(column), _) => format!(":{line}:{column}"),
                    (_, _, Some(pointer)) => format!(" at {pointer}"),
                    _ => String::new(),
                };
                if let Some(file) = file {
                    let _ = writeln!(text, "  --> {}{place}", shown(file));
                } else if !place.is_empty() {
                    let _ = writeln!(text, "  -->{place}");
                }
                if let Some(hint) = error.hint() {
                    let _ = writeln!(text, "  hint: {hint}");
                }
                if let Some(cause) = std::error::Error::source(error) {
                    let _ = writeln!(text, "  cause: {cause}");
                }
            }
        }
        text
    }

    fn file(&self) -> Option<&str> {
        match self {
            Failure::Usage { .. } => None,
            Failure::Io { file, .. } => Some(file),
            Failure::Core { file, .. } => file.as_deref(),
        }
    }

    /// The error's JSON form (`spec/errors.md`, *JSON form*), with the
    /// command line's own codes for its own errors.
    fn json(&self) -> Json {
        let own = |code: &str, message: &str, hint: Option<&str>| {
            let mut members = vec![
                ("code".to_owned(), Json::String(code.to_owned())),
                ("message".to_owned(), Json::String(message.to_owned())),
            ];
            if let Some(hint) = hint {
                members.push(("hint".to_owned(), Json::String(hint.to_owned())));
            }
            Json::Object(members)
        };
        match self {
            Failure::Usage { message, hint } => own("usage_error", message, Some(hint)),
            Failure::Io { message, hint, .. } => own("io_error", message, hint.as_deref()),
            Failure::Core { error, .. } => write_error(error),
        }
    }
}

/// A `usage_error` for arguments the parser refused: its message, on one
/// line, and the help of the command they name to look at.
pub(crate) fn usage(error: &clap::Error, arguments: &[std::ffi::OsString]) -> Failure {
    let message =
        if error.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
            "no command given: use validate, audit, test or draw".to_owned()
        } else {
            // The first paragraph of the parser's error, without its `error: `.
            let rendered = error.to_string();
            let first = rendered.split("\n\n").next().unwrap_or_default();
            let joined: Vec<&str> = first
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            let joined = joined.join(" ");
            joined.strip_prefix("error: ").unwrap_or(&joined).to_owned()
        };
    let command = arguments.iter().skip(1).find_map(|a| {
        ["validate", "audit", "test", "draw"]
            .into_iter()
            .find(|c| a == c)
    });
    let hint = match command {
        Some(command) => format!("see cleanrules {command} --help"),
        None => "see cleanrules --help".to_owned(),
    };
    Failure::Usage { message, hint }
}

/// A file as the text names it: `<stdin>` for standard input.
fn shown(file: &str) -> &str {
    if file == "-" { "<stdin>" } else { file }
}
