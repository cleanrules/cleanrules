//! The checks of an audit (`spec/audit.md`, *Checks*), their witnesses
//! (*Witnesses*) and what they left undecided (*What was checked*).

use std::collections::BTreeMap;

use super::domain::{ORDERED, Pick, ValueSet};
use super::read::{Part, Read, Reader, Region};
use super::{
    Audit, AuditCheck, AuditLimit, AuditLimits, CheckResult, CheckStatus, Diagnostic,
    DiagnosticCode, NotModelled, RulesSource, Severity, Undecided, UndecidedReason, table_policy,
};
use crate::ast::Node;
use crate::error::{ErrorCode, Segment};
use crate::eval::{Options, evaluate_tree, evaluate_value_tree};
use crate::logic::{Logic, Truth};
use crate::rules::{HitPolicy, RuleSet};
use crate::value::{Group, NullCause, Value};

/// The result of trying to show a finding with witnesses.
enum Shown {
    /// This witness shows it.
    Yes(Value),
    /// No witness showed it.
    No,
    /// The steps ran out.
    Stopped,
}

/// The steps ran out.
struct Stop;

/// One audit in progress, reasoning with `logic`.
pub(super) struct Auditor<'r, L> {
    ruleset: &'r RuleSet,
    logic: &'r L,
    limits: AuditLimits,
    reader: Reader,
    /// The supported values of each field subject.
    supported: Vec<Option<ValueSet>>,
    diagnostics: Vec<Diagnostic>,
    undecided: Vec<Undecided>,
}

/// A rule's `TRUE` descriptions.
struct Truths {
    wide: Vec<Region>,
    narrow: Option<Vec<Region>>,
}

impl Truths {
    fn from_read(read: Read) -> Self {
        Self {
            narrow: read.narrow.map(|d| d.t),
            wide: read.wide.t,
        }
    }

    fn narrow(&self) -> &[Region] {
        self.narrow.as_deref().unwrap_or(&self.wide)
    }
}

/// A box with a path, for building witnesses.
pub(super) enum Tree {
    Leaf(Value),
    Map(BTreeMap<String, Tree>),
    List(Vec<Option<Tree>>),
}

/// The largest list index a witness writes, and the longest path.
const MAX_WITNESS_INDEX: usize = 1024;
const MAX_WITNESS_DEPTH: usize = 256;

impl Tree {
    pub(super) fn new() -> Self {
        Tree::Map(BTreeMap::new())
    }

    fn container(next: &Segment) -> Self {
        match next {
            Segment::Key(_) => Tree::Map(BTreeMap::new()),
            Segment::Index(_) => Tree::List(Vec::new()),
        }
    }

