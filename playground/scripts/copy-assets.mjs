#!/usr/bin/env node
// Copies build artifacts and source assets into public/ before Vite builds.
//
// Must be run after building tools/bridge.wasm and target/boot.wasm,
// and after `npm install` / `npm ci` (provides tree-sitter.wasm).
//
// Usage:  node scripts/copy-assets.mjs
//         npm run copy-assets

import { cpSync, copyFileSync, mkdirSync, existsSync, readFileSync } from 'fs'
import { fileURLToPath } from 'url'
import { join, dirname } from 'path'
import { Parser, Language, Query } from 'web-tree-sitter'

const __dirname = dirname(fileURLToPath(import.meta.url))
const projectRoot = join(__dirname, '../..')   // twinkle repo root
const publicDir   = join(__dirname, '../public')
const highlightsQuery = join(projectRoot, 'tree-sitter-twinkle/queries/highlights.scm')

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true })
}

function requirePath(path) {
  if (!existsSync(path)) {
    throw new Error(`missing required asset: ${path}`)
  }
}

function copyFile(src, dest) {
  requirePath(src)
  copyFileSync(src, dest)
  console.log(`  copied: ${dest.replace(publicDir + '/', 'public/')}`)
}

function copyDir(src, dest) {
  requirePath(src)
  // dereference: follow symlinks (boot/prelude and boot/stdlib are symlinks)
  cpSync(src, dest, { recursive: true, force: true, dereference: true })
  console.log(`  copied: ${dest.replace(publicDir + '/', 'public/')} (dir)`)
}

ensureDir(publicDir)

// ── Wasm artifacts ───────────────────────────────────────────────────────
console.log('\nwasm artifacts:')
copyFile(join(projectRoot, 'tools/bridge.wasm'),      join(publicDir, 'bridge.wasm'))
copyFile(join(projectRoot, 'target/playground.wasm'),  join(publicDir, 'boot.wasm'))

// ── Twinkle source files (always present) ─────────────────────────────────
console.log('\ntwinkle source:')
copyDir(join(projectRoot, 'boot/prelude'), join(publicDir, 'prelude'))
copyDir(join(projectRoot, 'boot/stdlib'),  join(publicDir, 'stdlib'))

// ── Tree-sitter wasm (requires prior `npm install` / `npm ci`) ───────────
console.log('\ntree-sitter:')
const tsWasm = join(__dirname, '../node_modules/web-tree-sitter/tree-sitter.wasm')
const publicTsWasm = join(publicDir, 'tree-sitter.wasm')
copyFile(tsWasm, publicTsWasm)

// ── Tree-sitter-twinkle wasm (requires prior `tree-sitter build --wasm`) ──
const tsTwinkleWasm = join(projectRoot, 'tree-sitter-twinkle/tree-sitter-twinkle.wasm')
const publicTwinkleWasm = join(publicDir, 'tree-sitter-twinkle.wasm')
copyFile(tsTwinkleWasm, publicTwinkleWasm)

// Fail the build if the checked-in grammar wasm is stale relative to the
// highlighting query. Otherwise the playground silently falls back to plain text.
await Parser.init({ locateFile: () => publicTsWasm })
const lang = await Language.load(publicTwinkleWasm)
new Query(lang, readFileSync(highlightsQuery, 'utf8'))
console.log('  validated: tree-sitter highlight query')

console.log('\ncopy-assets done')
