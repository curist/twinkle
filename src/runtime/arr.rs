use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Build the `rt.arr` module: persistent (COW) array operations.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.arr");

    m.funcs.push(make_fn());
    m.funcs.push(get_fn());
    m.funcs.push(set_fn());
    m.funcs.push(len_fn());
    m.funcs.push(concat_fn());
    m.funcs.push(slice_fn());
    m.funcs.push(builder_new_fn());
    m.funcs.push(builder_from_fn());
    m.funcs.push(builder_push_fn());
    m.funcs.push(builder_freeze_fn());
    m.funcs.push(make_i64_fn());
    m.funcs.push(get_i64_fn());
    m.funcs.push(set_i64_fn());
    m.funcs.push(len_i64_fn());
    m.funcs.push(concat_i64_fn());
    m.funcs.push(slice_i64_fn());
    m.funcs.push(push_i64_fn());
    m.funcs.push(builder_from_i64_fn());
    m.funcs.push(builder_push_i64_fn());
    m.funcs.push(builder_freeze_i64_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

const BUILDER_INITIAL_CAPACITY: i32 = 8;

/// `make(len: i32, fill: anyref) -> Array`
fn make_fn() -> FuncDef {
    // array.new $Array: [anyref, i32] → [ref $Array]
    // Stack: push fill, push len, array.new
    FuncDef {
        name: "make".into(),
        params: vec![ValType::I32, ValType::Anyref], // p0=len, p1=fill
        results: vec![ref_array()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(1), // fill
            Instr::LocalGet(0), // len
            Instr::ArrayNew(T_ARRAY.into()),
        ],
    }
}

/// `make_i64(len: i32, fill: i64) -> Vector_i64`
fn make_i64_fn() -> FuncDef {
    FuncDef {
        name: "make_i64".into(),
        params: vec![ValType::I32, ValType::I64],
        results: vec![ref_vector_i64()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(1),
            Instr::LocalGet(0),
            Instr::ArrayNew(T_VECTOR_I64.into()),
        ],
    }
}

/// `get(arr: Array, i: i32) -> anyref`
fn get_fn() -> FuncDef {
    FuncDef {
        name: "get".into(),
        params: vec![ref_array_null(), ValType::I32], // p0=arr, p1=i
        results: vec![ValType::Anyref],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::ArrayGet(T_ARRAY.into()),
        ],
    }
}

/// `get_i64(arr: Vector_i64, i: i32) -> i64`
fn get_i64_fn() -> FuncDef {
    FuncDef {
        name: "get_i64".into(),
        params: vec![ref_vector_i64_null(), ValType::I32],
        results: vec![ValType::I64],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::ArrayGet(T_VECTOR_I64.into()),
        ],
    }
}

