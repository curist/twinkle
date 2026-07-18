# Dynamic Module Loader

This document sketches a host-backed dynamic module loader for Twinkle programs.
The goal is to let Twinkle code control hot-loading policy while the host handles
mechanics that are outside Twinkle's current runtime model: filesystem discovery,
compilation, Wasm instantiation, cache invalidation, and socket/event-loop
integration.

The motivating use case is PHP-style file-based web routing: upload or edit a
page file, and the running server can pick it up without a process restart. The
same mechanism should also support non-web hot swap, such as replacing a game
logic module while a loop keeps running.

---

## Problem

Twinkle is compiled, so a running process cannot simply `require` a changed
source file the way Lua/Fennel or PHP can. A host can compile a `.tw` file to
Wasm and instantiate it dynamically, but if all routing and cache policy live in
host code, the framework becomes hard to customize from Twinkle itself.

The desired split is:

* **Host mechanism:** scan files, compile modules, instantiate Wasm, maintain a
  last-good cache, expose metadata, and call exported functions.
* **Twinkle policy:** decide which files are routable, map requests to modules,
  choose fallback behavior, wrap raw dynamic exports into typed capabilities, and
  decide when to invalidate or replace loaded modules.

This preserves Twinkle's explicit module/import model for ordinary code while
adding a controlled dynamic boundary for framework and plugin systems.

---

## Current ABI Constraint

The public Twinkle extern boundary is intentionally narrow. Host functions can
currently exchange scalars, `String`, opaque extern references and their nullable
form, plus a few byte/string vector shapes such as `Vector<Byte>`,
`Vector<String>`, and `Result<Vector<Byte>, String>`. They cannot exchange
Twinkle records, callbacks, arbitrary enums, arbitrary `Result` shapes, or
`Vector<T>` for general `T`.

Therefore the host cannot directly provide this value as an extern result or
argument:

```tw
pub type ModuleLoader = .{
  files: fn(String) Result<Vector<FileInfo>, LoadError>,
  inspect: fn(String) Result<ModuleInfo, LoadError>,
  load: fn(String) Result<LoadedModule, LoadError>,
  invalidate: fn(String) Void,
}
```

The MVP must expose narrow extern-safe host functions first. A Twinkle library
can then wrap those functions into ordinary Twinkle records for application code.
The record-of-functions shape remains the Twinkle-facing capability pattern; it
is not the raw ABI.

---

## Design Principles

* **Capabilities, not ambient magic.** Dynamic loading is exposed to application
  code as an explicit capability record. Under the current ABI that record is
  constructed by Twinkle wrapper code around opaque host handles and extern
  functions.
* **Host-backed, Twinkle-directed.** The host performs compilation and
  instantiation; Twinkle decides what should be loaded and how it is used.
* **Narrow encoded dynamic ABI first.** Raw dynamic calls use encoded bytes at
  the host boundary. Twinkle libraries may expose a `Dyn` helper type above that
  encoding, but `Dyn` itself is not assumed to be extern-safe.
* **Explicit adapters.** Dynamically loaded modules must either export
  adapter-shaped entrypoints, or a future compiler/host layer must generate those
  adapters from export metadata.
* **Last-good hot swap.** Replacing a module should be atomic. A failed compile
  should not necessarily take down the running app.
* **Stable state lives outside swapped modules.** Hot-swapped modules should not
  be the owner of long-lived application state unless they also define an
  explicit migration boundary.
* **No broad runtime reflection.** Twinkle should inspect explicit host-provided
  metadata, not gain arbitrary reflective access to all runtime values.
* **Allowlisted host powers.** Loading a dynamic module must not implicitly give
  it access to the default Node host capability set.
* **Scoped loader authority.** A loader handle is created with immutable allowed
  roots and compilation/import policies. Twinkle can choose among files the
  loader is authorized to access, but it cannot expand that authority by passing
  arbitrary paths.

