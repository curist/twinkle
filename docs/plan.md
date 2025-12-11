# Twinkle Implementation Plan

## Vision: Aggressive Self-Hosting via WebAssembly

**End Goal:** Ship only:
- `twinkle-compiler.wasm` - entire toolchain compiled to WebAssembly
- `prelude/*.tw` - standard library sources
- Optional thin runtime wrappers (Rust, Node.js, Deno, etc.)

**Key Properties:**
- ✅ **Language-agnostic distribution** - runs on any Wasm runtime
- ✅ **Single source of truth** - all logic written in Twinkle
- ✅ **Extreme portability** - browser, server, embedded, anywhere Wasm runs
- ✅ **Simple distribution** - one .wasm file + prelude sources
- ✅ **Multi-platform** - zero native code, zero platform-specific builds

### Target Architecture

```
┌─────────────────────────────────────┐
│ Thin Runtime Wrappers               │  100-200 lines each
│  • Rust CLI (wasmtime)              │  Just invoke Wasm + I/O
│  • Node.js CLI                      │
│  • Deno CLI                         │
│  • Browser playground               │
└──────────┬──────────────────────────┘
           │ Load & invoke
           ▼
┌─────────────────────────────────────┐
│ twinkle-compiler.wasm               │  ALL logic here
│  Written entirely in Twinkle        │
│  • Lexer                            │
│  • Parser                           │
│  • Type checker (HM + traits)      │
│  • Code generator (→ Wasm GC)      │
│  • LSP server logic                 │
│  • Code formatter                   │
│  • Error diagnostics                │
│  • Package manager (future)         │
└─────────────────────────────────────┘
           │ Uses
           ▼
┌─────────────────────────────────────┐
│ Prelude (Standard Library)          │
│  Written in Twinkle                 │
│  • Primitives (int, string, etc.)  │
│  • Collections (array, dict)        │
│  • I/O (WASI file operations)      │
│  • JSON parser/serializer           │
│  • String manipulation              │
│  • Show, Iterable, Eq, Ord traits  │
└─────────────────────────────────────┘
```

### Usage Examples

```bash
# Rust wrapper (convenience, ships with wasmtime)
$ twinkle compile main.tw

# Node.js wrapper
$ node twinkle.js compile main.tw

# Deno wrapper
$ deno run twinkle.ts compile main.tw

# Pure wasmtime (any platform)
$ wasmtime twinkle-compiler.wasm -- compile main.tw

# Browser playground (zero install)
<script type="module">
  import init, { compile } from './twinkle-compiler.wasm';
  await init();
  const result = compile(sourceCode);
</script>
```

---

## Design Principles for Self-Hosting

The Rust bootstrap implementation is a **reference implementation** that will be ported to Twinkle. Every design decision should optimize for portability.

### 1. Pure, Simple, Portable

Write Rust code that translates directly to Twinkle:

```rust
// ✅ GOOD - Pure function, easily portable
fn lex(source: &str) -> Vec<Token> {
    let mut tokens = vec![];
    for c in source.chars() {
        match c {
            '0'..='9' => tokens.push(lex_number(c)),
            'a'..='z' => tokens.push(lex_ident(c)),
            // Simple, mechanical logic
        }
    }
    tokens
}

// ❌ AVOID - Complex Rust-specific patterns
async fn lex_async<'a, 'b: 'a>(...) { }  // Hard to port
#[derive(Parser)] struct Expr { }        // Macro magic
```

**Direct translation to Twinkle:**

```tw
fn lex(source: string) -> array<Token> {
  tokens: array<Token> = []
  for c in source.chars() {
    case c {
      '0', '1', '2', '3', '4', '5', '6', '7', '8', '9' =>
        tokens.push(lex_number(c)),
      // Same logic, Twinkle syntax!
    }
  }
  tokens
}
```

### 2. Separate I/O from Logic

The core compiler must be **pure** - no file I/O, no network, no environment access.

```rust
// ✅ GOOD - Core compiler is pure
pub mod compiler {
    pub fn compile_module(source: String) -> Result<Vec<u8>, CompileError> {
        let tokens = lex(&source)?;
        let ast = parse(tokens)?;
        let typed = typecheck(ast)?;
        let wasm = codegen(typed)?;
        Ok(wasm)
    }
    // Zero I/O, all in-memory transformations
}

// Wrapper handles ALL I/O
fn main() {
    // I/O here
    let source = std::fs::read_to_string("main.tw")?;

    // Pure computation
    let wasm_bytes = compiler::compile_module(source)?;

    // I/O here
    std::fs::write("output.wasm", wasm_bytes)?;
}
```

**When self-hosted:**
- Core compiler in Twinkle: pure functions
- Wrapper uses WASI for file I/O
- Same architecture, different implementation language

### 3. Design for WASI

File operations will eventually use WebAssembly System Interface (WASI):

```tw
// Future Twinkle code using WASI
import "wasi"

fn compile_file(path: string) -> Result<array<u8>, string> {
  source := wasi.fs.read_to_string(path)?
  result := compile_module(source)?
  .Ok(result)
}
```

**Implications for Rust bootstrap:**
- Keep I/O interfaces simple and WASI-compatible
- Use standard file operations (read, write, exists)
- Avoid platform-specific APIs

### 4. Avoid Rust-Specific Complexity

