// Readers for the packaging check (check-pack.js): the files of an npm
// tarball, and what a native addon is (its format and processor, and the
// symbol versions an ELF one needs from the C library). Node only, no
// dependencies.

import zlib from 'node:zlib'

/**
 * The regular files of a gzipped tarball, by name (`files`), and the names of
 * its other entries (`others`: directories, links...). npm writes ustar
 * entries, with a pax header for a long name.
 */
export function untar(gzipped) {
  const data = zlib.gunzipSync(gzipped)
  const files = new Map()
  const others = []
  let paxPath = null
  for (let at = 0; at + 512 <= data.length; ) {
    const header = data.subarray(at, at + 512)
    if (header.every((byte) => byte === 0)) break
    const field = (start, length) => {
      const raw = header.subarray(start, start + length)
      const end = raw.indexOf(0)
      return raw.subarray(0, end < 0 ? length : end).toString('utf8')
    }
    const size = Number.parseInt(field(124, 12).trim() || '0', 8)
    const type = field(156, 1) || '0'
    const prefix = field(345, 155)
    const body = data.subarray(at + 512, at + 512 + size)
    at += 512 + Math.ceil(size / 512) * 512
    if (type === 'x') {
      paxPath = /(?:^|\n)\d+ path=([^\n]*)\n/.exec(body.toString('utf8'))?.[1] ?? null
      continue
    }
    if (type === 'g') continue
    const name = paxPath ?? (prefix ? `${prefix}/${field(0, 100)}` : field(0, 100))
    paxPath = null
    if (type === '0') files.set(name, body)
    else others.push(name)
  }
  return { files, others }
}

/** `{ format, arch }` of an executable's header, or null if it is none of the three formats. */
export function binaryKind(bytes) {
  if (bytes.length >= 20 && bytes.readUInt32BE(0) === 0x7f454c46) {
    return { format: 'ELF', arch: { 0x3e: 'x64', 0xb7: 'arm64' }[bytes.readUInt16LE(18)] ?? 'other' }
  }
  if (bytes.length >= 8 && bytes.readUInt32LE(0) === 0xfeedfacf) {
    return { format: 'Mach-O', arch: { 0x01000007: 'x64', 0x0100000c: 'arm64' }[bytes.readUInt32LE(4)] ?? 'other' }
  }
  if (bytes.length >= 64 && bytes.readUInt16BE(0) === 0x4d5a) {
    const pe = bytes.readUInt32LE(0x3c)
    if (pe + 6 <= bytes.length && bytes.readUInt32BE(pe) === 0x50450000) {
      return { format: 'PE', arch: { 0x8664: 'x64', 0xaa64: 'arm64' }[bytes.readUInt16LE(pe + 4)] ?? 'other' }
    }
  }
  return null
}

/**
 * The symbol versions a 64-bit little-endian ELF object needs, as
 * `{ file, version }` (from its `SHT_GNU_verneed` section: what `objdump -T`
 * and `readelf -V` show as required versions).
 */
export function neededVersions(bytes) {
  const shoff = Number(bytes.readBigUInt64LE(0x28))
  const shentsize = bytes.readUInt16LE(0x3a)
  const shnum = bytes.readUInt16LE(0x3c)
  const section = (index) => {
    const at = shoff + index * shentsize
    return {
      type: bytes.readUInt32LE(at + 4),
      offset: Number(bytes.readBigUInt64LE(at + 24)),
      link: bytes.readUInt32LE(at + 40),
      info: bytes.readUInt32LE(at + 44),
    }
  }
  const text = (strings, offset) => {
    const start = strings.offset + offset
    return bytes.subarray(start, bytes.indexOf(0, start)).toString('latin1')
  }
  const needed = []
  for (let index = 0; index < shnum; index++) {
    const verneed = section(index)
    if (verneed.type !== 0x6ffffffe) continue
    const strings = section(verneed.link)
    let entry = verneed.offset
    for (let n = 0; n < verneed.info; n++) {
      const file = text(strings, bytes.readUInt32LE(entry + 4))
      let aux = entry + bytes.readUInt32LE(entry + 8)
      for (let m = 0; m < bytes.readUInt16LE(entry + 2); m++) {
        needed.push({ file, version: text(strings, bytes.readUInt32LE(aux + 8)) })
        aux += bytes.readUInt32LE(aux + 12)
      }
      entry += bytes.readUInt32LE(entry + 12)
    }
  }
  return needed
}
