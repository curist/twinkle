# Rebinding receiver shorthand

## Context

Twinkle's immutable-by-default model means accumulation and transformation
patterns require explicit rebinding:

```tw
deps = deps.append(item)
canonical_paths = canonical_paths.append(path)
buf = buf.concat(other)
items = items.filter(fn(x) { x.active }).map(fn(x) { x.name })
state.items = state.items.append(entry)
````

The left-hand side appears twice on every such line. This is the most common
source of repetition in the boot compiler, appearing in nearly every function
that builds up a collection or threads state through method calls.

## Goal

Allow a method chain that begins with `.method(args)` on the right-hand side of
a rebinding assignment to implicitly use the assignment target as its receiver:

```tw
deps = .append(item)
canonical_paths = .append(path)
buf = .concat(other)
items = .filter(fn(x) { x.active }).map(fn(x) { x.name })
state.items = .append(entry)
```

Each of these desugars mechanically:

```tw
x = .method(args)
// becomes:
x = x.method(args)

obj.field = .method(args)
// becomes:
obj.field = obj.field.method(args)
```

When the method call is followed by a chain, the entire chain is part of the
right-hand side:

```tw
x = .foo().bar().baz()
// becomes:
x = x.foo().bar().baz()
```

This feature eliminates repeated assignment targets. It does not introduce a
general implicit receiver.

## Why not a separate `.=` operator?

An earlier version of this idea used syntax like:

```tw
foo .= append(bar)
```

That is where the original name "compound rebinding" came from.

However, Twinkle deliberately avoids compound assignment operators such as
`+=`, `-=`, `*=`, and so on. Introducing `.=` as a one-off special form would
create an operator family of one, which is inconsistent with the language's
preference for a single `=` rebinding form.

Using `=` keeps the assignment story unified:

```tw
x = expr
x = .method(args)
```

Both forms still read as:

1. compute a new value
2. rebind the target to that value

The shorthand is therefore best understood as receiver elision at the head of a
rebinding RHS, not as a new assignment operator.

## Non-goals

* No new semantics beyond syntactic desugaring into existing rebinding.
* No interaction with `Cell` or mutable references.
* No compound arithmetic operators such as `+=`, `-=`, `*=`, etc.
* No `.=` operator.
* No general implicit receiver expression form.
* No nested receiver shorthand inside arbitrary RHS expressions.
* No bare field shorthand in the first version.

In particular, the shorthand is valid only when the RHS of an assignment begins
with a lowercase-dot method call.

These are valid:

```tw
x = .append(item)
x = .filter(pred).map(f)
state.items = .append(entry)
```

These are invalid:

```tw
let y = .append(item)
return .append(item)
foo(.append(item))
x = f(.append(item))
x = .concat([.append(foo), .append(bar)])
x = .len
x = .items.len
```

If the user wants to reference the assignment target more than once inside a
larger expression, they should write it explicitly:

```tw
x = x.concat([x.append(foo), x.append(bar)])
```

or introduce local bindings:

```tw
fooed := x.append(foo)
bared := x.append(bar)
x = x.concat([fooed, bared])
```

The shorthand eliminates duplicated assignment targets at the start of the RHS.
It does not eliminate every possible repeated reference to the same value.

## Design

### Syntax

The shorthand applies to any valid rebinding assignment target:

```tw
<ident> = .<method>(...)
<lvalue>.<field> = .<method>(...)
<lvalue>[<index>] = .<method>(...)
```

When the right-hand side of a rebinding assignment starts with `.lowercase(`,
the parser treats that leading method call as if the left-hand side had been
written as the receiver.

```tw
target = .method(args)
// becomes:
target = target.method(args)
```

Further method or field accesses after the leading method call are parsed as a
normal postfix chain:

```tw
target = .method(args).next().field
// becomes:
target = target.method(args).next().field
```

This does not apply when the right-hand side starts with `.Uppercase` or `.{`.
Those retain their existing contextual meanings.

### Disambiguation

The parser decides based on the token after `.`:

| RHS starts with          | Meaning                      | Example          |
| ------------------------ | ---------------------------- | ---------------- |
| `.lowercase(`            | Rebinding receiver shorthand | `x = .append(1)` |
| `.Uppercase`             | Variant literal              | `x = .None`      |
| `.{`                     | Anonymous record literal     | `x = .{ a: 1 }`  |
| `.lowercase` without `(` | Invalid in v1                | `x = .len`       |

The shorthand depends on Twinkle's existing grammar-level naming convention:

* lowercase names are methods or fields
* uppercase names are variants

Because of that convention, these remain unambiguous:

```tw
x = .append(1) // receiver shorthand
x = .None      // variant literal
x = .Some(v)   // variant literal
x = .{ a: 1 }  // anonymous record literal
```

### Head-position-only rule

The shorthand is only recognized at the head of the assignment RHS.

Valid:

```tw
x = .append(foo)
x = .append(foo).append(bar)
x = .filter(pred).map(f)
```

Invalid:

```tw
x = foo(.append(bar))
x = [.append(foo), .append(bar)]
x = .concat([.append(foo), .append(bar)])
```

This keeps the feature local and predictable. There is no implicit receiver
scope for the whole RHS.

### Evaluation order for index targets

```tw
arr[i] = .method()
```

desugars semantically to:

```tw
arr[i] = arr[i].method()
```

However, implementations must preserve the same evaluation behavior as existing
update lowering. The assignment target and its subexpressions should be
evaluated according to the normal rebinding/update rules.

In particular, if existing update lowering evaluates the left-hand side once by
introducing temporaries for complex targets, this shorthand must use the same
lowering strategy.

The shorthand should not introduce extra observable evaluations of target
subexpressions beyond what the equivalent explicit update would require.

### Type checking

No changes.

After desugaring, the result is a regular rebinding assignment. The type checker
handles it exactly as if the user had written the receiver explicitly.

```tw
xs = .append(x)
```

is checked as:

```tw
xs = xs.append(x)
```

If the method does not exist on the receiver type, the existing method
resolution error applies.

### Codegen

No changes.

The desugared AST uses existing assignment, field access, index access, and
method call nodes.

### Error messages

If the left-hand side is not a rebindable target, the existing rebinding error
applies.

If the method does not exist on the target's type, the existing method
resolution error applies.

If `.lowercase(...)` appears outside the head of an assignment RHS, the parser
should report that receiver shorthand is only valid directly after `=` in a
rebinding assignment.

Examples:

```tw
foo(.append(x))
// error: receiver shorthand is only valid at the start of a rebinding RHS

let y = .append(x)
// error: receiver shorthand is only valid at the start of a rebinding RHS

x = .concat([.append(foo)])
// error: receiver shorthand is only valid at the start of a rebinding RHS
```

## Implementation

### Parser interception point

Assignment is handled as an infix operator in the Pratt parser
(`parse_expr_bp` in `parser.tw`). When the parser sees `=`, it currently calls
`parse_expr_bp(c.advance(), info.right)` to parse the RHS, which enters
`parse_prefix`.

The desugaring should intercept before the recursive `parse_expr_bp` call for
the RHS.

In the `.Eq` infix handler:

1. After consuming `=`, check whether the next tokens are `.` + lowercase
   `Ident` + `(`.
2. If so, parse the RHS as a receiver shorthand:

   * reconstruct or clone the LHS as the receiver expression
   * parse the leading `.method(args)` as a method call on that receiver
   * continue parsing any normal postfix chain after the method call
   * emit the normal `Binary(.Assign, lhs, rhs)` node
3. If the next tokens are `.` + uppercase `Ident`, fall through to the existing
   parse path for variant literals.
4. If the next tokens are `.{`, fall through to the existing parse path for
   anonymous record literals.
5. If the next tokens are `.` + lowercase `Ident` without `(`, report an error
   in v1.
6. Otherwise, fall through to the existing RHS parse path.

This keeps the change localized to assignment parsing and does not create a
general `.lowercase(...)` expression form.

### Scope of the shorthand

The shorthand applies only in the assignment RHS context described above.

It does not make `.lowercase(args)` a valid expression in other positions:

```tw
let y = .append(x)      // invalid
return .append(x)      // invalid
foo(.append(x))        // invalid
x = foo(.append(x))    // invalid
```

The `parse_prefix` handler for `.Dot` should continue to handle existing
contextual forms such as variants and anonymous records as it does today.

### Files to change

* `boot/compiler/parser.tw`

  * intercept in the `=` infix handler
* `src/parse/parser.rs`

  * same interception in stage0
* `docs/grammar.ebnf`

  * document the shorthand in the assignment expression rule
* `tree-sitter-twinkle/grammar.js`

  * update assignment parsing to recognize `.lowercase(...)` only at the head
    of assignment RHS
  * preserve existing variant and anonymous record behavior

## Examples

Before:

```tw
deps = deps.append(item)
parts = parts.append(segment)
errors = errors.concat(new_errors)
env = env.set(key, value)
items = items.filter(fn(x) { x.active }).map(fn(x) { x.name })
out = out.concat(prefix)
out = out.concat(body)
out = out.concat(suffix)
state.items = state.items.append(entry)
state.errors = state.errors.concat(new_errors)
ctx.locals = ctx.locals.set(idx, frame)
```

After:

```tw
deps = .append(item)
parts = .append(segment)
errors = .concat(new_errors)
env = .set(key, value)
items = .filter(fn(x) { x.active }).map(fn(x) { x.name })
out = .concat(prefix)
out = .concat(body)
out = .concat(suffix)
state.items = .append(entry)
state.errors = .concat(new_errors)
ctx.locals = .set(idx, frame)
```

Nested targets:

```tw
a.b.c = .method()
arr[i] = .append(x)
```

Explicit form is still preferred when the target is referenced multiple times:

```tw
x = x.concat([x.append(foo), x.append(bar)])
```

not:

```tw
x = .concat([.append(foo), .append(bar)])
```

## Tests

* Basic method shorthand:

  * `xs = .append(1)` on a `Vector<Int>` produces the expected value.
* Chained methods:

  * `xs = .filter(...).map(...)` applies the full chain.
* Record field target:

  * `r.items = .append(1)` behaves like `r.items = r.items.append(1)`.
* Nested field target:

  * `a.b.c = .method()` behaves like `a.b.c = a.b.c.method()`.
* Index target:

  * `arr[i] = .method()` behaves like `arr[i] = arr[i].method()`, while
    preserving normal update evaluation behavior.
* Variant literals still work:

  * `x = .None`
  * `x = .Some(v)`
* Anonymous record literals still work:

  * `x = .{ a: 1 }`
* Bare field shorthand is rejected in v1:

  * `x = .len`
  * `x = .items.len`
* Receiver shorthand outside assignment RHS is rejected:

  * `let y = .append(1)`
  * `return .append(1)`
  * `foo(.append(1))`
* Nested receiver shorthand is rejected:

  * `x = f(.append(1))`
  * `x = .concat([.append(foo), .append(bar)])`
* Type error:

  * `.method()` with a non-existent method reports the standard method
    resolution error.
* Scope error:

  * using the shorthand on a name that is not rebindable reports the standard
    rebinding error.

