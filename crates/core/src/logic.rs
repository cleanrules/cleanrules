//! Logics: how truth values combine (`spec/ast.md`, *Truth values*, *Logics*).
//!
//! The evaluator never combines truth values with `&&`, `||` or `!`: everything
//! goes through a [`Logic`] (`CLAUDE.md`, invariant 4). [`Kleene`] is the crisp
//! logic, and each [`Fuzzy`] logic, with the capability [`Graded`], combines
//! intervals of degrees with a norm of fuzzytool.

use std::fmt::Debug;

use crate::error::{Error, ErrorCode, Result};
use crate::value::{Group, Value};

mod fuzzy;

pub use fuzzy::{Complement, Fuzzy, Interval, Norm};

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

/// The capability of a logic whose truth values are degrees: intervals
/// `[lo, hi]` in `[0, 1]` (`spec/ast.md`, *Truth values*).
pub trait Graded: Logic {
    /// The bounds `(lo, hi)` of a truth value.
    fn bounds(&self, truth: Self::Truth) -> (f64, f64);

    /// The degree to which a rule with this truth value fires: `lo`.
    fn firing_degree(&self, truth: Self::Truth) -> f64 {
        self.bounds(truth).0
    }
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

/// The logics of the core, by name (`spec/ast.md`, *Logics*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LogicName {
    /// Kleene's three-valued logic.
    Boolean,
    /// Fuzzy, with the minimum and the maximum.
    Min,
    /// Fuzzy, with the product and the probabilistic sum.
    Prod,
    /// Fuzzy, with Łukasiewicz's norms.
    Lukasiewicz,
    /// Fuzzy, with Einstein's norms.
    Einstein,
    /// Fuzzy, with Hamacher's family (`gamma`, default 0).
    Hamacher,
    /// Fuzzy, with the drastic norms.
    Drastic,
    /// Fuzzy, with the nilpotent minimum and maximum.
    Nilpotent,
    /// Fuzzy, with Yager's family (`p`, required).
    Yager,
    /// Fuzzy, with Dombi's family (`p`, required).
    Dombi,
    /// Fuzzy, with Frank's family (`s`, required).
    Frank,
}

impl LogicName {
    /// Every logic this build of the core has, sorted by name.
    pub const ALL: &'static [LogicName] = &[
        LogicName::Boolean,
        LogicName::Dombi,
        LogicName::Drastic,
        LogicName::Einstein,
        LogicName::Frank,
        LogicName::Hamacher,
        LogicName::Lukasiewicz,
        LogicName::Min,
        LogicName::Nilpotent,
        LogicName::Prod,
        LogicName::Yager,
    ];

    /// The registered name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogicName::Boolean => "boolean",
            LogicName::Min => "min",
            LogicName::Prod => "prod",
            LogicName::Lukasiewicz => "lukasiewicz",
            LogicName::Einstein => "einstein",
            LogicName::Hamacher => "hamacher",
            LogicName::Drastic => "drastic",
            LogicName::Nilpotent => "nilpotent",
            LogicName::Yager => "yager",
            LogicName::Dombi => "dombi",
            LogicName::Frank => "frank",
        }
    }

    /// Whether the logic is graded: every logic but `boolean`.
    #[must_use]
    pub fn is_graded(self) -> bool {
        self != LogicName::Boolean
    }

    /// The key of the logic's parameter in its JSON form (`gamma`, `p` or
    /// `s`), for the logics that take one.
    #[must_use]
    pub fn parameter_key(self) -> Option<&'static str> {
        match self {
            LogicName::Hamacher => Some("gamma"),
            LogicName::Yager | LogicName::Dombi => Some("p"),
            LogicName::Frank => Some("s"),
            _ => None,
        }
    }

    /// The logics that take a parameter, with its key, for hints.
    pub(crate) fn parameters_hint() -> String {
        let with: Vec<String> = Self::ALL
            .iter()
            .filter_map(|l| {
                l.parameter_key()
                    .map(|key| format!("{} ({key})", l.as_str()))
            })
            .collect();
        format!("the logics with a parameter are {}", with.join(", "))
    }

    /// Look a logic up by name; fuzzytool's alias `product` is `prod`.
    ///
    /// # Errors
    ///
    /// `registry_error`, naming the logics that exist, when the name is unknown.
    pub fn from_name(name: &str) -> Result<Self> {
        let canonical = if name == "product" { "prod" } else { name };
        Self::ALL
            .iter()
            .copied()
            .find(|l| l.as_str() == canonical)
            .ok_or_else(|| {
                let available: Vec<String> =
                    Self::ALL.iter().map(|l| l.as_str().to_owned()).collect();
                let mut error =
                    Error::new(ErrorCode::RegistryError, format!("unknown logic {name:?}"))
                        .with_hint(format!("the logics are {}", available.join(", ")));
                let context = error.context_mut();
                context.name = Some(name.to_owned());
                context.available = available;
                error
            })
    }

    /// Look up the logic of a rule set or decision table by name: graded logics
    /// are `unsupported` there until firing degrees are specified (task J.2;
    /// `spec/json-format.md`, *Rule sets*).
    pub(crate) fn for_rules(name: &str) -> Result<Self> {
        Self::from_name(name)?.check_for_rules()
    }

    /// This logic, if a rule set can use it: `unsupported` for a graded one.
    pub(crate) fn check_for_rules(self) -> Result<Self> {
        if self.is_graded() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!(
                    "rule sets cannot use the graded logic {:?} yet",
                    self.as_str()
                ),
            )
            .with_hint("use \"boolean\"; graded logics evaluate one condition (evaluate_tree)")
            .unsupported(
                "a rule set under a graded logic",
                "firing degrees are not specified yet",
            ));
        }
        Ok(self)
    }
}

