// `require` of the installed package, on Node (the `node` condition: the addon).

'use strict'

const api = require('cleanrules')
const native = require('cleanrules/native')
const { checkAddon, checkResolved, evaluate, platform } = require('./common.cjs')

checkResolved(require.resolve('cleanrules'), 'index.js', 'require cleanrules')
checkResolved(require.resolve('cleanrules/native'), 'native.js', 'require cleanrules/native')
evaluate(api, native, 'require')
checkAddon('require')
console.log(`require: cleanrules ${api.version} on the addon of @cleanrules/node-${platform}`)
