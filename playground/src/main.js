import { CodeJar } from 'codejar'
import { Parser, Language } from 'web-tree-sitter'
import highlightsScm from '../../tree-sitter-twinkle/queries/highlights.scm?raw'

// ---------------------------------------------------------------------------
// Examples
// ---------------------------------------------------------------------------
const EXAMPLES = {
  hello: `println("Hello, World!")
`,

  fizzbuzz_enum: `// Enums + pattern matching
type FB = { Fizz, Buzz, FizzBuzz, Num(Int) }

fn classify(i: Int) FB {
  if i % 15 == 0 { .FizzBuzz }
  else if i % 3 == 0 { .Fizz }
  else if i % 5 == 0 { .Buzz }
  else { .Num(i) }
}

for i in range_from(1, 21) {
  case classify(i) {
    .Fizz    => println("Fizz"),
    .Buzz    => println("Buzz"),
    .FizzBuzz => println("FizzBuzz"),
    .Num(x)  => println("\${x}"),
  }
}
`,

  primes: `// Primes via trial division + collect/continue
fn is_prime(n: Int) Bool {
  if n < 2 { return false }
  if n == 2 { return true }
  if n % 2 == 0 { return false }
  i := 3
  for i * i <= n {
    if n % i == 0 { return false }
    i = i + 2
  }
  true
}

primes := collect n in range_from(2, 80) {
  if is_prime(n) { n } else { continue }
}
print("Primes up to 80: [")
for n, i in primes {
  if i > 0 { print(", ") }
  print("\${n}")
}
println("]")
println("Count: \${primes.len()}")

// Twin primes (pairs differing by 2)
twins := collect i in range_from(0, primes.len() - 1) {
  if primes[i + 1] - primes[i] == 2 {
    "(\${primes[i]}, \${primes[i + 1]})"
  } else {
    continue
  }
}
print("Twin primes: [")
for t, i in twins {
  if i > 0 { print(", ") }
  print(t)
}
println("]")
`,

  bst: `// Immutable binary search tree
type BST = { Empty, Node(Int, BST, BST) }

fn insert(t: BST, n: Int) BST {
  case t {
    .Empty => .Node(n, .Empty, .Empty),
    .Node(v, left, right) => {
      if n < v { .Node(v, insert(left, n), right) }
      else if n > v { .Node(v, left, insert(right, n)) }
      else { t }
    },
  }
}

fn inorder(t: BST) Vector<Int> {
  case t {
    .Empty => [],
    .Node(v, left, right) =>
      inorder(left).append(v).concat(inorder(right)),
  }
}

fn contains(t: BST, n: Int) Bool {
  case t {
    .Empty => false,
    .Node(v, left, right) => {
      if n == v { true }
      else if n < v { contains(left, n) }
      else { contains(right, n) }
    },
  }
}

tree: BST = .Empty
for v in [5, 3, 8, 1, 4, 7, 9, 2, 6] { tree = insert(tree, v) }

sorted := inorder(tree)
print("Sorted: [")
for v, i in sorted {
  if i > 0 { print(", ") }
  print("\${v}")
}
println("]")
println("Has 4?  \${contains(tree, 4)}")
println("Has 10? \${contains(tree, 10)}")
`,

  closures: `// First-class functions and closures
fn make_adder(n: Int) fn(Int) Int {
  fn(x) { x + n }
}

fn make_multiplier(n: Int) fn(Int) Int {
  fn(x) { x * n }
}

add10  := make_adder(10)
triple := make_multiplier(3)

println("add10(7)         = \${add10(7)}")
println("triple(7)        = \${triple(7)}")
println("add10(triple(7)) = \${add10(triple(7))}")
println("triple(add10(7)) = \${triple(add10(7))}")

// Closures capture their environment
adders := collect n in range_from(1, 6) { make_adder(n) }
print("add 1..5 to 100: ")
for add in adders { print("\${add(100)} ") }
println("")
`,

  word_count: `// Word frequency with Dict
fn count_words(words: Vector<String>) Dict<String, Int> {
  counts: Dict<String, Int> = Dict.new()
  for word in words {
    case counts[word] {
      .Some(n) => counts[word] = n + 1,
      .None    => counts[word] = 1,
    }
  }
  counts
}

fn most_common(counts: Dict<String, Int>) String? {
  keys := counts.keys()
  if keys.len() == 0 { return .None }
  best := keys[0]
  best_n := counts[best].unwrap_or(0)
  for word in keys {
    n := counts[word].unwrap_or(0)
    if n > best_n {
      best = word
      best_n = n
    }
  }
  .Some(best)
}

words := ["the", "quick", "brown", "fox", "jumps",
          "over", "the", "lazy", "dog", "the", "fox"]

counts := count_words(words)
for word in counts.keys() {
  println("\${word}: \${counts[word].unwrap_or(0)}")
}

case most_common(counts) {
  .Some(w) => println("\\nmost common: '\${w}' (\${counts[w].unwrap_or(0)}×)"),
  .None    => {},
}
`,

  caesar: `// Caesar cipher — string ↔ byte manipulation
fn shift_char(b: Byte, shift: Int) Byte {
  n := b.to_int()
  if n >= 65 and n <= 90 {
    Byte.from_int((n - 65 + shift) % 26 + 65).unwrap_or(b)
  } else if n >= 97 and n <= 122 {
    Byte.from_int((n - 97 + shift) % 26 + 97).unwrap_or(b)
  } else {
    b
  }
}

fn caesar(text: String, shift: Int) String {
  rotated := text.utf8_bytes().map(fn(b) { shift_char(b, shift) })
  String.from_utf8(rotated).unwrap_or(text)
}

message := "The Quick Brown Fox Jumps Over The Lazy Dog"
encoded := caesar(message, 13)
decoded := caesar(encoded, 13)

println("Original: \${message}")
println("ROT-13:   \${encoded}")
println("Decoded:  \${decoded}")

for shift in [1, 3, 7, 13] {
  println("shift \${shift}: \${caesar("Hello", shift)}")
}
`,
}

