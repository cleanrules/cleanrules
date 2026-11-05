// `import` of the installed package, on Node (the `node` condition: the addon).

import { createRequire } from 'node:module'

import * as api from 'cleanrules'
import * as native from 'cleanrules/native'

const { checkAddon, checkResolved, evaluate, platform } = createRequire(import.meta.url)('./common.cjs')

checkResolved(import.meta.resolve('cleanrules'), 'index.js', 'import cleanrules')
checkResolved(import.meta.resolve('cleanrules/native'), 'native.js', 'import cleanrules/native')
evaluate(api, native, 'import')
checkAddon('import')
console.log(`import: cleanrules ${api.version} on the addon of @cleanrules/node-${platform}`)