```rust
// ❌ AVOID
async fn parse(...) { }              // Twinkle has no async (yet)
fn parse<'a, 'b: 'a>(...) { }       // Complex lifetimes
impl Parser for &mut dyn Iterator   // Trait objects + lifetimes
use procedural_macros::derive       // Can't port macros

// ✅ PREFER
fn parse(tokens: Vec<Token>) -> Ast           // Simple ownership
fn parse_ref(tokens: &[Token]) -> Ast         // Simple borrows only
struct Parser { tokens: Vec<Token> }          // Concrete types
```

### 5. Hand-Written, No Code Generation

```rust
// ❌ AVOID - Parser generators
lalrpop! { /* grammar */ }
pest::parser! { /* grammar */ }

// ✅ PREFER - Hand-written recursive descent
fn parse_expr(tokens: &mut TokenStream) -> Expr {
    match tokens.peek() {
        Token::Int(n) => {
            tokens.next();
            Expr::Int(n)
        }
        // Mechanical, easy to port
    }
}
```

**Reasoning:** Parser generators produce complex generated code that's hard to understand and port. Hand-written parsers are:
- Easy to debug
- Easy to port to Twinkle
- Give full control over error messages
- Simple enough for self-hosting

### 6. Source Locations from Day 1

Every AST node needs position information for LSP and error reporting:

```rust
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub file_id: FileId,
}

pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,  // Essential!
    pub ty: Option<Type>,
}
```

**This enables:**
- Good error messages with source locations
- LSP features (go-to-definition, hover, etc.)
- Source-preserving pretty-printing

### 7. Error Recovery

Design for multiple errors, even if initial implementation panics on first error:

```rust
pub struct ParseResult {
    pub ast: Program,
    pub errors: Vec<ParseError>,  // Design for multiple errors
}

pub struct TypeCheckResult {
    pub typed_ast: Program,
    pub errors: Vec<TypeError>,
}

// Even if Stage 1 does:
fn parse(...) -> Result<Ast, ParseError> {
    // Single error for now
}

// Design the API for the future:
fn parse_resilient(...) -> ParseResult {
    // Multiple errors later
}
```

---

## Bootstrap Strategy

### Phase 1: Rust Implementation (Stages 0-7)

Build a complete, working compiler in Rust:
- Hand-written, simple, pure functional style
- Reference implementation for porting to Twinkle
- Proves the language design works

### Phase 2: Self-Hosting (Stage 8)

Rewrite the compiler in Twinkle:
- Translate Rust → Twinkle systematically
- Core compiler logic first
- LSP, formatter, tooling next
- Standard library (JSON, WASI, etc.)

### Phase 3: Thin Wrappers (Stage 9)

Create minimal runtime wrappers:
- Rust: ~100 lines using wasmtime crate
- Node.js: ~50 lines
- Deno: ~50 lines
- Browser: Direct Wasm module loading

### Phase 4: Dogfooding (Ongoing)

Use Twinkle to build Twinkle tooling:
- Package manager in Twinkle
- Documentation generator in Twinkle
- Test runner in Twinkle
- Build system in Twinkle

---

## Implementation Stages

### Stage 0: Repository Skeleton

**Goal:** Project structure, test harness, CI setup

**Structure:**
```
twinkle/
  src/
    main.rs         # CLI entry (minimal)
    lib.rs          # Compiler API (exports compile_module)
    ast.rs          # AST with Span on every node
    lexer.rs        # Hand-written lexer
    parser.rs       # Hand-written recursive descent
    typecheck.rs    # Type inference engine
    codegen.rs      # Wasm GC output (later)
    error.rs        # Error types + formatting
  tests/
    parser_cases/   # Round-trip parsing tests
    type_ok/        # Programs that typecheck
    type_err/       # Programs with type errors
  examples/         # .tw example programs
  docs/            # Specifications
```

**Test Infrastructure:**
- Use `insta` crate for snapshot testing
- Golden tests: input `.tw` → expected output
- Test categories: parser, type checker, codegen

**Deliverable:**
- `cargo test` runs
- Basic project structure in place
- CI pipeline (GitHub Actions)

---

### Stage 1: Lexer + Parser + Pretty-Printer

**Goal:** Parse and pretty-print a small expression language

**Subset:**
- Literals: `123`, `3.14`, `"hello"`, `true`, `false`
- Binary operators: `+`, `-`, `*`, `/`, `%`
- Variables: `x`, `foo_bar`
- Function declarations:
  ```tw
  fn add(x: int, y: int) -> int { x + y }
  ```
- Function calls: `f(1, 2)`
- Blocks: `{ a; b; c }`

**Implementation:**

1. **Add Span to every node:**
   ```rust
   pub struct Span {
       pub file_id: FileId,
       pub start: usize,
       pub end: usize,
   }

   pub struct Expr {
       pub kind: ExprKind,
       pub span: Span,
   }

   pub enum ExprKind {
       Int(i64),
       Float(f64),
       String(String),
       Bool(bool),
       Var(String),
       Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
       Call { func: Box<Expr>, args: Vec<Expr> },
       Block(Vec<Stmt>),
   }
   ```

2. **Hand-written lexer:**
   ```rust
   pub fn lex(source: &str) -> Vec<Token> {
       // State machine, no regex
       // Return tokens with spans
   }
   ```

3. **Hand-written recursive descent parser:**
   ```rust
   pub fn parse(tokens: &[Token]) -> Result<Program, ParseError> {
       // Operator precedence climbing or Pratt parsing
       // Preserve spans for every node
   }
   ```

