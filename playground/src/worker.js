// Web Worker — runs Twinkle in the browser by sharing the published compiler
// runtime (@twinkle-lang/twinkle) instead of a hand-maintained fork.
//
// Receives: { type: 'run', code: string, offscreenCanvas? }
// Posts:    { type: 'ready' | 'status' | 'stdout' | 'stderr' | 'done' | 'error', ... }
//
// The package's runtime.mjs is host-agnostic: it routes every filesystem and
// stdin host import through the `host` adapter we inject below, and resolves
// `extern` imports against globalThis (canvas/http/timer). The full boot.wasm
// embeds the prelude + stdlib via core_lib, so the only file the compiler reads
// is the user's /input/main.tw.

import { runWasmBytesAsync } from '@twinkle-lang/twinkle/runtime.mjs'
import bootWasmUrl from '@twinkle-lang/twinkle/boot.wasm?url'
import bridgeWasmUrl from '@twinkle-lang/twinkle/bridge.wasm?url'

const textDecoder = new TextDecoder()
const textEncoder = new TextEncoder()

// ---------------------------------------------------------------------------
// Compiler payloads (loaded once)
// ---------------------------------------------------------------------------

let cachedBootBytes = null
let cachedBridgeBytes = null
let loadPromise = null

function loadResources() {
  if (!loadPromise) loadPromise = doLoadResources()
  return loadPromise
}

async function doLoadResources() {
  self.postMessage({ type: 'status', text: 'Loading compiler…' })
  const [boot, bridge] = await Promise.all([
    fetch(bootWasmUrl).then((r) => r.arrayBuffer()),
    fetch(bridgeWasmUrl).then((r) => r.arrayBuffer()),
  ])
  cachedBootBytes = new Uint8Array(boot)
  cachedBridgeBytes = new Uint8Array(bridge)
}

// ---------------------------------------------------------------------------
// Browser host adapter — in-memory VFS + EOF stdin
// ---------------------------------------------------------------------------

function makeBrowserHost(files) {
  const norm = (p) => {
    if (!p.startsWith('/')) p = '/' + p
    return p.replace(/\/+/g, '/')
  }
  return {
    resolvePath(cwd, p) {
      if (p.startsWith('/')) return norm(p)
      return norm((cwd.endsWith('/') ? cwd : cwd + '/') + p)
    },
    readFile(path) {
      const data = files.get(norm(path))
      if (data === undefined) throw new Error(`file not found: ${path}`)
      return data
    },
    writeFile(path, text) { files.set(norm(path), textEncoder.encode(text)) },
    writeBytes(path, bytes) { files.set(norm(path), bytes) },
    exists(path) {
      const np = norm(path)
      if (files.has(np)) return true
      const prefix = np.endsWith('/') ? np : np + '/'
      for (const k of files.keys()) if (k.startsWith(prefix)) return true
      return false
    },
    listDir(path) {
      const prefix = norm(path).replace(/\/?$/, '/')
      const names = new Set()
      for (const k of files.keys()) {
        if (k.startsWith(prefix)) {
          const name = k.slice(prefix.length).split('/')[0]
          if (name) names.add(name)
        }
      }
      return [...names].sort()
    },
    mkdirp() { /* virtual dirs are implicit */ },
    readStdin(_maxBytes, _timeoutMs, runtime) {
      runtime.stdinEof = true
      return new Uint8Array(0)
    },
    readStdinAsync(_maxBytes, _timeoutMs, runtime) {
      runtime.stdinEof = true
      return Promise.resolve(new Uint8Array(0))
    },
  }
}

// ---------------------------------------------------------------------------
// Extern globals (resolved by the runtime via globalThis)
// ---------------------------------------------------------------------------

globalThis.timer = {
  sleep_ms: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
}

globalThis.http = {
  fetch: async (url) => {
    const r = await fetch(url)
    if (!r.ok) throw new Error(`HTTP ${r.status}: ${r.statusText}`)
    return r.text()
  },
  fetch_bytes: async (url) => {
    const r = await fetch(url)
    if (!r.ok) throw new Error(`HTTP ${r.status}: ${r.statusText}`)
    return new Uint8Array(await r.arrayBuffer())
  },
}

