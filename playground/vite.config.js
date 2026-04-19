import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'

export default defineConfig({
  plugins: [wasm()],

  // Relative base so the built app works on GitHub Pages at any sub-path
  base: './',

  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // Required for top-level await and wasm
    target: 'esnext',
  },

  // web-tree-sitter must not be pre-bundled; it loads its own .wasm at runtime
  optimizeDeps: {
    exclude: ['web-tree-sitter'],
  },
})
