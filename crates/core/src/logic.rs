//! Logics: how truth values combine (`spec/ast.md`, *Truth values*).
//!
//! The evaluator never combines truth values with `&&`, `||` or `!`: everything
//! goes through a [`Logic`] (`CLAUDE.md`, invariant 4), so the fuzzy logics of
//! phase J plug in without touching it.

use std::fmt::Debug;

use crate::error::{Error, ErrorCode, Result};
use crate::value::{Group, Value};

/// A logic: its truth values, connectives and activation test.
pub trait Logic: Debug + Send + Sync {
    /// The logic's truth values.
    type Truth: Copy + Debug + PartialEq + Send + Sync;

    /// The registered name, as written in rule sets (`"boolean"`).
    fn name(&self) -> &'static str;

    /// The truth value of a crisp result.
    fn truth(&self, value: bool) -> Self::Truth;

    /// The unknown truth value.
    fn unknown(&self) -> Self::Truth;

    /// Whether a truth value is unknown (then it carries a reason).
    fn is_unknown(&self, truth: Self::Truth) -> bool;

    /// Conjunction.
    fn and(&self, a: Self::Truth, b: Self::Truth) -> Self::Truth;

    /// Disjunction.
    fn or(&self, a: Self::Truth, b: Self::Truth) -> Self::Truth;

    /// Negation.
    fn not(&self, a: Self::Truth) -> Self::Truth;

    /// Conjunction of any number of truth values; true for none.
    fn all(&self, values: &[Self::Truth]) -> Self::Truth {
        values
            .iter()
            .fold(self.truth(true), |acc, &v| self.and(acc, v))
    }

    /// Disjunction of any number of truth values; false for none.
    fn any(&self, values: &[Self::Truth]) -> Self::Truth {
        values
            .iter()
            .fold(self.truth(false), |acc, &v| self.or(acc, v))
    }

    /// A value used directly as a condition: `Ok(None)` for null (unknown), or
    /// the group that makes it a type mismatch.
    ///
    /// # Errors
    ///
    /// The value's group, when the logic cannot read it as a truth value.
    fn value_truth(&self, value: &Value) -> std::result::Result<Option<Self::Truth>, Group>;

    /// The activation test: whether a rule with this truth value fires.
    fn fires(&self, truth: Self::Truth) -> bool;
}

/// A Kleene truth value, ordered `False < Unknown < True`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Truth {
    /// False.
    False,
    /// Unknown: a missing field, a null, or a type mismatch.
    Unknown,
    /// True.
    True,
}

/// Kleene's three-valued logic, registered as `"boolean"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kleene;

impl Logic for Kleene {
    type Truth = Truth;

    fn name(&self) -> &'static str {
        "boolean"
    }

    fn truth(&self, value: bool) -> Truth {
        if value { Truth::True } else { Truth::False }
    }

    fn unknown(&self) -> Truth {
        Truth::Unknown
    }

    fn is_unknown(&self, truth: Truth) -> bool {
        truth == Truth::Unknown
    }

    /// The minimum, with `False < Unknown < True`.
    fn and(&self, a: Truth, b: Truth) -> Truth {
        a.min(b)
    }

    /// The maximum.
    fn or(&self, a: Truth, b: Truth) -> Truth {
        a.max(b)
    }

    fn not(&self, a: Truth) -> Truth {
        match a {
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
            Truth::True => Truth::False,
        }
    }

    fn value_truth(&self, value: &Value) -> std::result::Result<Option<Truth>, Group> {
        match value {
            Value::Bool(b) => Ok(Some(self.truth(*b))),
            Value::Null(_) => Ok(None),
            other => Err(other.group().unwrap_or(Group::Other)),
        }
    }

    fn fires(&self, truth: Truth) -> bool {
        truth == Truth::True
    }
}

/// The logics of the core, by name (`spec/json-format.md`, *Rule sets*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogicName {
    /// Kleene's three-valued logic.
    Boolean,
}

impl LogicName {
    /// Every logic this build of the core has.
    pub const ALL: [LogicName; 1] = [LogicName::Boolean];

    /// The registered name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogicName::Boolean => "boolean",
        }
    }

    /// Look a logic up by name.
    ///
    /// # Errors
    ///
    /// `registry_error`, naming the logics that exist, when the name is unknown.
    pub fn from_name(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.as_str() == name)
            .ok_or_else(|| {
                let mut error =
                    Error::new(ErrorCode::RegistryError, format!("unknown logic {name:?}"))
                        .with_hint("this version of the core has the logic \"boolean\"");
                let context = error.context_mut();
                context.name = Some(name.to_owned());
                context.available = Self::ALL.iter().map(|l| l.as_str().to_owned()).collect();
                error
            })
    }
}

#[cfg(test)]
mod tests {
    use super::Truth::{False as F, True as T, Unknown as U};
    use super::{Kleene, Logic, LogicName, Truth};
    use crate::error::ErrorCode;
    use crate::value::{Group, NullCause, Value};

    const ALL: [Truth; 3] = [F, U, T];

    #[test]
    fn truth_tables_of_the_spec() {
        let k = Kleene;
        for (a, b, and, or) in [
            (T, T, T, T),
            (T, F, F, T),
            (T, U, U, T),
            (F, F, F, F),
            (F, U, F, U),
            (U, U, U, U),
        ] {
            for (x, y) in [(a, b), (b, a)] {
                assert_eq!(k.and(x, y), and, "{x:?} and {y:?}");
                assert_eq!(k.or(x, y), or, "{x:?} or {y:?}");
            }
        }
        assert_eq!([k.not(T), k.not(F), k.not(U)], [F, T, U]);
    }

    #[test]
    fn laws_hold_on_every_combination() {
        let k = Kleene;
        for a in ALL {
            assert_eq!(k.not(k.not(a)), a);
            for b in ALL {
                assert_eq!(k.not(k.and(a, b)), k.or(k.not(a), k.not(b)), "De Morgan");
                assert_eq!(k.not(k.or(a, b)), k.and(k.not(a), k.not(b)), "De Morgan");
                assert_eq!(k.and(a, b), k.and(b, a));
                for c in ALL {
                    assert_eq!(k.and(k.and(a, b), c), k.and(a, k.and(b, c)));
                    assert_eq!(k.or(k.or(a, b), c), k.or(a, k.or(b, c)));
                    assert_eq!(k.and(a, k.or(b, c)), k.or(k.and(a, b), k.and(a, c)));
                }
            }
        }
        assert_eq!(k.all(&[]), T);
        assert_eq!(k.any(&[]), F);
        assert_eq!(k.all(&[T, U, T]), U);
        assert_eq!(k.any(&[F, U, T]), T);
    }

    #[test]
    fn values_as_conditions() {
        let k = Kleene;
        assert_eq!(k.value_truth(&Value::Bool(true)), Ok(Some(T)));
        assert_eq!(k.value_truth(&Value::Null(NullCause::Null)), Ok(None));
        assert_eq!(k.value_truth(&Value::Int(1)), Err(Group::Number));
        assert!(k.fires(T) && !k.fires(U) && !k.fires(F));
    }

    #[test]
    fn registry() {
        assert_eq!(LogicName::from_name("boolean").unwrap(), LogicName::Boolean);
        let error = LogicName::from_name("magic").unwrap_err();
        assert_eq!(error.code(), ErrorCode::RegistryError);
        assert_eq!(error.context().name.as_deref(), Some("magic"));
        assert_eq!(error.context().available, ["boolean"]);
    }
}
