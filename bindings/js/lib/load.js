'use strict'

// Loads the native addon (`binding.js`, the loader napi-rs generates), checks
// that it is the package's own version, and registers the binding's
// JavaScript half with it. Both entry points, `cleanrules` and
// `cleanrules/native`, share it, so it runs once.

const native = require('../binding.js')
const { version } = require('../package.json')
const actions = require('./actions.js')
const errors = require('./errors.js')
const read = require('./read.js')
const values = require('./values.js')

if (native.version() !== version) {
  throw new Error(
    `cleanrules: the native addon is version ${native.version()}, but the package is ${version}; ` +
      'reinstall cleanrules (for example, remove node_modules and run npm install)',
  )
}

read.useTypes(require('node:util').types)
values.setDecimalCheck(native.decimalProblem)
native.setHooks({
  read: read.read,
  error: errors.fromNative,
  actions: actions.actions,
  call: actions.call,
})

module.exports = native
