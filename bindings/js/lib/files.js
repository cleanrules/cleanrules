// Reading and writing files for `load` and `dump`, in the runtimes that have
// a file system: Deno (`Deno.readFile`, `Deno.writeTextFile`) and Node
// (`node:fs/promises` through `process.getBuiltinModule`, so that nothing is
// imported and the module still loads in browsers). Browsers have none:
// there `load` and `dump` reject with an `UnsupportedError` that names
// `loads` and `dumps`.

import { UnsupportedError } from './errors.js'

/**
 * The file system of the runtime, for `operation` (`'load'` or `'dump'`):
 * `size(path)`, `read(path)` (the bytes) and `write(path, text)` (as UTF-8),
 * each a promise.
 */
function fileSystem(operation) {
  const deno = globalThis.Deno
  if (typeof deno?.readFile === 'function') {
    return {
      size: async (path) => (await deno.stat(path)).size,
      read: (path) => deno.readFile(path),
      write: (path, text) => deno.writeTextFile(path, text),
    }
  }
  const builtin = globalThis.process?.getBuiltinModule
  if (typeof builtin === 'function') {
    const fs = builtin('node:fs/promises')
    return {
      size: async (path) => (await fs.stat(path)).size,
      read: (path) => fs.readFile(path),
      write: (path, text) => fs.writeFile(path, text, 'utf8'),
    }
  }
  const instead = operation === 'load' ? 'loads(text, { format })' : 'dumps(value, { format })'
  throw new UnsupportedError(`${operation} needs a file system, which this runtime does not have`, {
    hint: `use ${instead} instead, and fetch or save the text yourself`,
    context: { operation, reason: 'this runtime has no file system (a browser)' },
  })
}

export { fileSystem }
