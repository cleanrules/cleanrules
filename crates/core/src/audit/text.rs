//! The text of an audit (`spec/audit.md`, *Text*), as the command line prints
//! it. Fixed by snapshot tests.

use std::fmt;

use super::{Audit, AuditSource, CheckStatus, UndecidedReason};
use crate::format::write_value;
use crate::json::to_string;

impl fmt::Display for Audit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.source {
            AuditSource::RuleSet => "a rule set",
            AuditSource::DecisionTable => "a decision table",
        };
        let count = self.rules.len();
        write!(
            f,
            "Audit of {what}: {count} {}, hit policy {}",
            if count == 1 { "rule" } else { "rules" },
            self.hit_policy.as_str()
        )?;
        if let Some(aggregate) = self.aggregate {
            write!(f, ", aggregate {}", aggregate.as_str())?;
        }
        write!(f, ", logic {}", self.logic.as_str())?;
        if self.strict {
            f.write_str(", strict")?;
        }
        writeln!(f)?;
        if self.diagnostics.is_empty() {
            writeln!(f, "No findings.")?;
        }
        for d in &self.diagnostics {
            write!(f, "{:<8} {}  ", d.severity.as_str(), d.code.as_str())?;
            if !d.rules.is_empty() {
                write!(f, "{}: ", d.rules.join(", "))?;
            }
            writeln!(f, "{}", d.message)?;
            if let Some(pointer) = &d.pointer {
                writeln!(f, "           at {pointer}")?;
            }
            if let Some(path) = &d.path {
                writeln!(f, "           field {}", crate::text::path(path))?;
            }
            if !d.groups.is_empty() {
                let groups: Vec<&str> = d.groups.iter().map(|g| g.as_str()).collect();
                writeln!(f, "           groups {}", groups.join(", "))?;
            }
            if let Some(witness) = &d.witness {
                writeln!(
                    f,
                    "           witness: {}",
                    to_string(&write_value(witness))
                )?;
            }
        }
        let checks: Vec<String> = self
            .checks
            .iter()
            .map(|c| {
                let status = match c.status {
                    CheckStatus::NotApplicable => "not applicable",
                    other => other.as_str(),
                };
                format!("{} {status}", c.check.as_str())
            })
            .collect();
        writeln!(f, "Checks: {}", checks.join(", "))?;
        if !self.not_modelled.is_empty() {
            writeln!(f, "Not modelled ({}):", self.not_modelled.len())?;
            for n in &self.not_modelled {
                write!(f, "  {}  {} at {}", n.rule, n.op, n.pointer)?;
                if let Some(limit) = n.limit {
                    write!(f, " ({})", limit.as_str())?;
                }
                writeln!(f)?;
            }
        }
        if !self.undecided.is_empty() {
            writeln!(f, "Undecided ({}):", self.undecided.len())?;
            for u in &self.undecided {
                write!(f, "  {}", u.check.as_str())?;
                if !u.rules.is_empty() {
                    write!(f, "  {}", u.rules.join(", "))?;
                }
                write!(f, "  {}", u.reason.as_str())?;
                if let UndecidedReason::Limit(limit) = u.reason {
                    write!(f, " ({})", limit.as_str())?;
                }
                if let Some(remaining) = u.remaining {
                    write!(f, ", {remaining} not reached")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