/// A logic chosen at run time, with its parameters (`spec/ast.md`, *Logics*):
/// what a logic's name, or its JSON form, selects.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum AnyLogic {
    /// Kleene's logic, `boolean`.
    Boolean(Kleene),
    /// A fuzzy logic.
    Fuzzy(Fuzzy),
}

impl AnyLogic {
    /// The logic of a name, with every parameter at its default.
    ///
    /// # Errors
    ///
    /// `registry_error` for an unknown name, and `rule_definition_error` for a
    /// family whose parameter has no default (`yager`, `dombi`, `frank`).
    pub fn from_name(name: &str) -> Result<Self> {
        Self::with_parameter(LogicName::from_name(name)?, None, Complement::Standard)
    }

    /// The logic named `name`, with its family's parameter (`None` for the
    /// default) and a complement.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` for a parameter the logic lacks, one it needs,
    /// or one out of its range ([`Norm::with_parameter`]), and for a
    /// complement other than the standard one under `boolean`.
    pub fn with_parameter(
        name: LogicName,
        parameter: Option<f64>,
        complement: Complement,
    ) -> Result<Self> {
        if name == LogicName::Boolean {
            if parameter.is_some() || complement != Complement::Standard {
                return Err(Error::new(
                    ErrorCode::RuleDefinitionError,
                    "the logic \"boolean\" takes no parameter and no complement",
                )
                .with_hint("parameters and complements belong to the fuzzy logics"));
            }
            return Ok(AnyLogic::Boolean(Kleene));
        }
        let norm = Norm::with_parameter(name, parameter)?;
        Ok(AnyLogic::Fuzzy(Fuzzy::new(norm, complement)))
    }

    /// The logic named `name` (`spec/ast.md`, *Logics*), with its parameter as
    /// a key and a value, and its complement (the standard one for `None`),
    /// as its JSON form gives them (`spec/json-format.md`, *Logics*).
    ///
    /// # Errors
    ///
    /// `registry_error` for an unknown name; `rule_definition_error` for a
    /// parameter under the key of another logic, a missing one, one out of
    /// its range, or a complement under `boolean`.
    pub fn from_parts(
        name: &str,
        parameter: Option<(&str, f64)>,
        complement: Option<Complement>,
    ) -> Result<Self> {
        let name = LogicName::from_name(name)?;
        let value = match (parameter, name.parameter_key()) {
            (Some((key, value)), Some(own)) if key == own => Some(value),
            (Some((key, _)), own) => {
                return Err(Error::new(
                    ErrorCode::RuleDefinitionError,
                    format!("the logic {:?} takes no parameter {key:?}", name.as_str()),
                )
                .with_hint(match own {
                    Some(own) => format!("its parameter is {own}"),
                    None => LogicName::parameters_hint(),
                }));
            }
            (None, _) => None,
        };
        Self::with_parameter(name, value, complement.unwrap_or_default())
    }

    /// Its name.
    #[must_use]
    pub fn name(&self) -> LogicName {
        match self {
            AnyLogic::Boolean(_) => LogicName::Boolean,
            AnyLogic::Fuzzy(fuzzy) => fuzzy.norm().name(),
        }
    }

