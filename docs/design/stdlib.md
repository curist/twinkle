# Standard Library

Standard library modules are imported with the `@` sigil:

```tw
use @std.fs
use @std.path
```

They are authored in Twinkle, compiled via the same Wasm GC backend pipeline as
user code, and embedded in `twc.wasm` at compiler build time. There is no
separate installation step — the stdlib is versioned with the compiler.

---

## Design Decisions

* **Embedded at build time.** Stdlib `.tw` sources are compiled to `ModuleIR`
  and linked into `twc.wasm`. The host only provides file I/O for user source
  files and build outputs.

* **Pure Twinkle where possible.** Modules that do not require host interaction
  (e.g. `@std.path`) are written entirely in Twinkle with no host imports.

* **Thin host wrappers where necessary.** Modules that touch the outside world
  (e.g. `@std.fs`) are thin wrappers over the host import interface. Host
  imports are not exposed directly to user code.

* **Logical paths.** All paths use `/` as separator. The host maps them to OS
  paths or virtual FS roots.

* **Deterministic.** No clock, no randomness, no process spawning.

---

## MVP Modules

### `@std.fs` — File system

Thin wrapper over host file I/O imports. All operations return `Result`.

#### Types

```tw
pub type FsError = { NotFound, PermissionDenied, InvalidUtf8, Other(String) }
pub type EntryKind = { File, Dir, Other }
pub type DirEntry = .{ name: String, kind: EntryKind }
```

#### API

```tw
pub fn read_bytes(path: String) -> Result<Vector<Byte>, FsError>
pub fn read_text(path: String) -> Result<String, FsError>
pub fn write_text(path: String, content: String) -> Result<Void, FsError>
pub fn write_bytes(path: String, bytes: Vector<Byte>) -> Result<Void, FsError>
pub fn mkdirp(path: String) -> Result<Void, FsError>
pub fn list_dir(path: String) -> Result<Vector<DirEntry>, FsError>
pub fn exists(path: String) -> Bool
```

---

### `@std.path` — Path manipulation

Pure Twinkle — no host imports. All functions treat paths as strings with `/`
as separator.

#### API

```tw
pub fn join(base: String, part: String) -> String
pub fn join_all(parts: Array<String>) -> String
pub fn dirname(path: String) -> String
pub fn basename(path: String) -> String
pub fn stem(path: String) -> String
pub fn extension(path: String) -> String
pub fn normalize(path: String) -> String
pub fn is_absolute(path: String) -> Bool
```

---

### `@std.proc` — Process environment

Thin wrapper over host process imports.

#### API

```tw
pub fn args() -> Array<String>
pub fn env(name: String) -> Option<String>
pub fn cwd() -> String
pub fn exit(code: Int) -> Never
```

---

## Prelude Additions

Two prelude functions for writing to stderr (available without `use`):

```tw
eprint(s: String) -> Void
eprintln(s: String) -> Void
```

These mirror `print`/`println` but target stderr.

---

## Future Modules

| Module       | Purpose |
|--------------|---------|
| `@std.json`  | JSON parse and encode |
| `@std.math`  | `sqrt`, `floor`, `ceil`, `pow`, `abs`, trig |
| `@std.io`    | Buffered stdout/stderr writes |
| `@std.test`  | Test runner primitives (`assert`, `assert_eq`, `describe`) |