// ---------------------------------------------------------------------------
// Syntax highlighting
// ---------------------------------------------------------------------------

// Maps tree-sitter capture names (no @) to CSS classes defined in index.html
const CAPTURE_CLASSES = {
  'keyword':           'hl-keyword',
  'keyword.function':  'hl-keyword',
  'keyword.type':      'hl-keyword',
  'keyword.control':   'hl-keyword',
  'keyword.return':    'hl-keyword',
  'keyword.modifier':  'hl-keyword',
  'keyword.import':    'hl-keyword',
  'keyword.operator':  'hl-keyword',
  'keyword.exception': 'hl-keyword',
  'operator':          'hl-operator',
  'string':            'hl-string',
  'string.escape':     'hl-str-esc',
  'comment':           'hl-comment',
  'number':            'hl-number',
  'number.float':      'hl-number',
  'boolean':           'hl-number',
  'type':              'hl-type',
  'type.builtin':      'hl-type',
  'type.definition':   'hl-type',
  'type.parameter':    'hl-param',
  'function':          'hl-function',
  'function.call':     'hl-function',
  'function.method.call': 'hl-function',
  'constructor':       'hl-ctor',
  'variable.parameter': 'hl-param',
  'property':          'hl-prop',
  'module':            'hl-module',
  'punctuation.bracket':   'hl-punct',
  'punctuation.delimiter': 'hl-punct',
  'punctuation.special':   'hl-punct-sp',
  'error':             'hl-error',
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
let tsQuery  = null

function highlight(el) {
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
  tsQuery = lang.query(highlightsScm)
  // Re-highlight the content that's already in the editor
  jar.updateCode(jar.toString())
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------
const editorEl = document.getElementById('editor')
const output   = document.getElementById('output')
const runBtn   = document.getElementById('run-btn')
const status   = document.getElementById('status')
const examples = document.getElementById('examples')
const divider  = document.getElementById('divider')

// CodeJar replaces the div with a contenteditable code editor.
// It calls highlight() after every change; Tab inserts two spaces.
const jar = CodeJar(editorEl, highlight, { tab: '  ' })
jar.updateCode(EXAMPLES.fizzbuzz_enum)

// Kick off tree-sitter load in the background; editor works without it
initTreeSitter().catch(e => console.warn('tree-sitter unavailable:', e.message))

examples.addEventListener('change', () => {
  const code = EXAMPLES[examples.value]
  if (code) { jar.updateCode(code); examples.value = '' }
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
