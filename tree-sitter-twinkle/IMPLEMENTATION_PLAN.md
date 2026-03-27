# Tree-Sitter Grammar Implementation Plan for Twinkle

## Overview

Implement a production-quality tree-sitter grammar for the Twinkle language based on the EBNF specification in `docs/grammar.ebnf`.

**Current State:**
- Tree-sitter infrastructure already exists at `tree-sitter-twinkle/`
- `grammar.js` has placeholder content that needs to be replaced
- Need to create external scanner for ASI and string interpolation

## Implementation Approach

### Phase 1: Core Grammar Structure
**Goal:** Build basic grammar without ASI or string interpolation

**Tasks:**
1. Replace placeholder `grammar.js` with full grammar structure
2. Implement program structure (imports, types, functions, statements)
3. Implement type system (primitives, records, sum types, generics, function types)
4. Implement expression hierarchy with correct operator precedence
5. Implement literals (int, float, bool, simple strings)
6. Implement control flow (if, case, for, collect, try)
7. Implement patterns (enum, wildcard, literal, identifier)
8. Use explicit semicolons (no ASI yet)

**Key Design Decisions:**
- **Operator Precedence** (lowest to highest): assign → or → and → equality → comparison → additive → multiplicative → unary → postfix
- **Conflicts:** Expected conflicts for `.{` ambiguity (record literal vs field access) and variant expressions
- **Keywords:** Reserve: fn, if, else, true, false, import, pub, type, case, try, and, or, for, in, collect, break, continue, return

**Validation:** Parse simple example files with explicit semicolons

### Phase 2: External Scanner for ASI
**Goal:** Implement Go-style automatic semicolon insertion

**Tasks:**
1. Create `src/scanner.c` with external scanner boilerplate
2. Implement ASI state machine:
   - Track nesting depth for `()`, `[]`, `{}`
   - Track last token type (identifier, literal, `)`, `]`, `}`, break, continue)
   - Insert semicolon at newline only if depth=0 and last token allows it
3. Update grammar to use `$._semicolon: $ => choice(';', $._newline)`
4. Add `externals: $ => [$._newline, ...]`

**ASI Rules:**
- Insert after: identifier, int/float/string/bool literals, `)`, `]`, `}`, break, continue
- Don't insert after: operators (+, -, *, /, etc.), keywords (fn, if, etc.), `,`, `.`, `(`, `[`, `{`

**Edge Cases:**
- Multi-line expressions: `x = 1 +\n 2` (no semicolon after `+`)
- Chained calls: `foo()\n.bar()` (no semicolon after `)` before `.`)

**Validation:** Parse all example files with proper ASI handling

### Phase 3: String Interpolation
**Goal:** Support `"text ${expr} more"` syntax

**Tasks:**
1. Extend external scanner with string state machine
2. Add tokens: `string_start`, `string_content`, `interpolation_start`, `interpolation_end`, `string_end`
3. Implement string parsing:
   - `"` → emit `string_start`, enter STRING state
   - Regular chars/escapes → emit `string_content`
   - `${` → emit `interpolation_start`, return to NORMAL state
   - `}` (in interpolation) → emit `interpolation_end`, return to STRING state
   - `"` (in STRING) → emit `string_end`, return to NORMAL state
4. Update grammar:
   ```javascript
   string_literal: $ => seq(
     $.string_start,
     repeat(choice($.string_content, $.interpolation)),
     $.string_end
   ),
   interpolation: $ => seq(
     $.interpolation_start,
     $.expression,
     $.interpolation_end
   )
   ```

**Edge Cases:**
- Nested braces in interpolation: `"${ case x { .Some(y) => y } }"`
- Escaped interpolation: `"\${x}"` should not interpolate
- Multiple interpolations: `"a${x}b${y}c"`

**Validation:** Parse examples with string interpolation (point.tw, safe_math.tw)

