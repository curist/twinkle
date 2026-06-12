import { CodeJar } from 'codejar'
import { Parser, Language, Query } from 'web-tree-sitter'
import highlightsScm from 'tree-sitter-twinkle/queries/highlights.scm?raw'
import treeSitterWasmUrl from 'web-tree-sitter/tree-sitter.wasm?url'
import grammarWasmUrl from 'tree-sitter-twinkle/tree-sitter-twinkle.wasm?url'

// __TWINKLE_COMPILER_VERSION__ is injected by vite.config (define); logging it on
// load lets you confirm which @twinkle-lang/twinkle the deployed site is running.
console.log(
  `%c✨ Twinkle%c playground %c@twinkle-lang/twinkle@${__TWINKLE_COMPILER_VERSION__} `,
  'color:#0d1117;background:#d2a8ff;padding:3px 8px;border-radius:6px 0 0 6px;font-weight:700',
  'color:#8b949e;background:#21262d;padding:3px 4px',
  'color:#56d364;background:#21262d;padding:3px 8px;border-radius:0 6px 6px 0;font-weight:600',
)

document.getElementById('compiler-version').textContent = `twk ${__TWINKLE_COMPILER_VERSION__}`

// ---------------------------------------------------------------------------
// Examples — fetched from ./examples/<name>.tw
// ---------------------------------------------------------------------------
const exampleCache = new Map()

async function loadExample(name) {
  if (exampleCache.has(name)) return exampleCache.get(name)
  const resp = await fetch(`./examples/${name}.tw`)
  const text = await resp.text()
  exampleCache.set(name, text)
  return text
}

// ---------------------------------------------------------------------------
// Syntax highlighting
// ---------------------------------------------------------------------------

// Maps tree-sitter capture names (no @) to CSS classes defined in index.html
const CAPTURE_CLASSES = {
  'keyword': 'hl-keyword',
  'keyword.function': 'hl-keyword',
  'keyword.type': 'hl-keyword',
  'keyword.control': 'hl-keyword',
  'keyword.return': 'hl-keyword',
  'keyword.modifier': 'hl-keyword',
  'keyword.import': 'hl-keyword',
  'keyword.operator': 'hl-keyword',
  'keyword.exception': 'hl-keyword',
  'operator': 'hl-operator',
  'string': 'hl-string',
  'string.escape': 'hl-str-esc',
  'comment': 'hl-comment',
  'number': 'hl-number',
  'number.float': 'hl-number',
  'boolean': 'hl-number',
  'type': 'hl-type',
  'type.builtin': 'hl-type',
  'type.definition': 'hl-type',
  'type.parameter': 'hl-param',
  'function': 'hl-function',
  'function.call': 'hl-function',
  'function.method.call': 'hl-function',
  'constructor': 'hl-ctor',
  'variable.parameter': 'hl-param',
  'property': 'hl-prop',
  'module': 'hl-module',
  'punctuation.bracket': 'hl-punct',
  'punctuation.delimiter': 'hl-punct',
  'punctuation.special': 'hl-punct-sp',
  'error': 'hl-error',
}

// Paint capture classes onto a character array, then build an HTML string.
// Captures are applied in document order; later captures override earlier ones
// (tree-sitter returns child nodes after parents, so specificity works out).
function buildHighlightedHTML(source, captures) {
  const classes = new Array(source.length).fill(null)
  for (const { name, node } of captures) {
    const cls = CAPTURE_CLASSES[name]
    if (!cls) continue
    for (let i = node.startIndex; i < node.endIndex && i < source.length; i++) {
      classes[i] = cls
    }
  }

  let html = ''
  let i = 0
  while (i < source.length) {
    const cls = classes[i]
    let j = i + 1
    while (j < source.length && classes[j] === cls) j++
    const chunk = source.slice(i, j)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
    html += cls ? `<span class="${cls}">${chunk}</span>` : chunk
    i = j
  }
  return html
}

// Stateful tree-sitter handles; null until async init completes
let tsParser = null
let tsQuery = null

function updateLineNumbers() {
  const count = (editorEl.textContent ?? '').split('\n').length
  lineNumbers.textContent = Array.from({ length: count }, (_, i) => i + 1).join('\n')
}

function highlight(el) {
  updateLineNumbers()
  if (!tsParser || !tsQuery) return
  const src = el.textContent ?? ''
  const tree = tsParser.parse(src)
  el.innerHTML = buildHighlightedHTML(src, tsQuery.captures(tree.rootNode))
}