---

## MVP Host ABI

The MVP should define a small host module that uses extern handles and encoded
metadata/call payloads.

```tw
pub extern dynloader {
  type LoaderHandle
  type ModuleHandle
  type LoadAttemptHandle

  fn files(loader: LoaderHandle, root: String) Result<Vector<Byte>, String>
  fn inspect(loader: LoaderHandle, path: String) Result<Vector<Byte>, String>
  fn load(loader: LoaderHandle, path: String) LoadAttemptHandle
  fn load_state(attempt: LoadAttemptHandle) Int
  fn load_module(attempt: LoadAttemptHandle) ModuleHandle?
  fn load_error(attempt: LoadAttemptHandle) String
  fn module_info(module: ModuleHandle) Result<Vector<Byte>, String>
  fn module_exports(module: ModuleHandle) Result<Vector<Byte>, String>
  fn module_call(module: ModuleHandle, name: String, args: Vector<Byte>) Result<Vector<Byte>, String>
  fn invalidate(loader: LoaderHandle, path: String)
}
```

Notes:

* `LoaderHandle`, `ModuleHandle`, and `LoadAttemptHandle` are opaque host
  references.
* Fallible operations returning data use the currently extern-safe
  `Result<Vector<Byte>, String>` shape.
* `load` returns an operation-scoped attempt handle instead of a loader-scoped
  last-error slot. The wrapper queries `load_state`, then reads either
  `load_module` or `load_error` from that same attempt, so concurrent loads on one
  loader cannot overwrite each other's diagnostics. `load_state` uses a small
  numeric protocol (`0` pending, `1` loaded, `2` failed) until Twinkle has an
  extern-safe enum/result ABI for this shape. A future FFI can replace this with
  `Result<ModuleHandle, LoadError>`.
* Byte payloads use a stable framework encoding. JSON text is acceptable for an
  early implementation, but the design should not assume an unstandardized
  `lib.json` module is public stdlib API. If JSON becomes the public encoding,
  standardize and document it first.
* Host-side compilation/loading is asynchronous in JavaScript. The attempt handle
  supports a polling wrapper. An implementation with JSPI/task-aware suspension
  may make `load` complete the attempt before returning, but diagnostics remain
  scoped to the returned attempt.

The host creates the initial `LoaderHandle` and passes it to the stable Twinkle
controller using an extern-safe mechanism for that embedding. Application code
should not construct these handles. This handle-oriented ABI is intentionally
low-level; it is a compatibility layer for today's extern rules, not the desired
long-term application API. If Twinkle gains async externs with richer result
shapes, `load` should collapse toward a direct typed result such as
`Result<ModuleHandle, LoadError>`.

---

## Twinkle Wrapper API

A Twinkle library can decode the host payloads and expose ergonomic records.
These records are ordinary Twinkle values; they do not cross the host extern
boundary.

```tw
pub type LoadError = {
  NotFound(String),
  CompileError(String),
  MissingExport(String),
  BadExport(String),
  DecodeError(String),
  Trap(String),
  HostError(String),
}

pub type ModuleId = .{ value: String }
pub type ModuleRevision = .{ value: String }
pub type AdapterAbiId = .{ value: String }
pub type AdapterAbiRevision = .{ value: String }
pub type EncodingId = .{ value: String }
pub type ContractId = .{ value: String }

pub type AbiType = {
  Bytes,
}

pub type DependencyInfo = {
  Unknown,
  Precise(Vector<ModuleId>),
}

pub type AbiInfo = .{
  params: Vector<AbiType>,
  ret: AbiType,
}

pub type AdapterInfo = .{
  id: AdapterAbiId,
  revision: AdapterAbiRevision,
}

pub type EncodingInfo = .{
  id: EncodingId,
}

pub type ContractInfo = .{
  id: ContractId,
}

pub type ExportInfo = .{
  name: String,
  abi: AbiInfo,
  adapter: AdapterInfo,
  encoding: EncodingInfo,
  contract: ContractInfo,
}

pub type ModuleInfo = .{
  id: ModuleId,
  path: String,
  revision: ModuleRevision,
  exports: Vector<ExportInfo>,
  dependencies: DependencyInfo,
}

pub type LoadedModule = .{
  info: ModuleInfo,
  has: fn(String) Bool,
  export_info: fn(String) ExportInfo?,
  call_bytes: fn(String, Vector<Byte>) Result<Vector<Byte>, LoadError>,
}

pub type FileInfo = .{
  path: String,
  rel_path: String,
  modified_ms: Int,
  size: Int,
  is_dir: Bool,
}

pub type ModuleLoader = .{
  files: fn(String) Result<Vector<FileInfo>, LoadError>,
  inspect: fn(String) Result<ModuleInfo, LoadError>,
  load: fn(String) Result<LoadedModule, LoadError>,
  invalidate: fn(String) Void,
}
```

