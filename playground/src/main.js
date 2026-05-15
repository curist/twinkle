import { CodeJar } from 'codejar'
import { Parser, Language, Query } from 'web-tree-sitter'
import highlightsScm from '../../tree-sitter-twinkle/queries/highlights.scm?raw'

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
  await Parser.init({ locateFile: () => './tree-sitter.wasm' })
  const lang = await Language.load('./tree-sitter-twinkle.wasm')
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
loadExample(examples.value).then(code => jar.updateCode(code))

// Sync line number scroll with editor
editorEl.addEventListener('scroll', () => { lineNumbers.scrollTop = editorEl.scrollTop })

// Kick off tree-sitter load in the background; editor works without it
initTreeSitter().catch(e => console.warn('tree-sitter unavailable:', e.message))

examples.addEventListener('change', async () => {
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
divider.addEventListener('mousedown', (e) => {
  dragging = true
  divider.classList.add('dragging')
  e.preventDefault()
})
document.addEventListener('mousemove', (e) => {
  if (!dragging) return
  const workspace = document.querySelector('.workspace')
  const rect = workspace.getBoundingClientRect()
  const ratio = (e.clientX - rect.left) / rect.width
  const pct = Math.min(Math.max(ratio * 100, 20), 80)
  document.getElementById('editor-pane').style.flex = 'none'
  document.getElementById('editor-pane').style.width = pct + '%'
  document.getElementById('output-pane').style.flex = '1'
})
document.addEventListener('mouseup', () => {
  if (dragging) { dragging = false; divider.classList.remove('dragging') }
})

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------
const worker = new Worker('./worker.js')

worker.onmessage = (e) => {
  const { type, text, exitCode, message } = e.data
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
    case 'error':
      setRunning(false)
      appendOutput('out-error', `\nInternal error: ${message}`)
      status.textContent = 'Error'
      break
  }
}

worker.onerror = (e) => {
  setRunning(false)
  appendOutput('out-error', `\nWorker error: ${e.message}`)
  status.textContent = 'Error'
}

function setRunning(running) {
  runBtn.disabled = running
  runBtn.textContent = running ? '⏳ Running…' : '▶ Run'
}

function run() {
  output.innerHTML = ''
  setRunning(true)
  status.textContent = 'Starting…'
  worker.postMessage({ type: 'run', code: jar.toString() })
}

runBtn.addEventListener('click', run)
