//! Fuzzy logics: t-norms, s-norms and complements (`spec/ast.md`, *Logics*),
//! ported from fuzzytool, with unknown as the interval `[0, 1]`
//! (`spec/ast.md`, *Truth values*).
//!
//! Each norm is fuzzytool's formula, evaluated in the same order of floating
//! point operations, so interior degrees match fuzzytool's; the boundary
//! conditions of norms hold exactly (`T(1, b) = b`, `T(0, b) = 0`,
//! `S(0, b) = b`, `S(1, b) = 1`), which fuzzytool's formulas only approximate
//! for some norms, so that crisp truth values combine as in Kleene's tables.

// The norms test exact degrees on purpose: 0 and 1 are the boundary
// conditions of `spec/ast.md`, and fuzzytool's formulas compare the same way.
#![allow(clippy::float_cmp)]

use crate::error::{Error, ErrorCode, Result};
use crate::logic::{Graded, Logic, LogicName};
use crate::value::{Group, Number, Value};

/// fuzzytool's guard for denominators that vanish only in a removable limit
/// (`fuzzytool.norms._TINY`).
const TINY: f64 = 1e-12;

/// A graded truth value: an interval `[lo, hi]` of degrees in `[0, 1]`
/// (`spec/ast.md`, *Truth values*). A known degree `d` is `[d, d]`, and
/// unknown is `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    /// Unknown: `[0, 1]`.
    pub const UNKNOWN: Interval = Interval { lo: 0.0, hi: 1.0 };

    /// A known degree `[d, d]`, or `None` when `d` is not in `[0, 1]`. A
    /// negative zero is the degree `0`.
    #[must_use]
    pub fn degree(d: f64) -> Option<Self> {
        Self::new(d, d)
    }

    /// The interval `[lo, hi]`, or `None` unless `0 ≤ lo ≤ hi ≤ 1`.
    #[must_use]
    pub fn new(lo: f64, hi: f64) -> Option<Self> {
        ((0.0..=1.0).contains(&lo) && (lo..=1.0).contains(&hi)).then(|| Self {
            lo: unit(lo),
            hi: unit(hi),
        })
    }

    /// The lower bound: the degree to which the condition surely holds.
    #[must_use]
    pub fn lo(self) -> f64 {
        self.lo
    }

    /// The upper bound: the degree to which it may hold.
    #[must_use]
    pub fn hi(self) -> f64 {
        self.hi
    }

    /// Whether the interval is a single degree.
    #[must_use]
    pub fn is_degree(self) -> bool {
        self.lo == self.hi
    }

    /// An interval from bounds a connective computed: rounding never puts `hi`
    /// below `lo` (`spec/ast.md`, *Truth values*).
    fn bounds(lo: f64, hi: f64) -> Self {
        Self {
            lo,
            hi: if hi < lo { lo } else { hi },
        }
    }
}

/// A degree a formula computed, clamped to `[0, 1]`, with a negative zero
/// (or a NaN, which no valid parameter produces) as `0`.
fn unit(x: f64) -> f64 {
    if x > 0.0 { x.min(1.0) } else { 0.0 }
}

/// `x` to the power `y`. The exponents that `NumPy` computes without `pow` (1,
/// -1, 2 and 0.5) use the exact IEEE operations; the others use the `libm`
/// crate, so every platform computes the same bits (`spec/ast.md`, *Logics*).
fn pow(x: f64, y: f64) -> f64 {
    if y == 1.0 {
        x
    } else if y == 2.0 {
        x * x
    } else if y == 0.5 {
        x.sqrt()
    } else if y == -1.0 {
        1.0 / x
    } else {
        libm::pow(x, y)
    }
}

/// A t-norm with its dual s-norm under the standard complement: the
/// connectives of a fuzzy logic (`spec/ast.md`, *Logics*).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Norm {
    /// Minimum and maximum (Gödel, Zadeh).
    Min,
    /// Product and probabilistic sum.
    Prod,
    /// Łukasiewicz: `max(0, a + b − 1)` and `min(1, a + b)`.
    Lukasiewicz,
    /// Einstein product and sum.
    Einstein,
    /// Hamacher's family, `gamma ≥ 0`; `0` is fuzzytool's Hamacher product,
    /// `1` the product and `2` Einstein's.
    #[non_exhaustive]
    Hamacher {
        /// The parameter γ.
        gamma: f64,
    },
    /// Drastic product and sum.
    Drastic,
    /// Nilpotent minimum and maximum.
    Nilpotent,
    /// Yager's family, `p > 0`.
    #[non_exhaustive]
    Yager {
        /// The parameter.
        p: f64,
    },
    /// Dombi's family, `p > 0`.
    #[non_exhaustive]
    Dombi {
        /// The parameter.
        p: f64,
    },
    /// Frank's family, `s > 0` and `s ≠ 1`.
    #[non_exhaustive]
    Frank {
        /// The parameter.
        s: f64,
    },
}

