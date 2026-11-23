//! The commands (`spec/cli.md`): each reads its document, calls the core and
//! writes what the core gives.

use std::io::{self, Write};

use cleanrules::agents::{Catalog, agent_error};
use cleanrules::audit::{AuditLimits, audit_document};
use cleanrules::explain::summary;
use cleanrules::format::{Document, DocumentFormat, write_audit_as};
use cleanrules::json::{Json, Limits, to_string, utf8_text};
use cleanrules::visual::{DiagramFormat, draw_document};

use crate::args::{AuditArgs, DrawArgs, ValidateArgs};
use crate::failure::{Exit, Failure};
use crate::read::{io_failure, load, read};

/// `validate` (`spec/cli.md`, *`validate`*): exit 1 for a document that
/// does not read, whose error is the result.
pub(crate) fn validate(args: &ValidateArgs) -> Result<Exit, Failure> {
    let limits = Limits::default();
    let loaded = load(&args.source, &limits)?;
    if args.format.is_json() {
        // The agents' tool, given the text once, so the result is theirs.
        let result = match utf8_text(&loaded.bytes) {
            Ok(text) => {
                let call = Json::Object(vec![
                    ("document".to_owned(), Json::String(text.to_owned())),
                    (
                        "format".to_owned(),
                        Json::String(loaded.format.as_str().to_owned()),
                    ),
                ]);
                Catalog::new().call("validate", &call).content
            }
            // Text that is not UTF-8, which a tool call cannot carry: the
            // error the reader gives for it, as the tool writes errors.
            Err(error) => Json::Object(vec![
                ("valid".to_owned(), Json::Bool(false)),
                ("error".to_owned(), agent_error(&error)),
            ]),
        };
        write_stdout(&to_string(&result))?;
        let valid = matches!(&result, Json::Object(members)
            if members.iter().any(|(k, v)| k == "valid" && *v == Json::Bool(true)));
        return Ok(if valid { Exit::Clean } else { Exit::Findings });
    }
    let file = shown(&loaded.file);
    let document = match loaded.document(&limits) {
        Ok(document) => document,
        Err(failure) => {
            write_stdout(&format!("{file}: invalid\n{}", failure.render(false)))?;
            return Ok(Exit::Findings);
        }
    };
    let mut text = format!("{file}: valid {}", document.content_key());
    if !matches!(document, Document::Condition(_) | Document::Cases(_)) {
        let ruleset = document.into_ruleset().map_err(|error| Failure::Core {
            file: Some(loaded.file.clone()),
            error,
        })?;
        text.push('\n');
        text.push_str(&summary(&ruleset).to_string());
    }
    write_stdout(&text)?;
    Ok(Exit::Clean)
}

/// `audit` (`spec/cli.md`, *`audit`*).
pub(crate) fn audit(args: &AuditArgs) -> Result<Exit, Failure> {
    let (document, loaded) = read(&args.source, &Limits::default())?;
    let mut limits = AuditLimits::default();
    if let Some(n) = args.max_terms {
        limits = limits.with_max_terms(n);
    }
    if let Some(n) = args.max_steps {
        limits = limits.with_max_steps(n);
    }
    let core = |error| Failure::Core {
        file: Some(loaded.file.clone()),
        error,
    };
    let report = audit_document(&document, &limits)
        .map_err(core)?
        .with_ruleset_digest(args.ruleset_digest);
    let format = if args.format.is_json() {
        DocumentFormat::Json
    } else {
        DocumentFormat::Text
    };
    write_stdout(&write_audit_as(&report, format).map_err(core)?)?;
    // At or above the threshold, as the core orders severities.
    let fails = args
        .fail_on
        .severity()
        .is_some_and(|threshold| report.at_least(threshold));
    Ok(if fails { Exit::Findings } else { Exit::Clean })
}

/// `draw` (`spec/cli.md`, *`draw`*).
pub(crate) fn draw(args: &DrawArgs) -> Result<Exit, Failure> {
    let (document, loaded) = read(&args.source, &Limits::default())?;
    let format = args.format.map_or_else(
        || DiagramFormat::for_document(&document),
        crate::args::Diagram::format,
    );
    let mut limits = Limits::default();
    if let Some(n) = args.max_diagram_size {
        limits.max_diagram_size = n;
    }
    let diagram =
        draw_document(&document, None, format, &limits).map_err(|error| Failure::Core {
            file: Some(loaded.file.clone()),
            error,
        })?;
    match &args.output {
        Some(path) => {
            let file = path.to_string_lossy().into_owned();
            std::fs::write(path, with_line_break(&diagram).as_bytes())
                .map_err(|error| io_failure(&file, &format!("cannot write {file}"), &error))?;
        }
        None => write_stdout(&diagram)?,
    }
    Ok(Exit::Clean)
}

/// Write `text` to standard output, ending with a line break. A reader that
/// stops early (`| head`) is not an error.
fn write_stdout(text: &str) -> Result<(), Failure> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(with_line_break(text).as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            Err(io_failure("-", "cannot write to standard output", &error))
        }
        _ => Ok(()),
    }
}

fn with_line_break(text: &str) -> std::borrow::Cow<'_, str> {
    if text.ends_with('\n') {
        text.into()
    } else {
        format!("{text}\n").into()
    }
}

fn shown(file: &str) -> &str {
    if file == "-" { "<stdin>" } else { file }
}
