/// ANF (Administrative Normal Form) IR for the Twinkle compiler.
///
/// ANF makes evaluation order explicit by ensuring every intermediate
/// computation is bound to a named local. This simplifies code generation
/// for stack-machine and SSA-like backends (e.g., WAT/Wasm).
///
/// The ANF pipeline sits between Core IR and the WAT/Wasm backend:
///   Core IR → ANF IR → WAT/Wasm backend
///
/// The interpreter continues to use Core IR directly; ANF is backend-only.
use std::fmt;

use crate::ir::core::{CorePattern, FieldId, FuncId, LocalId, VariantId};
use crate::syntax::ast::{BinOp, UnOp};
use crate::types::ty::{MonoType, TypeId};

/// The operand type for binary and unary operations.
///
/// The WAT emitter uses this to select the correct Wasm instruction family
/// (e.g. `i64.add` vs `f64.add` vs `i32.and` vs `call $str_eq`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpKind {
    Int,
    Float,
    Bool,
    String,
}

/// Distinguishes array indexing from dict indexing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndexKind {
    Array,
    Dict,
}

/// An atom — a trivially available value that requires no computation.
///
/// Every non-trivial subexpression must be let-bound before use.
/// This is the key ANF invariant: all operation operands are atoms.
#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    /// A local variable reference
    ALocal(LocalId),
    /// A reference to a global function (atomic; produces a closure at runtime)
    AGlobalFunc(FuncId),
    /// Integer literal
    ALitInt(i64),
    /// Float literal
    ALitFloat(f64),
    /// Bool literal
    ALitBool(bool),
    /// String literal
    ALitStr(String),
    /// Void literal
    ALitVoid,
}

/// ANF expression — a flat let-chain terminating in an atom or a terminal.
///
/// Every `AnfExpr` either:
///   - Binds a computation to a local and continues (`Let`)
///   - Terminates with a value (`Atom`)
///   - Terminates with control flow (`Return`, `Break`, `Continue`)
#[derive(Debug, Clone, PartialEq)]
pub enum AnfExpr {
    /// Bind the result of `op` to `local`, then evaluate `body`.
    Let {
        local: LocalId,
        op: Box<AnfOp>,
        body: Box<AnfExpr>,
    },
    /// Function return (terminal — no continuation).
    Return(Option<Atom>),
    /// Loop break with an optional value (terminal — no continuation).
    Break(Option<Atom>),
    /// Loop continue (terminal — no continuation).
    Continue,
    /// Terminal atom — the final value of this expression sequence.
    Atom(Atom),
}

/// A single non-atomic computation whose result is bound by an enclosing `Let`.
///
/// Every `AnfOp` produces exactly one value (bound by `AnfExpr::Let`).
#[derive(Debug, Clone, PartialEq)]
pub enum AnfOp {
    /// Function call: callee and all args are atoms.
    ACall { callee: Atom, args: Vec<Atom> },
    /// Conditional: cond is atom; branches are full ANF sub-expressions.
    AIf {
        cond: Atom,
        then_branch: Box<AnfExpr>,
        else_branch: Box<AnfExpr>,
    },
    /// Pattern match: scrutinee is atom; arm bodies are full ANF sub-expressions.
    AMatch {
        scrutinee: Atom,
        arms: Vec<AnfMatchArm>,
    },
    /// Loop: body is a full ANF sub-expression (typically ends with Break/Continue).
    ALoop { body: Box<AnfExpr> },
    /// Binary operation: both operands are atoms.
    ABinOp {
        op: BinOp,
        left: Atom,
        right: Atom,
        operand_ty: OpKind,
    },
    /// Unary operation: operand is atom.
    AUnOp {
        op: UnOp,
        expr: Atom,
        operand_ty: OpKind,
    },
    /// Closure creation: func_id is a literal; free_vars are already locals.
    AMakeClosure {
        func_id: FuncId,
        free_vars: Vec<LocalId>,
    },
    /// Record construction: all field values are atoms.
    ARecord {
        type_id: TypeId,
        fields: Vec<(FieldId, Atom)>,
    },
    /// Record field read: target is atom.
    ARecordGet {
        target: Atom,
        field: FieldId,
        type_id: TypeId,
    },
    /// Functional record update: base and value are atoms.
    ///
    /// `can_reuse_in_place` is set by the liveness pass (Stage 7.5) when the
    /// base local is provably dead after this update. The WAT backend may then
    /// emit an in-place `struct.set` instead of allocating a new struct.
    ARecordUpdate {
        base: Atom,
        field: FieldId,
        value: Atom,
        can_reuse_in_place: bool,
        type_id: TypeId,
    },
    /// Variant construction: all args are atoms.
    AVariant {
        type_id: TypeId,
        variant: VariantId,
        args: Vec<Atom>,
    },
    /// Array literal: all elements are atoms.
    AArrayLit(Vec<Atom>),
    /// Array/dict index: base and index are atoms.
    AIndex {
        base: Atom,
        index: Atom,
        base_ty: IndexKind,
    },
    /// Initial binding — introduces a new local for the first time.
    /// Used for `CoreExprKind::Let` lowering. Maps to Wasm `local.set` of a
    /// freshly declared local. Distinct from `AAssign` (existing-local mutation).
    /// The enclosing `AnfExpr::Let.local` names the local being initialized.
    AInit { value: Atom },
    /// Local mutation (maps to Wasm `local.set`): value is atom.
    /// Used for `CoreExprKind::Assign` lowering. Mutates an already-declared local.
    /// Result is void; the binding local in the enclosing `Let` is a fresh temp.
    AAssign { local: LocalId, value: Atom },
    /// Deferred expression — preserves the deferred sub-expression through ANF
    /// linearization. Eliminated by the defer_elim pass before reaching the WAT
    /// backend; no `ADefer` node survives into code generation.
    ADefer(Box<AnfExpr>),
}

