// A typed use of the package, checked by `npm test` (tsc --noEmit).

import {
  ActionContext,
  ActionError,
  type Aggregate as RootAggregate,
  Cause,
  CauseGroup,
  CleanRulesError,
  Const,
  type Constant,
  Counterfactual,
  Decimal,
  Decision as RootDecision,
  Explanation,
  Expr,
  F,
  FORMAT_VERSION,
  Field,
  type FieldRef,
  type Fields,
  type Gap,
  type Tag,
  FieldUsage,
  type HitPolicy as RootHitPolicy,
  type Kind,
  MissingFieldError,
  NaiveDateTime,
  Note,
  type Output,
  Reason,
  Rule,
  RuleExplanation,
  RuleNotFoundError,
  RuleResult,
  RuleSet as RootRuleSet,
  RuleSummary,
  type Status,
  Summary,
  type Truth as RootTruth,
  type Value,
  ZonedDateTime,
  and,
  constant,
  date,
  datetime,
  decimal,
  field,
  float,
  not,
  or,
  version,
} from 'cleanrules'
import {
  type ActionCall,
  type Aggregate,
  Decision,
  Document,
  type DocumentKind,
  type Format,
  type HitPolicy,
  type Limits,
  Node,
  RuleSet,
  type Truth,
} from 'cleanrules/native'

const limits: Limits = { maxSize: 1_000_000, maxDepth: 64 }
const format: Format = 'text'
const ruleset: RuleSet = RuleSet.read('RULE vip\nIF customer.years > 5\nTHEN discount = 0.15\nDO notify', { format, limits })
const names: string[] = ruleset.ruleNames()
const paths: Array<Array<string | number>> = ruleset.fieldPaths()
const policy: HitPolicy = ruleset.hitPolicy
const aggregate: Aggregate | null = ruleset.aggregate

const seen: NaiveDateTime = datetime(2026, 11, 4, 9, 30)
const decision: Decision = ruleset.evaluate({
  customer: { years: 6, since: date(2020, 1, 1), seen },
  price: decimal('10.50'),
  weight: float(2),
})
const selected: number[] = decision.selected()
const fired: boolean = decision.firedAt(0)
const truth: Truth = decision.truth(0)
const output: string | null = decision.outputJson()
const ruleOutput: string | null = decision.ruleOutputJson(0)
const explained: string = decision.explain()

const fromJson: Decision = ruleset.evaluateJson('{"customer": {"years": 6}}')
const batch: Decision[] = ruleset.evaluateMany([{ customer: { years: 1 } }, { customer: { years: 9 } }])
const recorded: Decision = ruleset.readDecision(decision.write('yaml'), { format: 'yaml' })

const ran: Decision = ruleset.run(
  { customer: { years: 6 } },
  {
    notify: (call: ActionCall) => {
      const rule: string = call.ruleName
      const position: number = call.position
      const condition: Truth = call.truth
      const payload: string = call.outputJson
      const same: Decision = call.decision
      void [rule, position, condition, payload, same]
    },
  },
)

const document: Document = Document.read(new Uint8Array([123, 125]), { format: 'json' })
const kind: DocumentKind = document.kind
const node: Node = Node.read('x > 1', { format: 'text' })
const tree: string = node.evaluateTree({ x: 2 }, { logic: 'boolean', strict: true })
const valueTree: string = node.evaluateValueTreeJson('{"x": 2}', { strict: false, limits })

try {
  ruleset.evaluate({})
} catch (error) {
  if (error instanceof MissingFieldError) {
    const path: ReadonlyArray<string | number> | undefined = error.context.path
    const code: string = error.code
    const hint: string | null = error.hint
    void [path, code, hint]
  } else if (error instanceof ActionError) {
    const decided: Decision | RootDecision | null = error.decision
    const executed: readonly string[] = error.context.executed
    void [decided, executed]
  } else if (error instanceof CleanRulesError) {
    const form: Record<string, unknown> = error.toJSON()
    void form
  }
}

const made = new CleanRulesError('custom', { code: 'format_error', context: { pointer: '/a' } })
const versions: [number, string] = [FORMAT_VERSION, version]

// @ts-expect-error: formats are a closed set.
Document.read('{}', { format: 'xml' })

// @ts-expect-error: rule sets come from RuleSet.read, not `new`.
new RuleSet()

void [names, paths, policy, aggregate, selected, fired, truth, output, ruleOutput, explained, fromJson, batch, recorded, ran, kind, tree, valueTree, made, versions]

// --- The idiomatic API ---

const adult: Expr = F.customer.age.ge(18).and(F.country.isIn(['ES', 'PT']), not(F.blocked.eq(true)))
const age: FieldRef = F.customer.age
const deeper: FieldRef = F.customer.address.city
const indexed: Field = field('items', 0, 'price')
const clash: FieldRef = F.x.at('and')
const segments: ReadonlyArray<string | number> = indexed.path
const one: Const = constant(decimal('0.10'))
const value: Value = one.value
const total: Expr = F.price.mul(F.quantity).sub(5).add(constant(1)).div(2).neg().abs()
const tests: Expr[] = [
  F.name.matches('^A'),
  F.name.startsWith('A'),
  F.name.endsWith('z'),
  F.name.contains('m'),
  F.note.isNull(),
  F.age.between(18, 65, { closed: 'left' }),
  F.day.lt(date(2026, 1, 1)),
  F.at.le(new Date(0)),
  or(F.a.ne(1), F.b.gt(2n)).not(),
  and(F.a.eq('x'), F.b.lt(float(2))),
]
const constants: Constant[] = [null, true, 1, 2n, 'x', decimal('1'), float(1), date(2026, 1, 1), new Date()]
const tag: string = adult.tag
const nodeKind: Kind = adult.kind
const children: readonly Expr[] = adult.children()
const same: boolean = adult.sameAs(adult)
const text: string = adult.toString()
const json: Record<string, unknown> = adult.toJSON()

