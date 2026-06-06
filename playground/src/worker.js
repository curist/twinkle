// Web Worker — runs Twinkle in the browser via the published compiler's
// browser entry (@twinkle-lang/twinkle/web). The package self-loads its wasm
// and provides the in-memory filesystem, so this worker only deals with what is
// genuinely playground-specific: the canvas/http/timer externs and piping
// stdout/stderr back to the UI.
//
// Externs are passed through the runtime's `imports` option (module → fn, or
// `{ fn, args }` when an arg needs an explicit marshal hint). No globalThis.
//
// Receives: { type: 'run', code: string, offscreenCanvas? }
// Posts:    { type: 'ready' | 'status' | 'stdout' | 'stderr' | 'done' | 'error', ... }

import { run, load } from '@twinkle-lang/twinkle/web'

// ---------------------------------------------------------------------------
// Externs
// ---------------------------------------------------------------------------

const timer = {
  sleep_ms: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
}

const http = {
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

// Canvas args carry the 2D-context externref, which must be passed through
// untouched (`args: ['raw', ...]`): decoding it as a Wasm GC string recurses
// until a stack overflow in some engines (notably Safari).
const RAW = (n) => Array(n).fill('raw')

function canvasImports(offscreen) {
  const ctx = offscreen.getContext('2d')
  return {
    get_context: { fn: () => ctx ?? null, args: ['string'] },
    get_width: { fn: () => offscreen.width },
    get_height: { fn: () => offscreen.height },
    set_fill_style: { fn: (c, color) => { c.fillStyle = color }, args: ['raw', 'string'] },
    fill_rect: { fn: (c, x, y, w, h) => c.fillRect(x, y, w, h), args: RAW(5) },
    clear_rect: { fn: (c, x, y, w, h) => c.clearRect(x, y, w, h), args: RAW(5) },
    set_stroke_style: { fn: (c, color) => { c.strokeStyle = color }, args: ['raw', 'string'] },
    stroke_rect: { fn: (c, x, y, w, h) => c.strokeRect(x, y, w, h), args: RAW(5) },
    begin_path: { fn: (c) => c.beginPath(), args: ['raw'] },
    close_path: { fn: (c) => c.closePath(), args: ['raw'] },
    move_to: { fn: (c, x, y) => c.moveTo(x, y), args: RAW(3) },
    line_to: { fn: (c, x, y) => c.lineTo(x, y), args: RAW(3) },
    arc: { fn: (c, x, y, r, s, e) => c.arc(x, y, r, s, e), args: RAW(6) },
    fill: { fn: (c) => c.fill(), args: ['raw'] },
    stroke: { fn: (c) => c.stroke(), args: ['raw'] },
    set_line_width: { fn: (c, w) => { c.lineWidth = w }, args: RAW(2) },
    set_global_alpha: { fn: (c, a) => { c.globalAlpha = a }, args: RAW(2) },
    set_font: { fn: (c, font) => { c.font = font }, args: ['raw', 'string'] },
    fill_text: { fn: (c, text, x, y) => c.fillText(text, x, y), args: ['raw', 'string', 'raw', 'raw'] },
  }
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
load() // warm the compiler wasm cache so the first run is snappy

self.onmessage = async (event) => {
  const { type, code, offscreenCanvas } = event.data
  if (type !== 'run') return

  try {
    const imports = { timer, http }
    if (offscreenCanvas) imports.canvas = canvasImports(offscreenCanvas)

    self.postMessage({ type: 'status', text: 'Running…' })

    const exitCode = await run(code, {
      env: { NO_COLOR: '1' },
      stdout: makeStream('stdout'),
      stderr: makeStream('stderr'),
      imports,
    })

    self.postMessage({ type: 'done', exitCode })
  } catch (e) {
    self.postMessage({ type: 'error', message: e?.message ?? String(e) })
  }
}