impl Norm {
    /// The name of the logic this norm makes.
    #[must_use]
    pub fn name(self) -> LogicName {
        match self {
            Norm::Min => LogicName::Min,
            Norm::Prod => LogicName::Prod,
            Norm::Lukasiewicz => LogicName::Lukasiewicz,
            Norm::Einstein => LogicName::Einstein,
            Norm::Hamacher { .. } => LogicName::Hamacher,
            Norm::Drastic => LogicName::Drastic,
            Norm::Nilpotent => LogicName::Nilpotent,
            Norm::Yager { .. } => LogicName::Yager,
            Norm::Dombi { .. } => LogicName::Dombi,
            Norm::Frank { .. } => LogicName::Frank,
        }
    }

    /// The parameter, with its key, for the families that take one.
    #[must_use]
    pub fn parameter(self) -> Option<(&'static str, f64)> {
        match self {
            Norm::Hamacher { gamma } => Some(("gamma", gamma)),
            Norm::Yager { p } | Norm::Dombi { p } => Some(("p", p)),
            Norm::Frank { s } => Some(("s", s)),
            _ => None,
        }
    }

    /// Hamacher's family.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `gamma` is a finite number ≥ 0.
    pub fn hamacher(gamma: f64) -> Result<Self> {
        Norm::Hamacher { gamma }.checked()
    }

    /// Yager's family.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `p` is a finite number > 0.
    pub fn yager(p: f64) -> Result<Self> {
        Norm::Yager { p }.checked()
    }

    /// Dombi's family.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `p` is a finite number > 0.
    pub fn dombi(p: f64) -> Result<Self> {
        Norm::Dombi { p }.checked()
    }

    /// Frank's family.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `s` is a finite number > 0 and ≠ 1.
    pub fn frank(s: f64) -> Result<Self> {
        Norm::Frank { s }.checked()
    }

