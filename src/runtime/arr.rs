use crate::wasm::ir::*;
use crate::runtime::types::*;

/// Build the `rt.arr` module: persistent (COW) array operations.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.arr");

    m.funcs.push(make_fn());
    m.funcs.push(get_fn());
    m.funcs.push(set_fn());
    m.funcs.push(len_fn());
    m.funcs.push(concat_fn());
    m.funcs.push(slice_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

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

/// `len(arr: Array) -> i32`
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_array_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
        ],
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