/// `set(arr: Array, i: i32, val: anyref) -> Array`  — COW: allocates a new array
fn set_fn() -> FuncDef {
    // Locals: p3 = new_arr (ref $Array), p4 = arr_len (i32)
    FuncDef {
        name: "set".into(),
        params: vec![ref_array_null(), ValType::I32, ValType::Anyref], // p0=arr, p1=i, p2=val
        results: vec![ref_array()],
        locals: vec![ref_array_null(), ValType::I32], // p3=new_arr, p4=len
        body: vec![
            // p4 = array.len(arr)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(4),
            // p3 = array.new $Array (fill=null, len=p4)
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(3),
            // array.copy dst=p3 dst_off=0 src=p0 src_off=0 len=p4
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(4),
            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            // array.set p3 p1 p2
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::ArraySet(T_ARRAY.into()),
            // return p3
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

/// `set_i64(arr: Vector_i64, i: i32, val: i64) -> Vector_i64`
fn set_i64_fn() -> FuncDef {
    FuncDef {
        name: "set_i64".into(),
        params: vec![ref_vector_i64_null(), ValType::I32, ValType::I64],
        results: vec![ref_vector_i64()],
        locals: vec![ref_vector_i64_null(), ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(4),
            Instr::I64Const(0),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_VECTOR_I64.into()),
            Instr::LocalSet(3),
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(4),
            Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::ArraySet(T_VECTOR_I64.into()),
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

/// `len(arr: Array) -> i32`
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_array_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::RefAsNonNull, Instr::ArrayLen],
    }
}

/// `len_i64(arr: Vector_i64) -> i32`
fn len_i64_fn() -> FuncDef {
    FuncDef {
        name: "len_i64".into(),
        params: vec![ref_vector_i64_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::RefAsNonNull, Instr::ArrayLen],
    }
}

/// `concat(a: Array, b: Array) -> Array`
fn concat_fn() -> FuncDef {
    // Locals: p2=len_a (i32), p3=len_b (i32), p4=total (i32), p5=result (ref $Array)
    FuncDef {
        name: "concat".into(),
        params: vec![ref_array_null(), ref_array_null()], // p0=a, p1=b
        results: vec![ref_array()],
        locals: vec![ValType::I32, ValType::I32, ValType::I32, ref_array_null()],
        body: vec![
            // p2 = len(a)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            // p3 = len(b)
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            // p4 = p2 + p3
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::I32Add,
            Instr::LocalSet(4),
            // p5 = array.new $Array (null, p4)
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(5),
            // copy a into result[0 .. len_a]
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(2),
            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            // copy b into result[len_a .. total]
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::LocalGet(2), // dst_offset = len_a
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            // return result
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
        ],
    }
}

/// `concat_i64(a: Vector_i64, b: Vector_i64) -> Vector_i64`
fn concat_i64_fn() -> FuncDef {
    FuncDef {
        name: "concat_i64".into(),
        params: vec![ref_vector_i64_null(), ref_vector_i64_null()],
        results: vec![ref_vector_i64()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ref_vector_i64_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::I32Add,
            Instr::LocalSet(4),
            Instr::I64Const(0),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_VECTOR_I64.into()),
            Instr::LocalSet(5),
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(2),
            Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
        ],
    }
}

/// `slice(arr: Array, start: i32, end: i32) -> Array`
fn slice_fn() -> FuncDef {
    // Locals: p3=new_len (i32), p4=result (ref $Array)
    FuncDef {
        name: "slice".into(),
        params: vec![ref_array_null(), ValType::I32, ValType::I32], // p0=arr, p1=start, p2=end
        results: vec![ref_array()],
        locals: vec![ValType::I32, ref_array_null()],
        body: vec![
            // p3 = end - start
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::I32Sub,
            Instr::LocalSet(3),
            // p4 = array.new $Array (null, p3)
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(3),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(4),
            // array.copy result 0 arr start new_len
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            // return result
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

/// `slice_i64(arr: Vector_i64, start: i32, end: i32) -> Vector_i64`
fn slice_i64_fn() -> FuncDef {
    FuncDef {
        name: "slice_i64".into(),
        params: vec![ref_vector_i64_null(), ValType::I32, ValType::I32],
        results: vec![ref_vector_i64()],
        locals: vec![ValType::I32, ref_vector_i64_null()],
        body: vec![
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::I32Sub,
            Instr::LocalSet(3),
            Instr::I64Const(0),
            Instr::LocalGet(3),
            Instr::ArrayNew(T_VECTOR_I64.into()),
            Instr::LocalSet(4),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

/// `push_i64(arr: Vector_i64, elem: i64) -> Vector_i64`
fn push_i64_fn() -> FuncDef {
    FuncDef {
        name: "push_i64".into(),
        params: vec![ref_vector_i64_null(), ValType::I64],
        results: vec![ref_vector_i64()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Const(1),
            Instr::LocalGet(1),
            Instr::Call("make_i64".into()),
            Instr::Call("concat_i64".into()),
        ],
    }
}

/// `builder_new() -> Array`
///
/// Builder layout (all anyref fields in a 3-slot rt_types__Array):
///   [0] = buf : nullable vector buffer payload
///   [1] = len : BoxedInt(i64)
///   [2] = cap : BoxedInt(i64)
fn builder_new_fn() -> FuncDef {
    FuncDef {
        name: "builder_new".into(),
        params: vec![],
        results: vec![ref_array()],
        locals: vec![],
        body: vec![
            Instr::RefNull(HeapType::None),
            Instr::I64Const(0),
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::I64Const(0),
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::ArrayNewFixed(T_ARRAY.into(), 3),
        ],
    }
}

/// `builder_from(vec: Array) -> Array`
///
/// Seed a builder from an existing immutable vector without copying.
/// Builder layout matches `builder_new`: [buf, len, cap].
fn builder_from_fn() -> FuncDef {
    FuncDef {
        name: "builder_from".into(),
        params: vec![ref_array_null()],
        results: vec![ref_array()],
        locals: vec![
            ValType::I32, // p1: len
            ValType::I32, // p2: cap
        ],
        body: vec![
            // len = array.len(vec)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(1),
            // cap = len
            // IMPORTANT: builder_from reuses the original fixed-size buffer.
            // Capacity must match that buffer's true length, otherwise
            // builder_push could attempt an out-of-bounds array.set.
            Instr::LocalGet(1),
            Instr::LocalSet(2),
            // [buf=vec, len=BoxedInt(len), cap=BoxedInt(cap)]
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::I64ExtendI32S,
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::LocalGet(2),
            Instr::I64ExtendI32S,
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::ArrayNewFixed(T_ARRAY.into(), 3),
        ],
    }
}

/// `builder_from_i64(vec: Vector_i64) -> Array`
fn builder_from_i64_fn() -> FuncDef {
    FuncDef {
        name: "builder_from_i64".into(),
        params: vec![ref_vector_i64_null()],
        results: vec![ref_array()],
        locals: vec![
            ValType::I32, // p1: len
            ValType::I32, // p2: cap
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(1),
            Instr::LocalGet(1),
            Instr::LocalSet(2),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::I64ExtendI32S,
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::LocalGet(2),
            Instr::I64ExtendI32S,
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::ArrayNewFixed(T_ARRAY.into(), 3),
        ],
    }
}

/// `builder_push(builder: Array, elem: anyref) -> void`
///
/// Amortized growth strategy: write in place when `len < cap`, otherwise
/// allocate `cap*2`, copy old elements once, then continue.
fn builder_push_fn() -> FuncDef {
    // Params:
    //   p0 = builder (ref null $Array)
    //   p1 = elem (anyref)
    //
    // Locals:
    //   p2 = buf      (ref null $Array)
    //   p3 = len_i32  (i32)
    //   p4 = cap_i32  (i32)
    //   p5 = new_cap  (i32)
    //   p6 = new_buf  (ref null $Array)
    let mut body = vec![
        // p2 = builder[0] as Array
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(0),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.into()),
        },
        Instr::LocalSet(2),
        // p3 = unbox(builder[1])
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(1),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: false,
            heap: HeapType::Named(T_BOXED_INT.into()),
        },
        Instr::StructGet(T_BOXED_INT.into(), 0),
        Instr::I32WrapI64,
        Instr::LocalSet(3),
        // p4 = unbox(builder[2])
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(2),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: false,
            heap: HeapType::Named(T_BOXED_INT.into()),
        },
        Instr::StructGet(T_BOXED_INT.into(), 0),
        Instr::I32WrapI64,
        Instr::LocalSet(4),
        // cond: len < cap
        Instr::LocalGet(3),
        Instr::LocalGet(4),
        Instr::I32LtS,
    ];

    let then_body = vec![
        // buf[len] = elem
        Instr::LocalGet(2),
        Instr::RefAsNonNull,
        Instr::LocalGet(3),
        Instr::LocalGet(1),
        Instr::ArraySet(T_ARRAY.into()),
        // len += 1
        Instr::LocalGet(3),
        Instr::I32Const(1),
        Instr::I32Add,
        Instr::LocalSet(3),
        // builder[1] = BoxedInt(len)
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(1),
        Instr::LocalGet(3),
        Instr::I64ExtendI32S,
        Instr::StructNew(T_BOXED_INT.into()),
        Instr::ArraySet(T_ARRAY.into()),
    ];

    let else_body = vec![
        // new_cap = if cap == 0 { 8 } else { cap * 2 }
        Instr::LocalGet(4),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ValType::I32),
            then_body: vec![Instr::I32Const(BUILDER_INITIAL_CAPACITY)],
            else_body: vec![Instr::LocalGet(4), Instr::I32Const(2), Instr::I32Mul],
        },
        Instr::LocalSet(5),
        // new_buf = array.new $Array (null, new_cap)
        Instr::RefNull(HeapType::None),
        Instr::LocalGet(5),
        Instr::ArrayNew(T_ARRAY.into()),
        Instr::LocalSet(6),
        // array.copy new_buf[0..len] <- buf[0..len] when builder already owns data
        Instr::LocalGet(3),
        Instr::I32Eqz,
        Instr::If {
            result: None,
            then_body: vec![],
            else_body: vec![
                Instr::LocalGet(6),
                Instr::RefAsNonNull,
                Instr::I32Const(0),
                Instr::LocalGet(2),
                Instr::RefAsNonNull,
                Instr::I32Const(0),
                Instr::LocalGet(3),
                Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            ],
        },
        // new_buf[len] = elem
        Instr::LocalGet(6),
        Instr::RefAsNonNull,
        Instr::LocalGet(3),
        Instr::LocalGet(1),
        Instr::ArraySet(T_ARRAY.into()),
        // len += 1
        Instr::LocalGet(3),
        Instr::I32Const(1),
        Instr::I32Add,
        Instr::LocalSet(3),
        // cap = new_cap
        Instr::LocalGet(5),
        Instr::LocalSet(4),
        // builder[0] = new_buf
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(0),
        Instr::LocalGet(6),
        Instr::ArraySet(T_ARRAY.into()),
        // builder[1] = BoxedInt(len)
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(1),
        Instr::LocalGet(3),
        Instr::I64ExtendI32S,
        Instr::StructNew(T_BOXED_INT.into()),
        Instr::ArraySet(T_ARRAY.into()),
        // builder[2] = BoxedInt(cap)
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(2),
        Instr::LocalGet(4),
        Instr::I64ExtendI32S,
        Instr::StructNew(T_BOXED_INT.into()),
        Instr::ArraySet(T_ARRAY.into()),
    ];

    body.push(Instr::If {
        result: None,
        then_body,
        else_body,
    });

    FuncDef {
        name: "builder_push".into(),
        params: vec![ref_array_null(), ValType::Anyref],
        results: vec![],
        locals: vec![
            ref_array_null(), // p2: buf
            ValType::I32,     // p3: len
            ValType::I32,     // p4: cap
            ValType::I32,     // p5: new_cap
            ref_array_null(), // p6: new_buf
        ],
        body,
    }
}

/// `builder_push_i64(builder: Array, elem: i64) -> void`
fn builder_push_i64_fn() -> FuncDef {
    let body = vec![
        // p2 = builder[0] as Vector_i64?
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(0),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_VECTOR_I64.into()),
        },
        Instr::LocalSet(2),
        // p3 = unbox(builder[1])
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(1),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: false,
            heap: HeapType::Named(T_BOXED_INT.into()),
        },
        Instr::StructGet(T_BOXED_INT.into(), 0),
        Instr::I32WrapI64,
        Instr::LocalSet(3),
        // p4 = unbox(builder[2])
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(2),
        Instr::ArrayGet(T_ARRAY.into()),
        Instr::RefCast {
            nullable: false,
            heap: HeapType::Named(T_BOXED_INT.into()),
        },
        Instr::StructGet(T_BOXED_INT.into(), 0),
        Instr::I32WrapI64,
        Instr::LocalSet(4),
        // cond: len < cap
        Instr::LocalGet(3),
        Instr::LocalGet(4),
        Instr::I32LtS,
        Instr::If {
            result: None,
            then_body: vec![
                Instr::LocalGet(2),
                Instr::RefAsNonNull,
                Instr::LocalGet(3),
                Instr::LocalGet(1),
                Instr::ArraySet(T_VECTOR_I64.into()),
                Instr::LocalGet(3),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::LocalSet(3),
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::I32Const(1),
                Instr::LocalGet(3),
                Instr::I64ExtendI32S,
                Instr::StructNew(T_BOXED_INT.into()),
                Instr::ArraySet(T_ARRAY.into()),
            ],
            else_body: vec![
                Instr::LocalGet(4),
                Instr::I32Eqz,
                Instr::If {
                    result: Some(ValType::I32),
                    then_body: vec![Instr::I32Const(BUILDER_INITIAL_CAPACITY)],
                    else_body: vec![Instr::LocalGet(4), Instr::I32Const(2), Instr::I32Mul],
                },
                Instr::LocalSet(5),
                Instr::I64Const(0),
                Instr::LocalGet(5),
                Instr::ArrayNew(T_VECTOR_I64.into()),
                Instr::LocalSet(6),
                Instr::LocalGet(3),
                Instr::I32Eqz,
                Instr::If {
                    result: None,
                    then_body: vec![],
                    else_body: vec![
                        Instr::LocalGet(6),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(3),
                        Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
                    ],
                },
                Instr::LocalGet(6),
                Instr::RefAsNonNull,
                Instr::LocalGet(3),
                Instr::LocalGet(1),
                Instr::ArraySet(T_VECTOR_I64.into()),
                Instr::LocalGet(3),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::LocalSet(3),
                Instr::LocalGet(5),
                Instr::LocalSet(4),
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::I32Const(0),
                Instr::LocalGet(6),
                Instr::ArraySet(T_ARRAY.into()),
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::I32Const(1),
                Instr::LocalGet(3),
                Instr::I64ExtendI32S,
                Instr::StructNew(T_BOXED_INT.into()),
                Instr::ArraySet(T_ARRAY.into()),
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::I32Const(2),
                Instr::LocalGet(4),
                Instr::I64ExtendI32S,
                Instr::StructNew(T_BOXED_INT.into()),
                Instr::ArraySet(T_ARRAY.into()),
            ],
        },
    ];
    FuncDef {
        name: "builder_push_i64".into(),
        params: vec![ref_array_null(), ValType::I64],
        results: vec![],
        locals: vec![
            ref_vector_i64_null(), // p2: buf
            ValType::I32,          // p3: len
            ValType::I32,          // p4: cap
            ValType::I32,          // p5: new_cap
            ref_vector_i64_null(), // p6: new_buf
        ],
        body,
    }
}

