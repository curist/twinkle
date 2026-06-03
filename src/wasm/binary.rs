use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use wasm_encoder as we;
use wasm_encoder::Encode;

use crate::wasm::ir::{
    FieldDef, FuncDef, HeapType, Instr, TypeDef, ValType,
};
use crate::wasm::linker::LinkedModuleIR;

#[derive(Debug, Clone)]
struct Ctx {
    type_idx: HashMap<String, u32>,
    func_idx: HashMap<String, u32>,
    global_idx: HashMap<String, u32>,
    table_idx: HashMap<String, u32>,
    anon_ft_idx: HashMap<String, u32>,
}

pub fn emit_wasm(module: &LinkedModuleIR) -> Vec<u8> {
    let ordered_sccs = compute_type_order(&module.types);
    let ctx = build_ctx(module, &ordered_sccs);

    let mut out = we::Module::new();

    let types = encode_type_section(module, &ordered_sccs, &ctx);
    if !types.is_empty() {
        out.section(&types);
    }

    if !module.imports.is_empty() {
        let mut imports = we::ImportSection::new();
        for imp in &module.imports {
            imports.import(
                &imp.module,
                &imp.name,
                we::EntityType::Function(anon_ft_idx(&ctx, &imp.params, &imp.results)),
            );
        }
        out.section(&imports);
    }

    if !module.funcs.is_empty() {
        let mut funcs = we::FunctionSection::new();
        for f in &module.funcs {
            funcs.function(anon_ft_idx(&ctx, &f.params, &f.results));
        }
        out.section(&funcs);
    }

    if !module.tables.is_empty() {
        let mut tables = we::TableSection::new();
        for t in &module.tables {
            let we::ValType::Ref(element_type) = enc_val_type(&t.elem_ty, &ctx) else {
                panic!("wasm: table element type must be a reference type")
            };
            tables.table(we::TableType {
                element_type,
                table64: false,
                minimum: t.min as u64,
                maximum: t.max.map(u64::from),
                shared: false,
            });
        }
        out.section(&tables);
    }

    if !module.globals.is_empty() {
        let mut globals = we::GlobalSection::new();
        for g in &module.globals {
            let raw = encode_instrs(&g.init, &ctx, &mut Vec::new());
            globals.global(
                we::GlobalType { val_type: enc_val_type(&g.ty, &ctx), mutable: g.mutable, shared: false },
                &we::ConstExpr::raw(raw),
            );
        }
        out.section(&globals);
    }

    if !module.exports.is_empty() {
        let mut exports = we::ExportSection::new();
        for e in &module.exports {
            exports.export(&e.wasm_name, we::ExportKind::Func, func_idx(&ctx, &e.func_sym));
        }
        out.section(&exports);
    }

    if let Some(start) = &module.start {
        out.section(&we::StartSection { function_index: func_idx(&ctx, start) });
    }

    let declared_ref_funcs = collect_declared_ref_funcs(module, &ctx);
    if !module.elems.is_empty() || !declared_ref_funcs.is_empty() {
        let mut elems = we::ElementSection::new();
        for e in &module.elems {
            let off = encode_instrs(&e.offset, &ctx, &mut Vec::new());
            let funcs: Vec<u32> = e.funcs.iter().map(|f| func_idx(&ctx, f)).collect();
            elems.segment(we::ElementSegment {
                mode: we::ElementMode::Active { table: Some(table_idx(&ctx, &e.table)), offset: &we::ConstExpr::raw(off) },
                elements: we::Elements::Functions(Cow::Owned(funcs)),
            });
        }
        if !declared_ref_funcs.is_empty() {
            elems.segment(we::ElementSegment {
                mode: we::ElementMode::Declared,
                elements: we::Elements::Functions(Cow::Owned(declared_ref_funcs)),
            });
        }
        out.section(&elems);
    }

    if !module.code_placeholder().is_empty() {}

    if !module.funcs.is_empty() {
        let mut code = we::CodeSection::new();
        for f in &module.funcs {
            code.function(&encode_func(f, &ctx));
        }
        out.section(&code);
    }

    if !module.data.is_empty() {
        let mut data = we::DataSection::new();
        for d in &module.data {
            let mode = if d.offset.is_empty() {
                we::DataSegmentMode::Passive
            } else {
                let off = encode_instrs(&d.offset, &ctx, &mut Vec::new());
                we::DataSegmentMode::Active { memory_index: 0, offset: &we::ConstExpr::raw(off) }
            };
            data.segment(we::DataSegment { mode, data: d.bytes.clone() });
        }
        out.section(&data);
    }

    out.finish()
}

