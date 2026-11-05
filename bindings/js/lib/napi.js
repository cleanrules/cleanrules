// The Node addon (`binding.cjs`, the loader napi-rs generates), checked to be
// the package's own version, with the JavaScript half registered. Node only:
// the `node` condition of `package.json` picks the entries that import it.

import { createRequire } from 'node:module'
import { types } from 'node:util'

import { install } from './install.js'

const require = createRequire(import.meta.url)
const native = require('../binding.cjs')
const { version } = require('../package.json')

if (native.version() !== version) {
  throw new Error(
    `cleanrules: the native addon is version ${native.version()}, but the package is ${version}; ` +
      'reinstall cleanrules (for example, remove node_modules and run npm install)',
  )
}

install(native, types)

export default native