/// A match arm in ANF IR. Reuses `CorePattern` from Core IR (already fully resolved).
#[derive(Debug, Clone, PartialEq)]
pub struct AnfMatchArm {
    pub pattern: CorePattern,
    pub body: AnfExpr,
}

/// A function definition in ANF IR.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfFunctionDef {
    pub func_id: FuncId,
    pub name: String,
    pub params: Vec<LocalId>,
    pub param_tys: Vec<MonoType>,
    pub body: AnfExpr,
    pub return_ty: MonoType,
}

/// An ANF module — a flat list of function definitions plus the init entry point.
#[derive(Debug, Clone)]
pub struct AnfModule {
    pub functions: Vec<AnfFunctionDef>,
    /// The `__init__` FuncId for the entry module.
    pub init_func_id: Option<FuncId>,
    /// All module `__init__` FuncIds in dependency order.
    pub all_init_func_ids: Vec<FuncId>,
}

// ── Display implementations ──────────────────────────────────────────────────

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Atom::ALocal(id) => write!(f, "L{}", id.0),
            Atom::AGlobalFunc(id) => write!(f, "GlobalFunc({})", id.0),
            Atom::ALitInt(n) => write!(f, "{}", n),
            Atom::ALitFloat(v) => write!(f, "{}", v),
            Atom::ALitBool(b) => write!(f, "{}", b),
            Atom::ALitStr(s) => write!(f, "{:?}", s),
            Atom::ALitVoid => write!(f, "void"),
        }
    }
}

fn print_anf_expr(expr: &AnfExpr, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
    let pad = "  ".repeat(indent);
    match expr {
        AnfExpr::Let { local, op, body } => {
            write!(f, "{}let L{} = ", pad, local.0)?;
            print_anf_op(op, f, indent)?;
            writeln!(f)?;
            print_anf_expr(body, f, indent)
        }
        AnfExpr::Return(None) => writeln!(f, "{}return void", pad),
        AnfExpr::Return(Some(atom)) => writeln!(f, "{}return {}", pad, atom),
        AnfExpr::Break(None) => writeln!(f, "{}break", pad),
        AnfExpr::Break(Some(atom)) => writeln!(f, "{}break {}", pad, atom),
        AnfExpr::Continue => writeln!(f, "{}continue", pad),
        AnfExpr::Atom(atom) => writeln!(f, "{}{}", pad, atom),
    }
}