trait NoopModuleHack { fn code_placeholder(&self) -> &[()]; }
impl NoopModuleHack for LinkedModuleIR { fn code_placeholder(&self) -> &[()] { &[] } }

fn encode_func(f: &FuncDef, ctx: &Ctx) -> we::Function {
    let locals = f.locals.iter().map(|t| enc_val_type(t, ctx));
    let mut func = we::Function::new_with_locals_types(locals);
    let bytes = encode_instrs(&f.body, ctx, &mut Vec::new());
    func.raw(bytes);
    func.instruction(&we::Instruction::End);
    func
}

fn encode_instrs(instrs: &[Instr], ctx: &Ctx, labels: &mut Vec<String>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for instr in instrs {
        encode_instr(instr, ctx, labels, &mut bytes);
    }
    bytes
}

fn encode_instr(instr: &Instr, ctx: &Ctx, labels: &mut Vec<String>, bytes: &mut Vec<u8>) {
    use Instr::*;
    let i = match instr {
        LocalGet(x) => we::Instruction::LocalGet(*x),
        LocalSet(x) => we::Instruction::LocalSet(*x),
        LocalTee(x) => we::Instruction::LocalTee(*x),
        GlobalGet(s) => we::Instruction::GlobalGet(global_idx(ctx, s)),
        GlobalSet(s) => we::Instruction::GlobalSet(global_idx(ctx, s)),
        I32Const(x) => we::Instruction::I32Const(*x),
        I64Const(x) => we::Instruction::I64Const(*x),
        F64Const(x) => we::Instruction::F64Const((*x).into()),
        I32Add => we::Instruction::I32Add, I32Sub => we::Instruction::I32Sub, I32Mul => we::Instruction::I32Mul,
        I32DivS => we::Instruction::I32DivS, I32RemS => we::Instruction::I32RemS,
        I32And => we::Instruction::I32And, I32Or => we::Instruction::I32Or, I32Xor => we::Instruction::I32Xor,
        I32Shl => we::Instruction::I32Shl, I32ShrU => we::Instruction::I32ShrU, I32ShrS => we::Instruction::I32ShrS,
        I32Eq => we::Instruction::I32Eq, I32Ne => we::Instruction::I32Ne, I32LtS => we::Instruction::I32LtS,
        I32GtS => we::Instruction::I32GtS, I32LeS => we::Instruction::I32LeS, I32GeS => we::Instruction::I32GeS,
        I32LtU => we::Instruction::I32LtU, I32GtU => we::Instruction::I32GtU, I32LeU => we::Instruction::I32LeU, I32GeU => we::Instruction::I32GeU,
        I32Eqz => we::Instruction::I32Eqz,
        I64Add => we::Instruction::I64Add, I64Sub => we::Instruction::I64Sub, I64Mul => we::Instruction::I64Mul,
        I64DivS => we::Instruction::I64DivS, I64RemS => we::Instruction::I64RemS,
        I64And => we::Instruction::I64And, I64Or => we::Instruction::I64Or, I64Xor => we::Instruction::I64Xor,
        I64Shl => we::Instruction::I64Shl, I64ShrS => we::Instruction::I64ShrS, I64ShrU => we::Instruction::I64ShrU,
        I64Eq => we::Instruction::I64Eq, I64Ne => we::Instruction::I64Ne, I64LtS => we::Instruction::I64LtS,
        I64GtS => we::Instruction::I64GtS, I64LeS => we::Instruction::I64LeS, I64GeS => we::Instruction::I64GeS,
        I64Eqz => we::Instruction::I64Eqz,
        F64Add => we::Instruction::F64Add, F64Sub => we::Instruction::F64Sub, F64Mul => we::Instruction::F64Mul, F64Div => we::Instruction::F64Div,
        F64Neg => we::Instruction::F64Neg, F64Eq => we::Instruction::F64Eq, F64Ne => we::Instruction::F64Ne,
        F64Lt => we::Instruction::F64Lt, F64Gt => we::Instruction::F64Gt, F64Le => we::Instruction::F64Le, F64Ge => we::Instruction::F64Ge,
        I64ExtendI32S => we::Instruction::I64ExtendI32S, I64ExtendI32U => we::Instruction::I64ExtendI32U,
        I32WrapI64 => we::Instruction::I32WrapI64, I64ReinterpretF64 => we::Instruction::I64ReinterpretF64,
        Select => we::Instruction::Select,
        RefNull(h) => we::Instruction::RefNull(enc_heap_type(h, ctx)),
        RefIsNull => we::Instruction::RefIsNull, RefAsNonNull => we::Instruction::RefAsNonNull, RefEq => we::Instruction::RefEq,
        RefI31 => we::Instruction::RefI31, I31GetS => we::Instruction::I31GetS, I31GetU => we::Instruction::I31GetU,
        RefCast { nullable, heap } => if *nullable { we::Instruction::RefCastNullable(enc_heap_type(heap, ctx)) } else { we::Instruction::RefCastNonNull(enc_heap_type(heap, ctx)) },
        AnyConvertExtern => we::Instruction::AnyConvertExtern, ExternConvertAny => we::Instruction::ExternConvertAny,
        RefTest { nullable, heap } => if *nullable { we::Instruction::RefTestNullable(enc_heap_type(heap, ctx)) } else { we::Instruction::RefTestNonNull(enc_heap_type(heap, ctx)) },
        StructNew(t) => we::Instruction::StructNew(type_idx(ctx, t)),
        StructGet(t, f) => we::Instruction::StructGet { struct_type_index: type_idx(ctx, t), field_index: *f },
        StructGetS(t, f) => we::Instruction::StructGetS { struct_type_index: type_idx(ctx, t), field_index: *f },
        StructSet(t, f) => we::Instruction::StructSet { struct_type_index: type_idx(ctx, t), field_index: *f },
        ArrayNew(t) => we::Instruction::ArrayNew(type_idx(ctx, t)),
        ArrayNewDefault(t) => we::Instruction::ArrayNewDefault(type_idx(ctx, t)),
        ArrayNewFixed(t, n) => we::Instruction::ArrayNewFixed { array_type_index: type_idx(ctx, t), array_size: *n },
        ArrayNewData(t, d) => we::Instruction::ArrayNewData { array_type_index: type_idx(ctx, t), array_data_index: *d },
        ArrayGet(t) => we::Instruction::ArrayGet(type_idx(ctx, t)),
        ArrayGetU(t) => we::Instruction::ArrayGetU(type_idx(ctx, t)),
        ArraySet(t) => we::Instruction::ArraySet(type_idx(ctx, t)),
        ArrayLen => we::Instruction::ArrayLen,
        ArrayCopy(d, s) => we::Instruction::ArrayCopy { array_type_index_dst: type_idx(ctx, d), array_type_index_src: type_idx(ctx, s) },
        Call(s) => we::Instruction::Call(func_idx(ctx, s)),
        CallIndirect { ty, table } => we::Instruction::CallIndirect { type_index: type_idx(ctx, ty), table_index: *table },
        RefFunc(s) => we::Instruction::RefFunc(func_idx(ctx, s)),
        CallRef(t) => we::Instruction::CallRef(type_idx(ctx, t)),
        ReturnCall(s) => we::Instruction::ReturnCall(func_idx(ctx, s)),
        ReturnCallRef(t) => we::Instruction::ReturnCallRef(type_idx(ctx, t)),
        Drop => we::Instruction::Drop, Return => we::Instruction::Return, Unreachable => we::Instruction::Unreachable, Nop => we::Instruction::Nop,
        Br(l) => we::Instruction::Br(label_depth(labels, l)),
        BrIf(l) => we::Instruction::BrIf(label_depth(labels, l)),
        BrTable { targets, default } => {
            let ts: Vec<u32> = targets.iter().map(|l| label_depth(labels, l)).collect();
            we::Instruction::BrTable(Cow::Owned(ts), label_depth(labels, default))
        }
        If { result, then_body, else_body } => {
            we::Instruction::If(block_type(result, ctx)).encode(bytes);
            labels.push(String::new());
            bytes.extend(encode_instrs(then_body, ctx, labels));
            we::Instruction::Else.encode(bytes);
            bytes.extend(encode_instrs(else_body, ctx, labels));
            labels.pop();
            we::Instruction::End.encode(bytes);
            return;
        }
        Block { label, result, body } => {
            we::Instruction::Block(block_type(result, ctx)).encode(bytes);
            labels.push(label.clone());
            bytes.extend(encode_instrs(body, ctx, labels));
            labels.pop();
            we::Instruction::End.encode(bytes);
            return;
        }
        Loop { label, result, body } => {
            we::Instruction::Loop(block_type(result, ctx)).encode(bytes);
            labels.push(label.clone());
            bytes.extend(encode_instrs(body, ctx, labels));
            labels.pop();
            we::Instruction::End.encode(bytes);
            return;
        }
    };
    i.encode(bytes);
}