function setupCanvas(offscreen) {
  const ctx = offscreen.getContext('2d')
  globalThis.canvas = {
    get_context: () => ctx ?? null,
    get_width: () => offscreen.width,
    get_height: () => offscreen.height,
    set_fill_style: (c, color) => { c.fillStyle = color },
    fill_rect: (c, x, y, w, h) => c.fillRect(x, y, w, h),
    clear_rect: (c, x, y, w, h) => c.clearRect(x, y, w, h),
    set_stroke_style: (c, color) => { c.strokeStyle = color },
    stroke_rect: (c, x, y, w, h) => c.strokeRect(x, y, w, h),
    begin_path: (c) => c.beginPath(),
    close_path: (c) => c.closePath(),
    move_to: (c, x, y) => c.moveTo(x, y),
    line_to: (c, x, y) => c.lineTo(x, y),
    arc: (c, x, y, r, s, e) => c.arc(x, y, r, s, e),
    fill: (c) => c.fill(),
    stroke: (c) => c.stroke(),
    set_line_width: (c, w) => { c.lineWidth = w },
    set_global_alpha: (c, a) => { c.globalAlpha = a },
    set_font: (c, font) => { c.font = font },
    fill_text: (c, text, x, y) => c.fillText(text, x, y),
  }
}

// Canvas args carry the 2D-context externref, which must be passed through
// untouched: decoding it as a Wasm GC string recurses until a stack overflow in
// some engines (notably Safari). The runtime honors this per-arg marshal spec.
const RAW = (n) => Array(n).fill('raw')
const marshalSpec = {
  canvas: {
    get_context: ['string'],
    set_fill_style: ['raw', 'string'],
    fill_rect: RAW(5),
    clear_rect: RAW(5),
    set_stroke_style: ['raw', 'string'],
    stroke_rect: RAW(5),
    begin_path: ['raw'],
    close_path: ['raw'],
    move_to: RAW(3),
    line_to: RAW(3),
    arc: RAW(6),
    fill: ['raw'],
    stroke: ['raw'],
    set_line_width: RAW(2),
    set_global_alpha: RAW(2),
    set_font: ['raw', 'string'],
    fill_text: ['raw', 'string', 'raw', 'raw'],
  },
}

// ---------------------------------------------------------------------------
// stdout / stderr streams over postMessage
// ---------------------------------------------------------------------------

function makeStream(stream) {
  // Stream-decode so a multi-byte UTF-8 sequence split across byte writes is
  // not corrupted. String chunks (print/println) pass through directly.
  const dec = new TextDecoder()
  return {
    write(chunk) {
      const text = typeof chunk === 'string' ? chunk : dec.decode(chunk, { stream: true })
      if (text) self.postMessage({ type: stream, text })
      return true
    },
  }
}

// ---------------------------------------------------------------------------
// Message handler
// ---------------------------------------------------------------------------

self.postMessage({ type: 'ready' })
loadResources()

self.onmessage = async (event) => {
  const { type, code, offscreenCanvas } = event.data
  if (type !== 'run') return

  try {
    if (offscreenCanvas) setupCanvas(offscreenCanvas)
    await loadResources()

    const files = new Map()
    files.set('/input/main.tw', textEncoder.encode(code))

    self.postMessage({ type: 'status', text: 'Running…' })

    const exitCode = await runWasmBytesAsync(cachedBootBytes, {
      programPath: 'twk.wasm',
      guestArgs: ['run', '/input/main.tw'],
      cwd: '/',
      env: { NO_COLOR: '1' },
      stdout: makeStream('stdout'),
      stderr: makeStream('stderr'),
      bridgeBytes: cachedBridgeBytes,
      host: makeBrowserHost(files),
      marshalSpec,
    })

    self.postMessage({ type: 'done', exitCode })
  } catch (e) {
    self.postMessage({ type: 'error', message: e?.message ?? String(e) })
  }
}