4. **Pretty-printer (serves as code formatter):**
   ```rust
   pub fn format_program(prog: &Program) -> String {
       // Format AST back to source
       // This is your code formatter!
   }
   ```

5. **Preserve comments (at least doc comments):**
   ```rust
   pub struct FunctionDecl {
       pub doc_comment: Option<String>,
       pub name: String,
       pub params: Vec<Param>,
       pub return_ty: Option<Type>,
       pub body: Expr,
       pub span: Span,
   }
   ```

**Tests:**
- Round-trip: parse → pretty-print → parse again = same AST
- Operator precedence: `1 + 2 * 3` parses correctly
- Error recovery: collect multiple parse errors (design for it)

**Deliverable:**
- Lexer + parser working
- Pretty-printer serves as formatter
- Solid foundation for all future stages

---

### Stage 2: Monomorphic Type Checker

**Goal:** Type check simple programs without generics or traits

**Add to Subset:**
- Explicit types: `int`, `float`, `bool`, `string`, `void`
- Let bindings:
  ```tw
  x := 1           // Infer type
  y: int = x + 2   // Explicit type
  ```
- If expressions:
  ```tw
  if cond { a } else { b }  // Both branches must have same type
  ```

**Implementation:**

```rust
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Void,
    // More types added in later stages
}

pub struct Env {
    vars: HashMap<String, Type>,
}

pub fn typecheck_expr(expr: &Expr, env: &Env) -> Result<Type, TypeError> {
    match &expr.kind {
        ExprKind::Int(_) => Ok(Type::Int),
        ExprKind::Binary { op, lhs, rhs } => {
            let lhs_ty = typecheck_expr(lhs, env)?;
            let rhs_ty = typecheck_expr(rhs, env)?;
            // Check operator compatibility
            typecheck_binop(op, lhs_ty, rhs_ty)
        }
        // ...
    }
}
```

**Tests:**
- Good: `fn add(x: int, y: int) -> int { x + y }`
- Good: `x := 1; y := x + 2`
- Bad: `fn bad() -> int { "hello" }`  // Type mismatch
- Bad: `if true { 1 } else { "no" }`  // Branch type mismatch

**Deliverable:**
- First real type errors from Twinkle
- Type-annotated AST output

---

### Stage 3: Records, Modules, Inherent Methods

**Goal:** Records, module system, dot syntax desugaring

**Add to Subset:**
- Record type declarations:
  ```tw
  type Point = .{ x: int, y: int }
  ```
- Record literals (both forms):
  ```tw
  p: Point = .{ x: 1, y: 2 }      // Anonymous
  p := Point.{ x: 1, y: 2 }        // Named constructor
  ```
- Field access: `p.x`, `p.y`
- Modules + imports:
  ```tw
  // point.tw
  pub type Point = .{ x: int, y: int }
  pub fn translate(p: Point, dx: int, dy: int) -> Point { ... }

  // main.tw
  import "point"
  fn main() -> void {
    p := Point.{ x: 1, y: 2 }
    p2 := p.translate(3, 4)  // Desugars to point.translate(p, 3, 4)
  }
  ```

**Implementation:**

```rust
pub struct ModuleTable {
    types: HashMap<String, TypeDef>,
    functions: HashMap<String, FuncSignature>,
}

pub fn resolve_type_name(name: &str, modules: &ModuleTable) -> Option<Type> {
    // Resolve Point, module.Point, etc.
}

pub fn desugar_dot_call(expr: &Expr) -> Expr {
    // p.translate(1, 2) → point.translate(p, 1, 2)
    // Check: is 'translate' a field? No → look for inherent method
}
```

**Tests:**
- Multi-file programs with imports
- Dot syntax resolution (field vs method)
- Type errors: field doesn't exist, method doesn't exist

**Deliverable:**
- Records working
- Module imports working
- Inherent method sugar working

---

### Stage 4: Enums, Pattern Matching, Try

**Goal:** Algebraic data types, exhaustive pattern matching, error handling

**Add to Subset:**
- Enum declarations:
  ```tw
  enum Option<T> { None, Some(T) }
  enum Result<T, E> { Ok(T), Err(E) }
  enum Shape { Circle(float), Rect(float, float), Unit }
  ```
- Pattern matching:
  ```tw
  case shape {
    .Circle(r) => r * r * 3.14159,
    .Rect(w, h) => w * h,
    .Unit => 1.0,
  }
  ```
- Try expressions:
  ```tw
  fn compute() -> Result<int, string> {
    x := try divide(10, 2)   // Unwraps Ok, returns early on Err
    y := try divide(x, 0)
    .Ok(y)
  }
  ```

**Implementation:**

```rust
pub enum Type {
    // ... existing types
    Enum { name: String, type_args: Vec<Type> },
}

pub fn check_exhaustiveness(patterns: &[Pattern], scrutinee_ty: &Type) -> Result<(), TypeError> {
    // Ensure all variants covered or _ present
}

pub fn desugar_try(expr: &Expr) -> Expr {
    // try x → case x { .Ok(v) => v, .Err(e) => return .Err(e) }
}
```

**Tests:**
- Exhaustiveness checking (positive and negative)
- Type consistency across case arms
- Try expression desugaring
- Nested pattern matching