/// `builder_freeze(builder: Array) -> Array`
///
/// Return an exactly-sized immutable snapshot of the first `len` elements.
fn builder_freeze_fn() -> FuncDef {
    FuncDef {
        name: "builder_freeze".into(),
        params: vec![ref_array_null()],
        results: vec![ref_array()],
        locals: vec![
            ref_array_null(), // p1: buf
            ValType::I32,     // p2: len
            ref_array_null(), // p3: out
        ],
        body: vec![
            // p1 = builder[0] as Array
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.into()),
            },
            Instr::LocalSet(1),
            // p2 = unbox(builder[1])
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(1),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::StructGet(T_BOXED_INT.into(), 0),
            Instr::I32WrapI64,
            Instr::LocalSet(2),
            // p3 = array.new $Array (null, len)
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(2),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(3),
            // out[0..len] <- buf[0..len] when the builder was seeded or pushed into
            Instr::LocalGet(2),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![],
                else_body: vec![
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(2),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
            },
            // return out
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

/// `builder_freeze_i64(builder: Array) -> Vector_i64`
fn builder_freeze_i64_fn() -> FuncDef {
    FuncDef {
        name: "builder_freeze_i64".into(),
        params: vec![ref_array_null()],
        results: vec![ref_vector_i64()],
        locals: vec![
            ref_vector_i64_null(), // p1: buf
            ValType::I32,          // p2: len
            ref_vector_i64_null(), // p3: out
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_VECTOR_I64.into()),
            },
            Instr::LocalSet(1),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(1),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::StructGet(T_BOXED_INT.into(), 0),
            Instr::I32WrapI64,
            Instr::LocalSet(2),
            Instr::I64Const(0),
            Instr::LocalGet(2),
            Instr::ArrayNew(T_VECTOR_I64.into()),
            Instr::LocalSet(3),
            Instr::LocalGet(2),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![],
                else_body: vec![
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(2),
                    Instr::ArrayCopy(T_VECTOR_I64.into(), T_VECTOR_I64.into()),
                ],
            },
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}
