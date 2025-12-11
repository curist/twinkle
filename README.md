# Twinkle 🌟

A lightweight statically typed programming language targeting WebAssembly GC.

## Overview

Twinkle combines the elegance of functional programming with the practicality of modern systems languages:

- **Hindley-Milner type inference** (Gleam/OCaml style)
- **Capability records over traits** - no global typeclass resolution
- **Unboxed primitives** with GC-managed references
- **WebAssembly GC target** - leveraging `struct`, `array`, and reference types
- **Lightweight syntax** - scripting-like feel with static safety

## Quick Example

```tw
type Point = .{ x: Float, y: Float }

pub fn distance_squared(p1: Point, p2: Point) Float {
  dx := p2.x - p1.x
  dy := p2.y - p1.y
  dx * dx + dy * dy
}

pub fn to_string(p: Point) String {
  "(${p.x}, ${p.y})"
}

fn main() Void {
  p1 := Point.{ x: 1.0, y: 2.0 }
  p2 := Point.{ x: 4.0, y: 6.0 }

  dist := p1.distance_squared(p2)
  println("${p1.to_string()} to ${p2.to_string()}: distance² = ${dist}")
}
```

## Key Features

### Records & Modules
```tw
type Point = .{ x: Int, y: Int }

// Inherent methods via module functions
pub fn translate(p: Point, dx: Int, dy: Int) Point {
  Point.{ x: p.x + dx, y: p.y + dy }
}

// Dot syntax desugars: p.translate(1,2) → point.translate(p,1,2)
```

### Enums & Pattern Matching
```tw
type Tree<T> = {
  Empty,
  Node(T, Tree<T>, Tree<T>),
}

fn sum(t: Tree<Int>) Int {
  case t {
    .Empty => 0,
    .Node(val, left, right) => val + sum(left) + sum(right),
  }
}
```

### Error Handling
```tw
fn safe_divide(a: Int, b: Int) Result<Int, String> {
  if b == 0 {
    .Err("division by zero")
  } else {
    .Ok(a / b)
  }
}

fn compute() Result<Int, String> {
  x := try safe_divide(10, 2)  // Early return on Err
  y := try safe_divide(x, 0)
  .Ok(y)
}
```

### Generics & Capabilities
```tw
fn map<A, B>(xs: array<A>, f: fn(A) B) array<B> {
  collect x in xs { f(x) }
}

type Show<T> = .{ to_string: fn(T) String }

fn log<T>(x: T, show: Show<T>) Void {
  println(show.to_string(x))
}
```

## Documentation

- **[Language Specification](docs/spec.md)** - Complete language reference
- **[Grammar](docs/grammar.ebnf)** - Formal EBNF grammar
- **[Examples](examples/)** - Sample programs demonstrating key features

## Design Principles

- **No traits** - capabilities are explicit records of functions
- **Module-based inherent methods** - Dot syntax desugars to module function calls
- **Explicit over implicit** - No hidden method resolution or dynamic dispatch

## License

[License TBD]

## Acknowledgments

This project's initial design, documentation, and examples were developed with assistance from **AI-powered development tools**. The grammar specifications, language design decisions, and comprehensive example suite were collaboratively created through iterative refinement between human insight and machine assistance.

The implementation itself will be human-driven, using the AI-assisted design as a foundation.

---

*Twinkle is an experimental language project exploring the intersection of functional programming, static typing, and WebAssembly GC.*