async function initTreeSitter() {
  await Parser.init({ locateFile: () => treeSitterWasmUrl })
  const lang = await Language.load(grammarWasmUrl)
  tsParser = new Parser()
  tsParser.setLanguage(lang)
  tsQuery = new Query(lang, highlightsScm)
  // Re-highlight the content that's already in the editor
  jar.updateCode(jar.toString())
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------
const editorEl = document.getElementById('editor')
const lineNumbers = document.getElementById('line-numbers')
const output = document.getElementById('output')
const runBtn = document.getElementById('run-btn')
const status = document.getElementById('status')
const examples = document.getElementById('examples')
const divider = document.getElementById('divider')

// CodeJar replaces the div with a contenteditable code editor.
// It calls highlight() after every change; Tab inserts two spaces.
const jar = CodeJar(editorEl, highlight, { tab: '  ' })

function selectedExampleFromUrl() {
  const params = new URLSearchParams(window.location.search)
  const requested = params.get('example')
  if (!requested) return examples.value
  return [...examples.options].some(option => option.value === requested)
    ? requested
    : examples.value
}

function updateExampleUrl(name) {
  const url = new URL(window.location)
  url.searchParams.set('example', name)
  history.replaceState(null, '', url)
}

const initialExample = selectedExampleFromUrl()
examples.value = initialExample
loadExample(initialExample).then(code => jar.updateCode(code))

// Sync line number scroll with editor
editorEl.addEventListener('scroll', () => { lineNumbers.scrollTop = editorEl.scrollTop })

// iOS Safari: reset stuck scroll when keyboard dismisses, and format when the
// editor loses focus. Clicking Run skips this blur formatter because Run formats
// first and then executes the formatted source.
editorEl.addEventListener('blur', (e) => {
  window.scrollTo(0, 0)
  const nextFocus = e.relatedTarget
  setTimeout(() => {
    if (!running && document.activeElement !== runBtn && nextFocus !== runBtn) formatEditor()
  }, 0)
})

// Kick off tree-sitter load in the background; editor works without it
initTreeSitter().catch(e => console.warn('tree-sitter unavailable:', e.message))

examples.addEventListener('change', async () => {
  if (running) stop()
  output.innerHTML = ''
  updateExampleUrl(examples.value)
  const code = await loadExample(examples.value)
  if (code) jar.updateCode(code)
})

function appendOutput(cls, text) {
  const span = document.createElement('span')
  span.className = cls
  span.textContent = text
  output.appendChild(span)
  output.scrollTop = output.scrollHeight
}

// Ctrl/Cmd+Enter → run
editorEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault()
    run()
  }
})

// ---------------------------------------------------------------------------
// Divider drag
// ---------------------------------------------------------------------------
let dragging = false
const isVertical = () => window.matchMedia('(max-width: 640px)').matches

function onDragStart(e) {
  dragging = true
  divider.classList.add('dragging')
  e.preventDefault()
}
function onDragMove(clientX, clientY) {
  if (!dragging) return
  const workspace = document.querySelector('.workspace')
  const rect = workspace.getBoundingClientRect()
  const editorPane = document.getElementById('editor-pane')
  const outputPane = document.getElementById('output-pane')
  if (isVertical()) {
    const ratio = (clientY - rect.top) / rect.height
    const pct = Math.min(Math.max(ratio * 100, 20), 80)
    editorPane.style.flex = 'none'
    editorPane.style.width = ''
    editorPane.style.height = pct + '%'
    outputPane.style.flex = '1'
  } else {
    const ratio = (clientX - rect.left) / rect.width
    const pct = Math.min(Math.max(ratio * 100, 20), 80)
    editorPane.style.flex = 'none'
    editorPane.style.height = ''
    editorPane.style.width = pct + '%'
    outputPane.style.flex = '1'
  }
}
function onDragEnd() {
  if (dragging) { dragging = false; divider.classList.remove('dragging') }
}

divider.addEventListener('mousedown', onDragStart)
document.addEventListener('mousemove', (e) => onDragMove(e.clientX, e.clientY))
document.addEventListener('mouseup', onDragEnd)

