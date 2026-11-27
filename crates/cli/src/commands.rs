//! The commands (`spec/cli.md`): each reads its document, calls the core and
//! writes what the core gives.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cleanrules::agents::{Catalog, agent_error};
use cleanrules::audit::{AuditLimits, audit_document};
use cleanrules::bundle::{Bundle, EngineLimits};
use cleanrules::cases::{CaseFile, CaseFileBytes, Runner, TestRunLimits};
use cleanrules::explain::summary;
use cleanrules::format::{Document, DocumentFormat, size_limit, write_audit_as, write_test_run_as};
use cleanrules::json::{Json, Limits, to_string, utf8_text};
use cleanrules::visual::{DiagramFormat, draw_document};

use crate::args::{AuditArgs, DrawArgs, Source, Syntax, TestArgs, ValidateArgs};
use crate::failure::{Exit, Failure};
use crate::read::{bounded, io_failure, is_stdin, load, read, syntax_of};

/// `validate` (`spec/cli.md`, *`validate`*): exit 1 for a document that
/// does not read, whose error is the result.
pub(crate) fn validate(args: &ValidateArgs) -> Result<Exit, Failure> {
    let limits = Limits::default();
    let loaded = load(&args.source, &limits)?;
    if args.format.is_json() {
        // The agents' tool, given the text once, so the result is theirs.
        let result = match loaded
            .too_large(&limits)
            .map_or_else(|| utf8_text(&loaded.bytes), Err)
        {
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
            // Text beyond the size limit, which was not loaded whole, or
            // that is not UTF-8, which a tool call cannot carry: the error
            // the reader gives for it, as the tool writes errors.
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

/// `test` (`spec/cli.md`, *`test`*): the core runs the cases and writes the
/// test run; exit 0 when every case ran and passed, 1 when one failed or
/// erred, when the run is partial or when it has no cases.
pub(crate) fn test(args: &TestArgs) -> Result<Exit, Failure> {
    let usage = |message: String| Failure::Usage {
        message,
        hint: "see cleanrules test --help".to_owned(),
    };
    // Usage first, before any file is read.
    let stdin = std::iter::once(&args.rules)
        .chain(&args.cases)
        .filter(|path| is_stdin(path))
        .count();
    if stdin > 1 {
        return Err(usage(
            "standard input (-) is given more than once: it holds one document".to_owned(),
        ));
    }
    let formats = args
        .cases
        .iter()
        .map(|path| case_syntax(path).ok_or_else(|| usage(case_suffix(path))))
        .collect::<Result<Vec<_>, _>>()?;
    let limits = Limits::default();
    let source = Source {
        file: args.rules.clone(),
        syntax: args.syntax,
    };
    let (document, loaded) = read(&source, &limits)?;
    let core = |error| Failure::Core {
        file: Some(loaded.file.clone()),
        error,
    };
    // A rule gives the rule set of that rule; a condition and a case file
    // give the core's error (`spec/json-format.md`, *A rule set from a
    // document*).
    let rules = match document {
        Document::DecisionTable(table) => Rules::Table(table),
        Document::Bundle(bundle) => Rules::Bundle(bundle),
        other => Rules::RuleSet(other.into_ruleset().map_err(core)?),
    };
    let own_cases = args.cases.is_empty();
    let runner = match &rules {
        Rules::Bundle(bundle) if own_cases => Runner::for_bundle(bundle),
        Rules::Bundle(bundle) => Runner::for_bundle_rules(bundle),
        Rules::Table(table) => Runner::for_table(table),
        Rules::RuleSet(ruleset) => Runner::new(ruleset),
    };
    let files = match (&rules, own_cases) {
        (Rules::Bundle(bundle), true) => bundle_case_files(bundle, &args.rules)?,
        (_, true) => {
            return Err(usage(format!(
                "no case files given to run against {}: give them after the rules, or give a \
                 bundle that lists its own",
                shown(&loaded.file)
            )));
        }
        (_, false) => args
            .cases
            .iter()
            .zip(formats)
            .map(|(path, format)| case_file(path, format, &limits))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mut run_limits = TestRunLimits::default();
    if let Some(n) = args.max_results {
        run_limits = run_limits.with_max_results(n);
    }
    if let Some(n) = args.max_steps {
        run_limits = run_limits.with_max_steps(n);
    }
    if let Some(n) = args.max_explained {
        run_limits = run_limits.with_max_explained(n);
    }
    let mut runner = runner.with_limits(run_limits);
    if !(own_cases && matches!(rules, Rules::Bundle(_))) {
        // Case files given on the command line are read with the default
        // limits, which the test run records; a bundle's, with its own.
        runner = runner.with_engine_limits(EngineLimits::of(&limits).map_err(core)?);
    }
    let run = runner.run(&files).with_ruleset_digest(args.ruleset_digest);
    let format = if args.format.is_json() {
        DocumentFormat::Json
    } else {
        DocumentFormat::Text
    };
    write_stdout(&write_test_run_as(&run, format).map_err(core)?)?;
    if run.summary.total() == 0 && !run.summary.is_partial() {
        note(
            "no_cases",
            "no cases ran, so nothing was tested: the run exits with 1",
            args.format.is_json(),
        );
    }
    // A run passes when every case ran and passed, and there was one.
    Ok(if run.all_passed() && run.summary.total() > 0 {
        Exit::Clean
    } else {
        Exit::Findings
    })
}

/// The rules `test` runs cases against.
enum Rules {
    RuleSet(cleanrules::rules::RuleSet),
    Table(cleanrules::table::DecisionTable),
    Bundle(Box<Bundle>),
}

/// The syntax of a case file given on the command line: JSON or YAML, by its
/// suffix, and JSON for standard input.
fn case_syntax(path: &Path) -> Option<DocumentFormat> {
    if is_stdin(path) {
        return Some(DocumentFormat::Json);
    }
    match syntax_of(path)? {
        Syntax::Json => Some(DocumentFormat::Json),
        Syntax::Yaml => Some(DocumentFormat::Yaml),
        Syntax::Text | Syntax::Csv => None,
    }
}

fn case_suffix(path: &Path) -> String {
    format!(
        "cannot read {} as a case file: case files are .json, .yaml or .yml",
        path.to_string_lossy()
    )
}

/// A case file given on the command line, read as `format` with `limits`:
/// with its path as given, written with `/` and without a leading `./`
/// (`spec/cli.md`, *`test`*), or without one from standard input. A file
/// that cannot be opened stops the command; one that does not read as a
/// case file is its own result.
fn case_file(path: &Path, format: DocumentFormat, limits: &Limits) -> Result<CaseFile, Failure> {
    let file = path.to_string_lossy().into_owned();
    let stdin = is_stdin(path);
    let (bytes, size) = bounded(path, size_limit(format, limits)).map_err(|error| {
        let shown = if stdin { "standard input" } else { &file };
        io_failure(&file, &format!("cannot read {shown}"), &error)
    })?;
    let recorded = recorded_path(&file);
    let name = (!stdin).then_some(recorded.as_str());
    Ok(if bytes.len() > size_limit(format, limits) {
        CaseFile::too_large(name, format, size, limits)
    } else {
        CaseFile::read_as(name, &bytes, format, limits)
    })
}

/// The path a run records for a case file given on the command line: with
/// `/` between its segments on every system, and without a leading `./`, so
/// that runs line up across systems and invocations.
fn recorded_path(file: &str) -> String {
    let mut path = if cfg!(windows) {
        file.replace('\\', "/")
    } else {
        file.to_owned()
    };
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_owned();
    }
    path
}

/// The case files a bundle lists, at their paths relative to the bundle's
/// folder (the current one for standard input), loaded up to the bound the
/// core gives and checked by the core against their digests
/// (`CaseFile::bundle_files_bounded`).
fn bundle_case_files(bundle: &Bundle, file: &Path) -> Result<Vec<CaseFile>, Failure> {
    let folder = if is_stdin(file) {
        PathBuf::new()
    } else {
        file.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    let shown_bundle = shown(&file.to_string_lossy()).to_owned();
    CaseFile::bundle_files_bounded(bundle, |case_ref, bound| {
        let path = folder.join(case_ref.path());
        let (bytes, size) = bounded(&path, bound).map_err(|error| {
            let shown = path.to_string_lossy().into_owned();
            let mut failure = io_failure(&shown, &format!("cannot read {shown}"), &error);
            if let Failure::Io { hint, .. } = &mut failure
                && error.kind() == io::ErrorKind::NotFound
            {
                let index = bundle
                    .cases()
                    .iter()
                    .position(|c| c.path() == case_ref.path())
                    .unwrap_or_default();
                *hint = Some(format!(
                    "the bundle {shown_bundle} lists it at /bundle/cases/{index}/path, relative to \
                     the bundle's folder, or the current folder when the bundle comes from \
                     standard input"
                ));
            }
            failure
        })?;
        Ok(if bytes.len() > bound {
            CaseFileBytes::TooLarge { size }
        } else {
            CaseFileBytes::Whole(bytes)
        })
    })
}

/// Write a note to standard error (`spec/cli.md`, *Errors*): in text, or as
/// one line of JSON.
fn note(code: &str, message: &str, json: bool) {
    let line = if json {
        let note = Json::Object(vec![
            ("code".to_owned(), Json::String(code.to_owned())),
            ("message".to_owned(), Json::String(message.to_owned())),
        ]);
        to_string(&Json::Object(vec![("note".to_owned(), note)]))
    } else {
        format!("note[{code}]: {message}")
    };
    // Nothing is left to report a failure to write a note to.
    let _ = writeln!(io::stderr().lock(), "{line}");
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
