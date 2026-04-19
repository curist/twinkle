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


  // Allow ?raw imports from outside the playground/ root (e.g. highlights.scm)
  server: {
    fs: { allow: ['../..'] },
  },
})
