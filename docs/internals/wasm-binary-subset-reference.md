# Boot Wasm Binary Subset Reference

This document gathers the binary-format details the boot wasm serializer needs
for the Wasm IR Twinkle emits today.

It is intentionally narrow:

- only the module structure, types, sections, and instructions used by
  `boot/compiler/codegen/wasm_ir.tw`
- only the encodings the serializer needs to emit real `.wasm` binaries
- no attempt to restate the full WebAssembly spec

Primary sources used while assembling this reference:

- `boot/compiler/codegen/wasm_ir.tw`
- `boot/compiler/codegen/wat.tw`
- `boot/compiler/codegen/runtime/types.tw`
- WebAssembly GC draft binary spec:
  - <https://webassembly.github.io/gc/core/binary/types.html>
  - <https://webassembly.github.io/gc/core/binary/modules.html>
  - <https://webassembly.github.io/gc/core/binary/instructions.html>
  - <https://webassembly.github.io/gc/core/binary/values.html>

## 1. Serializer-facing Twinkle IR subset

Current boot IR surface in `boot/compiler/codegen/wasm_ir.tw`:

### Heap types

```tw
pub type HeapType = {
  Named(String),
  Any, Eq, I31, Func, None_, Extern,
}
```

### Value types

```tw
pub type ValType = {
  I8,
  I32, I64, F64,
  Anyref, I31ref, Funcref,
  Ref(Bool, HeapType),
}
```

### Type definitions

```tw
pub type TypeDef = {
  Struct(String, Vector<FieldDef>, String?, Bool),
  Array(String, FieldDef),
  FuncType(String, Vector<ValType>, Vector<ValType>),
}
```

The `Struct` payload is:

- type name
- fields
- optional supertype name
- `is_final`

That means the serializer must support:

- final struct types with no supertype
- non-final struct types with no supertype
- subtypes with an explicit supertype
- array types
- function types

### Module pieces

```tw
pub type WasmModule = .{
  namespace: String,
  types: Vector<TypeDef>,
  imports: Vector<ImportDef>,
  funcs: Vector<FuncDef>,
  globals: Vector<GlobalDef>,
  tables: Vector<TableDef>,
  elems: Vector<ElemDef>,
  exports: Vector<ExportDef>,
  data: Vector<DataSegment>,
  start: String?,
}
```

## 2. Core binary building blocks

### Magic and version

Every module starts with:

- magic: `00 61 73 6D`
- version: `01 00 00 00`

### Vectors

A wasm `vec(T)` is encoded as:

- `u32` length
- followed by each item in order

This is used everywhere:

- section payload vectors
- result types
- fields
- locals groups
- function indices in elem segments
- bytes in names and data segments

### Names

A wasm `name` is:

- UTF-8 bytes of the string
- wrapped as `vec(byte)`

Twinkle-side helper mapping:

- `String.utf8_bytes()` gives the payload bytes
- then emit `u32` byte count + raw bytes

### Integer encodings

Use LEB128.

- `u32` values use unsigned LEB128
- `i32` and `i64` immediates use signed LEB128
- uninterpreted integer immediates in the spec are also encoded as signed LEB128

Important serializer rule:

- **named heap types and block type indices are encoded as positive `s33`, not
  `u32`**
- for small positive values, signed and unsigned LEB often match
- but they do **not** always match once the sign bit of the last byte would be
  ambiguous
- the serializer should therefore have a dedicated helper for positive signed
  indices used in:
  - `heaptype` when encoding `Named(typeidx)`
  - block type indices if/when they are used

### Floating-point encodings

- `f64.const` immediate is 8 bytes, IEEE-754 little-endian

## 3. Section ids and ordering

Relevant section ids:

| Id | Section |
|---|---|
| 1 | type |
| 2 | import |
| 3 | function |
| 4 | table |
| 6 | global |
| 7 | export |
| 8 | start |
| 9 | element |
| 10 | code |
| 11 | data |
| 12 | data count |

The serializer should emit sections in canonical order.

For the current boot subset, the normal order is:

1. type
2. import
3. function
4. table
5. global
6. export
7. start
8. element
9. optional data count
10. code
11. data

Empty sections should simply be omitted.

## 4. Type encodings needed by Twinkle

### Number and packed types

| Twinkle IR | Binary |
|---|---|
| `I32` | `0x7F` |
| `I64` | `0x7E` |
| `F64` | `0x7C` |
| `I8` | `0x78` |

`I8` is a packed storage type for fields and array elements. It is **not** a
standalone value type.

### Abstract heap types

| HeapType | Binary |
|---|---|
| `Func` | `0x70` |
| `Extern` | `0x6F` |
| `Any` | `0x6E` |
| `Eq` | `0x6D` |
| `I31` | `0x6C` |
| `None_` | `0x71` |