The wrapper converts host strings and encoded byte payloads into these types. If
host decoding fails, the wrapper returns `DecodeError` instead of trapping when
possible.

`ExportInfo` is deliberately layered rather than a flat bag of strings:

* `abi` describes the low-level call shape. In the MVP, dynamic adapter exports
  are always `Vector<Byte> -> Vector<Byte>`, represented as `params: [.Bytes]`
  and `ret: .Bytes`. Other ABI types belong to a future typed-thunk layer and
  should not be accepted by the MVP loader.
* `adapter` names the byte-call convention and its opaque revision.
* `encoding` names the payload format inside the byte vector.
* `contract` names the framework-level meaning of the export.

Those layers change for different reasons, so the metadata keeps them separate
from the start. `DependencyInfo.Unknown` means dependency information is
unavailable and invalidation must be conservative; `Precise(deps)` is the only
form that may be used for correctness-sensitive invalidation decisions.

### Optional `Dyn` Convenience Layer

A library may define a `Dyn` type above `call_bytes`:

```tw
pub type Dyn = {
  Void,
  Int(Int),
  Bool(Bool),
  Float(Float),
  Str(String),
  Bytes(Vector<Byte>),
  Json(String),
}
```

`Dyn.Json` is represented here as a string until JSON is a documented public
stdlib value. `Dyn` is a Twinkle-side encoding helper, not a direct extern ABI.

The small `...Id` and `...Revision` record wrappers are nominal types. They still
encode strings at the host boundary, but they prevent Twinkle code from
accidentally passing a framework contract id where an encoding id or adapter ABI
id is expected.

---

## Contracts as Typed Wrappers

Raw dynamic calls are deliberately low-level. Twinkle code should turn a loaded
module into a typed capability by validating export metadata and installing
encoders/decoders.

```tw
pub type RequiredExport = .{
  name: String,
  abi: AbiInfo,
  adapter: AdapterInfo,
  encoding: EncodingInfo,
}

pub type ModuleContract<C> = .{
  id: ContractId,
  required_exports: Vector<RequiredExport>,
  optional_exports: Vector<RequiredExport>,
  build: fn(LoadedModule) Result<C, LoadError>,
}

pub fn load_as<C>(loader: ModuleLoader, path: String, contract: ModuleContract<C>) Result<C, LoadError> {
  module := try loader.load(path)

  for expected in contract.required_exports {
    actual := try module.export_info(expected.name).ok_or(.MissingExport(expected.name))
    if !export_matches(actual, expected, contract.id) {
      return .Err(.BadExport(expected.name))
    }
  }

  for expected in contract.optional_exports {
    case module.export_info(expected.name) {
      .Some(actual) => if !export_matches(actual, expected, contract.id) {
        return .Err(.BadExport(expected.name))
      },
      .None => {},
    }
  }

  contract.build(module)
}
```

