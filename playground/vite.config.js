import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'
import { fileURLToPath } from 'node:url'
import { readFileSync } from 'node:fs'

const repoRoot = fileURLToPath(new URL('../', import.meta.url))
const at = (p) => fileURLToPath(new URL(p, import.meta.url))

// The compiler version baked into this build, logged to the console on load so
// you can confirm which @twinkle-lang/twinkle the deployed site is running. For
// the published build it's the version installed in node_modules; for the
// TWINKLE_LOCAL build it's the in-repo version tagged `+local`.
function compilerVersion() {
  try {
    if (process.env.TWINKLE_LOCAL) {
      return JSON.parse(readFileSync(at('../tools/npm/package.json'), 'utf8')).version + '+local'
    }
    return JSON.parse(readFileSync(at('node_modules/@twinkle-lang/twinkle/package.json'), 'utf8')).version
  } catch {
    return 'unknown'
  }
}

// Local-development override (TWINKLE_LOCAL=1): resolve the published packages
// to in-repo build artifacts so the playground runs against current source
// without a publish. The Makefile's playground targets set this. Without it,
// the bare specifiers resolve from node_modules (the published packages).
// Regex finds (not strings) so a `?url` / `?raw` query suffix on the importee
// is preserved: @rollup/plugin-alias replaces only the matched path prefix.
const localAlias = process.env.TWINKLE_LOCAL
  ? [
      { find: /^@twinkle-lang\/twinkle\/web$/, replacement: at('../tools/js_runtime/web.mjs') },
      { find: /^@twinkle-lang\/twinkle\/runtime\.mjs$/, replacement: at('../tools/js_runtime/runtime.mjs') },
      { find: /^@twinkle-lang\/twinkle\/boot\.wasm/, replacement: at('../target/boot.wasm') },
      { find: /^tree-sitter-twinkle\/queries\/highlights\.scm/, replacement: at('../tree-sitter-twinkle/queries/highlights.scm') },
      { find: /^tree-sitter-twinkle\/tree-sitter-twinkle\.wasm/, replacement: at('../tree-sitter-twinkle/tree-sitter-twinkle.wasm') },
    ]
  : []

export default defineConfig({
  plugins: [wasm()],

  // Relative base so the built app works on GitHub Pages at any sub-path
  base: './',

  define: {
    __TWINKLE_COMPILER_VERSION__: JSON.stringify(compilerVersion()),
  },

  resolve: { alias: localAlias },

  // @twinkle-lang/twinkle/web self-loads its wasm via `new URL('./boot.wasm',
  // import.meta.url)`. esbuild's dep pre-bundling would rewrite import.meta.url
  // and break that, so keep the package out of optimizeDeps.
  optimizeDeps: { exclude: ['@twinkle-lang/twinkle'] },

  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // Required for top-level await and wasm
    target: 'esnext',
  },

  // Emit the worker as an ES module so it can `import` the runtime + wasm assets
  worker: { format: 'es' },

  server: {
    // Allow importing in-repo artifacts (tools/, target/, tree-sitter-twinkle/)
    // when the local override is active.
    fs: { allow: [repoRoot] },
  },
})