Also defined by the spec but not currently represented directly in Twinkle IR:

- `nofunc = 0x73`
- `noextern = 0x72`
- `struct = 0x6B`
- `array = 0x6A`

### Reference types

General forms:

- `ref ht` → `0x64 <heaptype>`
- `ref null ht` → `0x63 <heaptype>`

Short forms exist for nullable abstract heap types:

| Twinkle IR | Meaning | Binary |
|---|---|---|
| `Anyref` | `ref null any` | `0x6E` |
| `I31ref` | `ref null i31` | `0x6C` |
| `Funcref` | `ref null func` | `0x70` |

Recommended serializer rule:

- use short forms for `Anyref`, `I31ref`, `Funcref`
- use full `0x63` / `0x64` forms for `Ref(nullable, heap)`

### Heap type indices for named types

For `HeapType.Named(name)`:

- look up the type index for `name`
- encode it as positive `s33`

Do **not** encode it as `u32`.

### Mutability

| Meaning | Binary |
|---|---|
| const | `0x00` |
| var | `0x01` |

### Limits

| Form | Binary |
|---|---|
| min only | `0x00 <u32 min>` |
| min + max | `0x01 <u32 min> <u32 max>` |

### Table types

Table type is:

- `<reftype> <limits>`

Current Twinkle table IR:

```tw
pub type TableDef = .{
  name: String,
  elem_type: ValType,
  min: Int,
  max: Int?,
}
```

That means the serializer only needs table types for reference element types.

### Global types

Global type is:

- `<valtype> <mutability>`

### Field types

Field type is:

- `<storagetype> <mutability>`

Where storage type is either:

- a value type
- or a packed type (`i8`)

### Composite type tags

| Composite type | Binary |
|---|---|
| array | `0x5E` |
| struct | `0x5F` |
| func | `0x60` |

### Subtypes and recursive types

The type section stores `rectype`, which contains `subtype` values.

Useful encodings:

| Form | Binary |
|---|---|
| final subtype with no supertypes, shorthand | `<comptype>` |
| final subtype with explicit supertypes | `0x4F <vec(typeidx)> <comptype>` |
| non-final subtype | `0x50 <vec(typeidx)> <comptype>` |
| recursive group | `0x4E <vec(subtype)>` |

Current Twinkle needs all of these:

- final structs with no supertype can use the shorthand
- non-final structs like runtime `Closure` and `IterState` need `0x50`
- subtype chains need explicit supertypes via `0x4F` / `0x50`
- recursive groups are needed when named types in the same strongly connected
  component refer to each other

Recommended serializer rule:

- follow the same SCC grouping strategy already used by `boot/compiler/codegen/wat.tw`
- emit each SCC as:
  - a single subtype shorthand if it is a singleton final/no-super/no-self-cycle type
  - otherwise a `rec` group (`0x4E`) containing ordered subtype members

## 5. Import, export, function, code, element, and data encodings

### Import descriptions

| Import kind | Binary |
|---|---|
| func | `0x00 <typeidx>` |
| table | `0x01 <tabletype>` |
| mem | `0x02 <memtype>` |
| global | `0x03 <globaltype>` |

Current `ImportDef` only models function imports, so the first serializer pass
only needs:

- module name
- import name
- desc tag `0x00`
- function type index

### Function section

The function section stores only type indices for defined functions.

It must line up 1:1 with the code section.

### Export descriptions

| Export kind | Binary |
|---|---|
| func | `0x00 <funcidx>` |
| table | `0x01 <tableidx>` |
| mem | `0x02 <memidx>` |
| global | `0x03 <globalidx>` |

Current `ExportDef` only models function exports.

### Start section

The start section payload is just:

- `<funcidx>`

### Code section

Each code entry is:

- `u32 size`
- function payload

Function payload is:

- `vec(local_group)`
- expression body

Each local group is:

- `u32 count`
- `valtype`

Important serializer rule:

- locals should be compressed into runs of identical `ValType`
- body expression is the instruction stream followed by `0x0B` (`end`)

### Element segments

Current Twinkle IR:

```tw
pub type ElemDef = .{
  table: String,
  offset: Vector<Instr>,
  func_syms: Vector<String>,
}
```

The simplest binary form matching current IR is active-func-indices form with
explicit table index:

- tag `2`
- `tableidx`
- offset expr
- elemkind `0x00`
- `vec(funcidx)`

If the table is known to be table 0, form `0` is also possible:

- tag `0`
- offset expr
- `vec(funcidx)`

Serializer recommendation:

- start by always emitting form `2`
- only shrink to form `0` later if desired

### Data segments

Current Twinkle IR:

```tw
pub type DataSegment = .{
  name: String,
  offset: Vector<Instr>,
  bytes: Vector<Byte>,
}
```