    /// Put `value` at `path`, unless something is already there or on the
    /// way. Nothing is created when it fails: a path can only fail where it
    /// meets what is already there, and what it creates is new.
    pub(super) fn place(&mut self, path: &[Segment], value: Value) -> bool {
        let index = |segment: &Segment| match segment {
            Segment::Index(i) => usize::try_from(*i).ok().filter(|i| *i <= MAX_WITNESS_INDEX),
            Segment::Key(_) => Some(0),
        };
        if path.len() > MAX_WITNESS_DEPTH || path.iter().any(|s| index(s).is_none()) {
            return false;
        }
        let Some((last, init)) = path.split_last() else {
            return false;
        };
        let mut node = self;
        for (at, segment) in init.iter().enumerate() {
            let next = path.get(at + 1).unwrap_or(last);
            node = match (segment, node) {
                (Segment::Key(key), Tree::Map(map)) => map
                    .entry(key.clone())
                    .or_insert_with(|| Tree::container(next)),
                (Segment::Index(_), Tree::List(items)) => {
                    let i = index(segment).unwrap_or(0);
                    if items.len() <= i {
                        items.resize_with(i + 1, || None);
                    }
                    match items.get_mut(i) {
                        Some(slot) => slot.get_or_insert_with(|| Tree::container(next)),
                        None => return false,
                    }
                }
                _ => return false,
            };
        }
        match (last, node) {
            (Segment::Key(key), Tree::Map(map)) if !map.contains_key(key) => {
                map.insert(key.clone(), Tree::Leaf(value));
                true
            }
            (Segment::Index(_), Tree::List(items)) => {
                let i = index(last).unwrap_or(0);
                if items.len() <= i {
                    items.resize_with(i + 1, || None);
                }
                match items.get_mut(i) {
                    Some(slot @ None) => {
                        *slot = Some(Tree::Leaf(value));
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// The value this tree builds; holes in lists are null.
    pub(super) fn into_value(self) -> Value {
        match self {
            Tree::Leaf(value) => value,
            Tree::Map(map) => {
                Value::Map(map.into_iter().map(|(k, t)| (k, t.into_value())).collect())
            }
            Tree::List(items) => Value::List(
                items
                    .into_iter()
                    .map(|t| t.map_or(Value::Null(NullCause::Null), Tree::into_value))
                    .collect(),
            ),
        }
    }
}

fn names(ruleset: &RuleSet, rules: &[usize]) -> Vec<String> {
    rules
        .iter()
        .filter_map(|&i| ruleset.rules().get(i))
        .map(|r| r.name().to_owned())
        .collect()
}

fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What the rule set's own evaluation gave on a witness.
enum Evaluation {
    /// A decision: whether each rule fired.
    Fired(Vec<bool>),
    /// An error, with its code and the rules it names.
    Failed(ErrorCode, Vec<String>),
}

impl<'r, L: Logic<Truth = Truth>> Auditor<'r, L> {
    pub(super) fn new(ruleset: &'r RuleSet, logic: &'r L, limits: &AuditLimits) -> Self {
        Self {
            ruleset,
            logic,
            limits: *limits,
            reader: Reader::new(limits.max_terms, limits.max_steps),
            supported: Vec::new(),
            diagnostics: Vec::new(),
            undecided: Vec::new(),
        }
    }

    fn name(&self, rule: usize) -> String {
        self.ruleset
            .rules()
            .get(rule)
            .map_or_else(String::new, |r| r.name().to_owned())
    }

    pub(super) fn run(
        &mut self,
        parts: &[Vec<Part<'_>>],
        rows: &[String],
        unlisted: Vec<Diagnostic>,
        fields: CheckStatus,
    ) -> Audit {
        for (rule, conditions) in parts.iter().enumerate() {
            for part in conditions {
                self.reader.scan(rule, part.node);
            }
        }
        let logic = self.logic;
        let truths: Vec<Truths> = parts
            .iter()
            .enumerate()
            .map(|(rule, conditions)| {
                let row = rows.get(rule).map(String::as_str);
                Truths::from_read(self.reader.read_rule(logic, rule, conditions, row))
            })
            .collect();
        self.supported = (0..self.reader.subjects.list.len())
            .map(|id| self.supported_set(id))
            .collect();
        self.types();
        self.diagnostics.extend(unlisted);
        let unsatisfiable = self.unsatisfiable(&truths);
        let policy = self.ruleset.hit_policy();
        let shadowed = if matches!(
            policy,
            HitPolicy::First | HitPolicy::Any | HitPolicy::Priority
        ) {
            self.shadowed(&truths, &unsatisfiable)
        } else {
            CheckStatus::NotApplicable
        };
        let overlap = if policy.single_hit() {
            self.overlaps(&truths, &unsatisfiable)
        } else {
            CheckStatus::NotApplicable
        };
        let unsatisfiable_status = self.status(AuditCheck::Unsatisfiable);
        let gap = self.gap(&truths);
        let mut unmodelled = std::mem::take(&mut self.reader.unmodelled);
        unmodelled.sort_by_key(|u| (u.rule, u.seq));
        let check = |check, status| CheckResult { check, status };
        Audit {
            source: RulesSource::RuleSet,
            document: None,
            hit_policy: table_policy(policy),
            aggregate: self.ruleset.aggregate(),
            logic: self.ruleset.logic(),
            strict: self.ruleset.strict(),
            ruleset_digest: None,
            limits: self.limits,
            rules: Vec::new(),
            checks: vec![
                check(AuditCheck::Types, CheckStatus::Complete),
                check(AuditCheck::Fields, fields),
                check(AuditCheck::Unsatisfiable, unsatisfiable_status),
                check(AuditCheck::Shadowed, shadowed),
                check(AuditCheck::Overlap, overlap),
                check(AuditCheck::Gap, gap),
            ],
            not_modelled: unmodelled
                .into_iter()
                .map(|u| NotModelled {
                    rule: self.name(u.rule),
                    op: u.op,
                    pointer: u.pointer,
                    limit: u.limit,
                })
                .collect(),
            undecided: std::mem::take(&mut self.undecided),
            diagnostics: std::mem::take(&mut self.diagnostics),
            digest: self.ruleset.digest(),
            headers: None,
        }
    }

    /// Complete, or partial when an item of `check` was left undecided.
    fn status(&self, check: AuditCheck) -> CheckStatus {
        if self.undecided.iter().any(|u| u.check == check) {
            CheckStatus::Partial
        } else {
            CheckStatus::Complete
        }
    }

    /// The values of a supported input for a field (`spec/audit.md`, *Gaps*).
    fn supported_set(&self, subject: usize) -> Option<ValueSet> {
        self.reader.subjects.list[subject].path.as_ref()?;
        let groups: Vec<Group> = match self.reader.expected.get(&subject) {
            Some(expected) if !expected.groups.is_empty() => {
                expected.groups.iter().copied().collect()
            }
            _ => ORDERED.to_vec(),
        };
        Some(
            groups
                .iter()
                .fold(ValueSet::none(), |set, g| set.join(&ValueSet::group(*g))),
        )
    }

    fn types(&mut self) {
        let mut incompatible = self.reader.incompatible.clone();
        incompatible.sort_by_key(|i| (i.rule, i.seq));
        for found in incompatible {
            let name = self.name(found.rule);
            let why = if found.null_constant {
                "its constant is null (an expression such as 1 / 0 has no value): fix the constant"
            } else {
                "the values it tests can never be compared with its constant (booleans are not \
                 ordered, arithmetic gives numbers): compare with a value of the type the field \
                 holds"
            };
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::IncompatibleTypes,
                Severity::Warning,
                vec![name.clone()],
                format!(
                    "in rule {name}, {} is UNKNOWN for every input: {why}",
                    found.text
                ),
            );
            diagnostic.pointer = Some(found.pointer);
            self.diagnostics.push(diagnostic);
        }
        let mut mixed: Vec<(usize, usize)> = self
            .reader
            .expected
            .iter()
            .filter(|(id, e)| e.groups.len() > 1 && self.reader.subjects.list[**id].path.is_some())
            .map(|(id, e)| (e.rules.first().copied().unwrap_or(0), *id))
            .collect();
        mixed.sort_unstable();
        for (_, id) in mixed {
            let Some(expected) = self.reader.expected.get(&id) else {
                continue;
            };
            let path = self.reader.subjects.list[id]
                .path
                .clone()
                .unwrap_or_default();
            let groups: Vec<Group> = expected.groups.iter().copied().collect();
            let rules: Vec<usize> = expected.rules.iter().copied().collect();
            let rule_names = names(self.ruleset, &rules);
            let group_names: Vec<&str> = groups.iter().map(|g| g.as_str()).collect();
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::MixedTypes,
                Severity::Warning,
                rule_names.clone(),
                format!(
                    "the field {} is used with values of different types ({}) in {} {}: a value \
                     matches one type only, so use one type for the field, such as 5 or '5' but \
                     not both",
                    crate::text::path(&path),
                    group_names.join(", "),
                    if rule_names.len() == 1 {
                        "rule"
                    } else {
                        "rules"
                    },
                    listed(&rule_names)
                ),
            );
            diagnostic.path = Some(path);
            diagnostic.groups = groups;
            self.diagnostics.push(diagnostic);
        }
    }

    /// Why an item involving `rules` was not decided, before the steps ran out.
    fn reason(&self, rules: &[usize]) -> UndecidedReason {
        let mut unmodelled: Vec<_> = self
            .reader
            .unmodelled
            .iter()
            .filter(|u| rules.contains(&u.rule))
            .collect();
        unmodelled.sort_by_key(|u| (u.rule, u.seq));
        if let Some(limit) = unmodelled.iter().find_map(|u| u.limit) {
            UndecidedReason::Limit(limit)
        } else if unmodelled.is_empty() {
            UndecidedReason::NoWitness
        } else {
            UndecidedReason::NotModelled
        }
    }

    fn undecided(&mut self, check: AuditCheck, rules: &[usize]) {
        let reason = self.reason(rules);
        self.undecided.push(Undecided {
            check,
            rules: if check == AuditCheck::Gap {
                Vec::new()
            } else {
                names(self.ruleset, rules)
            },
            reason,
            remaining: None,
        });
    }

    /// The one entry for the items of `check` that `max_steps` did not
    /// reach, if there were any (`spec/audit.md`, *What was checked*).
    fn unreached(&mut self, check: AuditCheck, remaining: usize) {
        if remaining > 0 {
            self.undecided.push(Undecided {
                check,
                rules: Vec::new(),
                reason: UndecidedReason::Limit(AuditLimit::MaxSteps),
                remaining: Some(remaining),
            });
        }
    }

    /// A witness from a box (`spec/audit.md`, *Witnesses*): `None` when a
    /// field the box constrains cannot be given a value, or, for a supported
    /// input (`complete`), when any field a condition reads cannot.
    fn witness(&self, region: &Region, complete: bool) -> Option<Value> {
        let mut root = Tree::new();
        for (subject, set) in &region.0 {
            // A subject that is not a field is not solved for: the fields it
            // reads get the values of a supported input, below.
            let Some(path) = self.reader.subjects.list[*subject].path.as_ref() else {
                continue;
            };
            let pick = match &self.supported[*subject] {
                Some(supported) => set.pick_within(supported),
                None => set.pick(),
            };
            let Pick::Found(value) = pick else {
                return None;
            };
            if !root.place(path, value) {
                return None;
            }
        }
        for subject in &self.reader.fields {
            if region.0.contains_key(subject) {
                continue;
            }
            let (Some(path), Some(supported)) = (
                self.reader.subjects.list[*subject].path.as_ref(),
                self.supported[*subject].as_ref(),
            ) else {
                continue;
            };
            let placed = match supported.pick() {
                Pick::Found(value) => root.place(path, value),
                _ => false,
            };
            // A field on the path of another is left out, but for a
            // supported input, which gives every field a value.
            if !placed && complete {
                return None;
            }
        }
        Some(root.into_value())
    }

    /// Whether `rule` fires on `input`, without strict mode, as the reasoning
    /// goes; `None` when the steps ran out.
    fn fires(&mut self, rule: usize, input: &Value) -> Option<bool> {
        if !self.reader.work.step() {
            return None;
        }
        let rule = self.ruleset.rules().get(rule)?;
        Some(
            evaluate_tree(rule.when(), input, self.logic, Options::default())
                .ok()
                .and_then(|tree| tree.truth())
                .is_some_and(|truth| self.logic.fires(truth)),
        )
    }

    /// Whether `rules` all fire (`want`) or none does, or `None`.
    fn all_fire(&mut self, rules: &[usize], want: bool, input: &Value) -> Option<bool> {
        for &rule in rules {
            if self.fires(rule, input)? != want {
                return Some(false);
            }
        }
        Some(true)
    }

    /// The rule set's own evaluation of a witness, with its logic and
    /// strictness (`spec/audit.md`, *Witnesses*): one step per rule, or
    /// `None` when they ran out.
    fn evaluate(&mut self, input: &Value) -> Option<Evaluation> {
        if !self.reader.work.take(self.ruleset.rules().len().max(1)) {
            return None;
        }
        Some(match self.ruleset.evaluate(input) {
            Ok(decision) => Evaluation::Fired(decision.results().iter().map(|r| r.fired).collect()),
            Err(error) => Evaluation::Failed(error.code(), error.context().rules.clone()),
        })
    }

    /// The first witness, from the boxes in order, that `shows` accepts.
    fn show(
        &mut self,
        regions: &[Region],
        complete: bool,
        mut shows: impl FnMut(&mut Self, &Value) -> Option<bool>,
    ) -> Shown {
        for region in regions {
            let Some(witness) = self.witness(region, complete) else {
                continue;
            };
            match shows(self, &witness) {
                Some(true) => return Shown::Yes(witness),
                Some(false) => {}
                None => return Shown::Stopped,
            }
        }
        Shown::No
    }

    /// Every intersection of a box of `a` with one of `b`.
    fn meet(&mut self, a: &[Region], b: &[Region]) -> Result<Vec<Region>, Stop> {
        let mut out = Vec::new();
        for x in a {
            for y in b {
                if !self.reader.work.step() {
                    return Err(Stop);
                }
                out.extend(x.meet(y));
            }
        }
        Ok(out)
    }

    /// The inputs of `from` outside every box of `take`.
    fn subtract(&mut self, mut from: Vec<Region>, take: &[Region]) -> Result<Vec<Region>, Stop> {
        for b in take {
            let mut next = Vec::new();
            for a in &from {
                if !self.reader.work.step() {
                    return Err(Stop);
                }
                next.extend(a.minus(b, &self.reader.subjects));
            }
            from = next;
            if from.is_empty() {
                break;
            }
        }
        Ok(from)
    }

    fn unsatisfiable(&mut self, truths: &[Truths]) -> Vec<bool> {
        let mut never = vec![false; truths.len()];
        let mut unreached = 0;
        for (rule, truth) in truths.iter().enumerate() {
            if truth.wide.is_empty() {
                never[rule] = true;
                let name = self.name(rule);
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticCode::Unsatisfiable,
                    Severity::Warning,
                    vec![name.clone()],
                    format!(
                        "rule {name} never fires: no input makes its condition TRUE; fix the \
                         condition, or remove the rule"
                    ),
                ));
                continue;
            }
            if self.reader.work.exhausted {
                unreached += 1;
                continue;
            }
            let mut candidates: Vec<Region> = truth.narrow().to_vec();
            if truth.narrow.is_some() {
                candidates.extend(truth.wide.iter().cloned());
            }
            match self.show(&candidates, false, |me, w| me.fires(rule, w)) {
                Shown::Yes(_) => {}
                Shown::No => self.undecided(AuditCheck::Unsatisfiable, &[rule]),
                Shown::Stopped => unreached += 1,
            }
        }
        self.unreached(AuditCheck::Unsatisfiable, unreached);
        never
    }

    /// The rules the policy selects before `rule` when both fire.
    fn ahead(&self, rule: usize) -> Vec<usize> {
        let rules = self.ruleset.rules();
        match self.ruleset.hit_policy() {
            HitPolicy::Priority => {
                let own = rules[rule].get_priority();
                (0..rules.len())
                    .filter(|&i| {
                        let p = rules[i].get_priority();
                        p > own || (p == own && i < rule)
                    })
                    .collect()
            }
            _ => (0..rule).collect(),
        }
    }

    /// Whether `rule` is shadowed: `Some` with the rules ahead of it that
    /// meet it, `None` when it is not, `Err` when the steps ran out.
    fn covered(
        &mut self,
        truths: &[Truths],
        rule: usize,
        ahead: &[usize],
    ) -> Result<Result<Vec<usize>, Vec<Region>>, Stop> {
        let mut remainder = truths[rule].wide.clone();
        for &i in ahead {
            remainder = self.subtract(remainder, truths[i].narrow())?;
            if remainder.is_empty() {
                break;
            }
        }
        if !remainder.is_empty() {
            return Ok(Err(remainder));
        }
        let mut sorted = ahead.to_vec();
        sorted.sort_unstable();
        let mut by = Vec::new();
        for i in sorted {
            if !self.meet(&truths[i].wide, &truths[rule].wide)?.is_empty() {
                by.push(i);
            }
        }
        Ok(Ok(by))
    }

    fn shadowed(&mut self, truths: &[Truths], never: &[bool]) -> CheckStatus {
        let mut unreached = 0;
        for (rule, &never) in never.iter().enumerate() {
            if never {
                continue;
            }
            let ahead = self.ahead(rule);
            if ahead.is_empty() {
                continue;
            }
            if self.reader.work.exhausted {
                unreached += 1;
                continue;
            }
            let mut involved = vec![rule];
            involved.extend(&ahead);
            involved.sort_unstable();
            let remainder = match self.covered(truths, rule, &ahead) {
                Err(Stop) => {
                    unreached += 1;
                    continue;
                }
                Ok(Ok(by)) => {
                    self.shadow(rule, &by);
                    continue;
                }
                Ok(Err(remainder)) => remainder,
            };
            let shown = self.show(&remainder, false, |me, w| {
                if !me.fires(rule, w)? {
                    return Some(false);
                }
                me.all_fire(&ahead, false, w)
            });
            match shown {
                Shown::Yes(_) => {}
                Shown::No => self.undecided(AuditCheck::Shadowed, &involved),
                Shown::Stopped => unreached += 1,
            }
        }
        self.unreached(AuditCheck::Shadowed, unreached);
        self.status(AuditCheck::Shadowed)
    }

    fn shadow(&mut self, rule: usize, by: &[usize]) {
        let name = self.name(rule);
        let by_names = names(self.ruleset, by);
        let mut rules = vec![name.clone()];
        rules.extend(by_names.iter().cloned());
        let policy = self.ruleset.hit_policy();
        let fix = if policy == HitPolicy::Priority {
            format!("give it a higher priority than {}", listed(&by_names))
        } else {
            format!("move it before {}", listed(&by_names))
        };
        let which = if by_names.len() == 1 {
            format!("rule {}", listed(&by_names))
        } else {
            format!("at least one of rules {}", listed(&by_names))
        };
        self.diagnostics.push(Diagnostic::new(
            DiagnosticCode::Shadowed,
            Severity::Warning,
            rules,
            format!(
                "rule {name} is never selected under the {} hit policy: whenever it fires, \
                 {which} fires too and is selected first; {fix}, or make the conditions exclusive",
                policy.as_str(),
            ),
        ));
    }

    fn overlaps(&mut self, truths: &[Truths], never: &[bool]) -> CheckStatus {
        let n = truths.len();
        let mut unreached = 0;
        for a in 0..n {
            for b in a + 1..n {
                if never[a] || never[b] {
                    continue;
                }
                if self.reader.work.exhausted {
                    unreached += 1;
                    continue;
                }
                let Ok(both) = self.meet(&truths[a].wide, &truths[b].wide) else {
                    unreached += 1;
                    continue;
                };
                if both.is_empty() {
                    continue;
                }
                match self.show(&both, false, |me, w| me.shows_overlap(a, b, w)) {
                    Shown::Yes(witness) => self.overlap(a, b, witness),
                    Shown::No => self.undecided(AuditCheck::Overlap, &[a, b]),
                    Shown::Stopped => unreached += 1,
                }
            }
        }
        self.unreached(AuditCheck::Overlap, unreached);
        self.status(AuditCheck::Overlap)
    }

    /// Whether the rule set's own evaluation of `input` shows that rules
    /// `a` and `b` overlap as their diagnostic says (`spec/audit.md`,
    /// *Overlaps*, *Witnesses*), or `None` when the steps ran out.
    fn shows_overlap(&mut self, a: usize, b: usize, input: &Value) -> Option<bool> {
        let policy = self.ruleset.hit_policy();
        let (x, y) = (self.name(a), self.name(b));
        Some(match self.evaluate(input)? {
            // Under unique both cannot fire without an error.
            Evaluation::Fired(fired) => {
                policy != HitPolicy::Unique
                    && fired.get(a).copied().unwrap_or(false)
                    && fired.get(b).copied().unwrap_or(false)
            }
            Evaluation::Failed(ErrorCode::HitPolicyError, rules)
                if rules.contains(&x) && rules.contains(&y) =>
            {
                match policy {
                    HitPolicy::Unique => true,
                    HitPolicy::Any => self.payloads_differ(a, b, input) == Some(true),
                    _ => false,
                }
            }
            Evaluation::Failed(..) => false,
        })
    }

    /// Whether the payloads of two rules differ on `input`: `None` when they
    /// are structurally identical, so equal on every input.
    fn payloads_differ(&self, a: usize, b: usize, input: &Value) -> Option<bool> {
        let rules = self.ruleset.rules();
        let (pa, pb) = (rules[a].then(), rules[b].then());
        let identical = pa.len() == pb.len()
            && pa
                .iter()
                .zip(pb)
                .all(|((ka, va), (kb, vb))| ka == kb && va == vb);
        if identical {
            return None;
        }
        let options = Options {
            strict: self.ruleset.strict(),
            rule: None,
        };
        let value = |node: &Node| {
            evaluate_value_tree(node, input, self.logic, options)
                .ok()
                .and_then(|tree| tree.value().cloned())
                .unwrap_or(Value::Null(NullCause::Null))
        };
        let same = pa.len() == pb.len()
            && pa
                .iter()
                .zip(pb)
                .all(|((ka, va), (kb, vb))| ka == kb && value(va).any_equal(&value(vb)));
        Some(!same)
    }

    fn overlap(&mut self, a: usize, b: usize, witness: Value) {
        let rules = self.ruleset.rules();
        let (x, y) = (self.name(a), self.name(b));
        let both = format!("rules {x} and {y} both fire for the witness");
        let (severity, message) = match self.ruleset.hit_policy() {
            HitPolicy::Unique => (
                Severity::Error,
                format!(
                    "{both}, so the unique hit policy raises hit_policy_error: make their \
                     conditions exclusive, or use first or priority"
                ),
            ),
            HitPolicy::Any => match self.payloads_differ(a, b, &witness) {
                None => (
                    Severity::Info,
                    format!("{both}; their outputs are the same, as the any hit policy requires"),
                ),
                Some(true) => (
                    Severity::Error,
                    format!(
                        "{both} with different outputs, so the any hit policy raises \
                         hit_policy_error: make their conditions exclusive, or their outputs equal"
                    ),
                ),
                Some(false) => (
                    Severity::Warning,
                    format!(
                        "{both}; their outputs agree here but may differ for another input, \
                         which the any hit policy refuses with hit_policy_error: make their \
                         conditions exclusive, or their outputs equal"
                    ),
                ),
            },
            HitPolicy::Priority => {
                let (pa, pb) = (rules[a].get_priority(), rules[b].get_priority());
                let winner = if pb > pa { &y } else { &x };
                let how = if pa == pb {
                    "with the same priority, rule order decides"
                } else {
                    "the higher priority wins"
                };
                (
                    Severity::Info,
                    format!("{both}; {winner} takes precedence ({how})"),
                )
            }
            _ => (
                Severity::Info,
                format!("{both}; {x} takes precedence over {y} (first hit policy)"),
            ),
        };
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::Overlap, severity, vec![x, y], message);
        diagnostic.witness = Some(witness);
        self.diagnostics.push(diagnostic);
    }

    fn gap(&mut self, truths: &[Truths]) -> CheckStatus {
        let mut supported = Region::default();
        for &field in &self.reader.fields {
            if let Some(set) = &self.supported[field] {
                supported.0.insert(field, set.clone());
            }
        }
        let all: Vec<usize> = (0..truths.len()).collect();
        if self.reader.work.exhausted {
            self.unreached(AuditCheck::Gap, 1);
            return self.status(AuditCheck::Gap);
        }
        let mut remainder = vec![supported];
        for truth in truths {
            let Ok(rest) = self.subtract(remainder, truth.narrow()) else {
                self.unreached(AuditCheck::Gap, 1);
                return self.status(AuditCheck::Gap);
            };
            remainder = rest;
            if remainder.is_empty() {
                return self.status(AuditCheck::Gap);
            }
        }
        // The rule set's own evaluation: no error and no rule fired.
        let shown = self.show(&remainder, true, |me, w| {
            Some(match me.evaluate(w)? {
                Evaluation::Fired(fired) => fired.iter().all(|f| !f),
                Evaluation::Failed(..) => false,
            })
        });
        match shown {
            Shown::Yes(witness) => self.report_gap(witness),
            Shown::No => self.undecided(AuditCheck::Gap, &all),
            Shown::Stopped => self.unreached(AuditCheck::Gap, 1),
        }
        self.status(AuditCheck::Gap)
    }

    fn report_gap(&mut self, witness: Value) {
        let (severity, consequence, fix) = match self.ruleset.hit_policy() {
            HitPolicy::First => (
                Severity::Warning,
                "the decision has no output",
                "add a rule for it, or a last rule with the condition TRUE",
            ),
            HitPolicy::Priority => (
                Severity::Warning,
                "the decision has no output",
                "add a rule for it, or a rule with the condition TRUE and the lowest priority",
            ),
            HitPolicy::Unique | HitPolicy::Any => (
                Severity::Warning,
                "the decision has no output",
                "add a rule whose condition covers it; a rule with the condition TRUE would fire \
                 with every other rule",
            ),
            _ => (
                Severity::Info,
                "the decision selects no rule",
                "add a rule for it if it needs an output",
            ),
        };
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::Gap,
            severity,
            Vec::new(),
            format!("no rule fires for the witness, so {consequence}: {fix}"),
        );
        diagnostic.witness = Some(witness);
        self.diagnostics.push(diagnostic);
    }
}
