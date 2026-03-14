# Dict Key Projection Adapters Plan

## Goal

Add ergonomic key-projection adapters for dictionaries so unsupported domain key
types can still be used cleanly without traits/interfaces.

Primary API:

* `Dict.by_string(fn(K) String)`
* `Dict.by_int(fn(K) Int)`
* `Dict.by_byte(fn(K) Byte)`

These produce reusable strategy values that map domain keys (`K`) to supported
dict key types.

---

## Why This Change

Twinkle intentionally keeps `Dict` key support narrow (`Int`, `String`, `Byte`)
for predictable semantics. That is good for runtime simplicity, but users still
need ergonomic ways to index by domain-specific keys (records/sums/etc.).

Projection adapters provide:

* explicit, reusable key mapping
* no trait system required
* ergonomic call sites for real-world code

---

## Current Baseline (2026-03-14)

* `Dict` operations work directly on whitelisted primitive keys.
* For domain keys, users must manually project at each call-site.
* No first-class helper exists for reusable key projection behavior.

---

## Target API (Sketch)

```tw
type KeyedBy<K, Key> = .{ key_of: fn(K) Key }

pub fn by_string<K>(key_of: fn(K) String) KeyedBy<K, String>
pub fn by_int<K>(key_of: fn(K) Int) KeyedBy<K, Int>
pub fn by_byte<K>(key_of: fn(K) Byte) KeyedBy<K, Byte>

pub fn set<K, Key, V>(ix: KeyedBy<K, Key>, d: Dict<Key, V>, k: K, v: V) Dict<Key, V>
pub fn get<K, Key, V>(ix: KeyedBy<K, Key>, d: Dict<Key, V>, k: K) Option<V>
pub fn has<K, Key, V>(ix: KeyedBy<K, Key>, d: Dict<Key, V>, k: K) Bool
pub fn remove<K, Key, V>(ix: KeyedBy<K, Key>, d: Dict<Key, V>, k: K) Dict<Key, V>
```

Expected usage:

```tw
users := Dict.by_string(fn(u: User) String { u.id })
scores: Dict<String, Int> = Dict.new()
scores = users.set(scores, alice, 10)
alice_score := users.get(scores, alice)
```

Note: method style on adapter values is preferred for ergonomics; qualified
forms should also work.

---

## Scope

In scope:

* projection adapter constructors (`by_string/by_int/by_byte`)
* basic projected dict operations (`set/get/has/remove`)
* docs + examples + boot coverage

Out of scope:

* arbitrary dict key support beyond primitive whitelist
* hash/eq custom strategies and bucketed map abstractions
* replacing existing `Dict` core APIs
* migration guidance for existing call sites

---

## Delivery Plan

### Milestone 1 — Adapter Type + Constructors

Implementation sketch:

* define projection adapter record/type in prelude
* add `Dict.by_string`, `Dict.by_int`, `Dict.by_byte`

Likely files:

* `prelude/dict.tw`
* `docs/API.md`

Acceptance:

* constructors typecheck and are first-class values
* both method and qualified call shapes resolve

### Milestone 2 — Projected Operations

Implementation sketch:

* add adapter operations:
  - `set/get/has/remove`
* all operations should be pure and return standard dict values

Likely files:

* `prelude/dict.tw`
* `boot/tests/suites/api_dict_suite.tw`

Acceptance:

* projected operations behave identically to manual projection
* no behavior regression for existing direct dict usage

### Milestone 3 — Docs + Example + Ergonomics Validation

Implementation sketch:

* add API docs with usage snippets for:
  - `Dict.by_string`
  - `Dict.by_int`
  - `Dict.by_byte`
* add a compact runnable example in `examples/` (e.g.
  `examples/dict_key_projection.tw`) showing:
  - record/domain key projection
  - adapter-based `set/get/has/remove`
* include one byte-projection usage in docs or example for parity with
  `Dict<Byte, V>`

Likely files:

* `docs/API.md`
* `examples/dict_key_projection.tw` (or equivalent new example file)

Acceptance:

* docs clearly explain when to use projection adapters
* docs include concrete usage snippets
* example demonstrates reduced call-site boilerplate

---

## Risks and Mitigations

* Type inference noise around generic adapter values:
  - Mitigation: provide explicit examples with local type annotations.
* API clutter in `Dict` namespace:
  - Mitigation: keep surface minimal (`by_*` + four core ops only).
* Confusion between adapter and dict value:
  - Mitigation: docs emphasize adapter is a strategy, not storage.

---

## Exit Criteria

This plan is complete when:

1. `Dict.by_string/by_int/by_byte` exist and are documented.
2. Projected `set/get/has/remove` APIs are implemented and tested.
3. Boot suites pass in interpreter and Wasm with adapter coverage.
4. `docs/API.md` includes usage snippets for all `by_*` constructors.
5. At least one `examples/` program demonstrates ergonomic domain-key usage via adapters.