Validation must be stronger than `module.has(name)`. Typed wrappers rely on the
adapter ABI, so the contract checks names, arity, parameter shapes, return shape,
adapter ABI identity/revision, payload encoding, and framework contract identity
before installing the wrapper. This prevents an unrelated `Vector<Byte> ->
Vector<Byte>` export from satisfying a web page or game-module contract by shape
alone.

The `export_matches` helper is part of the dynamic-loader library. It should not
assume compiler-derived equality for arbitrary records. It compares these small
metadata values explicitly: nominal id wrappers by their `.value` strings,
`actual.contract.id` against the `ModuleContract.id`, `AbiType` by variant, and
`Vector<AbiType>` by length plus element-wise variant comparison. If Twinkle later
provides explicit `Eq` methods or supported derivation for these metadata types,
the helper can delegate to those methods.

---

## Dynamic Module Entry ABI

A dynamically loaded module does not initially export ordinary rich Twinkle
functions for direct cross-instance calls. Instead, it exports explicit adapter
entrypoints with an encoded byte ABI. In the strict MVP, every callable adapter
has the same physical shape:

```tw
fn some_export_dyn(payload: Vector<Byte>) Vector<Byte>
```

`module_call` always passes one byte vector and returns one byte vector. Scalar
and JSON-shaped ABI cases are intentionally not part of this MVP; JSON, if used,
is an encoding inside the byte payload.

For example, a web page module should export something like:

```tw
use @web
use @html as h
use app.components.layout

pub fn get(ctx: web.Context) web.Response {
  web.page(
    .{ title: "Home" },
    h.fragment([
      h.h1("Welcome"),
      h.p("This page was loaded dynamically."),
    ]),
  )
}

pub fn get_dyn(args: Vector<Byte>) Vector<Byte> {
  ctx := web.decode_context(args)
  response := get(ctx)
  web.encode_response(response)
}
```

The page author should not write this adapter by hand in normal framework usage.
The framework can provide a helper or source-generation step; a later compiler
feature can generate adapters from annotated exports. Until such a feature
exists, the implementable contract is the adapter export (`get_dyn`), not direct
calls to `get(ctx: web.Context) web.Response`.

A future typed-adapter path may use compiler-emitted library export metadata and
host-side generated thunks to call rich exports directly. That is a later layer,
not the MVP assumption.

---

## Web Page Loading

A web framework can define a page contract over the same dynamic loader.

```tw
use @web

pub type Page = .{
  get: fn(web.Context) Result<web.Response, LoadError>,
  post: fn(web.Context) Result<web.Response, LoadError>,
}

pub fn page_contract() ModuleContract<Page> {
  .{
    id: ContractId.{ value: "web.page" },
    required_exports: [
      .{
        name: "get_dyn",
        abi: .{ params: [.Bytes], ret: .Bytes },
        adapter: .{ id: AdapterAbiId.{ value: "twinkle.dynamic.bytes" }, revision: AdapterAbiRevision.{ value: "1" } },
        encoding: .{ id: EncodingId.{ value: "web.context-response.v1" } },
      }
    ],
    optional_exports: [
      .{
        name: "post_dyn",
        abi: .{ params: [.Bytes], ret: .Bytes },
        adapter: .{ id: AdapterAbiId.{ value: "twinkle.dynamic.bytes" }, revision: AdapterAbiRevision.{ value: "1" } },
        encoding: .{ id: EncodingId.{ value: "web.context-response.v1" } },
      }
    ],
    build: fn(module: LoadedModule) {
      .Ok(.{
        get: fn(ctx: web.Context) {
          raw := try module.call_bytes("get_dyn", web.encode_context(ctx))
          web.decode_response(raw)
        },
        post: fn(ctx: web.Context) {
          if module.has("post_dyn") {
            raw := try module.call_bytes("post_dyn", web.encode_context(ctx))
            web.decode_response(raw)
          } else {
            .Ok(web.method_not_allowed([web.Method.Get]))
          }
        },
      })
    },
  }
}
```