**Deliverable:**
- Enums with pattern matching working
- `Option`, `Result`, `try` working
- Exhaustiveness checking

---

### Stage 5: Traits (Contract Only) + Show

**Goal:** Trait system for compiler features only, string interpolation

**Add to Subset:**
- Trait declarations:
  ```tw
  trait Show(T) {
    fn show(x: T) -> string
  }
  ```
- Trait implementations:
  ```tw
  impl Show(int) {
    fn show(x: int) -> string { "${x}" }
  }

  impl Show(Point) {
    fn show(p: Point) -> string { "(${p.x}, ${p.y})" }
  }
  ```
- String interpolation:
  ```tw
  println("Point: ${p}")  // Requires Show(Point)
  ```

**Implementation:**

```rust
pub struct TraitEnv {
    impls: HashMap<String, Vec<(Type, ImplId)>>,
}

pub fn check_string_interpolation(expr: &Expr, env: &TraitEnv) -> Result<(), TypeError> {
    let ty = typecheck_expr(expr)?;
    if !env.has_impl("Show", &ty) {
        return Err(TypeError::MissingShowImpl { ty });
    }
    Ok(())
}
```

**Important:** Trait methods are NOT callable from user code!

**Tests:**
- String interpolation with builtin types
- String interpolation with custom types (Show impl required)
- Error: no Show impl for type

**Deliverable:**
- Trait system working
- String interpolation requires Show
- Clear error messages for missing impls

---

### Stage 6: Generics (HM) + Trait Constraints

**Goal:** Parametric polymorphism with trait bounds

**Add to Subset:**
- Generic functions:
  ```tw
  fn map<A, B>(xs: array<A>, f: (A) -> B) -> array<B> { ... }
  fn log<T: Show>(x: T) -> void { println("${x}") }
  ```
- Type inference with generics
- Trait constraints on type parameters

**Implementation:**

```rust
pub enum Type {
    Var(TypeVarId),
    // ... existing types
}

pub struct Scheme {
    vars: Vec<TypeVarId>,
    constraints: Vec<TraitConstraint>,  // T: Show, etc.
    ty: Type,
}

pub fn instantiate(scheme: &Scheme) -> Type {
    // Create fresh type variables
}

pub fn unify(a: &Type, b: &Type) -> Result<Substitution, TypeError> {
    // Standard unification algorithm
}
```

**Tests:**
- Generic functions with inference
- Trait constraints required
- Error: constraint missing

**Deliverable:**
- Full Hindley-Milner type inference
- Generic functions with constraints
- Solid foundation for all type features

---

### Stage 7: WebAssembly GC Backend

**Goal:** Generate WebAssembly GC output

**Options:**

1. **Simple interpreter first** (recommended):
   - Test semantics quickly
   - Good for debugging
   - Prove language design works

2. **Direct Wasm text output**:
   - Generate `.wat` text format
   - Start with simple cases (integers, functions)
   - Expand to full feature set

**Implementation:**

```rust
pub fn codegen(typed_ast: &Program) -> Result<Vec<u8>, CodegenError> {
    // Translate typed AST → WebAssembly GC
    // Use wasm-encoder crate or generate .wat text
}
```

**Deliverable:**
- Can compile simple Twinkle programs to Wasm
- Bootstrap compiler is complete and working

---

## Host ABI Contract (WASI)

**Goal:** Define the minimal, stable interface between Twinkle and the host environment

The core compiler and all Twinkle programs must remain pure. All I/O goes through a well-defined Host ABI based on WASI (WebAssembly System Interface).

### Minimal WASI Surface

Define this contract **before Stage 1** and keep it stable through all implementations:

```rust
// Host ABI Contract v1.0
// This interface must be implemented by all runtime wrappers

/// File System Operations
wasi::fd_read(fd: i32, iovs: *const iovec, iovs_len: i32) -> Result<i32, errno>
wasi::fd_write(fd: i32, iovs: *const iovec, iovs_len: i32) -> Result<i32, errno>
wasi::path_open(dirfd: i32, path: *const u8, path_len: i32, ...) -> Result<i32, errno>
wasi::fd_close(fd: i32) -> Result<(), errno>
wasi::path_filestat_get(dirfd: i32, path: *const u8, path_len: i32) -> Result<filestat, errno>

/// Process & Environment
wasi::args_get(argv: *mut *mut u8, argv_buf: *mut u8) -> Result<(), errno>
wasi::args_sizes_get() -> Result<(i32, i32), errno>
wasi::environ_get(env: *mut *mut u8, env_buf: *mut u8) -> Result<(), errno>
wasi::environ_sizes_get() -> Result<(i32, i32), errno>

/// Time (for profiling/logging only)
wasi::clock_time_get(clock_id: i32, precision: i64) -> Result<i64, errno>

/// Standard I/O
// Use fd_read/fd_write with fd 0 (stdin), 1 (stdout), 2 (stderr)

/// Exit
wasi::proc_exit(code: i32) -> !
```

**Twinkle Standard Library Wrapper:**

```tw
// prelude/wasi.tw - thin wrapper over host calls
pub fn read_file(path: string) -> Result<string, string> {
  // Call wasi::path_open, wasi::fd_read, wasi::fd_close
}

pub fn write_file(path: string, content: string) -> Result<void, string> {
  // Call wasi::path_open, wasi::fd_write, wasi::fd_close
}

pub fn get_args() -> array<string> {
  // Call wasi::args_get
}

pub fn get_env(key: string) -> string? {
  // Call wasi::environ_get
}
```