divider.addEventListener('touchstart', (e) => { onDragStart(e); }, { passive: false })
document.addEventListener('touchmove', (e) => {
  if (!dragging) return
  const t = e.touches[0]
  onDragMove(t.clientX, t.clientY)
}, { passive: true })
document.addEventListener('touchend', onDragEnd)

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------
let worker = null
let workerReady = null
let running = false
let formatSeq = 0
let pendingFormat = null

function initWorker() {
  worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })
  workerReady = new Promise((resolve) => {
    function onReady(e) {
      if (e.data?.type === 'ready') {
        worker.removeEventListener('message', onReady)
        resolve()
      }
    }
    worker.addEventListener('message', onReady)
  })

  worker.onmessage = (e) => {
    const { type, text, exitCode, message, id, code, stderr, stdout } = e.data
    switch (type) {
      case 'status':
        status.textContent = text
        break
      case 'stdout':
        appendOutput('out-stdout', text)
        break
      case 'stderr':
        appendOutput('out-stderr', text)
        break
      case 'done':
        setRunning(false)
        if (exitCode === 0) {
          status.textContent = 'Done (exit 0)'
        } else {
          appendOutput('out-meta', `\n[exit code ${exitCode}]`)
          status.textContent = `Done (exit ${exitCode})`
        }
        break
      case 'formatted':
        if (pendingFormat?.id === id) {
          const pending = pendingFormat
          pendingFormat = null
          if (pending.apply && jar.toString() === pending.code && code !== pending.code) jar.updateCode(code)
          status.textContent = pending.quiet ? status.textContent : 'Formatted'
          pending.resolve({ ok: true, code: code ?? pending.code })
        }
        break
      case 'fmt_failed':
        if (pendingFormat?.id === id) {
          const pending = pendingFormat
          pendingFormat = null
          if (!pending.quiet) status.textContent = (stderr || stdout) ? 'Format failed' : 'Ready'
          pending.resolve({ ok: false, code: pending.code, stderr, stdout })
        }
        break
      case 'error':
        if (pendingFormat) {
          pendingFormat.resolve({ ok: false, code: pendingFormat.code, error: message })
          pendingFormat = null
        }
        setRunning(false)
        appendOutput('out-error', `\nInternal error: ${message}`)
        status.textContent = 'Error'
        break
    }
  }

  worker.onerror = (e) => {
    setRunning(false)
    appendOutput('out-error', `\nWorker error: ${e.message ?? e.type ?? 'Unknown error'}`)
    status.textContent = 'Error'
  }
}

initWorker()

function setRunning(r) {
  running = r
  runBtn.textContent = running ? '⏹ Stop' : '▶ Run'
}

function stop() {
  worker.terminate()
  initWorker()
  setRunning(false)
  if (pendingFormat) pendingFormat.resolve({ ok: false, code: pendingFormat.code })
  pendingFormat = null
  status.textContent = 'Stopped'
}

async function formatEditor({ apply = true, quiet = false } = {}) {
  await workerReady
  const code = jar.toString()
  const id = ++formatSeq
  if (pendingFormat) pendingFormat.resolve({ ok: false, code: pendingFormat.code })
  const result = new Promise((resolve) => {
    pendingFormat = { id, code, apply, quiet, resolve }
  })
  worker.postMessage({ type: 'fmt', id, code })
  return result
}

async function run() {
  if (running) { stop(); return }

  output.innerHTML = ''
  setRunning(true)
  status.textContent = 'Formatting…'

  const formatted = await formatEditor({ apply: true, quiet: true })
  if (!running) return
  const code = formatted.ok ? formatted.code : jar.toString()
  status.textContent = 'Starting…'

  const needsCanvas = /extern\s+\w+\s+type\s+\w*Canvas\w*|extern\s+canvas\b/.test(code)

  if (needsCanvas) {
    const canvas = document.createElement('canvas')
    const dpr = window.devicePixelRatio || 1
    // Measure the content area of #output (excludes its own padding)
    const style = getComputedStyle(output)
    const w = output.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight)
    const h = output.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom)
    canvas.width = Math.round(w * dpr)
    canvas.height = Math.round(h * dpr)
    canvas.style.cssText = `display:block; width:${w}px; height:${h}px;`
    output.appendChild(canvas)
    const offscreen = canvas.transferControlToOffscreen()
    worker.postMessage({ type: 'run', code, offscreenCanvas: offscreen }, [offscreen])
  } else {
    worker.postMessage({ type: 'run', code })
  }
}

runBtn.addEventListener('click', run)
