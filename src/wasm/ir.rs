pub type TypeSym = String;
pub type FuncSym = String;
pub type GlobalSym = String;
pub type Label = String;

#[derive(Debug, Clone, PartialEq)]
pub enum HeapType {
    Named(TypeSym),
    Any,
    Eq,
    I31,
    Func,
    None,
    Extern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValType {
    I8, // packed storage type for array elements
    I32,
    I64,
    F32,
    F64,
    Anyref,
    I31ref,
    Funcref,
    Ref { nullable: bool, heap: HeapType },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: Option<String>,
    pub mutable: bool,
    pub ty: ValType,
}

impl FieldDef {
    pub fn new(ty: ValType) -> Self {
        FieldDef {
            name: None,
            mutable: false,
            ty,
        }
    }

    pub fn named(name: impl Into<String>, ty: ValType) -> Self {
        FieldDef {
            name: Some(name.into()),
            mutable: false,
            ty,
        }
    }

    pub fn mutable(mut self) -> Self {
        self.mutable = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDef {
    Struct {
        name: TypeSym,
        fields: Vec<FieldDef>,
        /// Optional supertype for Wasm GC struct subtyping (`sub $parent`).
        supertype: Option<TypeSym>,
        /// If true, declare as `(sub ...)` even without a supertype, making
        /// the type non-final so it can be subtyped.
        non_final: bool,
    },
    Array {
        name: TypeSym,
        elem: FieldDef,
    },
    FuncType {
        name: TypeSym,
        params: Vec<ValType>,
        results: Vec<ValType>,
    },
}

impl TypeDef {
    pub fn name(&self) -> &str {
        match self {
            TypeDef::Struct { name, .. } => name,
            TypeDef::Array { name, .. } => name,
            TypeDef::FuncType { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncSig {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    // Locals
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(GlobalSym),
    GlobalSet(GlobalSym),

    // Numeric constants
    I32Const(i32),
    I64Const(i64),
    F64Const(f64),

    // i32 arithmetic / comparison
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32RemS,
    I32And,
    I32Or,
    I32ShrU,
    I32Eq,
    I32Ne,
    I32LtS,
    I32GtS,
    I32LeS,
    I32GeS,
    I32LtU,
    I32GtU,
    I32LeU,
    I32GeU,
    I32Eqz,

    // i64 arithmetic / comparison
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64RemS,
    I64Eq,
    I64Ne,
    I64LtS,
    I64GtS,
    I64LeS,
    I64GeS,
    I64Eqz,

    // f64 arithmetic / comparison
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Neg,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,

    // Numeric conversions
    I64ExtendI32S,
    I64ExtendI32U,
    I32WrapI64,

    // Select (ternary: select picks one of two values based on i32 condition)
    Select,

    // Reference ops
    RefNull(HeapType),
    RefIsNull,
    RefAsNonNull,
    RefEq,
    RefI31,
    I31GetS,
    RefCast {
        nullable: bool,
        heap: HeapType,
    },
    RefTest {
        nullable: bool,
        heap: HeapType,
    },

    // Struct ops
    StructNew(TypeSym),
    StructGet(TypeSym, u32),
    StructGetS(TypeSym, u32),
    StructSet(TypeSym, u32),

    // Array ops
    ArrayNew(TypeSym),
    ArrayNewFixed(TypeSym, u32),
    ArrayNewData(TypeSym, u32),
    ArrayGet(TypeSym),
    ArrayGetU(TypeSym),
    ArraySet(TypeSym),
    ArrayLen,
    ArrayCopy(TypeSym, TypeSym),

    // Calls
    Call(FuncSym),
    CallIndirect {
        ty: TypeSym,
        table: u32,
    },
    /// `ref.func $sym` — produce a typed function reference without a table.
    RefFunc(FuncSym),
    /// `call_ref $type` — call via typed function reference (pops funcref from stack).
    CallRef(TypeSym),
    /// `return_call $sym` — tail call (direct).
    ReturnCall(FuncSym),
    /// `return_call_ref $type` — tail call via typed function reference.
    ReturnCallRef(TypeSym),

    // Control flow
    Drop,
    Return,
    Unreachable,
    Nop,
    If {
        result: Option<ValType>,
        then_body: Vec<Instr>,
        else_body: Vec<Instr>,
    },
    Block {
        label: Label,
        result: Option<ValType>,
        body: Vec<Instr>,
    },
    Loop {
        label: Label,
        result: Option<ValType>,
        body: Vec<Instr>,
    },
    Br(Label),
    BrIf(Label),
    BrTable {
        targets: Vec<Label>,
        default: Label,
    },
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: FuncSym,
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
    pub locals: Vec<ValType>,
    pub body: Vec<Instr>,
}

#[derive(Debug, Clone)]
pub struct ImportDef {
    pub module: String,
    pub name: String,
    pub as_sym: FuncSym,
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

#[derive(Debug, Clone)]
pub struct ExportDef {
    pub wasm_name: String,
    pub func_sym: FuncSym,
}

#[derive(Debug, Clone)]
pub struct GlobalDef {
    pub name: GlobalSym,
    pub mutable: bool,
    pub ty: ValType,
    pub init: Vec<Instr>,
}

#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,
    pub min: u32,
    pub max: Option<u32>,
    pub elem_ty: ValType,
}

#[derive(Debug, Clone)]
pub struct ElemDef {
    pub table: String,
    pub offset: Vec<Instr>,
    pub funcs: Vec<FuncSym>,
}

#[derive(Debug, Clone)]
pub struct DataSegment {
    pub name: String,
    pub offset: Vec<Instr>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ModuleIR {
    pub namespace: String,
    pub types: Vec<TypeDef>,
    pub imports: Vec<ImportDef>,
    pub funcs: Vec<FuncDef>,
    pub globals: Vec<GlobalDef>,
    pub tables: Vec<TableDef>,
    pub elems: Vec<ElemDef>,
    pub exports: Vec<ExportDef>,
    pub data: Vec<DataSegment>,
    pub start: Option<FuncSym>,
}

impl ModuleIR {
    pub fn new(namespace: impl Into<String>) -> Self {
        ModuleIR {
            namespace: namespace.into(),
            types: Vec::new(),
            imports: Vec::new(),
            funcs: Vec::new(),
            globals: Vec::new(),
            tables: Vec::new(),
            elems: Vec::new(),
            exports: Vec::new(),
            data: Vec::new(),
            start: None,
        }
    }
}