**Implementation Notes:**
- Rust bootstrap: Use `wasmtime-wasi` crate (implements this interface)
- Node.js wrapper: Use Node's `WASI` class
- Browser: Provide polyfill for file operations (use IndexedDB or virtual FS)
- All wrappers must implement identical semantics

**Testing:**
- Create `tests/wasi/` with programs that exercise each host call
- Ensure identical behavior across all runtime wrappers

---

## Enhanced Testing Strategy

### Test Categories

1. **Parser Tests** (Stage 1+)
   - Round-trip: `source → AST → pretty-print → AST'` (AST == AST')
   - Operator precedence
   - Error recovery (collect multiple errors)

2. **Type Checker Tests** (Stage 2+)
   - **NEW: Typed AST Golden Tests**
     ```
     tests/type_ok/simple.tw
     tests/type_ok/simple.expected.json  // Typed AST with inferred types
     ```
   - Type errors with precise error messages
   - Inference examples (ensure types are correctly inferred)

3. **Codegen Tests** (Stage 7+)
   - **NEW: Wasm Output Golden Tests**
     ```
     tests/codegen/simple.tw
     tests/codegen/simple.expected.wat  // Expected Wasm text output
     ```
   - Execute Wasm and check stdout
   - Binary output stability (same input → same bytes)

4. **Compatibility Suite** (Stage 8+)
   - **NEW: Bootstrap vs Self-Hosted Comparison**
     ```bash
     # Run both compilers on identical inputs
     $ ./twinkle-bootstrap compile tests/suite/example.tw -o bootstrap.wasm
     $ wasmtime twinkle-compiler.wasm -- compile tests/suite/example.tw -o selfhost.wasm

     # Outputs must be byte-for-byte identical
     $ diff bootstrap.wasm selfhost.wasm
     ```
   - Entire example suite (`examples/*.tw`) must compile identically
   - Execute both outputs, ensure identical behavior

5. **WASI Contract Tests** (All Stages)
   - Programs that exercise each host call
   - Run on all runtime wrappers (Rust, Node, Deno)
   - Ensure identical behavior

### Test Harness Structure

```
tests/
  parser_cases/
    simple.tw
    precedence.tw
    errors/
      unclosed_paren.tw
      unclosed_paren.expected_error

  type_ok/
    basic.tw
    basic.expected.json           # NEW: Typed AST
    inference.tw
    inference.expected.json

  type_err/
    mismatch.tw
    mismatch.expected_error

  codegen/
    hello.tw
    hello.expected.wat            # NEW: Expected Wasm text
    hello.expected_stdout.txt

  wasi/
    read_file.tw
    write_file.tw
    args.tw

  compatibility/                  # NEW: Bootstrap vs self-hosted
    suite.tw                      # Meta-test: compile all examples
    compare_outputs.sh            # Ensure identical compilation
```

### Testing Workflow

```bash
# Stage 1-7: Bootstrap compiler
$ cargo test                      # All Rust tests
$ cargo run -- test tests/        # Twinkle test suite

# Stage 8+: Self-hosted compiler
$ cargo run -- compile compiler/main.tw -o twinkle-compiler.wasm
$ ./run-compatibility-suite.sh    # Bootstrap vs self-hosted comparison
```

**Compatibility Suite Script:**

```bash
#!/bin/bash
# run-compatibility-suite.sh

BOOTSTRAP=./target/release/twinkle
SELFHOST="wasmtime twinkle-compiler.wasm --"

for test in tests/compatibility/*.tw; do
    echo "Testing $test..."

    # Compile with both compilers
    $BOOTSTRAP compile $test -o /tmp/bootstrap.wasm
    $SELFHOST compile $test -o /tmp/selfhost.wasm

    # Compare outputs
    if ! diff /tmp/bootstrap.wasm /tmp/selfhost.wasm; then
        echo "FAIL: Outputs differ for $test"
        exit 1
    fi

    # Run and compare execution
    bootstrap_out=$(wasmtime /tmp/bootstrap.wasm)
    selfhost_out=$(wasmtime /tmp/selfhost.wasm)

    if [ "$bootstrap_out" != "$selfhost_out" ]; then
        echo "FAIL: Execution differs for $test"
        exit 1
    fi

    echo "PASS: $test"
done

echo "All compatibility tests passed!"
```

---

## Stage 8: Self-Hosting - Core Compiler Only

**Goal:** Twinkle compiler (lexer, parser, typechecker, codegen) written in Twinkle

**Scope:** ONLY the core compiler. Defer tooling to later stages.

At this point, Twinkle language is feature-complete. Now rewrite just the compiler core:

Systematically translate Rust → Twinkle:

**Rust:**
```rust
fn lex(source: &str) -> Vec<Token> {
    let mut tokens = vec![];
    for c in source.chars() {
        match c {
            '0'..='9' => tokens.push(lex_number(c)),
            // ...
        }
    }
    tokens
}
```

**Twinkle:**
```tw
fn lex(source: string) -> array<Token> {
  tokens: array<Token> = []
  for c in source.chars() {
    case c {
      '0', '1', '2', '3', '4', '5', '6', '7', '8', '9' =>
        tokens.push(lex_number(c)),
      // Same logic!
    }
  }
  tokens
}
```

**Components to rewrite:**
- ✅ Lexer (easy)
- ✅ Parser (medium)
- ✅ Type checker (hard but mechanical)
- ✅ Code generator (medium)
- ✅ Error formatting (easy)

**Compile the self-hosted compiler:**
```bash
# Use Rust bootstrap to compile Twinkle compiler
$ ./twinkle-bootstrap compile compiler/main.tw -o twinkle-compiler.wasm

# Now use self-hosted compiler!
$ wasmtime twinkle-compiler.wasm -- compile hello.tw
```

### 8.2: Standard Library in Twinkle

Write essential libraries in Twinkle:

```tw
// prelude/json.tw
pub fn parse(s: string) -> Result<JsonValue, string> { ... }
pub fn stringify(v: JsonValue) -> string { ... }

// prelude/io.tw (WASI bindings)
pub fn read_file(path: string) -> Result<string, string> {
  // Call WASI host functions
}

pub fn write_file(path: string, content: string) -> Result<void, string> {
  // Call WASI host functions
}

// prelude/array.tw
pub fn map<A, B>(xs: array<A>, f: (A) -> B) -> array<B> {
  collect x in xs { f(x) }
}

pub fn filter<T>(xs: array<T>, pred: (T) -> bool) -> array<T> {
  collect x in xs {
    if pred(x) { x } else { continue }
  }
}
```

### 8.3: LSP Server Logic in Twinkle

```tw
// compiler/lsp.tw
pub fn handle_lsp_message(json: string) -> string {
  msg := json.parse(json)?

  result := case msg.method {
    "textDocument/completion" => handle_completion(msg.params),
    "textDocument/hover" => handle_hover(msg.params),
    "textDocument/definition" => handle_definition(msg.params),
    // All LSP logic in Twinkle!
  }

  json.stringify(result)
}

fn handle_completion(params: CompletionParams) -> CompletionResult {
  source := params.text_document.text
  position := params.position

  // Parse and typecheck
  ast := parse(source)
  typed := typecheck(ast)

  // Find completions at cursor
  completions := find_completions_at(typed, position)

  CompletionResult{ items: completions }
}
```

**Deliverable:**
- ✅ Twinkle compiler compiles itself
- ✅ Self-hosted compiler produces **byte-for-byte identical** output to bootstrap
- ✅ All compatibility tests pass
- ✅ Compiler is stable and ready for tooling

---

## Stage 9: Standard Library in Twinkle

**Goal:** Essential libraries written in Twinkle, not Rust

**Scope:** Core functionality needed by compiler and user programs

### Components

1. **WASI Wrapper** (`prelude/wasi.tw`)
   ```tw
   pub fn read_file(path: string) -> Result<string, string> { ... }
   pub fn write_file(path: string, content: string) -> Result<void, string> { ... }
   pub fn get_args() -> array<string> { ... }
   pub fn get_env(key: string) -> string? { ... }
   ```

2. **String Operations** (`prelude/string.tw`)
   ```tw
   pub fn split(s: string, sep: string) -> array<string> { ... }
   pub fn join(parts: array<string>, sep: string) -> string { ... }
   pub fn trim(s: string) -> string { ... }
   pub fn starts_with(s: string, prefix: string) -> bool { ... }
   ```

3. **Array Utilities** (`prelude/array.tw`)
   ```tw
   pub fn map<A, B>(xs: array<A>, f: (A) -> B) -> array<B> { ... }
   pub fn filter<T>(xs: array<T>, pred: (T) -> bool) -> array<T> { ... }
   pub fn find<T>(xs: array<T>, pred: (T) -> bool) -> T? { ... }
   pub fn reduce<T, A>(xs: array<T>, init: A, f: (A, T) -> A) -> A { ... }
   ```

4. **JSON** (`prelude/json.tw`)
   ```tw
   pub enum JsonValue {
     Null,
     Bool(bool),
     Number(float),
     String(string),
     Array(array<JsonValue>),
     Object(dict<string, JsonValue>),
   }

   pub fn parse(s: string) -> Result<JsonValue, string> { ... }
   pub fn stringify(v: JsonValue) -> string { ... }
   ```

5. **Dict Utilities** (`prelude/dict.tw`)
   ```tw
   pub fn keys<K, V>(d: dict<K, V>) -> array<K> { ... }
   pub fn values<K, V>(d: dict<K, V>) -> array<V> { ... }
   pub fn from_entries<K, V>(entries: array<(K, V)>) -> dict<K, V> { ... }
   ```

**Testing:**
- Each module has comprehensive unit tests
- Test WASI operations on all runtime wrappers
- JSON parser handles all valid JSON, rejects invalid
- String/array utilities match expected semantics

**Deliverable:**
- ✅ Standard library compiles with self-hosted compiler
- ✅ All library tests pass
- ✅ Compiler can use standard library
- ✅ Foundation for all future tooling

---

## Stage 10: Thin Runtime Wrappers

**Goal:** Minimal wrappers in various languages

### Rust Wrapper (Convenience)

```rust
// twinkle-cli/src/main.rs (~100 lines)
use wasmtime::*;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // Load compiler Wasm
    let engine = Engine::default();
    let module = Module::from_file(&engine, "twinkle-compiler.wasm")?;
    let mut store = Store::new(&engine, ());

    // Set up WASI for file I/O
    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_args()?
        .build();

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;

    // Instantiate
    let instance = linker.instantiate(&mut store, &module)?;

    // Call compile function
    let compile = instance.get_typed_func::<(), ()>(&mut store, "main")?;
    compile.call(&mut store, ())?;

    Ok(())
}
```

### Node.js Wrapper

```javascript
// twinkle.js (~50 lines)
import { readFile } from 'fs/promises';
import { WASI } from 'wasi';

const wasi = new WASI({
  args: process.argv,
  env: process.env,
});

const wasm = await WebAssembly.compile(
  await readFile('./twinkle-compiler.wasm')
);

const instance = await WebAssembly.instantiate(wasm, {
  wasi_snapshot_preview1: wasi.wasiImport,
});

wasi.start(instance);
```

### Browser Playground

```html
<!DOCTYPE html>
<html>
<head>
  <title>Twinkle Playground</title>
</head>
<body>
  <textarea id="code">fn main() { println("Hello!") }</textarea>
  <button onclick="compile()">Compile</button>
  <pre id="output"></pre>

  <script type="module">
    let compiler;

    async function init() {
      const response = await fetch('twinkle-compiler.wasm');
      const buffer = await response.arrayBuffer();
      const module = await WebAssembly.compile(buffer);
      compiler = await WebAssembly.instantiate(module);
    }

    window.compile = function() {
      const code = document.getElementById('code').value;
      const result = compiler.exports.compile(code);
      document.getElementById('output').textContent = result;
    };

    init();
  </script>
</body>
</html>
```

**Deliverable:**
- ✅ Multiple runtime wrappers (Rust, Node, Deno, Browser)
- ✅ Same Wasm works everywhere
- ✅ True language-agnostic distribution
- ✅ Users can run Twinkle on any platform

---

## Stage 11: Code Formatter in Twinkle

**Goal:** Pretty-printer/formatter as a Twinkle program

**Scope:** Format Twinkle source code consistently

The pretty-printer from Stage 1 served its purpose. Now rewrite as a proper formatter in Twinkle:

```tw
// tools/fmt/main.tw
import "compiler"  // Use self-hosted compiler's parser

pub fn format_file(path: string) -> Result<void, string> {
  source := wasi.read_file(path)?
  ast := compiler.parse(source)?
  formatted := pretty_print(ast)
  wasi.write_file(path, formatted)?
  .Ok()
}

fn pretty_print(ast: Program) -> string {
  // Format with consistent style:
  // - 2-space indentation
  // - Trailing commas in multi-line constructs
  // - Consistent spacing around operators
  // - Line length limit (80 chars)
}
```

**Features:**
- Parse → format → write back
- Preserve comments (doc comments at minimum)
- Idempotent: `format(format(x)) == format(x)`
- Fast: format large files in <100ms

**Integration:**
```bash
$ twinkle fmt main.tw              # Format one file
$ twinkle fmt src/                 # Format directory
$ twinkle fmt --check src/         # Check without modifying
```

**Deliverable:**
- ✅ Formatter written in Twinkle
- ✅ All examples formatted consistently
- ✅ CI checks formatting
- ✅ Fast and reliable

---

## Stage 12: LSP Server in Twinkle

**Goal:** Language Server Protocol implementation in Twinkle

**Scope:** Editor integration (completion, hover, go-to-def, diagnostics)

All LSP logic written in Twinkle, thin wrapper handles LSP protocol:

```tw
// tools/lsp/server.tw
pub fn handle_lsp_message(json: string) -> string {
  msg := json.parse(json)?

  result := case msg.method {
    "initialize" => handle_initialize(msg.params),
    "textDocument/completion" => handle_completion(msg.params),
    "textDocument/hover" => handle_hover(msg.params),
    "textDocument/definition" => handle_definition(msg.params),
    "textDocument/references" => handle_references(msg.params),
    "textDocument/formatting" => handle_formatting(msg.params),
    "textDocument/publishDiagnostics" => handle_diagnostics(msg.params),
    _ => .Err("Unknown method: ${msg.method}"),
  }

  json.stringify(result)
}

fn handle_completion(params: CompletionParams) -> CompletionList {
  source := params.text_document.text
  position := params.position

  // Parse (with error recovery)
  ast := compiler.parse_resilient(source)

  // Type check (partial, allows errors)
  typed := compiler.typecheck_partial(ast)

  // Find completions at cursor position
  completions := find_completions_at(typed, position)

  CompletionList{ items: completions }
}
```

**Features:**
- Diagnostics (errors/warnings) as you type
- Smart completions (context-aware)
- Hover for type information
- Go-to-definition
- Find references
- Rename refactoring
- Code formatting (calls formatter from Stage 11)

**LSP Wrapper:**
```rust
// lsp-wrapper/src/main.rs (~150 lines)
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let wasm = load_wasm("twinkle-lsp.wasm");

    let service = LspService::new(|client| {
        TwinkleLspBackend { client, wasm }
    });

    Server::new(stdin(), stdout(), service).serve().await;
}

impl LanguageServer for TwinkleLspBackend {
    async fn completion(&self, params: CompletionParams) -> Result<CompletionList> {
        let json = serde_json::to_string(&params)?;
        let result = self.wasm.call("handle_lsp_message", json);
        Ok(serde_json::from_str(&result)?)
    }
    // Forward all other methods similarly
}
```

**Editor Plugins:**
- VS Code extension (uses LSP wrapper)
- Neovim config (uses LSP wrapper)
- Zed, Helix, etc. (standard LSP)

**Deliverable:**
- ✅ LSP server logic in Twinkle
- ✅ Works with VS Code, Neovim, etc.
- ✅ Fast, responsive editing experience
- ✅ All features working

---

## Stage 13: Package Manager in Twinkle

**Goal:** Package manager for Twinkle ecosystem

**Scope:** Install, publish, version management

```tw
// tools/pkg/main.tw
pub fn install(package: string) -> Result<void, string> {
  // Parse package name + version
  // Fetch from registry
  // Resolve dependencies
  // Download and cache
}

pub fn publish(manifest: Manifest) -> Result<void, string> {
  // Validate package
  // Build and test
  // Upload to registry
}

pub fn update() -> Result<void, string> {
  // Check for updates
  // Resolve new dependencies
  // Update lock file
}
```

**Package Manifest:**
```tw
// twinkle.json
{
  "name": "myapp",
  "version": "1.0.0",
  "dependencies": {
    "http": "^2.0",
    "json": "^1.5"
  },
  "dev_dependencies": {
    "test_framework": "^0.3"
  }
}
```

**Commands:**
```bash
$ twinkle pkg install http         # Add dependency
$ twinkle pkg update                # Update all
$ twinkle pkg publish               # Publish to registry
$ twinkle pkg search "http server"  # Search packages
```

**Deliverable:**
- ✅ Package manager in Twinkle
- ✅ Central registry (initially simple, can improve)
- ✅ Dependency resolution
- ✅ Ecosystem can grow

---

## Stage 14: Documentation Generator

**Goal:** Generate documentation from source code

```tw
// tools/doc/main.tw
pub fn generate_docs(module_path: string) -> Result<void, string> {
  ast := compiler.parse_file(module_path)?

  // Extract doc comments
  // Generate HTML/Markdown
  // Create navigation
  // Write output
}
```

**Features:**
- Extract `/// doc comments`
- Generate searchable HTML
- Show type signatures
- Cross-reference (click to go to definition)
- Examples from code

**Deliverable:**
- ✅ Doc generator in Twinkle
- ✅ Beautiful, searchable docs
- ✅ All standard library documented

---

## Stage 15: Test Runner & Build System

**Goal:** Complete development toolchain

```tw
// tools/test/runner.tw
pub fn run_tests(pattern: string) -> TestResult {
  // Find all *_test.tw files
  // Compile and run
  // Report results
}

// tools/build/main.tw
pub fn build_project(config: BuildConfig) -> Result<void, string> {
  // Compile all modules
  // Link
  // Optimize
  // Output single Wasm
}
```

**Deliverable:**
- ✅ Test runner in Twinkle
- ✅ Build system in Twinkle
- ✅ All development tools self-hosted

---

## Summary

**Revised Stage Breakdown:**

| Stage | Goal | Scope |
|-------|------|-------|
| 0 | Repository Skeleton | Project structure, tests, CI |
| 1 | Lexer + Parser | Expression language, pretty-printer |
| 2 | Type Checker | Monomorphic types, inference |
| 3 | Records & Modules | Module system, inherent methods |
| 4 | Enums & Patterns | Pattern matching, try expressions |
| 5 | Traits (Contract) | Show trait, string interpolation |
| 6 | Generics (HM) | Parametric polymorphism, constraints |
| 7 | Wasm Backend | Code generation to WebAssembly GC |
| **8** | **Self-Hosting** | **Core compiler in Twinkle** |
| **9** | **Standard Library** | **WASI, JSON, string/array utils** |
| **10** | **Runtime Wrappers** | **Rust, Node, Deno, Browser** |
| **11** | **Formatter** | **Code formatting tool** |
| **12** | **LSP Server** | **Editor integration** |
| **13** | **Package Manager** | **Dependency management** |
| **14** | **Docs Generator** | **Documentation tooling** |
| **15** | **Test & Build** | **Complete toolchain** |

**Milestones:**

1. **Stages 0-7: Bootstrap Compiler (Rust)**
   - Simple, portable, pure functional style
   - Hand-written lexer/parser (no generators)
   - Full language implementation
   - Proves language design works

2. **Stage 8: Self-Hosting Achieved**
   - Twinkle compiler written in Twinkle
   - Byte-for-byte identical output to bootstrap
   - Compiler stabilizes before tooling

3. **Stage 9-10: Foundation for Tooling**
   - Standard library in Twinkle
   - Multi-platform distribution
   - Wasm-only deployment proven

4. **Stages 11-15: Complete Ecosystem**
   - All development tools in Twinkle
   - Professional-grade tooling
   - Self-sufficient ecosystem

**Key Success Metrics:**
- ✅ Rust bootstrap code is simple enough to port
- ✅ Self-hosted compiler passes all tests (compatibility suite)
- ✅ Distribution is single .wasm file + prelude sources
- ✅ Works on wasmtime, Node, Deno, browser without changes
- ✅ LSP, formatter, package manager all in Twinkle
- ✅ Stable Host ABI contract (WASI)
- ✅ Comprehensive test coverage at each stage

**Philosophy:**
> The best way to prove a language design works is to write the language in itself.
> The best way to ensure portability is to target WebAssembly.
> The best way to enable tooling is to make everything queryable and pure.

**With explicit milestones, stable Host ABI, and comprehensive testing, this plan achieves all three.**
