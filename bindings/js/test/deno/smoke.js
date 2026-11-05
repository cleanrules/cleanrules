// The package loads in Deno with no permission at all: `deno run
// test/deno/smoke.js` (part of `npm run test:deno`). The `deno` condition's
// entries import the module as a module source, so neither fetching nor
// reading a file is needed.

import { MissingFieldError, decimal, version } from 'cleanrules'
import { RuleSet } from 'cleanrules/native'

const rules = RuleSet.read('STRICT\n\nRULE a\nIF price > 1\nTHEN v = price', { format: 'text' })
const output = rules.evaluate({ price: decimal('1.50') }).outputJson()
if (output !== '{"v":{"$decimal":"1.50"}}') throw new Error(`unexpected output ${output}`)
let missing = false
try {
  rules.evaluate({})
} catch (error) {
  missing = error instanceof MissingFieldError
}
if (!missing) throw new Error('no MissingFieldError')
if (!import.meta.resolve('cleanrules/native').endsWith('/native.deno.js')) {
  throw new Error(`cleanrules/native resolved to ${import.meta.resolve('cleanrules/native')}`)
}
console.log(`cleanrules ${version} loads in Deno without permissions`)