A page file stays self-contained and uses ordinary explicit imports for shared
logic. The framework owns the dynamic boundary, so ordinary page authors do not
write raw extern declarations.

### Twinkle-Controlled Routing

The running application can keep route policy in Twinkle:

```tw
pub fn route_to_file(req: web.Request) Option<String> {
  if req.path == "/" {
    .Some("app/pages/index.tw")
  } else {
    candidate := "app/pages${req.path}.tw"
    .Some(candidate)
  }
}

pub fn handle(loader: ModuleLoader, req: web.Request) Result<web.Response, LoadError> {
  path := try route_to_file(req).ok_or(.NotFound(req.path))
  page := try load_as(loader, path, page_contract())
  ctx := web.context(req)

  case req.method {
    .Get => page.get(ctx),
    .Post => page.post(ctx),
    _ => .Ok(web.method_not_allowed([.Get, .Post])),
  }
}
```

The route result is only a candidate path. The host-backed loader still resolves
and canonicalizes it inside the loader's allowed roots before reading, compiling,
or invalidating anything.

A more complete framework would provide helpers for filesystem routing rules:
index files, extensionless URLs, ignored `_` helper files, API mounts, fragments,
and static-file fallbacks. Those helpers are ordinary Twinkle functions over
`Vector<FileInfo>`.

---

## Non-Web Hot Swap

The same loader can drive hot-swappable game logic.

```tw
pub type GameModule<State, Input, Frame> = .{
  update: fn(State, Input) Result<State, LoadError>,
  render: fn(State) Result<Frame, LoadError>,
}
```

An MVP game contract should encode `State`, `Input`, and `Frame` to bytes at the
boundary and require adapter exports such as:

```tw
pub fn update_dyn(payload: Vector<Byte>) Vector<Byte> { ... }
pub fn render_dyn(payload: Vector<Byte>) Vector<Byte> { ... }
```

A game framework can define a contract that expects those exports, then reload
that module when the host reports a changed revision.

Long-lived game state should be owned by the stable controller, a host service,
a database, or an explicitly encoded byte/JSON value. A normal source-level
`use` shared by separately compiled dynamic modules does not imply shared runtime
storage. A stable runtime module instance can hold shared state only if the
embedding explicitly supports importing that same live instance into dynamic
modules. If the state shape changes, the module should expose an explicit
migration export:

```tw
pub fn migrate_dyn(payload: Vector<Byte>) Vector<Byte> { ... }
```

The loader should not try to infer state migrations.

---

## Cache and Replacement Semantics

A host implementation should support at least these policies:

* **Development:** compile on first request after a source/dependency change;
  return rich diagnostics when compilation fails.
* **Dynamic production:** compile on demand and atomically replace the cached
  module only when compilation succeeds; keep serving the last-good module on
  failure.
* **Prebuilt production:** compile all dynamic modules ahead of time and serve
  only the built artifacts.

A candidate module is published to the host's raw module cache only after the
host-level candidate lifecycle succeeds:

1. compile the source;
2. instantiate the Wasm module;
3. run module start/top-level initialization;
4. read and validate raw export metadata; and
5. atomically publish the candidate for future raw loads.

Framework contract validation happens in the Twinkle wrapper (`load_as`) before a
typed capability is exposed to application code. If a framework wants
contract-specific last-good behavior, it should keep its own typed-capability
cache and replace that cache only after `load_as` succeeds.

If compilation, instantiation, initialization, or metadata decoding fails, the old
host-level last-good module remains current. Atomic replacement is atomic with
respect to module visibility, not arbitrary external side effects: allowlisted
host calls made during candidate initialization cannot be rolled back. Frameworks
may therefore prohibit effectful top-level initialization in dynamic modules, or
instantiate candidates with a reduced capability set until validation succeeds.