    /// The fuzzy logic, if it is one.
    #[must_use]
    pub fn as_fuzzy(&self) -> Option<&Fuzzy> {
        match self {
            AnyLogic::Fuzzy(fuzzy) => Some(fuzzy),
            AnyLogic::Boolean(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Truth::{False as F, True as T, Unknown as U};
    use super::{AnyLogic, Complement, Fuzzy, Kleene, Logic, LogicName, Norm, Truth};
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
        assert_eq!(error.context().available.len(), LogicName::ALL.len());
        assert_eq!(error.context().available[0], "boolean");
        let mut sorted = error.context().available.clone();
        sorted.sort();
        assert_eq!(sorted, error.context().available);
        for name in LogicName::ALL {
            assert_eq!(LogicName::from_name(name.as_str()).unwrap(), *name);
            assert_eq!(name.is_graded(), *name != LogicName::Boolean);
        }
        // `fuzzy` is no logic's name: vectors use it as an unknown one.
        assert!(LogicName::from_name("fuzzy").is_err());
    }

    #[test]
    fn rule_sets_take_boolean_only() {
        assert_eq!(LogicName::for_rules("boolean").unwrap(), LogicName::Boolean);
        let error = LogicName::for_rules("min").unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.context().operation.is_some());
        assert_eq!(
            LogicName::for_rules("nope").unwrap_err().code(),
            ErrorCode::RegistryError
        );
    }

    #[test]
    fn logics_by_name_and_parameters() {
        assert_eq!(
            AnyLogic::from_name("boolean").unwrap(),
            AnyLogic::Boolean(Kleene)
        );
        let min = AnyLogic::from_name("min").unwrap();
        assert_eq!(
            min,
            AnyLogic::Fuzzy(Fuzzy::new(Norm::Min, Complement::Standard))
        );
        assert_eq!(min.name(), LogicName::Min);
        assert_eq!(min.as_fuzzy().map(Fuzzy::norm), Some(Norm::Min));
        assert_eq!(AnyLogic::Boolean(Kleene).as_fuzzy(), None);
        // fuzzytool's alias.
        assert_eq!(
            AnyLogic::from_name("product").unwrap().name(),
            LogicName::Prod
        );
        let hamacher = AnyLogic::from_name("hamacher").unwrap();
        assert!(
            matches!(hamacher, AnyLogic::Fuzzy(f) if f.norm() == Norm::Hamacher { gamma: 0.0 })
        );
        for name in ["yager", "dombi", "frank"] {
            let error = AnyLogic::from_name(name).unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError, "{name}");
        }
        for (name, norm) in [
            (LogicName::Yager, Norm::Yager { p: 2.0 }),
            (LogicName::Dombi, Norm::Dombi { p: 2.0 }),
            (LogicName::Frank, Norm::Frank { s: 2.0 }),
            (LogicName::Hamacher, Norm::Hamacher { gamma: 2.0 }),
        ] {
            let logic = AnyLogic::with_parameter(name, Some(2.0), Complement::Standard).unwrap();
            assert!(matches!(logic, AnyLogic::Fuzzy(f) if f.norm() == norm));
            assert_eq!(logic.name(), name);
            let key = name.parameter_key().unwrap();
            assert_eq!(
                AnyLogic::from_parts(name.as_str(), Some((key, 2.0)), None).unwrap(),
                logic
            );
        }
        let errors = [
            (LogicName::Min, Some(1.0), Complement::Standard),
            (LogicName::Boolean, Some(1.0), Complement::Standard),
            (LogicName::Boolean, None, Complement::Sugeno { lam: 1.0 }),
            (LogicName::Yager, Some(-1.0), Complement::Standard),
        ];
        for (name, parameter, complement) in errors {
            let error = AnyLogic::with_parameter(name, parameter, complement).unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError, "{name:?}");
        }
        let sugeno = Complement::Sugeno { lam: 1.0 };
        let logic = AnyLogic::from_parts("prod", None, Some(sugeno)).unwrap();
        assert!(matches!(logic, AnyLogic::Fuzzy(f) if f.complement() == sugeno));
        for (name, parameter) in [("hamacher", Some(("p", 1.0))), ("min", Some(("s", 1.0)))] {
            let error = AnyLogic::from_parts(name, parameter, None).unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError, "{name}");
        }
        assert_eq!(
            AnyLogic::from_parts("nope", None, None).unwrap_err().code(),
            ErrorCode::RegistryError
        );
        assert!(LogicName::parameters_hint().contains("frank (s)"));
    }
}