    /// The norm of the logic `name`, with its parameter (`None` for the
    /// default; only `hamacher` has one, `gamma = 0`).
    ///
    /// # Errors
    ///
    /// `rule_definition_error` for `boolean`, which has no norm, for a
    /// parameter the logic does not take, one it needs, or one out of range.
    pub fn with_parameter(name: LogicName, parameter: Option<f64>) -> Result<Self> {
        let norm = match (name, parameter) {
            (LogicName::Min, None) => Norm::Min,
            (LogicName::Prod, None) => Norm::Prod,
            (LogicName::Lukasiewicz, None) => Norm::Lukasiewicz,
            (LogicName::Einstein, None) => Norm::Einstein,
            (LogicName::Hamacher, gamma) => Norm::Hamacher {
                gamma: gamma.unwrap_or(0.0),
            },
            (LogicName::Drastic, None) => Norm::Drastic,
            (LogicName::Nilpotent, None) => Norm::Nilpotent,
            (LogicName::Yager, Some(p)) => Norm::Yager { p },
            (LogicName::Dombi, Some(p)) => Norm::Dombi { p },
            (LogicName::Frank, Some(s)) => Norm::Frank { s },
            (LogicName::Boolean, _) => {
                return Err(Error::new(
                    ErrorCode::RuleDefinitionError,
                    "the logic \"boolean\" has no norm",
                )
                .with_hint(format!(
                    "norms belong to the fuzzy logics: {}",
                    LogicName::ALL
                        .iter()
                        .filter(|l| l.is_graded())
                        .map(|l| l.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            (name, None) => {
                let key = name.parameter_key().unwrap_or("its parameter");
                return Err(Error::new(
                    ErrorCode::RuleDefinitionError,
                    format!("the logic {:?} needs the parameter {key}", name.as_str()),
                )
                .with_hint(format!(
                    "write {{\"name\": \"{}\", \"{key}\": ...}}",
                    name.as_str()
                )));
            }
            (name, Some(_)) => {
                return Err(Error::new(
                    ErrorCode::RuleDefinitionError,
                    format!("the logic {:?} takes no parameter", name.as_str()),
                )
                .with_hint(LogicName::parameters_hint()));
            }
        };
        norm.checked()
    }

    /// This norm, if its parameter is in range.
    fn checked(self) -> Result<Self> {
        let Some((key, value)) = self.parameter() else {
            return Ok(self);
        };
        let (valid, range) = match self {
            Norm::Hamacher { .. } => (value >= 0.0, "a finite number ≥ 0"),
            Norm::Frank { .. } => (value > 0.0 && value != 1.0, "a finite number > 0, not 1"),
            _ => (value > 0.0, "a finite number > 0"),
        };
        if valid && value.is_finite() {
            Ok(self)
        } else {
            Err(parameter_error(
                &format!("logic {:?}", self.name().as_str()),
                key,
                value,
                range,
            ))
        }
    }

    /// The t-norm `T(a, b)`, for degrees in `[0, 1]`.
    #[must_use]
    pub fn tnorm(self, a: f64, b: f64) -> f64 {
        if a == 0.0 || b == 0.0 {
            return 0.0;
        }
        if a == 1.0 {
            return b;
        }
        if b == 1.0 {
            return a;
        }
        unit(match self {
            Norm::Min => a.min(b),
            Norm::Prod => a * b,
            Norm::Lukasiewicz => a + b - 1.0,
            Norm::Einstein => (a * b) / (2.0 - (a + b - a * b)),
            Norm::Hamacher { gamma } => {
                let den = gamma + (1.0 - gamma) * (a + b - a * b);
                if den > TINY { a * b / den } else { 0.0 }
            }
            // Both are below 1 here.
            Norm::Drastic => 0.0,
            Norm::Nilpotent => {
                if a + b > 1.0 {
                    a.min(b)
                } else {
                    0.0
                }
            }
            Norm::Yager { p } => {
                let s = pow(1.0 - a, p) + pow(1.0 - b, p);
                1.0 - pow(s, 1.0 / p)
            }
            Norm::Dombi { p } => {
                let (aa, bb) = (a.max(TINY), b.max(TINY));
                let s = pow(pow(1.0 / aa - 1.0, p) + pow(1.0 / bb - 1.0, p), 1.0 / p);
                1.0 / (1.0 + s)
            }
            Norm::Frank { s } => frank(s, a, b),
        })
    }

    /// The s-norm `S(a, b)`, the dual of [`Norm::tnorm`], for degrees in `[0, 1]`.
    #[must_use]
    pub fn snorm(self, a: f64, b: f64) -> f64 {
        if a == 1.0 || b == 1.0 {
            return 1.0;
        }
        if a == 0.0 {
            return b;
        }
        if b == 0.0 {
            return a;
        }
        unit(match self {
            Norm::Min => a.max(b),
            Norm::Prod => a + b - a * b,
            Norm::Lukasiewicz => a + b,
            Norm::Einstein => (a + b) / (1.0 + a * b),
            Norm::Hamacher { gamma } => {
                // fuzzytool's `(a + b − 2ab) / (1 − ab)` when γ is 0.
                let ab = a * b;
                let den = (1.0 - ab) + gamma * ab;
                if den > TINY {
                    (a + b - 2.0 * a * b + gamma * ab) / den
                } else {
                    1.0
                }
            }
            // Both are above 0 here.
            Norm::Drastic => 1.0,
            Norm::Nilpotent => {
                if a + b < 1.0 {
                    a.max(b)
                } else {
                    1.0
                }
            }
            Norm::Yager { p } => pow(pow(a, p) + pow(b, p), 1.0 / p),
            Norm::Dombi { p } => {
                let clip = |x: f64| x.clamp(TINY, 1.0 - TINY);
                let (aa, bb) = (clip(a), clip(b));
                let s = pow(pow(1.0 / aa - 1.0, -p) + pow(1.0 / bb - 1.0, -p), -1.0 / p);
                1.0 / (1.0 + s)
            }
            Norm::Frank { s } => 1.0 - frank(s, 1.0 - a, 1.0 - b),
        })
    }
}

/// Frank's t-norm, as fuzzytool computes it.
fn frank(s: f64, a: f64, b: f64) -> f64 {
    let inner = 1.0 + (pow(s, a) - 1.0) * (pow(s, b) - 1.0) / (s - 1.0);
    libm::log(inner.max(TINY)) / libm::log(s)
}

/// A fuzzy complement (`spec/ast.md`, *Logics*).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum Complement {
    /// `1 − a`.
    #[default]
    Standard,
    /// Sugeno's `(1 − a) / (1 + lam·a)`, `lam > −1`.
    #[non_exhaustive]
    Sugeno {
        /// The parameter λ.
        lam: f64,
    },
    /// Yager's `(1 − a^w)^(1/w)`, `w > 0`.
    #[non_exhaustive]
    Yager {
        /// The parameter.
        w: f64,
    },
}

impl Complement {
    /// The registered names of the complements, sorted.
    pub const NAMES: &'static [&'static str] = &["standard", "sugeno", "yager"];

    /// The registered name: `standard`, `sugeno` or `yager`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Complement::Standard => "standard",
            Complement::Sugeno { .. } => "sugeno",
            Complement::Yager { .. } => "yager",
        }
    }

    /// The parameter, with its key, for the complements that take one.
    #[must_use]
    pub fn parameter(self) -> Option<(&'static str, f64)> {
        match self {
            Complement::Standard => None,
            Complement::Sugeno { lam } => Some(("lam", lam)),
            Complement::Yager { w } => Some(("w", w)),
        }
    }

    /// Sugeno's complement.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `lam` is a finite number > −1.
    pub fn sugeno(lam: f64) -> Result<Self> {
        Complement::Sugeno { lam }.checked()
    }

    /// Yager's complement.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` unless `w` is a finite number > 0.
    pub fn yager(w: f64) -> Result<Self> {
        Complement::Yager { w }.checked()
    }

    /// The canonical name of a registered complement: fuzzytool's alias
    /// `zadeh` is `standard`.
    ///
    /// # Errors
    ///
    /// `registry_error`, with the name and the names that exist.
    pub fn registered(name: &str) -> Result<&'static str> {
        let name = if name == "zadeh" { "standard" } else { name };
        Self::NAMES
            .iter()
            .copied()
            .find(|&n| n == name)
            .ok_or_else(|| {
                let mut error = Error::new(
                    ErrorCode::RegistryError,
                    format!("unknown complement {name:?}"),
                )
                .with_hint(format!("the complements are {}", Self::NAMES.join(", ")));
                let context = error.context_mut();
                context.name = Some(name.to_owned());
                context.available = Self::NAMES.iter().map(|&n| n.to_owned()).collect();
                error
            })
    }

