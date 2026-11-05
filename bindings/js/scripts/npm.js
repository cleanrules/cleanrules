// `npm` as a child process, for the packaging scripts (check-pack.js and
// install-packed.js). On Windows npm is a `.cmd`, which Node only starts
// through a shell, so there the command is one line, with each argument that
// needs it in double quotes.

import { spawnSync } from 'node:child_process'

/** Runs `npm <args>` in `cwd` and returns its standard output; throws if it fails. */
export function npm(args, cwd) {
  const options = { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }
  let result
  if (process.platform === 'win32') {
    const quoted = args.map((arg) => {
      if (arg.includes('"')) throw new Error(`cannot pass ${arg} to npm through cmd.exe`)
      return /[\s&|<>^()%!]/.test(arg) ? `"${arg}"` : arg
    })
    result = spawnSync(['npm', ...quoted].join(' '), { ...options, shell: true })
  } else {
    result = spawnSync('npm', args, options)
  }
  if (result.status !== 0) {
    throw new Error(`npm ${args.join(' ')} failed in ${cwd} (exit ${result.status ?? result.signal})`)
  }
  return result.stdout
}
