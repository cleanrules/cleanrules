// The rules of discounts.rules, built in code with the idiomatic API of the
// JavaScript package: conditions with `F` and methods, rules and a rule set,
// native values back (a `Decimal` discount), the explanation as objects and
// as text, why a rule did not fire, the summary, a named action bound to a
// function, and a strict-mode error.
//
//     npm --prefix bindings/js install && npm --prefix bindings/js run build
//     node docs/examples/discounts.mjs
//
// The same file runs on the WebAssembly build (`npm run build:wasm`, then
// `deno run docs/examples/discounts.mjs`). In a project, `import { F, Rule,
// RuleSet, decimal } from 'cleanrules'`.

/** Run the example with the package `cr`; the lines it prints. */
export function main(cr) {
  const { F, MissingFieldError, Rule, RuleSet, decimal } = cr
  const lines = []

  const rules = new RuleSet(
    [
      new Rule('blocked', F.customer.blocked, { discount: decimal('0') }, {
        priority: 99,
        description: 'blocked customers never get a discount',
      }),
      new Rule('vip', F.customer.years.gt(5).and(F.order.total.gt(1000)), { discount: decimal('0.15') }, {
        priority: 10,
        tags: ['loyalty'],
        do: 'notify_sales',
      }),
      new Rule('loyal', F.customer.years.gt(2).or(F.customer.newsletter), { discount: decimal('0.05') }, {
        priority: 5,
        tags: ['loyalty'],
      }),
    ],
    { hitPolicy: 'priority' },
  )

  // Native data in, native values out: the discount is a Decimal.
  const order = { customer: { years: 7, newsletter: false }, order: { total: decimal('1500.00') } }
  const decision = rules.evaluate(order)
  lines.push(`discount: ${decision.output.discount}`, `fired: ${decision.fired.join(', ')}`, '')
  lines.push(String(decision.explain()), '')

  // Why a rule did not fire, as objects.
  const small = rules.evaluate({ customer: { years: 7, newsletter: false }, order: { total: decimal('900.00') } })
  for (const cause of small.whyNot('vip').causes) {
    const { op, gap } = cause.counterfactual ?? {}
    lines.push(`vip: ${cause.node} is ${cause.truth}${op ? `; needs ${op}, short by ${gap}` : ''}`)
  }

  lines.push('', String(rules.summary()), '')

  // A named action is data; the program binds it to a function.
  const bound = rules.bind({
    notify_sales: ({ rule, output }) => lines.push(`notify_sales for ${rule.name}: ${output.discount}`),
  })
  bound.run(order)

  // Strict mode throws instead of giving unknown.
  const strict = new RuleSet([new Rule('adult', F.customer.age.ge(18))], { strict: true })
  try {
    strict.evaluate({ customer: {} })
  } catch (error) {
    if (!(error instanceof MissingFieldError)) throw error
    lines.push('', `${error.code} in rule ${error.context.rule}: ${error.context.path.join('.')}`)
  }
  return lines
}

// Run directly (Node 22.12+ or Deno 2.6+), on the build the runtime loads.
const direct = import.meta.main ?? import.meta.filename === globalThis.process?.argv?.[1]
if (direct) {
  const entry = typeof globalThis.Deno === 'object' ? 'index.deno.js' : 'index.js'
  const cr = await import(`../../bindings/js/${entry}`)
  console.log(main(cr).join('\n'))
}