    /// The key of the parameter of the complement named `name`, if it takes one.
    fn parameter_key(name: &str) -> Option<&'static str> {
        match name {
            "sugeno" => Some("lam"),
            "yager" => Some("w"),
            _ => None,
        }
    }

    /// The complement named `name` (`spec/ast.md`, *Logics*), with its
    /// parameter as a key and a value.
    ///
    /// # Errors
    ///
    /// `registry_error` for an unknown name; `rule_definition_error` for a
    /// parameter under another key, a missing one, or one out of its range.
    pub fn from_name(name: &str, parameter: Option<(&str, f64)>) -> Result<Self> {
        let name = Self::registered(name)?;
        let own = Self::parameter_key(name);
        let what = format!("complement {name:?}");
        match (parameter, own) {
            (None, None) => Ok(Complement::Standard),
            (Some((key, value)), Some(own)) if key == own => {
                if name == "sugeno" {
                    Self::sugeno(value)
                } else {
                    Self::yager(value)
                }
            }
            (Some((key, _)), own) => Err(Error::new(
                ErrorCode::RuleDefinitionError,
                format!("the {what} takes no parameter {key:?}"),
            )
            .with_hint(match own {
                Some(own) => format!("its parameter is {own}"),
                None => "it takes no parameter".to_owned(),
            })),
            (None, Some(own)) => Err(Error::new(
                ErrorCode::RuleDefinitionError,
                format!("the {what} needs the parameter {own}"),
            )
            .with_hint(format!("write {{\"name\": \"{name}\", \"{own}\": ...}}"))),
        }
    }

    /// This complement, if its parameter is in range.
    fn checked(self) -> Result<Self> {
        let (valid, key, value, range) = match self {
            Complement::Standard => return Ok(self),
            Complement::Sugeno { lam } => (lam > -1.0, "lam", lam, "a finite number > -1"),
            Complement::Yager { w } => (w > 0.0, "w", w, "a finite number > 0"),
        };
        if valid && value.is_finite() {
            Ok(self)
        } else {
            Err(parameter_error(
                &format!("complement {:?}", self.as_str()),
                key,
                value,
                range,
            ))
        }
    }

    /// The complement of a degree in `[0, 1]`.
    #[must_use]
    pub fn apply(self, a: f64) -> f64 {
        unit(match self {
            Complement::Standard => 1.0 - a,
            Complement::Sugeno { lam } => (1.0 - a) / (1.0 + lam * a),
            Complement::Yager { w } => pow((1.0 - pow(a, w)).max(0.0), 1.0 / w),
        })
    }
}

/// The `rule_definition_error` of a parameter out of its range; `what` is
/// `logic "yager"` or `complement "sugeno"`.
fn parameter_error(what: &str, key: &str, value: f64, range: &str) -> Error {
    Error::new(
        ErrorCode::RuleDefinitionError,
        format!("the {what} takes {key} = {value}, out of its range"),
    )
    .with_hint(format!("{key} must be {range}"))
}

/// A fuzzy logic: a norm and its dual, and a complement, on intervals of
/// degrees (`spec/ast.md`, *Truth values*, *Logics*). It has the capability
/// [`Graded`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fuzzy {
    norm: Norm,
    complement: Complement,
}

impl Fuzzy {
    /// A fuzzy logic: a norm and a complement, whose parameters their
    /// constructors checked.
    #[must_use]
    pub fn new(norm: Norm, complement: Complement) -> Self {
        Self { norm, complement }
    }

    /// Its norm.
    #[must_use]
    pub fn norm(&self) -> Norm {
        self.norm
    }

    /// Its complement.
    #[must_use]
    pub fn complement(&self) -> Complement {
        self.complement
    }

    /// A left fold of a connective over the operands, from the first one, as
    /// fuzzytool nests `a & b & c` (the identity would round under some norms).
    fn fold(values: &[Interval], empty: Interval, f: impl Fn(f64, f64) -> f64) -> Interval {
        let Some((&first, rest)) = values.split_first() else {
            return empty;
        };
        rest.iter().fold(first, |acc, v| {
            Interval::bounds(f(acc.lo, v.lo), f(acc.hi, v.hi))
        })
    }
}