`ModuleInfo.revision` is an opaque host-chosen identity for the loaded module
contents, such as a content hash or mtime/dependency digest. Twinkle should
compare revisions for equality only; it should not parse their format or assume
semantic-version ordering.

`ModuleInfo.id` is the host/framework identity for the logical module. It may be
a canonical route path, package-qualified plugin name, UUID, or another stable
identifier chosen by the embedding. `path` is where this revision was loaded
from; it is not necessarily the long-term identity of the module.

For implementations based on the current JS runtime, dynamic compilation can use
`compile(path, { lib: true })` and instantiation can use `loadLib`, but dynamic
instantiation needs a stricter mode than today's default embedding. The loader
must pass a restricted `imports`/host capability set, disable ambient
`globalThis` extern resolution, and avoid the default Node filesystem, process,
stdin/stdout, and run-wasm host powers unless the framework deliberately allows
specific operations.

---

## Dependency Invalidation

The MVP host can conservatively invalidate all dynamic modules when shared code
changes. That is simple and sound.

Precise invalidation requires dependency metadata that is not part of the current
loaded-module metadata contract. A later compiler/host integration can expose
import dependencies by query API or by a custom section in compiled Wasm:

```tw
pub type DependencyInfo = {
  Unknown,
  Precise(Vector<ModuleId>),
}

pub type ModuleInfo = .{
  id: ModuleId,
  path: String,
  revision: ModuleRevision,
  exports: Vector<ExportInfo>,
  dependencies: DependencyInfo,
}
```

`Unknown` means the host cannot provide complete dependency data, so invalidation
must be conservative. `Precise([])` means the host knows the module has no dynamic
invalidation dependencies. Best-effort dependency observations should be exposed
through a separate diagnostics/debug API, not through `DependencyInfo`, because
correctness-sensitive invalidation must not rely on incomplete data.

---

## Safety and Limits

### Dynamic calls are checked at the boundary

The host should reject calls with the wrong arity or values that cannot be
marshalled into the target export. In the MVP this primarily means verifying that
the named export is an accepted bytes adapter before calling it. Twinkle-side
decoders should convert malformed results into `LoadError.DecodeError` rather
than trapping when possible.

`module_call`'s host-level `Result<Vector<Byte>, String>` represents invocation
transport failures: missing export after validation, host protocol errors, and
Wasm traps. The wrapper should classify these into `BadExport`, `HostError`, or
`Trap` where the host provides enough detail. A successful host-level byte result
may still encode a framework-level error envelope; for example, a page adapter can
return an encoded `Result<Response, PageError>` without trapping. Framework-level
envelopes decode into contract-specific result/error types, not `HostError`,
`Trap`, or `DecodeError`; malformed envelopes become `DecodeError`. The page
examples use `LoadError` only for loader/transport/decode failures, while ordinary
HTTP/application failures are represented as `web.Response` values.

### Scoped loader authority

A `LoaderHandle` is created with an immutable set of allowed roots and compilation
policies. Every path supplied to `files`, `inspect`, `load`, or `invalidate` must
be resolved against those roots, canonicalized for `.` and `..`, checked for
symlink escapes, and rejected if it is absolute or outside the scope unless the
embedding explicitly enables that authority. The canonical in-scope identity, not
raw user input, should be used as the cache key.

Twinkle routing policy is not the security boundary. Twinkle chooses paths within
the authority granted by the loader; the host enforces that authority.

### No implicit access to process capabilities

Loading a module should not grant it arbitrary host powers. Dynamic-module
instantiation must run in an imports-only, deny-ambient mode: no `globalThis`
fallback for externs, no default Node/process/filesystem/stdin/stdout/run-wasm
imports, and no host module unless the framework explicitly allowlists it. Host
services such as filesystem, database, network, process execution, or graphics
should remain explicit capabilities passed into the stable controller or encoded
as framework-specific request data.

### Async effects are explicit in the embedding

