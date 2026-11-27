//! The arguments of every command (`spec/cli.md`).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use cleanrules::audit::Severity;
use cleanrules::format::DocumentFormat;
use cleanrules::visual::DiagramFormat;

const EXIT_CODES: &str = "\
Exit codes:
  0  done, nothing found at or above --fail-on; for test, every case ran and
     passed
  1  findings at or above --fail-on (audit), a document that does not read
     (validate), or a case that failed or erred, a partial run or no cases
     (test)
  2  usage error
  3  a file could not be read or written, a document did not read, or the
     operation is not supported for it

Specification: https://github.com/cleanrules/cleanrules/blob/main/spec/cli.md";

/// Audit, test, validate and draw rule sets and decision tables.
#[derive(Debug, Parser)]
#[command(name = "cleanrules", bin_name = "cleanrules", version, after_help = EXIT_CODES)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Read a document and say what it holds
    Validate(ValidateArgs),
    /// Audit the rules of a rule set, decision table, bundle or rule
    Audit(AuditArgs),
    /// Run test cases against rules
    Test(TestArgs),
    /// Draw the diagram of a document
    Draw(DrawArgs),
}

/// The document a command reads.
#[derive(Debug, Args)]
pub(crate) struct Source {
    /// The document's file, or - for standard input
    #[arg(value_name = "FILE")]
    pub(crate) file: PathBuf,
    /// Its syntax; by default, from the file's suffix (.json, .yaml, .yml,
    /// .rules, .csv), and json for standard input
    #[arg(long, value_enum, value_name = "SYNTAX")]
    pub(crate) syntax: Option<Syntax>,
}

/// A syntax documents are read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Syntax {
    /// JSON
    Json,
    /// YAML
    Yaml,
    /// The rule syntax
    Text,
    /// CSV, for decision tables
    Csv,
}

impl Syntax {
    pub(crate) fn format(self) -> DocumentFormat {
        match self {
            Syntax::Json => DocumentFormat::Json,
            Syntax::Yaml => DocumentFormat::Yaml,
            Syntax::Text => DocumentFormat::Text,
            Syntax::Csv => DocumentFormat::Csv,
        }
    }
}

/// What a report is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ReportFormat {
    /// Text for people
    Text,
    /// The JSON document, on one line
    Json,
}

impl ReportFormat {
    pub(crate) fn is_json(self) -> bool {
        self == ReportFormat::Json
    }
}

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    #[command(flatten)]
    pub(crate) source: Source,
    /// What to write
    #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
    pub(crate) format: ReportFormat,
}

/// The severity at and above which `audit` exits with 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum FailOn {
    /// Errors only
    Error,
    /// Warnings and errors
    Warning,
    /// Any diagnostic
    Info,
    /// Never: exit with 0 whatever the audit finds
    Never,
}

impl FailOn {
    /// The least serious severity that fails, if any.
    pub(crate) fn severity(self) -> Option<Severity> {
        self.to_possible_value()
            .and_then(|value| Severity::from_name(value.get_name()))
    }
}

#[derive(Debug, Args)]
#[command(after_help = EXIT_CODES)]
pub(crate) struct AuditArgs {
    #[command(flatten)]
    pub(crate) source: Source,
    /// What to write
    #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
    pub(crate) format: ReportFormat,
    /// Exit with 1 when a diagnostic has this severity or a more serious one
    #[arg(long, value_enum, value_name = "SEVERITY", default_value_t = FailOn::Error)]
    pub(crate) fail_on: FailOn,
    #[arg(
        long,
        value_name = "N",
        help = "The most boxes of one node's description (audit limit max_terms) [default: 256]"
    )]
    pub(crate) max_terms: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        help = "The most steps of the whole audit (audit limit max_steps) [default: 1000000]"
    )]
    pub(crate) max_steps: Option<usize>,
    /// Carry the rule set digest of the rules audited in the JSON
    #[arg(long)]
    pub(crate) ruleset_digest: bool,
}

#[derive(Debug, Args)]
#[command(after_help = EXIT_CODES)]
pub(crate) struct TestArgs {
    /// The rules: a rule set, decision table, bundle or rule, or - for
    /// standard input
    #[arg(value_name = "RULES")]
    pub(crate) rules: PathBuf,
    /// Case files (.json, .yaml, .yml), or - for standard input; without
    /// them, a bundle runs the case files it lists
    #[arg(value_name = "CASES")]
    pub(crate) cases: Vec<PathBuf>,
    /// The syntax of the rules; by default, from their file's suffix (.json,
    /// .yaml, .yml, .rules, .csv), and json for standard input
    #[arg(long, value_enum, value_name = "SYNTAX")]
    pub(crate) syntax: Option<Syntax>,
    /// What to write
    #[arg(long, value_enum, default_value_t = ReportFormat::Text)]
    pub(crate) format: ReportFormat,
    #[arg(
        long,
        value_name = "N",
        help = "Stop the run after this many results (run limit max_results)"
    )]
    pub(crate) max_results: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        help = "Stop the run before it takes more steps, one per rule and case (run limit \
                max_steps)"
    )]
    pub(crate) max_steps: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        help = "Explain at most this many failed cases (run limit max_explained) [default: 5]"
    )]
    pub(crate) max_explained: Option<usize>,
    /// Carry the rule set digest of the rules tested in the JSON
    #[arg(long)]
    pub(crate) ruleset_digest: bool,
}

/// A diagram format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Diagram {
    /// A Mermaid flowchart
    Mermaid,
    /// A Graphviz graph
    Dot,
    /// A standalone HTML document
    Html,
    /// An HTML fragment, to embed in a page
    #[value(name = "html_fragment")]
    HtmlFragment,
}

impl Diagram {
    pub(crate) fn format(self) -> DiagramFormat {
        match self {
            Diagram::Mermaid => DiagramFormat::Mermaid,
            Diagram::Dot => DiagramFormat::Dot,
            Diagram::Html => DiagramFormat::Html,
            Diagram::HtmlFragment => DiagramFormat::HtmlFragment,
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct DrawArgs {
    #[command(flatten)]
    pub(crate) source: Source,
    /// The diagram format [default: mermaid, or html for a decision table]
    #[arg(long, value_enum)]
    pub(crate) format: Option<Diagram>,
    /// Write the diagram to this file instead of standard output
    #[arg(long, value_name = "PATH")]
    pub(crate) output: Option<PathBuf>,
    /// The most elements of the diagram [default: 20000]
    #[arg(long, value_name = "N")]
    pub(crate) max_diagram_size: Option<usize>,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use cleanrules::audit::AuditLimits;
    use cleanrules::cases::TestRunLimits;
    use cleanrules::json::Limits;

    use super::Cli;

    /// The defaults the help gives are the core's.
    #[test]
    fn help_gives_the_core_defaults() {
        let cli = Cli::command();
        let help = |command: &str, id: &str| {
            cli.find_subcommand(command)
                .and_then(|c| c.get_arguments().find(|a| a.get_id() == id))
                .and_then(|a| a.get_help())
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        let audit = AuditLimits::default();
        let draw = Limits::default();
        let run = TestRunLimits::default();
        for (command, id, default) in [
            ("audit", "max_terms", audit.max_terms),
            ("audit", "max_steps", audit.max_steps),
            ("draw", "max_diagram_size", draw.max_diagram_size),
            ("test", "max_explained", run.max_explained),
        ] {
            let text = help(command, id);
            assert!(
                text.ends_with(&format!("[default: {default}]")),
                "{id}: {text}"
            );
        }
    }
}
