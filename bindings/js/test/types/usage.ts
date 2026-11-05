// A typed use of the package, checked by `npm test` (tsc --noEmit).

import {
  ActionError,
  CleanRulesError,
  FORMAT_VERSION,
  MissingFieldError,
  NaiveDateTime,
  date,
  datetime,
  decimal,
  float,
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
    const decided: Decision | null = error.decision
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