Useful forms:

- passive: `1 <vec(byte)>`
- active memory 0: `0 <expr> <vec(byte)>`

Serializer recommendation:

- if `offset.len() == 0`, emit passive form `1`
- otherwise emit active form `0`
- add data-count section only if data indices are used from code paths that
  require it

## 6. Block type encoding rule

For `block`, `loop`, and `if`, block type is encoded as one of:

- `0x40` for empty result
- a single `valtype` for one unnamed result
- a positive `s33` type index for a named function type

Current Twinkle IR stores block result as `ValType?`, so the serializer only
needs the first two forms right now:

- `None` → `0x40`
- `Some(t)` → encode `t` as `valtype`

## 7. Instruction opcode reference for the current boot subset

Only opcodes currently represented in `wasm_ir.tw` are listed here.

### Control flow

| Instr | Binary |
|---|---|
| `Unreachable` | `0x00` |
| `Nop` | `0x01` |
| `Block(bt, body)` | `0x02 <blocktype> ... 0x0B` |
| `Loop(bt, body)` | `0x03 <blocktype> ... 0x0B` |
| `If(bt, then, else)` | `0x04 <blocktype> ... [0x05 ...] 0x0B` |
| `Br(label)` | `0x0C <labelidx>` |
| `BrIf(label)` | `0x0D <labelidx>` |
| `BrTable(labels, default)` | `0x0E <vec(labelidx)> <labelidx>` |
| `Return` | `0x0F` |
| `Call(name)` | `0x10 <funcidx>` |
| `CallIndirect(type, table)` | `0x11 <typeidx> <tableidx>` |
| `ReturnCall(name)` | `0x12 <funcidx>` |
| `CallRef(type)` | `0x14 <typeidx>` |
| `ReturnCallRef(type)` | `0x15 <typeidx>` |

### Parametric and variable

| Instr | Binary |
|---|---|
| `Drop` | `0x1A` |
| `Select` | `0x1B` |
| `LocalGet` | `0x20 <localidx>` |
| `LocalSet` | `0x21 <localidx>` |
| `LocalTee` | `0x22 <localidx>` |
| `GlobalGet` | `0x23 <globalidx>` |
| `GlobalSet` | `0x24 <globalidx>` |

### Reference and GC instructions

Single-byte reference opcodes:

| Instr | Binary |
|---|---|
| `RefNull(ht)` | `0xD0 <heaptype>` |
| `RefIsNull` | `0xD1` |
| `RefFunc(name)` | `0xD2 <funcidx>` |
| `RefEq` | `0xD3` |
| `RefAsNonNull` | `0xD4` |

GC / typed-reference prefixed opcodes all use `0xFB <u32 subopcode> ...`

| Instr | Subopcode |
|---|---:|
| `StructNew` | 0 |
| `StructGet` | 2 |
| `StructGetS` | 3 |
| `StructSet` | 5 |
| `ArrayNew` | 6 |
| `ArrayNewDefault` | 7 |
| `ArrayNewFixed` | 8 |
| `ArrayNewData` | 9 |
| `ArrayGet` | 11 |
| `ArrayGetU` | 13 |
| `ArraySet` | 14 |
| `ArrayLen` | 15 |
| `ArrayCopy` | 17 |
| `RefTest(non-null)` | 20 |
| `RefTest(nullable)` | 21 |
| `RefCast(non-null)` | 22 |
| `RefCast(nullable)` | 23 |
| `RefI31` | 28 |
| `I31GetS` | 29 |
| `I31GetU` | 30 |

Immediate shapes:

- `StructNew(type)` → `0xFB <0> <typeidx>`
- `StructGet(type, field)` → `0xFB <2> <typeidx> <fieldidx>`
- `StructGetS(type, field)` → `0xFB <3> <typeidx> <fieldidx>`
- `StructSet(type, field)` → `0xFB <5> <typeidx> <fieldidx>`
- `ArrayNew(type)` → `0xFB <6> <typeidx>`
- `ArrayNewDefault(type)` → `0xFB <7> <typeidx>`
- `ArrayNewFixed(type, n)` → `0xFB <8> <typeidx> <u32 n>`
- `ArrayNewData(type, dataidx)` → `0xFB <9> <typeidx> <dataidx>`
- `ArrayGet(type)` → `0xFB <11> <typeidx>`
- `ArrayGetU(type)` → `0xFB <13> <typeidx>`
- `ArraySet(type)` → `0xFB <14> <typeidx>`
- `ArrayLen` → `0xFB <15>`
- `ArrayCopy(dst, src)` → `0xFB <17> <dst typeidx> <src typeidx>`
- `RefTest(nullable, ht)` → `0xFB <21 or 20> <heaptype>`
- `RefCast(nullable, ht)` → `0xFB <23 or 22> <heaptype>`
- `RefI31` → `0xFB <28>`
- `I31GetS` → `0xFB <29>`
- `I31GetU` → `0xFB <30>`

