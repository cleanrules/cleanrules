// The decision table of risk.csv with the JavaScript package: read from its
// file, built again from rows in code, evaluated and explained, written as
// YAML; then one rule read from text, a result tree, and a rule set written
// as a document and read back with its action bound by name.
//
//     npm --prefix bindings/js install && npm --prefix bindings/js run build
//     node docs/examples/risk.mjs
//
// The same file runs on the WebAssembly build (`npm run build:wasm`, then
// `deno run --allow-read docs/examples/risk.mjs`). In a project, `import {
// load, DecisionTable } from 'cleanrules'`.

/** Run the example with the package `cr`; the lines it prints. */
export async function main(cr) {
  const { DecisionTable, F, RuleSet, TableOutput, decimal, dumps, evaluateTree, load, loads, ruleFromText } = cr
  const lines = []

  // A decision table from its CSV file (the format comes from the extension).
  const table = await load(new URL('risk.csv', import.meta.url))
  const order = { customer: { years: 9 }, order: { total: 6000 } }
  const decision = table.evaluate(order)
  lines.push(`risk: ${decision.output.risk}, discount: ${decision.output.discount}`)
  lines.push(String(decision.explain()).split('\n')[0])

  // The same table built from rows: strings are cells as the CSV writes them,
  // other values are constants, and a TableOutput lists an output's values.
  const built = new DecisionTable(
    ['RULE', F.customer.years, 'order.total', 'THEN discount', new TableOutput('risk', ['high', 'medium', 'low']), 'DESCRIPTION'],
    [
      ['new', F.customer.years.lt(1), '> 1000', 0, "'high'", 'New customers with large orders are checked first'],
      ['vip', '> 5', '-', decimal('0.15'), "'low'", 'Long-standing customers'],
      ['big', null, '> 5000', decimal('0.05'), "'medium'"],
      ['default', null, null, 0, "'low'"],
    ],
    { hitPolicy: 'priority' },
  )
  lines.push(`same table: ${dumps(built, { format: 'csv' }) === dumps(table, { format: 'csv' })}`)
  lines.push(`rows: ${built.rows.map((row) => row.name).join(', ')}`)
  lines.push(dumps(built, { format: 'yaml' }).split('\n').slice(0, 3).join('\n'))

  // One rule from text (fuzzytool's verb), and why its condition is unknown.
  const rule = ruleFromText('RULE vip IF customer.years > 5 AND order.total > 1000 THEN discount = 0.15')
  const tree = evaluateTree(rule.when, { customer: { years: 7 } })
  lines.push(`${rule.name}: ${tree.truth}; ${tree.children[1].node} is ${tree.children[1].truth} (${tree.children[1].reason.code})`)

  // A rule set written as a document and read back, its action bound by name.
  const rules = new RuleSet([ruleFromText('RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify')])
  const text = dumps(rules, { format: 'yaml' })
  const back = loads(text, {
    format: 'yaml',
    actions: { notify: ({ rule: fired, tree: why }) => lines.push(`notify for ${fired.name}: ${why.node} is ${why.truth}`) },
  })
  back.run(order)
  return lines
}

// Run directly (Node 22.12+ or Deno 2.6+), on the build the runtime loads.
const direct = import.meta.main ?? import.meta.filename === globalThis.process?.argv?.[1]
if (direct) {
  const entry = typeof globalThis.Deno === 'object' ? 'index.deno.js' : 'index.js'
  const cr = await import(`../../bindings/js/${entry}`)
  console.log((await main(cr)).join('\n'))
}