impl Logic for Fuzzy {
    type Truth = Interval;

    fn name(&self) -> &'static str {
        self.norm.name().as_str()
    }

    fn truth(&self, value: bool) -> Interval {
        let d = if value { 1.0 } else { 0.0 };
        Interval { lo: d, hi: d }
    }

    fn unknown(&self) -> Interval {
        Interval::UNKNOWN
    }

    /// An interval wider than one degree is undetermined, and carries a reason.
    fn is_unknown(&self, truth: Interval) -> bool {
        !truth.is_degree()
    }

    fn and(&self, a: Interval, b: Interval) -> Interval {
        Interval::bounds(self.norm.tnorm(a.lo, b.lo), self.norm.tnorm(a.hi, b.hi))
    }

    fn or(&self, a: Interval, b: Interval) -> Interval {
        Interval::bounds(self.norm.snorm(a.lo, b.lo), self.norm.snorm(a.hi, b.hi))
    }

    /// The complement reverses the order, so the bounds swap.
    fn not(&self, a: Interval) -> Interval {
        Interval::bounds(self.complement.apply(a.hi), self.complement.apply(a.lo))
    }

    fn all(&self, values: &[Interval]) -> Interval {
        Self::fold(values, self.truth(true), |a, b| self.norm.tnorm(a, b))
    }

    fn any(&self, values: &[Interval]) -> Interval {
        Self::fold(values, self.truth(false), |a, b| self.norm.snorm(a, b))
    }

    /// Booleans are the degrees 0 and 1, and numbers in `[0, 1]` are degrees
    /// (`spec/data-model.md`, *Fuzzy degrees*).
    fn value_truth(&self, value: &Value) -> std::result::Result<Option<Interval>, Group> {
        match value {
            Value::Bool(b) => Ok(Some(self.truth(*b))),
            Value::Null(_) => Ok(None),
            other => other
                .as_number()
                .filter(|n| n.compare(Number::Int(0)).is_ge() && n.compare(Number::Int(1)).is_le())
                .and_then(|n| Interval::degree(n.to_f64()))
                .map(Some)
                .ok_or_else(|| other.group().unwrap_or(Group::Other)),
        }
    }

    /// The crisp test, degree 1: rule sets take graded logics, with their
    /// activation threshold, only once firing degrees are specified (task J.2).
    fn fires(&self, truth: Interval) -> bool {
        truth.lo >= 1.0
    }
}

impl std::fmt::Display for Interval {
    /// A degree as its number (`0.25`), and an undetermined interval as
    /// `[lo, hi]` (`[0.0, 0.8]`), with the digits of the JSON form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_degree() {
            write!(f, "{:?}", self.lo)
        } else {
            write!(f, "[{:?}, {:?}]", self.lo, self.hi)
        }
    }
}

impl Graded for Fuzzy {
    fn bounds(&self, truth: Interval) -> (f64, f64) {
        (truth.lo, truth.hi)
    }
}

#[cfg(test)]
mod tests {
    use super::{Complement, Fuzzy, Interval, Norm, pow, unit};
    use crate::error::ErrorCode;
    use crate::logic::{Graded, Kleene, Logic, Truth};
    use crate::value::{Decimal, Group, NullCause, Value};

    const CLOSE: f64 = 1e-12;

    fn norms() -> Vec<Norm> {
        vec![
            Norm::Min,
            Norm::Prod,
            Norm::Lukasiewicz,
            Norm::Einstein,
            Norm::Hamacher { gamma: 0.0 },
            Norm::Hamacher { gamma: 0.5 },
            Norm::Hamacher { gamma: 3.0 },
            Norm::Drastic,
            Norm::Nilpotent,
            Norm::Yager { p: 0.5 },
            Norm::Yager { p: 2.0 },
            Norm::Dombi { p: 1.0 },
            Norm::Dombi { p: 3.0 },
            Norm::Frank { s: 0.3 },
            Norm::Frank { s: 2.0 },
        ]
    }

    fn complements() -> [Complement; 4] {
        [
            Complement::Standard,
            Complement::Sugeno { lam: -0.5 },
            Complement::Sugeno { lam: 2.0 },
            Complement::Yager { w: 3.0 },
        ]
    }

    fn logics() -> Vec<Fuzzy> {
        let mut all = Vec::new();
        for norm in norms() {
            for complement in complements() {
                all.push(Fuzzy::new(norm, complement));
            }
        }
        all
    }

    const GRID: [f64; 11] = [0.0, 0.05, 0.1, 0.25, 0.3, 0.5, 0.6, 0.75, 0.9, 0.99, 1.0];

    /// The Kleene value an interval on `{0, 1}` stands for.
    fn kleene(i: Interval) -> Truth {
        match (i.lo(), i.hi()) {
            (0.0, 0.0) => Truth::False,
            (1.0, 1.0) => Truth::True,
            (0.0, 1.0) => Truth::Unknown,
            _ => panic!("{i:?} is not a Kleene value"),
        }
    }