### Numeric constants

| Instr | Binary |
|---|---|
| `I32Const(n)` | `0x41 <i32>` |
| `I64Const(n)` | `0x42 <i64>` |
| `F64Const(z)` | `0x44 <f64 little-endian>` |

### i32 numeric ops

| Instr | Binary |
|---|---|
| `I32Eqz` | `0x45` |
| `I32Eq` | `0x46` |
| `I32Ne` | `0x47` |
| `I32LtS` | `0x48` |
| `I32LtU` | `0x49` |
| `I32GtS` | `0x4A` |
| `I32GtU` | `0x4B` |
| `I32LeS` | `0x4C` |
| `I32LeU` | `0x4D` |
| `I32GeS` | `0x4E` |
| `I32GeU` | `0x4F` |
| `I32Add` | `0x6A` |
| `I32Sub` | `0x6B` |
| `I32Mul` | `0x6C` |
| `I32DivS` | `0x6D` |
| `I32RemS` | `0x6F` |
| `I32And` | `0x71` |
| `I32Or` | `0x72` |
| `I32Shl` | `0x74` |
| `I32ShrU` | `0x76` |

### i64 numeric ops

| Instr | Binary |
|---|---|
| `I64Eqz` | `0x50` |
| `I64Eq` | `0x51` |
| `I64Ne` | `0x52` |
| `I64LtS` | `0x53` |
| `I64GtS` | `0x55` |
| `I64LeS` | `0x57` |
| `I64GeS` | `0x59` |
| `I64Add` | `0x7C` |
| `I64Sub` | `0x7D` |
| `I64Mul` | `0x7E` |
| `I64DivS` | `0x7F` |
| `I64RemS` | `0x81` |
| `I64And` | `0x83` |
| `I64Or` | `0x84` |
| `I64Xor` | `0x85` |
| `I64Shl` | `0x86` |
| `I64ShrS` | `0x87` |
| `I64ShrU` | `0x88` |

### f64 numeric ops

| Instr | Binary |
|---|---|
| `F64Eq` | `0x61` |
| `F64Ne` | `0x62` |
| `F64Lt` | `0x63` |
| `F64Gt` | `0x64` |
| `F64Le` | `0x65` |
| `F64Ge` | `0x66` |
| `F64Abs` | `0x99` |
| `F64Neg` | `0x9A` |
| `F64Ceil` | `0x9B` |
| `F64Floor` | `0x9C` |
| `F64Sqrt` | `0x9F` |
| `F64Add` | `0xA0` |
| `F64Sub` | `0xA1` |
| `F64Mul` | `0xA2` |
| `F64Div` | `0xA3` |

### Conversion ops

| Instr | Binary |
|---|---|
| `I32WrapI64` | `0xA7` |
| `I64ExtendI32S` | `0xAC` |
| `I64ExtendI32U` | `0xAD` |
| `I64TruncF64S` | `0xB0` |
| `F64ConvertI64S` | `0xB9` |

Note:

- `F64ReinterpretI32` exists in Twinkle IR/WAT emission, but it does not match a
  normal core wasm instruction spelling. Treat it as unsupported until its role
  in the current emitted subset is confirmed.

## 8. Recommended serializer shortcuts for the first implementation

To get the boot serializer working quickly, the first version should take the
most direct choices that still match the current IR.

Recommended shortcuts:

1. Use `Vector<Byte>` everywhere for byte accumulation.
2. Reuse the SCC grouping strategy from `boot/compiler/codegen/wat.tw` for type
   emission order.
3. Emit function imports only.
4. Emit function exports only.
5. Emit table elem segments in active explicit-table form (`tag 2`) first.
6. Emit passive data segments when `offset.len() == 0`, active memory-0 data
   segments otherwise.
7. Compress locals by identical consecutive `ValType`.
8. Keep unsupported IR forms as hard errors instead of widening the serializer.

## 9. First implementation checklist

The minimal direct-to-wasm bring-up can proceed in this order:

1. `emit_u8`, `emit_bytes`, `emit_u32_leb`, `emit_i32_leb`, `emit_i64_leb`
2. `emit_name`, `emit_vec`
3. `emit_val_type`, `emit_heap_type`, `emit_field_type`
4. `emit_type_def` / `emit_rec_group`
5. `emit_import_section`
6. `emit_function_section`
7. `emit_code_section`
8. `emit_export_section`
9. `emit_module`
10. validate under Node
11. add globals, tables, elems, start, data
12. switch the self-host loop from WAT bridging to direct `.wasm`
