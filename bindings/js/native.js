'use strict'

// `cleanrules/native`: the native layer (task F.1a), the classes of the addon
// with their JavaScript half registered. The idiomatic API of phase F.2 sits on
// top of it; errors and marker values are in `cleanrules`.

const native = require('./lib/load.js')

exports.Document = native.Document
exports.RuleSet = native.RuleSet
exports.Decision = native.Decision
exports.Node = native.Node
exports.FORMAT_VERSION = native.FORMAT_VERSION
