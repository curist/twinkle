# Twinkle 🌟

A lightweight statically typed programming language targeting WebAssembly GC.

## Overview

Twinkle combines the elegance of functional programming with the practicality of modern systems languages:

- **Hindley-Milner type inference** (Gleam/OCaml style)
- **Traits as contracts** - not callable methods, pure compile-time constraints
- **Unboxed primitives** with GC-managed references
- **WebAssembly GC target** - leveraging `struct`, `array`, and reference types
- **Lightweight syntax** - scripting-like feel with static safety

## Quick Example

```tw
type Point = .{ x: float, y: float }

pub fn distance_squared(p1: Point, p2: Point) -> float {
  dx := p2.x - p1.x
  dy := p2.y - p1.y
  dx * dx + dy * dy
}

impl Show(Point) {
  fn show(p: Point) -> string {
    "(${p.x}, ${p.y})"
  }
}

fn main() -> void {
  p1 := Point{ x: 1.0, y: 2.0 }
  p2 := Point{ x: 4.0, y: 6.0 }

  dist := p1.distance_squared(p2)
  println("${p1} to ${p2}: distance² = ${dist}")
}
```

## Key Features

### Records & Modules
```tw
type Point = .{ x: int, y: int }

// Inherent methods via module functions
pub fn translate(p: Point, dx: int, dy: int) -> Point {
  Point{ x: p.x + dx, y: p.y + dy }
}

// Dot syntax desugars: p.translate(1,2) → point.translate(p,1,2)
```

### Enums & Pattern Matching
```tw
enum Tree<T> {
  Empty,
  Node(T, Tree<T>, Tree<T>),
}

fn sum(t: Tree<int>) -> int {
  case t {
    .Empty => 0,
    .Node(val, left, right) => val + sum(left) + sum(right),
  }
}
```

### Error Handling
```tw
fn safe_divide(a: int, b: int) -> Result<int, string> {
  if b == 0 {
    .Err("division by zero")
  } else {
    .Ok(a / b)
  }
}

fn compute() -> Result<int, string> {
  x := try safe_divide(10, 2)  // Early return on Err
  y := try safe_divide(x, 0)
  .Ok(y)
}
```

### Generics & Traits
```tw
fn map<A, B>(xs: array<A>, f: (A) -> B) -> array<B> {
  collect x in xs { f(x) }
}

// Trait constraints
fn log<T: Show>(x: T) -> void {
  println("${x}")  // String interpolation requires Show trait
}
```

## Documentation

- **[Language Specification](docs/spec.md)** - Complete language reference
- **[Grammar](docs/grammar.ebnf)** - Formal EBNF grammar
- **[Examples](examples/)** - Sample programs demonstrating key features

## Design Principles

- **No trait methods in user code** - Traits define contracts for compiler features only
  - `Show` → string interpolation (`"${x}"`)
  - `Iterable` → for loops and collect
  - `Eq`, `Ord`, `Add`, etc. → operators

- **Module-based inherent methods** - Dot syntax desugars to module function calls

- **Explicit over implicit** - No hidden method resolution or dynamic dispatch

## License

[License TBD]

## Acknowledgments

This project's initial design, documentation, and examples were developed with assistance from **AI-powered development tools**. The grammar specifications, language design decisions, and comprehensive example suite were collaboratively created through iterative refinement between human insight and machine assistance.

The implementation itself will be human-driven, using the AI-assisted design as a foundation.

---

*Twinkle is an experimental language project exploring the intersection of functional programming, static typing, and WebAssembly GC.*
