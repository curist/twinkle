# Resolver Alias Ordering Plan (Complete)

## Problem

The resolver's pass 2b (`resolve_type_references`) processed type definitions
from a `HashMap`, which has non-deterministic iteration order. When an alias
references another alias (e.g. `type Q = P` where `type P = Point`), the
second alias could be resolved before the first, causing it to see the `Void`
placeholder target instead of the real resolved type.

This produced silent incorrect behavior: alias chains would intermittently
resolve to `Void` depending on HashMap ordering.

## Solution: Topological Sort (Option A)

Implemented in `src/types/resolve.rs`:

1. **Non-alias types first**: records and sums are resolved before any aliases,
   ensuring concrete type definitions are available.

2. **Alias dependency graph**: `collect_type_refs` extracts all type name
   references from an alias's AST type annotation. Dependencies are filtered
   to only those names that are themselves aliases.

3. **Kahn's algorithm**: `topo_sort_aliases` produces a deterministic
   processing order using topological sort. Aliases with no alias-dependencies
   are processed first, then their dependents, and so on.

4. **Single pass**: each alias is resolved exactly once, in the correct order.
   No redundant work, no spurious error messages.

Circular aliases are already detected by `detect_circular_aliases()` before
pass 2b runs, so the dependency graph is always a DAG.

## Tests

- `tests/typecheck/pass/alias_chain_depth3.tw` — depth-3 chain:
  `A -> B -> C -> Coord` with record constructors and type unification
- `tests/typecheck/pass/alias_chain_generic.tw` — generic chain:
  `IntW -> W -> Wrapper<Int>` with field access verification
- `tests/run/alias_chain_depth3.tw` — end-to-end depth-3 chain execution
- Existing: `record_constructor_alias.tw` covers depth-2 chains (`Q -> P -> Point`)
