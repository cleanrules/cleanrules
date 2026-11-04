'use strict'

// Both entry points, for the tests: `cleanrules` (errors, markers, versions)
// and `cleanrules/native` (the native classes).

module.exports = { ...require('../..'), ...require('../../native') }