Filesystem, compilation, and dynamic instantiation are async operations in the JS
host. The embedding must either run the loader externs through Twinkle's
JSPI/task-aware suspension path, or expose an explicit task/polling API. A design
that pretends these operations are always synchronous is not portable across host
runtimes.

### Loaded handles remain valid

Invalidating a path or replacing the cache does not invalidate existing
`LoadedModule` / `ModuleHandle` values. A handle is a reference to a specific
loaded revision and remains callable until no Twinkle-side references to it remain
and the host can reclaim it. New `loader.load(path)` calls may return a newer
revision, but old wrappers continue to target the revision they were built from.

This frozen-handle model avoids generation races where a page capability becomes
invalid while a request is already using it. Hosts may internally use reference
counts, GC finalizers, or epoch cleanup, but the observable rule is that
invalidation affects future loads, not existing handles.

### Concurrent calls are host-safe

A `ModuleHandle` identifies one instantiated module revision. Calls on that handle
target that instance. If the underlying Wasm instance or adapter ABI does not
support concurrent re-entry, the host must serialize calls for that handle rather
than silently cloning a different instance.

This gives stable observable semantics: top-level initialization runs once for the
handle, module globals belong to that instance, and a `Cell` hidden inside the
module has the same behavior on every conforming host. Frameworks that want a
stateless pool or per-call instantiation should expose a different abstraction,
such as a `ModuleFactory` or pooled contract, rather than changing
`ModuleHandle` semantics.

Invalidation may race with calls, but it only changes the cache for future loads.
It must not interrupt an in-flight `module_call` on an already-acquired handle.

### Shared state is explicit

Each loaded Wasm instance may have its own module globals. Programs that need
state shared across hot swaps should keep it in the stable controller, a host
service, a database, or an explicitly encoded state value.

### Resource handles need a separate policy

Long-lived resources such as sockets, files, database connections, and graphics
contexts should not be hidden inside replaceable modules. They should be owned by
stable host/framework capabilities and passed to dynamic modules through narrow
operations.

---

## Relationship to Twinkle Modules

This design does not replace Twinkle's static `use` system. Static imports remain
the right tool for normal program structure, type checking, and optimization.
Dynamic loading is for cases where the set of modules is intentionally chosen at
runtime: file-based pages, plugins, live game logic, scripting, and admin tools.

Dynamic modules should still use ordinary `use` imports internally. A page can be
self-contained, but shared components and services are still factored into normal
Twinkle modules and imported explicitly.

---

## Implementation Direction

A practical implementation can proceed in layers:

1. Define the narrow `dynloader` host ABI using `LoaderHandle`, `ModuleHandle`,
   encoded byte payloads, and extern-safe return shapes.
2. Build a Twinkle wrapper library that turns those extern calls into
   `ModuleLoader` / `LoadedModule` records for application code.
3. Support loading one `.tw` file as a library with a restricted host/import set
   and calling named `*_dyn` adapter exports through `Vector<Byte>` payloads.
4. Define export metadata for adapter validation: ABI shape, adapter ABI id and
   revision, payload encoding id, and framework contract id. As this stabilizes,
   split the byte-call convention and metadata schema into a dedicated dynamic
   module ABI document rather than keeping it embedded in this design note.
5. Build a web framework contract for page modules with `get_dyn` / `post_dyn`
   exports and framework-owned encoders/decoders.
6. Add file-based route helpers in Twinkle over `Vector<FileInfo>`.
7. Add last-good cache and development diagnostics.
8. Add dependency metadata and more precise invalidation.
9. Later, consider compiler-generated adapters or host-generated typed thunks so
   authors can write rich exports like `get(ctx: web.Context) web.Response`
   without manually maintaining byte-shaped entrypoints.

The important architectural constraint is that the host remains the loader, but
Twinkle owns the policy by receiving the loader as an explicit capability built
from current ABI-safe pieces.
