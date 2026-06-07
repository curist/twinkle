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

// Plain functions — the compiler-emitted twinkle.externs section tells the
// runtime which args are the 2D-context externref (passed through raw) vs
// strings vs numbers, so no per-arg marshal spec is needed here.
function canvasImports(offscreen) {
  const ctx = offscreen.getContext('2d')
  return {
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