    #[test]
    fn crisp_values_follow_kleene_exactly() {
        let k = Kleene;
        for f in logics() {
            let crisp = [f.truth(false), f.unknown(), f.truth(true)];
            for a in crisp {
                assert_eq!(kleene(f.not(a)), k.not(kleene(a)), "{f:?} not {a:?}");
                for b in crisp {
                    let (x, y) = (kleene(a), kleene(b));
                    assert_eq!(kleene(f.and(a, b)), k.and(x, y), "{f:?}: {a:?} and {b:?}");
                    assert_eq!(kleene(f.or(a, b)), k.or(x, y), "{f:?}: {a:?} or {b:?}");
                }
            }
        }
    }

    #[test]
    fn norms_are_norms() {
        for norm in norms() {
            for a in GRID {
                assert_eq!(norm.tnorm(a, 1.0), a, "{norm:?}");
                assert_eq!(norm.tnorm(1.0, a), a, "{norm:?}");
                assert_eq!(norm.tnorm(0.0, a), 0.0, "{norm:?}");
                assert_eq!(norm.snorm(a, 0.0), a, "{norm:?}");
                assert_eq!(norm.snorm(0.0, a), a, "{norm:?}");
                assert_eq!(norm.snorm(1.0, a), 1.0, "{norm:?}");
                for b in GRID {
                    let t = norm.tnorm(a, b);
                    let s = norm.snorm(a, b);
                    assert!((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s));
                    assert!(t <= a.min(b) + CLOSE, "{norm:?}: T({a}, {b}) = {t}");
                    assert!(s >= a.max(b) - CLOSE, "{norm:?}: S({a}, {b}) = {s}");
                    assert_eq!(t, norm.tnorm(b, a), "{norm:?} commutes");
                    assert_eq!(s, norm.snorm(b, a), "{norm:?} commutes");
                    // De Morgan under the standard complement.
                    let dual = 1.0 - norm.tnorm(1.0 - a, 1.0 - b);
                    assert!(
                        (s - dual).abs() < 1e-9,
                        "{norm:?}: S({a}, {b}) = {s}, {dual}"
                    );
                    for c in GRID.into_iter().filter(|&c| c >= b) {
                        assert!(norm.tnorm(a, c) >= t - CLOSE, "{norm:?} is monotone");
                        assert!(norm.snorm(a, c) >= s - CLOSE, "{norm:?} is monotone");
                    }
                }
            }
        }
    }

    #[test]
    fn hamacher_family_meets_product_and_einstein() {
        for a in GRID {
            for b in GRID {
                let h = |gamma: f64, t: bool| {
                    let n = Norm::Hamacher { gamma };
                    if t { n.tnorm(a, b) } else { n.snorm(a, b) }
                };
                for t in [true, false] {
                    let pick = |n: Norm| if t { n.tnorm(a, b) } else { n.snorm(a, b) };
                    assert!((h(1.0, t) - pick(Norm::Prod)).abs() < CLOSE);
                    assert!((h(2.0, t) - pick(Norm::Einstein)).abs() < CLOSE);
                }
            }
        }
        // Textbook value: T_γ(0.5, 0.5) = 0.25 / (γ + (1 − γ)·0.75).
        let t = Norm::Hamacher { gamma: 0.5 }.tnorm(0.5, 0.5);
        assert!((t - 0.25 / 0.875).abs() < CLOSE);
    }

    #[test]
    fn complements_reverse_and_keep_the_ends() {
        for c in complements() {
            assert_eq!(c.apply(0.0), 1.0);
            assert_eq!(c.apply(1.0), 0.0);
            for (i, a) in GRID.into_iter().enumerate() {
                for b in GRID.into_iter().skip(i) {
                    assert!(c.apply(a) >= c.apply(b), "{c:?} reverses the order");
                }
            }
        }
        assert_eq!(Complement::Standard.apply(0.25), 0.75);
        assert!((Complement::Sugeno { lam: 2.0 }.apply(0.5) - 0.25).abs() < CLOSE);
        assert!((Complement::Yager { w: 2.0 }.apply(0.6) - 0.8).abs() < CLOSE);
        assert_eq!(Complement::default(), Complement::Standard);
        assert_eq!(
            Complement::Sugeno { lam: 0.0 }.parameter(),
            Some(("lam", 0.0))
        );
        assert_eq!(Complement::Standard.parameter(), None);
    }