fn block_type(result: &Option<ValType>, ctx: &Ctx) -> we::BlockType {
    match result { Some(t) => we::BlockType::Result(enc_val_type(t, ctx)), None => we::BlockType::Empty }
}

fn label_depth(labels: &[String], label: &str) -> u32 {
    labels.iter().rposition(|l| l == label).map(|pos| (labels.len() - 1 - pos) as u32).unwrap_or_else(|| panic!("wasm: unknown label {label}"))
}

fn encode_type_section(module: &LinkedModuleIR, ordered_sccs: &[Vec<String>], ctx: &Ctx) -> we::TypeSection {
    let mut section = we::TypeSection::new();
    let by_name: HashMap<_, _> = module.types.iter().map(|t| (type_name(t).to_string(), t)).collect();
    for scc in ordered_sccs {
        let needs_rec = scc.len() > 1 || type_has_self_edge(by_name[scc[0].as_str()]);
        if needs_rec {
            let subtypes: Vec<_> = scc.iter().map(|n| subtype_for(by_name[n.as_str()], ctx)).collect();
            section.ty().rec(subtypes);
        } else {
            section.ty().subtype(&subtype_for(by_name[scc[0].as_str()], ctx));
        }
    }
    for (params, results) in anon_func_types(module) {
        section.ty().function(params.iter().map(|t| enc_val_type(t, ctx)), results.iter().map(|t| enc_val_type(t, ctx)));
    }
    section
}

