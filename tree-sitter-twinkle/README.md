# tree-sitter-twinkle

Tree-sitter grammar for the Twinkle programming language.

## Features

- ✅ Complete syntax support for Twinkle language
- ✅ Automatic Semicolon Insertion (ASI) for Go-style semicolon elision
- ✅ String interpolation with `${expr}` syntax
- ✅ Pattern matching and variants
- ✅ Type declarations and generic types
- ✅ Syntax highlighting queries
- ✅ Comprehensive test corpus

## Installation

### Node.js

```bash
npm install tree-sitter-twinkle
```

### From source

```bash
git clone https://github.com/yourusername/tree-sitter-twinkle
cd tree-sitter-twinkle
npm install
npm run build
```

## Testing

```bash
# Run parser tests
tree-sitter test

# Parse a specific file
tree-sitter parse path/to/file.tw

# Test highlighting
tree-sitter highlight path/to/file.tw
```

## Language Support

This grammar supports all Twinkle language features:

- **Type System**: Primitives, records, sum types, generics, optional types
- **Functions**: Named declarations, anonymous functions, type parameters
- **Expressions**: Binary/unary operators, function calls, field access, indexing
- **Pattern Matching**: Enum patterns, wildcard patterns, literal patterns
- **Control Flow**: if/else, case expressions, for loops, collect expressions
- **Statements**: Let bindings, assignments, return/break/continue
- **Literals**: Numbers, booleans, strings with interpolation, arrays, records

## Syntax Highlighting

The grammar includes comprehensive syntax highlighting queries that support:

- Keywords and control flow
- Function declarations and calls
- Type annotations and declarations
- Variables and parameters
- String interpolation
- Comments
- Operators and punctuation
- Constructors and patterns

## Test Coverage

- 7/7 corpus tests passing (100%)
- 8/8 example files parsing successfully
- 7/7 highlight test files validated

## Grammar Features

### Operator Precedence

From lowest to highest:
1. Assignment (`=`, `:=`, `+=`, etc.)
2. Logical OR (`or`)
3. Logical AND (`and`)
4. Equality (`==`, `!=`)
5. Comparison (`<`, `<=`, `>`, `>=`)
6. Additive (`+`, `-`)
7. Multiplicative (`*`, `/`, `%`)
8. Unary (`-`, `!`)
9. Postfix (calls, field access, indexing)

### String Interpolation

Supports embedded expressions in strings:

```twinkle
name := "Alice"
message := "Hello, ${name}!"
result := "2 + 2 = ${2 + 2}"
```

### Automatic Semicolon Insertion

Semicolons are automatically inserted at newlines following Go-style ASI rules:

```twinkle
x := 1  // ASI inserts semicolon
y := 2  // ASI inserts semicolon

// Multi-line expressions work correctly
z := 1 +
     2  // No semicolon inserted after +
```

## License

MIT