    #[test]
    fn intervals_follow_the_bounds() {
        let f = Fuzzy::new(Norm::Prod, Complement::Standard);
        let d = |x| Interval::degree(x).unwrap();
        let half = d(0.5);
        let unknown = f.unknown();
        assert_eq!(f.and(half, unknown), Interval::new(0.0, 0.5).unwrap());
        assert_eq!(f.or(half, unknown), Interval::new(0.5, 1.0).unwrap());
        assert_eq!(
            f.not(Interval::new(0.25, 0.5).unwrap()),
            Interval::new(0.5, 0.75).unwrap()
        );
        assert!(f.is_unknown(unknown) && !f.is_unknown(half));
        assert_eq!(f.bounds(f.and(half, half)), (0.25, 0.25));
        assert_eq!(f.firing_degree(Interval::new(0.25, 0.5).unwrap()), 0.25);
        assert_eq!(f.all(&[]), f.truth(true));
        assert_eq!(f.any(&[]), f.truth(false));
        assert_eq!(f.all(&[half, half, half]), d(0.125));
        assert_eq!(f.any(&[d(0.5), d(0.5)]), d(0.75));
        assert!(f.fires(f.truth(true)) && !f.fires(half));
        // Under Łukasiewicz an n-ary `and` is a left fold from the first operand.
        let luk = Fuzzy::new(Norm::Lukasiewicz, Complement::Standard);
        let x = [d(0.7), d(0.6), d(0.9)];
        assert_eq!(luk.all(&x), luk.and(luk.and(x[0], x[1]), x[2]));
        assert_eq!(luk.name(), "lukasiewicz");
        assert_eq!(luk.norm(), Norm::Lukasiewicz);
        assert_eq!(luk.complement(), Complement::Standard);
    }

    #[test]
    fn interval_constructors() {
        assert_eq!(Interval::degree(1.5), None);
        assert_eq!(Interval::degree(f64::NAN), None);
        assert_eq!(Interval::new(0.6, 0.5), None);
        let zero = Interval::degree(-0.0).unwrap();
        assert!(zero.lo().is_sign_positive());
        assert!(Interval::degree(0.3).unwrap().is_degree());
        assert!(!Interval::UNKNOWN.is_degree());
        // Rounding never makes an interval upside down.
        assert_eq!(Interval::bounds(0.5, 0.4), Interval::degree(0.5).unwrap());
        assert_eq!(unit(-0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(unit(f64::NAN), 0.0);
        assert_eq!(unit(1.5), 1.0);
    }

    #[test]
    fn powers_use_exact_operations_where_numpy_does() {
        assert_eq!(pow(0.3, 1.0), 0.3);
        assert_eq!(pow(0.3, 2.0), 0.3 * 0.3);
        assert_eq!(pow(0.3, 0.5), 0.3_f64.sqrt());
        assert_eq!(pow(0.3, -1.0), 1.0 / 0.3);
        assert_eq!(pow(4.0, 1.5), 8.0);
    }

    #[test]
    fn data_degrees() {
        let f = Fuzzy::new(Norm::Min, Complement::Standard);
        let degree = |v: Value| f.value_truth(&v);
        assert_eq!(degree(Value::Bool(true)), Ok(Some(f.truth(true))));
        assert_eq!(degree(Value::Null(NullCause::Null)), Ok(None));
        assert_eq!(degree(Value::Int(1)), Ok(Some(f.truth(true))));
        assert_eq!(degree(Value::Int(0)), Ok(Some(f.truth(false))));
        assert_eq!(degree(Value::Int(2)), Err(Group::Number));
        assert_eq!(degree(Value::Float(0.25)), Ok(Interval::degree(0.25)));
        assert_eq!(degree(Value::Float(-0.0)), Ok(Some(f.truth(false))));
        assert_eq!(degree(Value::Float(1.000_000_1)), Err(Group::Number));
        assert_eq!(degree(Value::Float(f64::INFINITY)), Err(Group::Number));
        let decimal = |t: &str| Value::Decimal(Decimal::parse(t).unwrap());
        assert_eq!(
            degree(decimal("0.10")),
            Ok(Some(Interval::degree(0.1).unwrap()))
        );
        assert_eq!(
            degree(decimal("1.000000000000000000001")),
            Err(Group::Number)
        );
        assert_eq!(degree(decimal("-inf")), Err(Group::Number));
        assert_eq!(degree(Value::String("0.5".into())), Err(Group::String));
        assert_eq!(degree(Value::List(vec![])), Err(Group::Other));
    }

    #[test]
    fn parameters_are_checked() {
        let bad = [
            Norm::hamacher(-0.1),
            Norm::yager(0.0),
            Norm::dombi(-1.0),
            Norm::frank(1.0),
            Norm::frank(0.0),
            Norm::yager(f64::INFINITY),
            Norm::dombi(f64::NAN),
        ];
        for norm in bad {
            let error = norm.unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError);
            assert!(
                error.message().starts_with("the logic \""),
                "{}",
                error.message()
            );
        }
        for complement in [Complement::sugeno(-1.0), Complement::yager(0.0)] {
            let error = complement.unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError);
            assert!(
                error.message().starts_with("the complement \""),
                "{}",
                error.message()
            );
        }
        assert_eq!(Norm::hamacher(0.0).unwrap(), Norm::Hamacher { gamma: 0.0 });
        assert_eq!(
            Complement::sugeno(0.0).unwrap(),
            Complement::Sugeno { lam: 0.0 }
        );
        assert_eq!(Norm::Min.parameter(), None);
        assert_eq!(Norm::frank(2.0).unwrap().parameter(), Some(("s", 2.0)));
    }

