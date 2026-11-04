// The rules of discounts.rules, from Node: read them, evaluate native data and
// JSON, print the output, the explanation and which rules fired, run the
// rules' named action (notify_sales) as a function, and catch an error.
//
//     npm --prefix bindings/js install && npm --prefix bindings/js run build
//     node docs/examples/discounts.js docs/examples/discounts.rules
//
// In a project with the package installed, `require('cleanrules/native')` for
// the native layer used here and `require('cleanrules')` for errors and values.

'use strict'

const fs = require('node:fs')
const path = require('node:path')

const js = path.join(__dirname, '..', '..', 'bindings', 'js')
const { MissingFieldError, decimal } = require(js)
const { RuleSet } = require(path.join(js, 'native'))

const file = process.argv[2]
if (file === undefined) {
  console.error('usage: node discounts.js discounts.rules')
  process.exit(2)
}
const rules = RuleSet.read(fs.readFileSync(file), { format: 'text' })

// Native data: only the fields the rules read are read. A getter counts as a
// field; `decimal(...)` is an exact decimal, as JSON's {"$decimal": ...}.
class Customer {
  constructor(years) {
    this.years = years
  }

  get newsletter() {
    return false
  }
}
const order = { customer: new Customer(7), order: { total: decimal('1500.00') } }

const decision = rules.evaluate(order)
console.log(`output: ${decision.outputJson()}\n`)
console.log(decision.explain())
for (let i = 0; i < decision.length; i += 1) {
  console.log(`${decision.ruleName(i)} ${decision.firedAt(i) ? 'fired' : 'did not fire'}`)
}

// The same input as JSON gives the same decision.
const json = rules.evaluateJson('{"customer": {"years": 7, "newsletter": false}, "order": {"total": {"$decimal": "1500.00"}}}')
console.log(`\nsame decision from JSON: ${json.write() === decision.write()}`)

// Run the rules: evaluate, then call the selected rules' actions.
rules.run(order, {
  notify_sales: ({ ruleName, outputJson }) => {
    console.log(`notify_sales for ${ruleName}: ${outputJson}`)
  },
})

// Errors are classes with a stable code and their context.
const strict = RuleSet.read('STRICT\n\nRULE adult\nIF customer.age >= 18', { format: 'text' })
try {
  strict.evaluate({ customer: {} })
} catch (error) {
  if (!(error instanceof MissingFieldError)) throw error
  console.log(`\n${error.code} in rule ${error.context.rule}: ${error.context.path.join('.')}`)
}
