// The rules of discounts.rules in Deno, on the WebAssembly build: read them,
// evaluate native data and JSON, print the output, the explanation and which
// rules fired, run the rules' named action, and catch an error.
//
//     npm --prefix bindings/js run build:wasm
//     deno run --allow-read=docs/examples docs/examples/discounts.deno.js docs/examples/discounts.rules
//
// Loading the package needs no permission; reading the rules file needs
// `--allow-read`. In a project, import from 'npm:cleanrules' and
// 'npm:cleanrules/native' (Deno 2.6 or later).

import { MissingFieldError, decimal } from '../../bindings/js/index.deno.js'
import { RuleSet } from '../../bindings/js/native.deno.js'

/** Run the example on the rules `text`; the lines it prints. */
export function main(text) {
  const lines = []
  const rules = RuleSet.read(text, { format: 'text' })

  const order = { customer: { years: 7, newsletter: false }, order: { total: decimal('1500.00') } }
  const decision = rules.evaluate(order)
  lines.push(`output: ${decision.outputJson()}`, '', decision.explain())
  for (let i = 0; i < decision.length; i += 1) {
    lines.push(`${decision.ruleName(i)} ${decision.firedAt(i) ? 'fired' : 'did not fire'}`)
  }

  const json = rules.evaluateJson(JSON.stringify({ ...order, order: { total: { $decimal: '1500.00' } } }))
  lines.push('', `same decision from JSON: ${json.write() === decision.write()}`)

  rules.run(order, {
    notify_sales: ({ ruleName, outputJson }) => lines.push(`notify_sales for ${ruleName}: ${outputJson}`),
  })

  const strict = RuleSet.read('STRICT\n\nRULE adult\nIF customer.age >= 18', { format: 'text' })
  try {
    strict.evaluate({ customer: {} })
  } catch (error) {
    if (!(error instanceof MissingFieldError)) throw error
    lines.push('', `${error.code} in rule ${error.context.rule}: ${error.context.path.join('.')}`)
  }
  return lines
}

if (import.meta.main) {
  const file = Deno.args[0] ?? new URL('./discounts.rules', import.meta.url)
  console.log(main(await Deno.readTextFile(file)).join('\n'))
}