fn subtype_for(td: &TypeDef, ctx: &Ctx) -> we::SubType {
    let (is_final, supertype_idx, inner) = match td {
        TypeDef::Struct { fields, supertype, non_final, .. } => (
            !*non_final,
            supertype.as_ref().map(|s| type_idx(ctx, s)),
            we::CompositeInnerType::Struct(we::StructType { fields: fields.iter().map(|f| field_type(f, ctx)).collect() }),
        ),
        TypeDef::Array { elem, .. } => (true, None, we::CompositeInnerType::Array(we::ArrayType(field_type(elem, ctx)))),
        TypeDef::FuncType { params, results, .. } => (
            true,
            None,
            we::CompositeInnerType::Func(we::FuncType::new(params.iter().map(|t| enc_val_type(t, ctx)), results.iter().map(|t| enc_val_type(t, ctx)))),
        ),
    };
    we::SubType { is_final, supertype_idx, composite_type: we::CompositeType { inner, shared: false, descriptor: None, describes: None } }
}

fn field_type(f: &FieldDef, ctx: &Ctx) -> we::FieldType {
    we::FieldType { element_type: storage_type(&f.ty, ctx), mutable: f.mutable }
}
fn storage_type(t: &ValType, ctx: &Ctx) -> we::StorageType {
    match t { ValType::I8 => we::StorageType::I8, _ => we::StorageType::Val(enc_val_type(t, ctx)) }
}
fn enc_val_type(t: &ValType, ctx: &Ctx) -> we::ValType {
    match t {
        ValType::I8 => panic!("wasm: i8 is not a value type"),
        ValType::I32 => we::ValType::I32,
        ValType::I64 => we::ValType::I64,
        ValType::F32 => we::ValType::F32,
        ValType::F64 => we::ValType::F64,
        ValType::Anyref => we::ValType::Ref(we::RefType::ANYREF),
        ValType::I31ref => we::ValType::Ref(we::RefType::I31REF),
        ValType::Funcref => we::ValType::Ref(we::RefType::FUNCREF),
        ValType::Ref { nullable, heap } => we::ValType::Ref(we::RefType { nullable: *nullable, heap_type: enc_heap_type(heap, ctx) }),
    }
}
fn enc_heap_type(h: &HeapType, ctx: &Ctx) -> we::HeapType {
    match h {
        HeapType::Named(n) => we::HeapType::Concrete(type_idx(ctx, n)),
        HeapType::Any => we::HeapType::ANY,
        HeapType::Eq => we::HeapType::Abstract { shared: false, ty: we::AbstractHeapType::Eq },
        HeapType::I31 => we::HeapType::I31,
        HeapType::Func => we::HeapType::FUNC,
        HeapType::None => we::HeapType::Abstract { shared: false, ty: we::AbstractHeapType::None },
        HeapType::Extern => we::HeapType::EXTERN,
    }
}