    #[test]
    fn norms_and_complements_by_name() {
        use crate::logic::LogicName;
        assert_eq!(
            Norm::with_parameter(LogicName::Prod, None).unwrap(),
            Norm::Prod
        );
        assert_eq!(
            Norm::with_parameter(LogicName::Hamacher, None).unwrap(),
            Norm::Hamacher { gamma: 0.0 }
        );
        for (name, parameter) in [
            (LogicName::Boolean, None),
            (LogicName::Yager, None),
            (LogicName::Min, Some(1.0)),
            (LogicName::Frank, Some(1.0)),
        ] {
            let error = Norm::with_parameter(name, parameter).unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError, "{name:?}");
        }
        assert_eq!(
            Complement::from_name("zadeh", None).unwrap(),
            Complement::Standard
        );
        assert_eq!(
            Complement::from_name("yager", Some(("w", 2.0))).unwrap(),
            Complement::Yager { w: 2.0 }
        );
        let error = Complement::from_name("nope", None).unwrap_err();
        assert_eq!(error.code(), ErrorCode::RegistryError);
        assert_eq!(error.context().available, Complement::NAMES);
        for (name, parameter) in [
            ("sugeno", None),
            ("sugeno", Some(("w", 1.0))),
            ("standard", Some(("lam", 1.0))),
            ("sugeno", Some(("lam", -2.0))),
        ] {
            let error = Complement::from_name(name, parameter).unwrap_err();
            assert_eq!(error.code(), ErrorCode::RuleDefinitionError, "{name}");
        }
    }

    #[test]
    fn intervals_display_as_their_json() {
        assert_eq!(Interval::degree(0.25).unwrap().to_string(), "0.25");
        assert_eq!(Interval::degree(1.0).unwrap().to_string(), "1.0");
        assert_eq!(Interval::UNKNOWN.to_string(), "[0.0, 1.0]");
    }

    mod properties {
        use proptest::prelude::*;

        use super::super::{Complement, Fuzzy, Interval, Norm};
        use crate::logic::Logic;

        fn norm() -> impl Strategy<Value = Norm> {
            prop_oneof![
                Just(Norm::Min),
                Just(Norm::Prod),
                Just(Norm::Lukasiewicz),
                Just(Norm::Einstein),
                (0.0..5.0_f64).prop_map(|gamma| Norm::Hamacher { gamma }),
                Just(Norm::Drastic),
                Just(Norm::Nilpotent),
                (0.2..8.0_f64).prop_map(|p| Norm::Yager { p }),
                (0.2..8.0_f64).prop_map(|p| Norm::Dombi { p }),
                (0.05..20.0_f64)
                    .prop_filter("s ≠ 1", |s| (s - 1.0).abs() > 1e-3)
                    .prop_map(|s| Norm::Frank { s }),
            ]
        }

        fn interval() -> impl Strategy<Value = Interval> {
            (0.0..=1.0_f64, 0.0..=1.0_f64)
                .prop_map(|(a, b)| Interval::new(a.min(b), a.max(b)).unwrap())
        }

        proptest! {
            /// Every degree an interval holds combines into the interval of
            /// the result (soundness of `spec/ast.md`, *Truth values*), and
            /// results stay intervals in `[0, 1]`.
            #[test]
            fn intervals_hold_every_combination(
                norm in norm(),
                x in interval(),
                y in interval(),
                s in 0.0..=1.0_f64,
                t in 0.0..=1.0_f64,
            ) {
                let f = Fuzzy::new(norm, Complement::Standard);
                let a = x.lo() + s * (x.hi() - x.lo());
                let b = y.lo() + t * (y.hi() - y.lo());
                let tolerance = 1e-9;
                for (result, degree) in [
                    (f.and(x, y), norm.tnorm(a, b)),
                    (f.or(x, y), norm.snorm(a, b)),
                    (f.not(x), 1.0 - a),
                ] {
                    prop_assert!(0.0 <= result.lo() && result.lo() <= result.hi() && result.hi() <= 1.0);
                    prop_assert!(
                        result.lo() - tolerance <= degree && degree <= result.hi() + tolerance,
                        "{norm:?}: {degree} outside {result:?}"
                    );
                }
            }

            /// De Morgan under the standard complement, and commutativity.
            #[test]
            fn de_morgan_and_commutativity(norm in norm(), x in interval(), y in interval()) {
                let f = Fuzzy::new(norm, Complement::Standard);
                let left = f.not(f.and(x, y));
                let right = f.or(f.not(x), f.not(y));
                prop_assert!((left.lo() - right.lo()).abs() < 1e-9 && (left.hi() - right.hi()).abs() < 1e-9);
                prop_assert_eq!(f.and(x, y), f.and(y, x));
                prop_assert_eq!(f.or(x, y), f.or(y, x));
            }
        }
    }
}