### Phase 4: Testing & Refinement
**Goal:** Ensure comprehensive test coverage and error recovery

**Tasks:**
1. Create test corpus in `test/corpus/`:
   - 01_basics.txt - basic parsing, comments, ASI
   - 02_types.txt - all type declaration forms
   - 03_functions.txt - function declarations and expressions
   - 04_expressions.txt - operator precedence and associativity
   - 05_patterns.txt - all pattern forms
   - 06_control_flow.txt - if, case, for, collect, try
   - 07_literals.txt - records, arrays, strings with interpolation
   - 08_modules.txt - imports and qualified names
   - 09_edge_cases.txt - ambiguous syntax, ASI edge cases
   - 10_errors.txt - error recovery
2. Test with `tree-sitter test`
3. Parse all files in `examples/` directory
4. Fix conflicts and improve error recovery
5. Optimize grammar rules

**Validation:** All corpus tests pass, all example files parse correctly

### Phase 5: Queries & Documentation
**Goal:** Add syntax highlighting and document the grammar

**Tasks:**
1. Create `queries/highlights.scm` for syntax highlighting
2. Create `queries/locals.scm` for scope analysis
3. Create `queries/injections.scm` if needed
4. Document grammar design decisions
5. Update README with usage instructions

## Critical Files

### Files to Modify:
- `tree-sitter-twinkle/grammar.js` - Complete rewrite with full grammar
- `tree-sitter-twinkle/src/scanner.c` - Create new external scanner

### Reference Files:
- `docs/grammar.ebnf` - EBNF specification (source of truth)
- `docs/spec.md` - Language semantics and examples

### Test Files:
- `examples/*.tw` - Real-world examples for validation

## Key Challenges

### 1. Ambiguity: `.{` - Record Literal vs Field Access
**Problem:** `.{ x: 1 }` (record) vs `obj.field { }` (field + block)
**Solution:** Use conflict resolution; tree-sitter GLR parsing will disambiguate based on context

### 2. Ambiguity: `.Some(x)` - Variant vs Field Access
**Problem:** `.Some(x)` (variant) vs `.field(args)` (field access + call)
**Solution:** Define as conflict; both are valid postfix operations, context determines meaning

### 3. String Interpolation with Nested Braces
**Problem:** `"${ case x { ... } }"` has nested braces
**Solution:** Track brace depth in external scanner's interpolation state

### 4. ASI with Operators
**Problem:** Don't insert semicolon after operators in multi-line expressions
**Solution:** Only insert after specific token types (identifiers, literals, closing delimiters, break, continue)

### 5. Function Types as Return Types
**Problem:** `fn make_adder() fn(Int) Int` has function type as return type
**Solution:** Allow any `type` in return position, including `function_type`

## Build & Test Commands

```bash
cd tree-sitter-twinkle

# Generate parser from grammar
tree-sitter generate

# Run test corpus
tree-sitter test

# Parse specific file
tree-sitter parse ../examples/point.tw

# Debug parse
tree-sitter parse --debug ../examples/fizzbuzz.tw

# Test specific corpus section
tree-sitter test -f "function"
```

## Success Criteria

1. ✅ All grammar rules from EBNF are implemented
2. ✅ Parser generates without conflicts (or expected conflicts are documented)
3. ✅ All example files in `examples/` parse successfully
4. ✅ ASI works correctly for all test cases
5. ✅ String interpolation parses correctly with nested expressions
6. ✅ Comprehensive test corpus passes
7. ✅ Syntax highlighting queries work
8. ✅ Parser performance is acceptable (< 1ms per file)

## Estimated Implementation Time

- Phase 1 (Core Grammar): ~2-3 hours
- Phase 2 (ASI): ~1 hour
- Phase 3 (String Interpolation): ~1 hour
- Phase 4 (Testing): ~1-2 hours
- Phase 5 (Queries): ~30 minutes

**Total: ~6-8 hours of focused work**
