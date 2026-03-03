# Twinkle Standard Library

## Overview

Standard library modules are imported with the `@` sigil:

```tw
use @std.fs
use @std.path
```

They are authored in Twinkle, compiled via the same Wasm GC backend + Runtime IR + Linker
pipeline as user code, and **embedded in `twc.wasm`** at compiler build time. There is no
separate stdlib installation step and no runtime path resolution — the stdlib is versioned
together with the compiler.

Stage0 note (Rust host, current implementation): `@std.*` imports are resolved from `stdlib/*.tw`
sources (or `TWINKLE_STDLIB_ROOT`) and compiled as part of normal module compilation. The
embedding behavior above is the self-hosting (`twc.wasm`) target architecture.

---

## Design decisions

* **Embedded at build time.** Stdlib `.tw` sources are compiled to `ModuleIR` and linked
  into `twc.wasm` alongside the compiler and runtime. The host only needs to provide file
  I/O for user source files and build outputs.

* **Pure Twinkle where possible.** Modules that do not require host interaction (e.g. `@std.path`)
  are written entirely in Twinkle with no host imports.

* **Thin host wrappers where necessary.** Modules that touch the outside world (e.g. `@std.fs`)
  are thin Twinkle wrappers over the host import interface (`host.read_file`, `host.write_file`,
  `host.write_bytes`, `host.mkdirp`, `host.list_dir`, `host.exists`). The host imports are
  not exposed directly to user code.

* **Logical paths.** All paths in the stdlib API use `/` as separator and treat paths as
  logical strings. The host is responsible for mapping them to OS-native paths or virtual FS
  roots. The compiler itself works with logical module paths and never assumes a particular
  OS path convention.

* **Deterministic.** No clock, no randomness, no process spawning. Compiler output is fully
  determined by source content.

---

## MVP modules

### `@std.fs` — File system

Thin wrapper over host file I/O imports. All operations return `Result` — callers handle
errors explicitly.

#### Types

```tw
pub type FsError = { NotFound, PermissionDenied, Other(String) }
```

#### API

```tw
// Read the full contents of a file as a UTF-8 string.
pub fn read_text(path: String) -> Result<String, FsError>

// Write a UTF-8 string to a file, creating or overwriting it.
pub fn write_text(path: String, content: String) -> Result<Void, FsError>

// Write raw bytes to a file, creating or overwriting it.
// Used for binary outputs (e.g. .wasm files).
pub fn write_bytes(path: String, bytes: Array<Int>) -> Result<Void, FsError>

// Create a directory and all missing parent directories.
pub fn mkdirp(path: String) -> Result<Void, FsError>

// List the entries of a directory.
pub fn list_dir(path: String) -> Result<Array<DirEntry>, FsError>

// Check whether a path exists (file or directory).
pub fn exists(path: String) -> Bool
```

#### `DirEntry`

```tw
pub type EntryKind = { File, Dir, Other }
pub type DirEntry = .{ name: String, kind: EntryKind }
```

`DirEntry.name` is the bare name of the entry (not a full path). Callers use `@std.path.join`
to construct absolute paths from directory path + entry name.

#### Example

```tw
use @std.fs
use @std.path

fn read_module(base: String, module: String) -> Result<String, fs.FsError> {
  fs.read_text(path.join(base, module))
}
```

---

### `@std.path` — Path manipulation

Pure Twinkle — no host imports. All functions treat paths as strings with `/` as the
separator. Behaviour is consistent across hosts.

#### API

```tw
// Join two path segments, inserting a separator if needed.
pub fn join(base: String, part: String) -> String

// Join an array of path segments.
pub fn join_all(parts: Array<String>) -> String

// Return the parent directory of a path.
// dirname("foo/bar/baz.tw") == "foo/bar"
// dirname("foo") == "."
pub fn dirname(path: String) -> String

// Return the last component of a path (including extension).
// basename("foo/bar/baz.tw") == "baz.tw"
pub fn basename(path: String) -> String

// Return the last component without its extension.
// stem("foo/bar/baz.tw") == "baz"
pub fn stem(path: String) -> String

// Return the file extension including the leading dot, or "" if none.
// extension("foo/bar/baz.tw") == ".tw"
// extension("foo/bar/baz")    == ""
pub fn extension(path: String) -> String

// Normalize a path: collapse ".", "..", and redundant separators.
// normalize("foo//bar/../baz") == "foo/baz"
pub fn normalize(path: String) -> String

// Whether the path starts with "/".
pub fn is_absolute(path: String) -> Bool
```

#### Example

```tw
use @std.path

// Turn a module identifier "foo.bar.baz" into a relative file path "foo/bar/baz.tw"
fn module_to_path(module_id: String) -> String {
  // (replace dots with slashes, add extension)
  path.join_all([module_id]) // illustrative; real impl replaces "." with "/"
}
```

---

### `@std.proc` — Process environment

Thin wrapper over host process imports. Provides access to CLI arguments, environment
variables, working directory, and process exit.

#### API

```tw
// Return the command-line arguments as an array of strings.
// The first element is the program name (if available).
pub fn args() -> Array<String>

// Read an environment variable. Returns None if not set.
pub fn env(name: String) -> Option<String>

// Return the current working directory as a logical path.
pub fn cwd() -> String

// Terminate the process with the given exit code.
// Does not return.
pub fn exit(code: Int) -> Never
```

#### Example

```tw
use @std.proc

fn main() {
  a := proc.args()
  if Array.len(a) < 2 {
    eprintln("usage: myapp <file>")
    proc.exit(1)
  }
  // ...
}
```

---

## Prelude additions — stderr output

Two new prelude functions for writing to stderr (available without any `use` import):

```tw
// Write a string to stderr, no newline.
eprint(s: String) -> Void

// Write a string to stderr with a trailing newline.
eprintln(s: String) -> Void
```

These mirror `print`/`println` but target stderr, which is essential for diagnostic
messages that should not pollute stdout output (e.g. compiler warnings, progress info).

---

## Future modules

These are not part of the MVP but are natural follow-ons:

| Module       | Purpose                                              |
|--------------|------------------------------------------------------|
| `@std.json`  | JSON parse and encode — useful for config, metadata, LSP protocol |
| `@std.math`  | `sqrt`, `floor`, `ceil`, `pow`, `abs`, trig — pure wrappers over Wasm numeric instructions |
| `@std.io`    | Buffered stdout/stderr writes beyond the basic prelude `println` |
| `@std.test`  | Test runner primitives (`assert`, `assert_eq`, `describe`) |

---

## Resolving TBC-001

The open questions from `tbc.md` TBC-001 are now answered:

* **Module path:** stdlib is embedded in `twc.wasm`; no install-prefix path needed.
* **What modules exist:** `@std.fs`, `@std.path`, and `@std.proc` for MVP; see future table above.
* **Ships as source or IR:** `.tw` source, compiled at `twc.wasm` build time via the Wasm GC
  backend + Runtime IR + Linker pipeline.
* **Caching / versioning:** versioned with `twc.wasm`; no separate cache mechanism needed.