fn build_ctx(module: &LinkedModuleIR, ordered_sccs: &[Vec<String>]) -> Ctx {
    let mut type_idx = HashMap::new();
    let mut next = 0u32;
    for scc in ordered_sccs { for n in scc { type_idx.insert(n.clone(), next); next += 1; } }
    let anon_base = next;
    let mut anon_ft_idx = HashMap::new();
    let mut i = 0u32;
    for (p, r) in anon_func_types(module) {
        anon_ft_idx.entry(ft_key(&p, &r)).or_insert_with(|| { let v = anon_base + i; i += 1; v });
    }
    let mut func_idx = HashMap::new();
    let mut fi = 0u32;
    for imp in &module.imports { func_idx.insert(imp.as_sym.clone(), fi); fi += 1; }
    for f in &module.funcs { func_idx.insert(f.name.clone(), fi); fi += 1; }
    Ctx {
        type_idx,
        anon_ft_idx,
        func_idx,
        global_idx: module.globals.iter().enumerate().map(|(i, g)| (g.name.clone(), i as u32)).collect(),
        table_idx: module.tables.iter().enumerate().map(|(i, t)| (t.name.clone(), i as u32)).collect(),
    }
}

fn anon_func_types(module: &LinkedModuleIR) -> Vec<(Vec<ValType>, Vec<ValType>)> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (p, r) in module.imports.iter().map(|i| (&i.params, &i.results)).chain(module.funcs.iter().map(|f| (&f.params, &f.results))) {
        let k = ft_key(p, r);
        if seen.insert(k) { out.push((p.clone(), r.clone())); }
    }
    out
}
fn ft_key(p: &[ValType], r: &[ValType]) -> String { format!("{:?}->{:?}", p, r) }
fn type_idx(ctx: &Ctx, n: &str) -> u32 { *ctx.type_idx.get(n).unwrap_or_else(|| panic!("wasm: unknown type {n}")) }
fn func_idx(ctx: &Ctx, n: &str) -> u32 { *ctx.func_idx.get(n).unwrap_or_else(|| panic!("wasm: unknown func {n}")) }
fn global_idx(ctx: &Ctx, n: &str) -> u32 { *ctx.global_idx.get(n).unwrap_or_else(|| panic!("wasm: unknown global {n}")) }
fn table_idx(ctx: &Ctx, n: &str) -> u32 { *ctx.table_idx.get(n).unwrap_or_else(|| panic!("wasm: unknown table {n}")) }
fn anon_ft_idx(ctx: &Ctx, p: &[ValType], r: &[ValType]) -> u32 { *ctx.anon_ft_idx.get(&ft_key(p, r)).unwrap() }
fn type_name(td: &TypeDef) -> &str { match td { TypeDef::Struct { name, .. } | TypeDef::Array { name, .. } | TypeDef::FuncType { name, .. } => name } }

fn type_deps(td: &TypeDef) -> Vec<String> {
    let mut out = Vec::new();
    match td {
        TypeDef::Struct { fields, supertype, .. } => { if let Some(s) = supertype { out.push(s.clone()); } for f in fields { val_deps(&f.ty, &mut out); } }
        TypeDef::Array { elem, .. } => val_deps(&elem.ty, &mut out),
        TypeDef::FuncType { params, results, .. } => { for t in params { val_deps(t, &mut out); } for t in results { val_deps(t, &mut out); } }
    }
    out.sort(); out.dedup(); out
}
fn val_deps(t: &ValType, out: &mut Vec<String>) { if let ValType::Ref { heap: HeapType::Named(n), .. } = t { out.push(n.clone()); } }
fn type_has_self_edge(td: &TypeDef) -> bool { type_deps(td).iter().any(|d| d == type_name(td)) }