fn print_anf_op(op: &AnfOp, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
    let pad = "  ".repeat(indent);
    match op {
        AnfOp::ACall { callee, args } => {
            write!(f, "call {} (", callee)?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", arg)?;
            }
            write!(f, ")")
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            writeln!(f, "if {} {{", cond)?;
            print_anf_expr(then_branch, f, indent + 1)?;
            writeln!(f, "{}}} else {{", pad)?;
            print_anf_expr(else_branch, f, indent + 1)?;
            write!(f, "{}}}", pad)
        }
        AnfOp::AMatch { scrutinee, arms } => {
            writeln!(f, "match {} {{", scrutinee)?;
            for arm in arms {
                writeln!(f, "{}  {:?} =>", pad, arm.pattern)?;
                print_anf_expr(&arm.body, f, indent + 2)?;
            }
            write!(f, "{}}}", pad)
        }
        AnfOp::ALoop { body } => {
            writeln!(f, "loop {{")?;
            print_anf_expr(body, f, indent + 1)?;
            write!(f, "{}}}", pad)
        }
        AnfOp::ABinOp {
            op,
            left,
            right,
            operand_ty,
        } => {
            write!(f, "binop({:?}, {}, {}, {:?})", op, left, right, operand_ty)
        }
        AnfOp::AUnOp {
            op,
            expr,
            operand_ty,
        } => {
            write!(f, "unop({:?}, {}, {:?})", op, expr, operand_ty)
        }
        AnfOp::AMakeClosure { func_id, free_vars } => {
            write!(f, "closure(FuncId({})", func_id.0)?;
            if !free_vars.is_empty() {
                write!(f, ", free=[")?;
                for (i, v) in free_vars.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "L{}", v.0)?;
                }
                write!(f, "]")?;
            }
            write!(f, ")")
        }
        AnfOp::ARecord { type_id, fields } => {
            write!(f, "record(Type#{}", type_id.0)?;
            for (fid, atom) in fields {
                write!(f, ", .{}={}", fid.0, atom)?;
            }
            write!(f, ")")
        }
        AnfOp::ARecordGet {
            target,
            field,
            type_id,
        } => {
            write!(
                f,
                "record_get({}, .{}, Type#{})",
                target, field.0, type_id.0
            )
        }
        AnfOp::ARecordUpdate {
            base,
            field,
            value,
            can_reuse_in_place,
            type_id,
        } => {
            let flag = if *can_reuse_in_place {
                " [in-place]"
            } else {
                ""
            };
            write!(
                f,
                "record_update({}, .{}={}, Type#{}{})",
                base, field.0, value, type_id.0, flag
            )
        }
        AnfOp::AVariant {
            type_id,
            variant,
            args,
        } => {
            write!(f, "variant(Type#{}.{})", type_id.0, variant.0)?;
            if !args.is_empty() {
                write!(f, "(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")?;
            }
            Ok(())
        }
        AnfOp::AArrayLit(elems) => {
            write!(f, "array[")?;
            for (i, elem) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", elem)?;
            }
            write!(f, "]")
        }
        AnfOp::AIndex {
            base,
            index,
            base_ty,
        } => {
            write!(f, "index({}, {}, {:?})", base, index, base_ty)
        }
        AnfOp::AInit { value } => {
            write!(f, "init({})", value)
        }
        AnfOp::AAssign { local, value } => {
            write!(f, "assign(L{} = {})", local.0, value)
        }
        AnfOp::ADefer(inner) => {
            writeln!(f, "defer {{")?;
            print_anf_expr(inner, f, indent + 1)?;
            write!(f, "{}}}", pad)
        }
    }
}

impl fmt::Display for AnfFunctionDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fn {}  [FuncId({})]  params=[",
            self.name, self.func_id.0
        )?;
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            if let Some(ty) = self.param_tys.get(i) {
                write!(f, "L{}: {:?}", p.0, ty)?;
            } else {
                write!(f, "L{}", p.0)?;
            }
        }
        writeln!(f, "]")?;
        writeln!(f, "  return_ty: {:?}", self.return_ty)?;
        writeln!(f, "  body:")?;
        print_anf_expr(&self.body, f, 2)?;
        Ok(())
    }
}

impl fmt::Display for AnfModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "// ANF Module")?;
        writeln!(f, "// {} function(s)", self.functions.len())?;
        if let Some(init_id) = self.init_func_id {
            writeln!(f, "// init = FuncId({})", init_id.0)?;
        }
        writeln!(f)?;
        for func in &self.functions {
            writeln!(f, "{}", func)?;
        }
        Ok(())
    }
}
