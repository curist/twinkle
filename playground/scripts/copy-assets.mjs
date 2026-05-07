#!/usr/bin/env node
// Copies build artifacts and source assets into public/ before Vite builds.
//
// Must be run after `cargo build` (produces bridge.wasm + boot.wasm)
// and after `bun install` / `npm install` (provides tree-sitter.wasm).
//
// Usage:  node scripts/copy-assets.mjs
//         bun run copy-assets

import { cpSync, copyFileSync, mkdirSync, existsSync } from 'fs'
import { fileURLToPath } from 'url'
import { join, dirname } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '../..')   // twinkle repo root
const publicDir   = join(__dirname, '../public')

let warnings = 0

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true })
}

function copyFile(src, dest) {
  if (!existsSync(src)) {
    console.warn(`  [warn] missing: ${src}`)
    warnings++
    return
  }
  copyFileSync(src, dest)
  console.log(`  copied: ${dest.replace(publicDir + '/', 'public/')}`)
}

function copyDir(src, dest) {
  if (!existsSync(src)) {
    console.warn(`  [warn] missing dir: ${src}`)
    warnings++
    return
  }
  // dereference: follow symlinks (boot/prelude and boot/stdlib are symlinks)
  cpSync(src, dest, { recursive: true, force: true, dereference: true })
  console.log(`  copied: ${dest.replace(publicDir + '/', 'public/')} (dir)`)
}

ensureDir(publicDir)

// ── Wasm artifacts (require prior `cargo build`) ──────────────────────────
console.log('\nwasm artifacts:')
copyFile(join(projectRoot, 'tools/bridge.wasm'),      join(publicDir, 'bridge.wasm'))
copyFile(join(projectRoot, 'target/boot.wasm'),  join(publicDir, 'boot.wasm'))

// ── Twinkle source files (always present) ─────────────────────────────────
console.log('\ntwinkle source:')
copyDir(join(projectRoot, 'boot/prelude'), join(publicDir, 'prelude'))
copyDir(join(projectRoot, 'boot/stdlib'),  join(publicDir, 'stdlib'))

// ── Tree-sitter wasm (requires prior `bun install` / `npm install`) ───────
console.log('\ntree-sitter:')
const tsWasm = join(__dirname, '../node_modules/web-tree-sitter/tree-sitter.wasm')
copyFile(tsWasm, join(publicDir, 'tree-sitter.wasm'))

// ── Tree-sitter-twinkle wasm (requires prior `tree-sitter build --wasm`) ──
const tsTwinkleWasm = join(projectRoot, 'tree-sitter-twinkle/tree-sitter-twinkle.wasm')
copyFile(tsTwinkleWasm, join(publicDir, 'tree-sitter-twinkle.wasm'))

console.log(warnings > 0
  ? `\ncopy-assets done with ${warnings} warning(s) — see above`
  : '\ncopy-assets done')