fn compute_type_order(types: &[TypeDef]) -> Vec<Vec<String>> {
    let names: Vec<String> = types.iter().map(|t| type_name(t).to_string()).collect();
    let by_name: HashMap<String, &TypeDef> = types.iter().map(|t| (type_name(t).to_string(), t)).collect();
    let name_set: BTreeSet<String> = names.iter().cloned().collect();
    let mut index = 0usize; let mut indices = HashMap::new(); let mut low = HashMap::new(); let mut stack = Vec::new(); let mut on = BTreeSet::new(); let mut comps = Vec::new();
    struct Tarjan<'a> { by_name: &'a HashMap<String, &'a TypeDef>, name_set: &'a BTreeSet<String>, index: &'a mut usize, indices: &'a mut HashMap<String, usize>, low: &'a mut HashMap<String, usize>, stack: &'a mut Vec<String>, on: &'a mut BTreeSet<String>, comps: &'a mut Vec<Vec<String>> }
    impl<'a> Tarjan<'a> { fn visit(&mut self, n: String) { self.indices.insert(n.clone(), *self.index); self.low.insert(n.clone(), *self.index); *self.index += 1; self.stack.push(n.clone()); self.on.insert(n.clone()); for dep in type_deps(self.by_name[&n]) { if !self.name_set.contains(&dep) { continue; } if !self.indices.contains_key(&dep) { self.visit(dep.clone()); let v = self.low[&n].min(self.low[&dep]); self.low.insert(n.clone(), v); } else if self.on.contains(&dep) { let v = self.low[&n].min(self.indices[&dep]); self.low.insert(n.clone(), v); } } if self.low[&n] == self.indices[&n] { let mut c = Vec::new(); loop { let x = self.stack.pop().unwrap(); self.on.remove(&x); c.push(x.clone()); if x == n { break; } } c.reverse(); self.comps.push(c); } } }
    { let mut t = Tarjan { by_name: &by_name, name_set: &name_set, index: &mut index, indices: &mut indices, low: &mut low, stack: &mut stack, on: &mut on, comps: &mut comps }; for n in names { if !t.indices.contains_key(&n) { t.visit(n); } } }
    let mut comp_id = HashMap::new(); let mut comp_members = BTreeMap::new(); let mut comp_ids = Vec::new();
    for c in comps { let id = c.join("|"); for m in &c { comp_id.insert(m.clone(), id.clone()); } comp_ids.push(id.clone()); comp_members.insert(id, c); }
    let mut comp_deps: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (id, members) in &comp_members { let mut deps = BTreeSet::new(); for m in members { for d in type_deps(by_name[m]) { if name_set.contains(&d) { let did = comp_id[&d].clone(); if did != *id { deps.insert(did); } } } } comp_deps.insert(id.clone(), deps); }
    let mut emitted = BTreeSet::new(); let mut ordered_ids = Vec::new();
    while emitted.len() < comp_ids.len() { let mut progress = false; for id in &comp_ids { if emitted.contains(id) { continue; } if comp_deps[id].iter().all(|d| emitted.contains(d)) { emitted.insert(id.clone()); ordered_ids.push(id.clone()); progress = true; } } if !progress { panic!("type order cycle failure"); } }
    ordered_ids.into_iter().map(|id| comp_members.remove(&id).unwrap()).collect()
}

fn collect_declared_ref_funcs(module: &LinkedModuleIR, ctx: &Ctx) -> Vec<u32> {
    let mut in_elems = BTreeSet::new();
    for e in &module.elems { for f in &e.funcs { in_elems.insert(f.clone()); } }
    let mut seen = BTreeSet::new();
    for f in &module.funcs { collect_ref_func_instrs(&f.body, &mut seen); }
    seen.into_iter().filter(|s| !in_elems.contains(s)).map(|s| func_idx(ctx, &s)).collect()
}
fn collect_ref_func_instrs(instrs: &[Instr], seen: &mut BTreeSet<String>) { for i in instrs { match i { Instr::RefFunc(s) => { seen.insert(s.clone()); } Instr::If { then_body, else_body, .. } => { collect_ref_func_instrs(then_body, seen); collect_ref_func_instrs(else_body, seen); } Instr::Block { body, .. } | Instr::Loop { body, .. } => collect_ref_func_instrs(body, seen), _ => {} } } }