const notify = (context: ActionContext): void => {
  const rule: Rule = context.rule
  const result: RuleResult = context.result
  const payload: Output = context.output
  const decision: RootDecision = context.decision
  void [rule, result, payload, decision, context.data]
}
const vip = new Rule('vip', F.customer.years.gt(5), { discount: decimal('0.15'), base: F.price }, {
  priority: 10,
  tags: ['loyalty'],
  description: 'long-standing customers',
  metadata: { owner: 'sales', since: date(2020, 1, 1) },
  do: notify,
})
const loyal = new Rule('loyal', F.customer.years.gt(2), new Map([['discount', 0.05]]), { do: 'notify', priority: 5n })
const rules = new RootRuleSet([vip, loyal], { hitPolicy: 'priority', strict: false, logic: 'boolean' })
const collect = new RootRuleSet([vip, loyal], { hitPolicy: 'collect', aggregate: 'sum' })
const bound: RootRuleSet = rules.bind({ notify })
const rulePolicy: RootHitPolicy = rules.hitPolicy
const ruleAggregate: RootAggregate | null = collect.aggregate
const first: Rule = rules.rule('vip')
const maybe: Rule | undefined = rules.get('vip')
const has: boolean = rules.has('vip')
const count: number = rules.length
for (const rule of rules) void rule.name
const ruleThen: Readonly<Record<string, Expr>> = vip.then
const priority: number | bigint = loyal.priority
const action: string | null = loyal.actionName
const decided: RootDecision = bound.run({ customer: { years: 6 }, price: 10 })
const many: RootDecision[] = rules.evaluateMany([{ customer: { years: 1 } }])
const replay: RootDecision = rules.readDecision(JSON.stringify(decided.toJSON()), { format: 'json' })
const out: Output | null = decided.output
const outs: readonly Output[] = decided.outputs
const firedNames: readonly string[] = decided.fired
const results: readonly RuleResult[] = decided.results
const truthOf: RootTruth = results[0].truth
const aggregated: Output | null = collect.evaluate({ customer: { years: 9 } }).aggregated
const explanation: Explanation = decided.explain()
const why: RuleExplanation = decided.whyNot('loyal')
const status: Status = why.status
const requirement: Cause | CauseGroup = why.requirement
if (requirement instanceof Cause) {
  const node: Expr = requirement.node
  const read: ReadonlyArray<readonly [ReadonlyArray<string | number>, Value]> = requirement.values
  const reason: Reason | null = requirement.reason
  const counterfactual: Counterfactual | null = requirement.counterfactual
  void [node, read, reason, counterfactual, counterfactual?.gap]
}
const note: Note | null = explanation.forRule('vip').note
const summary: Summary = rules.summary()
const ruleSummaries: readonly RuleSummary[] = summary.rules
const usage: readonly FieldUsage[] = summary.fields
const tags: Readonly<Record<string, readonly string[]>> = summary.tags
const when: unknown = out?.when
if (when instanceof ZonedDateTime) {
  const instant: bigint = when.epochMicroseconds
  const zone: string | null = when.timeZone
  const asDate: Date = when.toDate()
  void [instant, zone, asDate]
}
if (out?.discount instanceof Decimal) void out.discount.text
try {
  rules.get('nope')
} catch (error) {
  if (error instanceof RuleNotFoundError) void error.context.available
}

// @ts-expect-error: nodes are built with F and the functions, never `new`.
new Expr()
// @ts-expect-error: `closed` is a closed set.
F.age.between(1, 2, { closed: 'open' })
// @ts-expect-error: decisions come from rule sets.
new RootDecision()
// @ts-expect-error: hit policies are a closed set.
new RootRuleSet([], { hitPolicy: 'last' })

void [adult, age, deeper, clash, segments, value, total, tests, constants, tag, nodeKind, children, same, text, json]
void [maybe, first, has, count, ruleThen, priority, action, many, replay, outs, firedNames, truthOf, aggregated, status, note]
void [ruleSummaries, usage, tags, rulePolicy, ruleAggregate]

// --- After the API review ---

const nodeTag: Tag = adult.tag
const walked: Expr[] = [...adult.walk()]
const paths2: ReadonlyArray<ReadonlyArray<string | number>> = adult.fieldPaths()
const setPaths: ReadonlyArray<ReadonlyArray<string | number>> = rules.fieldPaths()
const thenOfField: undefined = F.customer.then
interface Shop {
  customer: { years: number; tags: string[] }
}
const S = F as Fields<Shop>
const typedYears: Field = S.customer.years
const typedTag: Field = S.customer.tags.at(0)
const gapOf: Gap | null | undefined = why.causes[0]?.counterfactual?.gap
// @ts-expect-error: a string is not a collection of constants.
F.country.isIn('ES')
// @ts-expect-error: typed keys below the first are checked.
void S.customer.age
// Rules and rule sets cannot be written yet: toJSON() throws (never returns).
type Written = ReturnType<Rule['toJSON']> | ReturnType<RootRuleSet['toJSON']>
const written: [Written] extends [never] ? true : false = true
void [nodeTag, walked, paths2, setPaths, thenOfField, typedYears, typedTag, gapOf, written]
