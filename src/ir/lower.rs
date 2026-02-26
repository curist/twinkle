use std::collections::{HashMap, HashSet};

use crate::syntax::ast::{
    BinOp, Block, CaseArm, Expr, ExprKind, FunctionDecl, Item, Literal, Pattern, SourceFile,
    Stmt, StringPart,
};
use crate::syntax::span::Span;
use crate::types::env::TypeEnv;
use crate::types::ty::{MonoType, RANGE_TYPE_ID, RESULT_TYPE_ID};
use crate::types::type_map::TypeMap;

use super::core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, FieldId, FuncId, FunctionDef,
    LocalId, MatchArm, VariantId,
};
use super::error::LowerError;
use super::local_allocator::LocalAllocator;

// ---------------------------------------------------------------------------
// Prelude function IDs (fixed)
// ---------------------------------------------------------------------------

pub mod prelude {
    use super::FuncId;

    pub const PRINT: FuncId = FuncId(1);
    pub const PRINTLN: FuncId = FuncId(2);
    pub const ERROR: FuncId = FuncId(3);

    pub const INT_TO_STRING: FuncId = FuncId(4);
    pub const FLOAT_TO_STRING: FuncId = FuncId(5);
    pub const BOOL_TO_STRING: FuncId = FuncId(6);
    pub const STRING_TO_STRING: FuncId = FuncId(7); // identity

    pub const STRING_LEN: FuncId = FuncId(8);
    pub const STRING_CONCAT: FuncId = FuncId(9);

    pub const ARRAY_LEN: FuncId = FuncId(10);
    pub const ARRAY_APPEND: FuncId = FuncId(11);

    pub const ARRAY_SET: FuncId = FuncId(12); // Array.set(arr, idx, val) -> Array<T>
    pub const DICT_SET:  FuncId = FuncId(13); // Dict.set(m, k, v) -> Dict<K,V>
    pub const DICT_KEYS: FuncId = FuncId(14); // Dict.keys(m) -> Array<K>

    pub const RANGE_FROM: FuncId = FuncId(15); // range_from(start, end) -> Range
    pub const RANGE:      FuncId = FuncId(16); // range(n) -> Range  (0..n)

    pub const CELL_NEW:    FuncId = FuncId(17); // Cell.new(v: T) Cell<T>
    pub const CELL_GET:    FuncId = FuncId(18); // Cell.get(c: Cell<T>) T
    pub const CELL_SET:    FuncId = FuncId(19); // Cell.set(c: Cell<T>, v: T) Void
    pub const CELL_UPDATE: FuncId = FuncId(20); // Cell.update(c: Cell<T>, f: fn(T) T) Void

    pub const DICT_GET: FuncId = FuncId(21); // dict_get(m, k) -> Option<V>
    pub const DICT_NEW: FuncId = FuncId(22); // Dict.new() -> Dict<K,V>

    pub const RANGE_STEP: FuncId = FuncId(23); // range_step(start, end, step) -> Range

    // User functions start here
    pub const USER_FUNC_START: u32 = 24;
}

// ---------------------------------------------------------------------------
// Lowerer
// ---------------------------------------------------------------------------

pub struct Lowerer {
    type_map: TypeMap,
    type_env: TypeEnv,
    /// Map from function name to its assigned FuncId
    func_table: HashMap<String, FuncId>,
    /// Set of module alias names (for cross-module call dispatch)
    module_aliases: HashSet<String>,
    errors: Vec<LowerError>,
    /// Per-function local variable allocator (reset for each function)
    local_allocator: LocalAllocator,
    /// Next FuncId for hoisted lambdas and __init__ (assigned after all user funcs)
    next_hoisted_id: u32,
    /// Hoisted lambda functions (and __init__) collected during lowering
    hoisted_functions: Vec<FunctionDef>,
}

impl Lowerer {
    pub fn new(type_map: TypeMap, type_env: TypeEnv) -> Self {
        let mut func_table = HashMap::new();

        // Register prelude functions
        func_table.insert("print".to_string(), prelude::PRINT);
        func_table.insert("println".to_string(), prelude::PRINTLN);
        func_table.insert("error".to_string(), prelude::ERROR);
        func_table.insert("range_from".to_string(), prelude::RANGE_FROM);
        func_table.insert("range".to_string(), prelude::RANGE);
        func_table.insert("range_step".to_string(), prelude::RANGE_STEP);
        func_table.insert("Cell.new".to_string(), prelude::CELL_NEW);
        func_table.insert("Cell.get".to_string(), prelude::CELL_GET);
        func_table.insert("Cell.set".to_string(), prelude::CELL_SET);
        func_table.insert("Cell.update".to_string(), prelude::CELL_UPDATE);
        func_table.insert("Dict.new".to_string(), prelude::DICT_NEW);

        // len is polymorphic and handled specially in lower_expr_call

        let mut module_aliases = HashSet::new();
        module_aliases.insert("Cell".to_string()); // built-in module alias
        module_aliases.insert("Dict".to_string()); // built-in module alias

        Self {
            type_map,
            type_env,
            func_table,
            module_aliases,
            errors: Vec::new(),
            local_allocator: LocalAllocator::new(),
            next_hoisted_id: prelude::USER_FUNC_START, // updated after user-func pass
            hoisted_functions: Vec::new(),
        }
    }

    /// Construct a Lowerer using a pre-built CompilationContext.
    /// The func_table, type_env, and module_aliases are taken from the context.
    pub fn new_with_context(
        type_map: TypeMap,
        ctx: &crate::module::context::CompilationContext,
    ) -> Self {
        Self {
            type_map,
            type_env: ctx.type_env.clone(),
            func_table: ctx.func_table.clone(),
            module_aliases: ctx.module_aliases.clone(),
            errors: Vec::new(),
            local_allocator: LocalAllocator::new(),
            next_hoisted_id: ctx.next_func_id,
            hoisted_functions: Vec::new(),
        }
    }

    /// Lower a complete source file to Core IR
    pub fn lower_module(mut self, ast: &SourceFile) -> Result<CoreModule, Vec<LowerError>> {
        // First pass: assign FuncIds to all user functions (source order)
        // Skip any that are already in the func_table (pre-assigned by context)
        let mut next_func_id = prelude::USER_FUNC_START;
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if !self.func_table.contains_key(&decl.name) {
                    let func_id = FuncId(next_func_id);
                    self.func_table.insert(decl.name.clone(), func_id);
                }
                // Advance counter past any pre-assigned IDs
                if let Some(existing) = self.func_table.get(&decl.name) {
                    if existing.0 >= next_func_id {
                        next_func_id = existing.0 + 1;
                    }
                }
            }
        }
        self.next_hoisted_id = next_func_id;

        // Second pass: lower each function
        let mut functions = Vec::new();
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if let Some(func_def) = self.lower_function(decl) {
                    functions.push(func_def);
                }
            }
        }

        // Lower top-level statements into __init__
        let init_func_id = self.lower_init_stmts(ast).map(|init_def| {
            let id = init_def.func_id;
            self.hoisted_functions.push(init_def);
            id
        });

        // Include hoisted lambdas and __init__ in the function list
        functions.extend(self.hoisted_functions.drain(..));

        if self.errors.is_empty() {
            Ok(CoreModule {
                functions,
                type_env: self.type_env,
                init_func_id,
            })
        } else {
            Err(self.errors)
        }
    }

    /// Lower only the functions of a source file, returning them as a Vec along
    /// with the Optional FuncId of the __init__ function (top-level stmts).
    /// FuncIds for user functions must already be pre-assigned in `self.func_table`.
    /// Used by the multi-module pipeline.
    pub fn lower_module_funcs(mut self, ast: &SourceFile) -> Result<(Vec<FunctionDef>, Option<FuncId>), Vec<LowerError>> {
        let mut functions = Vec::new();
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if let Some(func_def) = self.lower_function(decl) {
                    functions.push(func_def);
                }
            }
        }

        // Lower top-level statements into __init__
        let init_func_id = self.lower_init_stmts(ast).map(|init_def| {
            let id = init_def.func_id;
            functions.push(init_def);
            id
        });

        // Include any hoisted lambdas
        functions.extend(self.hoisted_functions.drain(..));

        if self.errors.is_empty() {
            Ok((functions, init_func_id))
        } else {
            Err(self.errors)
        }
    }

    /// Allocate the next FuncId for a hoisted function (lambda or __init__).
    fn alloc_hoisted_id(&mut self) -> FuncId {
        let id = FuncId(self.next_hoisted_id);
        self.next_hoisted_id += 1;
        id
    }

    /// Lower all top-level expression statements into a synthetic `__init__` function.
    /// Returns `None` if there are no top-level statements.
    fn lower_init_stmts(&mut self, ast: &SourceFile) -> Option<FunctionDef> {
        // Collect all top-level statements in source order
        let stmts: Vec<&Stmt> = ast.items.iter()
            .filter_map(|item| if let Item::Stmt(s) = item { Some(s) } else { None })
            .collect();

        if stmts.is_empty() {
            return None;
        }

        // Fresh allocator for the __init__ function
        self.local_allocator = LocalAllocator::new();

        // We need an owned Vec<Stmt> to call lower_stmts — clone for now
        let stmts_owned: Vec<Stmt> = stmts.iter().map(|s| (*s).clone()).collect();

        let span = match stmts_owned.first() {
            Some(s) => stmt_span(s),
            None => return None,
        };

        let body = self.lower_stmts(&stmts_owned, span)?;
        let func_id = self.alloc_hoisted_id();

        Some(FunctionDef {
            func_id,
            name: "__init__".to_string(),
            params: vec![],
            return_ty: MonoType::Void,
            body,
        })
    }

    // -----------------------------------------------------------------------
    // Function lowering
    // -----------------------------------------------------------------------

    fn lower_function(&mut self, decl: &FunctionDecl) -> Option<FunctionDef> {
        // Fresh LocalAllocator for each function
        self.local_allocator = LocalAllocator::new();

        // Bind parameters
        let mut params = Vec::new();
        for param in &decl.params {
            let local_id = self.local_allocator.alloc_and_bind(param.name.clone());
            params.push(local_id);
        }

        let body = self.lower_block(&decl.body)?;
        let return_ty = body.ty.clone();
        let func_id = *self.func_table.get(&decl.name)?;

        Some(FunctionDef {
            func_id,
            name: decl.name.clone(),
            params,
            body,
            return_ty,
        })
    }

    // -----------------------------------------------------------------------
    // Block / statement lowering
    // -----------------------------------------------------------------------

    fn lower_block(&mut self, block: &Block) -> Option<CoreExpr> {
        self.lower_stmts(&block.stmts, block.span)
    }

    fn lower_stmts(&mut self, stmts: &[Stmt], span: Span) -> Option<CoreExpr> {
        match stmts {
            [] => Some(CoreExpr {
                kind: CoreExprKind::LitVoid,
                ty: MonoType::Void,
                span,
            }),

            [stmt, rest @ ..] => self.lower_stmt_head(stmt, rest, span),
        }
    }

    fn lower_stmt_head(&mut self, stmt: &Stmt, rest: &[Stmt], span: Span) -> Option<CoreExpr> {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                // Check for simple rebinding: `x = expr` is a BinOp::Assign but
                // Stmt::Let without ColonEq token is treated as a new binding here.
                // All Stmt::Let create new bindings.
                match pattern {
                    Pattern::Ident(name, _) => {
                        let value_expr = self.lower_expr(value)?;
                        let local = self.local_allocator.alloc_and_bind(name.clone());
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local,
                                value: Box::new(value_expr),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        })
                    }
                    Pattern::Wildcard(_) => {
                        let value_expr = self.lower_expr(value)?;
                        let local = self.local_allocator.alloc();
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local,
                                value: Box::new(value_expr),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        })
                    }
                    _ => {
                        self.errors.push(LowerError::UnsupportedFeature {
                            feature: "complex pattern in let binding",
                            span: stmt_span(stmt),
                        });
                        None
                    }
                }
            }

            Stmt::Expr(e) => {
                // Check if this is a rebinding assignment: `x = expr`
                if let Some((name, value)) = extract_simple_assign(e) {
                    // Look up the existing LocalId for this name
                    if let Some(existing_local) = self.local_allocator.lookup(name) {
                        let value_expr = self.lower_expr(value)?;
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        // Assign mutates the existing slot; no new LocalId allocated
                        return Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: self.local_allocator.alloc(),
                                value: Box::new(CoreExpr {
                                    kind: CoreExprKind::Assign {
                                        local: existing_local,
                                        value: Box::new(value_expr),
                                    },
                                    ty: MonoType::Void,
                                    span,
                                }),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        });
                    }
                }

                // Non-ident lvalue assignment: r.field = val, arr[i] = val, m[k] = val
                if let ExprKind::Binary { op: BinOp::Assign, left, right } = &e.kind {
                    let rhs_core = self.lower_expr(right)?;
                    if let Some((root_local, update_expr)) = self.lower_lvalue_chain(left, rhs_core) {
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        return Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: self.local_allocator.alloc(),
                                value: Box::new(CoreExpr {
                                    kind: CoreExprKind::Assign {
                                        local: root_local,
                                        value: Box::new(update_expr),
                                    },
                                    ty: MonoType::Void,
                                    span,
                                }),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        });
                    }
                }

                if rest.is_empty() {
                    // Last statement: block value
                    self.lower_expr(e)
                } else {
                    // Side-effect expression, then continue
                    let e_expr = self.lower_expr(e)?;
                    let body = self.lower_stmts(rest, span)?;
                    let ty = body.ty.clone();
                    let temp = self.local_allocator.alloc();
                    Some(CoreExpr {
                        kind: CoreExprKind::Let {
                            local: temp,
                            value: Box::new(e_expr),
                            body: Box::new(body),
                        },
                        ty,
                        span,
                    })
                }
            }

            Stmt::Return { value, span: ret_span } => {
                let val = match value {
                    Some(v) => Some(Box::new(self.lower_expr(v)?)),
                    None => None,
                };
                Some(CoreExpr {
                    kind: CoreExprKind::Return { value: val },
                    ty: MonoType::Void,
                    span: *ret_span,
                })
            }

            Stmt::Break { value, span: brk_span } => {
                let val = match value {
                    Some(v) => Some(Box::new(self.lower_expr(v)?)),
                    None => None,
                };
                Some(CoreExpr {
                    kind: CoreExprKind::Break { value: val },
                    ty: MonoType::Void,
                    span: *brk_span,
                })
            }

            Stmt::Continue { span: cont_span } => Some(CoreExpr {
                kind: CoreExprKind::Continue,
                ty: MonoType::Void,
                span: *cont_span,
            }),

            Stmt::For { span: for_span, .. } | Stmt::ForCond { span: for_span, .. } => {
                self.lower_for_stmt(stmt, rest, span, *for_span)
            }
        }
    }

    fn lower_for_stmt(
        &mut self,
        stmt: &Stmt,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        match stmt {
            Stmt::ForCond { cond, body, .. } => {
                // for cond { body }
                // → Loop { If { cond: !cond, then: Break(None), else: body } }
                let cond_expr = self.lower_expr(cond)?;
                let not_cond = CoreExpr {
                    ty: MonoType::Bool,
                    span: cond_expr.span,
                    kind: CoreExprKind::UnOp {
                        op: crate::syntax::ast::UnOp::Not,
                        expr: Box::new(cond_expr),
                    },
                };
                let break_expr = CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: for_span,
                };

                self.local_allocator.push_scope();
                let body_expr = self.lower_block(body)?;
                self.local_allocator.pop_scope();

                let if_expr = CoreExpr {
                    ty: MonoType::Void,
                    span: for_span,
                    kind: CoreExprKind::If {
                        cond: Box::new(not_cond),
                        then_branch: Box::new(break_expr),
                        else_branch: Box::new(body_expr),
                    },
                };

                let loop_expr = CoreExpr {
                    kind: CoreExprKind::Loop {
                        body: Box::new(if_expr),
                    },
                    ty: MonoType::Void,
                    span: for_span,
                };

                // The loop is a statement; continue with rest
                let continuation = self.lower_stmts(rest, cont_span)?;
                let ty = continuation.ty.clone();
                let temp = self.local_allocator.alloc();
                Some(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: temp,
                        value: Box::new(loop_expr),
                        body: Box::new(continuation),
                    },
                    ty,
                    span: for_span,
                })
            }

            Stmt::For { pattern, index_pattern, iter, body, .. } => {
                let iter_ty = self.type_map.get_expr_type(iter.id).cloned();
                if matches!(iter_ty, Some(MonoType::Dict(_, _))) {
                    return self.lower_dict_for_stmt(
                        pattern, index_pattern.as_ref(), iter, body, rest, cont_span, for_span,
                    );
                }
                if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == RANGE_TYPE_ID) {
                    return self.lower_range_for_stmt(
                        pattern, index_pattern.as_ref(), iter, body, rest, cont_span, for_span,
                    );
                }

                // for x in arr { body }  (and  for x, i in arr { body })
                // → Let(arr_tmp, iter,
                //     Let(len_tmp, array_len(arr_tmp),
                //       Let(idx_tmp, 0,
                //         Loop {
                //           If(idx_tmp >= len_tmp,
                //             Break(None),
                //             Let(elem, arr_tmp[idx_tmp],
                //               <body>_then_Let(idx_tmp2, idx_tmp+1, <rebind idx_tmp, Continue>)))
                //         })))
                let iter_expr = self.lower_expr(iter)?;
                let iter_span = iter.span;

                // arr_tmp = iter
                let arr_tmp = self.local_allocator.alloc_and_bind("__arr".to_string());
                let len_tmp = self.local_allocator.alloc_and_bind("__len".to_string());
                let idx_tmp = self.local_allocator.alloc_and_bind("__idx".to_string());

                // array_len call
                let arr_len_func = CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::ARRAY_LEN),
                    ty: MonoType::Function {
                        params: vec![MonoType::Array(Box::new(MonoType::Int))],
                        ret: Box::new(MonoType::Int),
                    },
                    span: iter_span,
                };
                let arr_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(arr_tmp),
                    ty: iter_expr.ty.clone(),
                    span: iter_span,
                };
                let len_call = CoreExpr {
                    kind: CoreExprKind::Call {
                        callee: Box::new(arr_len_func),
                        args: vec![arr_local_expr.clone()],
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                };

                // Loop body
                self.local_allocator.push_scope();

                // Bind element variable
                let elem_ty = match &iter_expr.ty {
                    MonoType::Array(inner) => *inner.clone(),
                    _ => MonoType::Void,
                };

                let elem_local = match pattern {
                    Pattern::Ident(name, _) => {
                        self.local_allocator.alloc_and_bind(name.clone())
                    }
                    _ => self.local_allocator.alloc(),
                };

                // Optionally bind index variable
                let idx_user = index_pattern.as_ref().and_then(|ip| {
                    if let Pattern::Ident(name, _) = ip {
                        Some(self.local_allocator.alloc_and_bind(name.clone()))
                    } else {
                        None
                    }
                });

                // elem = arr_tmp[idx_tmp]
                let idx_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(idx_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let elem_value = CoreExpr {
                    kind: CoreExprKind::Index {
                        base: Box::new(arr_local_expr),
                        index: Box::new(idx_local_expr.clone()),
                    },
                    ty: elem_ty.clone(),
                    span: iter_span,
                };

                let body_expr = self.lower_block(body)?;
                self.local_allocator.pop_scope();

                // Increment idx using Assign (mutation) then Continue
                let one = CoreExpr {
                    kind: CoreExprKind::LitInt(1),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let idx_plus_one = CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(idx_local_expr.clone()),
                        right: Box::new(one),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let continue_expr = CoreExpr {
                    kind: CoreExprKind::Continue,
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Assign(idx_tmp, idx+1) then Continue
                let idx_inc = CoreExpr {
                    kind: CoreExprKind::Assign {
                        local: idx_tmp,
                        value: Box::new(idx_plus_one),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };
                let tmp_after_inc = self.local_allocator.alloc();
                let idx_rebind = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: tmp_after_inc,
                        value: Box::new(idx_inc),
                        body: Box::new(continue_expr),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // body_then_idx_rebind: body followed by idx increment+continue
                let body_then_inc = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(body_expr),
                        body: Box::new(idx_rebind),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Optionally bind user-visible index BEFORE the body
                let loop_body_inner = if let Some(user_idx) = idx_user {
                    CoreExpr {
                        kind: CoreExprKind::Let {
                            local: user_idx,
                            value: Box::new(idx_local_expr.clone()),
                            body: Box::new(body_then_inc),
                        },
                        ty: MonoType::Void,
                        span: iter_span,
                    }
                } else {
                    body_then_inc
                };

                // Let(elem, elem_value, loop_body_inner)
                let elem_let = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: elem_local,
                        value: Box::new(elem_value),
                        body: Box::new(loop_body_inner),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Break when idx >= len
                let len_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(len_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let cond_expr = CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Ge,
                        left: Box::new(idx_local_expr),
                        right: Box::new(len_local_expr),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                };
                let break_expr = CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: iter_span,
                };
                let if_expr = CoreExpr {
                    kind: CoreExprKind::If {
                        cond: Box::new(cond_expr),
                        then_branch: Box::new(break_expr),
                        else_branch: Box::new(elem_let),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                let loop_expr = CoreExpr {
                    kind: CoreExprKind::Loop {
                        body: Box::new(if_expr),
                    },
                    ty: MonoType::Void,
                    span: for_span,
                };

                // Wrap in Let(arr_tmp, iter, Let(len_tmp, len, Let(idx_tmp, 0, loop)))
                let zero = CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                };

                // Add continuation after the loop
                let continuation = self.lower_stmts(rest, cont_span)?;
                let cont_ty = continuation.ty.clone();
                let loop_then_cont = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(loop_expr),
                        body: Box::new(continuation),
                    },
                    ty: cont_ty.clone(),
                    span: for_span,
                };

                Some(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: arr_tmp,
                        value: Box::new(iter_expr),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: len_tmp,
                                value: Box::new(len_call),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: idx_tmp,
                                        value: Box::new(zero),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(),
                                    span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(),
                            span: for_span,
                        }),
                    },
                    ty: cont_ty,
                    span: for_span,
                })
            }

            _ => unreachable!(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_dict_for_stmt(
        &mut self,
        pattern: &Pattern,
        val_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        // for k, v in dict { body }
        // → Let(dict_tmp, dict,
        //     Let(keys_tmp, Dict.keys(dict_tmp),
        //       Let(len_tmp, array_len(keys_tmp),
        //         Let(idx_tmp, 0,
        //           Loop {
        //             If(idx_tmp >= len_tmp, Break,
        //               Let(k, keys_tmp[idx_tmp],
        //                 [Let(v, dict_tmp[k],]
        //                   body_then_Assign(idx_tmp, idx_tmp+1)_Continue))
        //           }))))
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let (key_ty, val_ty) = match &iter_expr.ty {
            MonoType::Dict(k, v) => (*k.clone(), *v.clone()),
            _ => unreachable!(),
        };

        let dict_tmp = self.local_allocator.alloc_and_bind("__dict".to_string());
        let keys_tmp = self.local_allocator.alloc_and_bind("__keys".to_string());
        let len_tmp  = self.local_allocator.alloc_and_bind("__len".to_string());
        let idx_tmp  = self.local_allocator.alloc_and_bind("__idx".to_string());

        let dict_local_expr = CoreExpr {
            kind: CoreExprKind::Local(dict_tmp),
            ty: iter_expr.ty.clone(),
            span: iter_span,
        };
        let keys_ty = MonoType::Array(Box::new(key_ty.clone()));

        // keys = Dict.keys(dict_tmp)
        let dict_keys_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::DICT_KEYS),
            ty: MonoType::Function {
                params: vec![iter_expr.ty.clone()],
                ret: Box::new(keys_ty.clone()),
            },
            span: iter_span,
        };
        let keys_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(dict_keys_func),
                args: vec![dict_local_expr.clone()],
            },
            ty: keys_ty.clone(),
            span: iter_span,
        };

        let keys_local_expr = CoreExpr {
            kind: CoreExprKind::Local(keys_tmp),
            ty: keys_ty.clone(),
            span: iter_span,
        };

        // len = array_len(keys_tmp)
        let arr_len_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::ARRAY_LEN),
            ty: MonoType::Function {
                params: vec![keys_ty.clone()],
                ret: Box::new(MonoType::Int),
            },
            span: iter_span,
        };
        let len_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(arr_len_func),
                args: vec![keys_local_expr.clone()],
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::Local(len_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };

        // Loop body: allocate k and v inside scope so body can reference them
        self.local_allocator.push_scope();

        let key_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let val_local = val_pattern.map(|vp| match vp {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        });

        let body_expr = self.lower_block(body)?;
        self.local_allocator.pop_scope();

        // key_value = keys_tmp[idx_tmp]
        let key_value = CoreExpr {
            kind: CoreExprKind::Index {
                base: Box::new(keys_local_expr),
                index: Box::new(idx_expr.clone()),
            },
            ty: key_ty.clone(),
            span: iter_span,
        };

        // Increment: Assign(idx_tmp, idx_tmp + 1) then Continue
        let idx_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_tmp,
                value: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(idx_expr),
                        right: Box::new(CoreExpr {
                            kind: CoreExprKind::LitInt(1),
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let idx_rebind = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(idx_inc),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Continue,
                    ty: MonoType::Void,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // body_then_tail
        let body_then_tail = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(body_expr),
                body: Box::new(idx_rebind),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Optionally wrap with: Let(v, dict_tmp[k], body_then_tail)
        let loop_body_inner = if let Some(vl) = val_local {
            let val_value = CoreExpr {
                kind: CoreExprKind::Index {
                    base: Box::new(dict_local_expr),
                    index: Box::new(CoreExpr {
                        kind: CoreExprKind::Local(key_local),
                        ty: key_ty,
                        span: iter_span,
                    }),
                },
                ty: val_ty,
                span: iter_span,
            };
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: vl,
                    value: Box::new(val_value),
                    body: Box::new(body_then_tail),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            body_then_tail
        };

        // Let(k, keys[idx], loop_body_inner)
        let key_let = CoreExpr {
            kind: CoreExprKind::Let {
                local: key_local,
                value: Box::new(key_value),
                body: Box::new(loop_body_inner),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // If(idx >= len, Break, key_let)
        let cond_expr = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(idx_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
                right: Box::new(len_expr),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let if_expr = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(cond_expr),
                then_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: for_span,
                }),
                else_branch: Box::new(key_let),
            },
            ty: MonoType::Void,
            span: for_span,
        };

        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop { body: Box::new(if_expr) },
            ty: MonoType::Void,
            span: for_span,
        };

        // Continuation after the loop
        let continuation = self.lower_stmts(rest, cont_span)?;
        let cont_ty = continuation.ty.clone();
        let loop_then_cont = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(continuation),
            },
            ty: cont_ty.clone(),
            span: for_span,
        };

        // Wrap: Let(dict, iter, Let(keys, Dict.keys(dict), Let(len, ..., Let(idx, 0, ...))))
        let zero = CoreExpr { kind: CoreExprKind::LitInt(0), ty: MonoType::Int, span: iter_span };
        Some(CoreExpr {
            kind: CoreExprKind::Let {
                local: dict_tmp,
                value: Box::new(iter_expr),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: keys_tmp,
                        value: Box::new(keys_call),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: len_tmp,
                                value: Box::new(len_call),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: idx_tmp,
                                        value: Box::new(zero),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(),
                                    span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(),
                            span: for_span,
                        }),
                    },
                    ty: cont_ty.clone(),
                    span: for_span,
                }),
            },
            ty: cont_ty,
            span: for_span,
        })
    }

    // -----------------------------------------------------------------------
    // Range for-loop lowering
    // -----------------------------------------------------------------------

    /// Lower `for x in range_expr { body }` (and indexed form) where range_expr: Range.
    ///
    /// Emits a simple integer counter loop — no array allocation:
    ///   Let(r, range_expr,
    ///     Let(cur, r.start,
    ///       Let(end, r.end,
    ///         Let(step, r.step,
    ///           Loop { If(cur >= end, Break,
    ///                     Let(x, cur, [Let(i, cur,)] body; Assign(cur, cur+step); Continue) }))))
    #[allow(clippy::too_many_arguments)]
    fn lower_range_for_stmt(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;
        let range_named_ty = MonoType::named(RANGE_TYPE_ID);

        let r_tmp   = self.local_allocator.alloc_and_bind("__r".to_string());
        let cur_tmp = self.local_allocator.alloc_and_bind("__cur".to_string());
        let end_tmp = self.local_allocator.alloc_and_bind("__end".to_string());
        let step_tmp = self.local_allocator.alloc_and_bind("__step".to_string());

        // Field access: r.start (0), r.end (1), r.step (2)
        let start_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty.clone(), span: iter_span }),
                field: FieldId(0),
            },
            ty: MonoType::Int, span: iter_span,
        };
        let end_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty.clone(), span: iter_span }),
                field: FieldId(1),
            },
            ty: MonoType::Int, span: iter_span,
        };
        let step_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty, span: iter_span }),
                field: FieldId(2),
            },
            ty: MonoType::Int, span: iter_span,
        };

        let cur_expr  = CoreExpr { kind: CoreExprKind::Local(cur_tmp),  ty: MonoType::Int, span: iter_span };
        let end_expr  = CoreExpr { kind: CoreExprKind::Local(end_tmp),  ty: MonoType::Int, span: iter_span };
        let step_expr = CoreExpr { kind: CoreExprKind::Local(step_tmp), ty: MonoType::Int, span: iter_span };

        // Loop body
        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };

        // Optional user-visible index variable (separate counter matching cur)
        let idx_user = index_pattern.and_then(|ip| {
            if let Pattern::Ident(name, _) = ip {
                Some(self.local_allocator.alloc_and_bind(name.clone()))
            } else {
                None
            }
        });

        let body_expr = self.lower_block(body)?;
        self.local_allocator.pop_scope();

        // cur = cur + step, then Continue
        let cur_plus_step = CoreExpr {
            kind: CoreExprKind::BinOp { op: BinOp::Add, left: Box::new(cur_expr.clone()), right: Box::new(step_expr) },
            ty: MonoType::Int, span: iter_span,
        };
        let continue_expr = CoreExpr { kind: CoreExprKind::Continue, ty: MonoType::Void, span: iter_span };
        let cur_inc = CoreExpr {
            kind: CoreExprKind::Assign { local: cur_tmp, value: Box::new(cur_plus_step) },
            ty: MonoType::Void, span: iter_span,
        };
        let after_body = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(body_expr),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(cur_inc),
                        body: Box::new(continue_expr),
                    },
                    ty: MonoType::Void, span: iter_span,
                }),
            },
            ty: MonoType::Void, span: iter_span,
        };

        // Optionally bind user-visible index before body
        let loop_body_inner = if let Some(user_idx) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: user_idx,
                    value: Box::new(cur_expr.clone()),
                    body: Box::new(after_body),
                },
                ty: MonoType::Void, span: iter_span,
            }
        } else {
            after_body
        };

        // Let(x, cur, loop_body_inner)
        let elem_let = CoreExpr {
            kind: CoreExprKind::Let { local: elem_local, value: Box::new(cur_expr.clone()), body: Box::new(loop_body_inner) },
            ty: MonoType::Void, span: iter_span,
        };

        // If(cur >= end, Break, elem_let)
        let break_expr = CoreExpr { kind: CoreExprKind::Break { value: None }, ty: MonoType::Void, span: for_span };
        let guard = CoreExpr {
            kind: CoreExprKind::BinOp { op: BinOp::Ge, left: Box::new(cur_expr), right: Box::new(end_expr) },
            ty: MonoType::Bool, span: iter_span,
        };
        let if_expr = CoreExpr {
            kind: CoreExprKind::If { cond: Box::new(guard), then_branch: Box::new(break_expr), else_branch: Box::new(elem_let) },
            ty: MonoType::Void, span: for_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop { body: Box::new(if_expr) },
            ty: MonoType::Void, span: for_span,
        };

        // Add continuation after the loop
        let continuation = self.lower_stmts(rest, cont_span)?;
        let cont_ty = continuation.ty.clone();
        let loop_then_cont = CoreExpr {
            kind: CoreExprKind::Let { local: self.local_allocator.alloc(), value: Box::new(loop_expr), body: Box::new(continuation) },
            ty: cont_ty.clone(), span: for_span,
        };

        // Wrap: Let(r, iter, Let(cur, start_get, Let(end, end_get, Let(step, step_get, loop_then_cont))))
        Some(CoreExpr {
            kind: CoreExprKind::Let {
                local: r_tmp,
                value: Box::new(iter_expr),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: cur_tmp,
                        value: Box::new(start_get),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: end_tmp,
                                value: Box::new(end_get),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: step_tmp,
                                        value: Box::new(step_get),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(), span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(), span: for_span,
                        }),
                    },
                    ty: cont_ty.clone(), span: for_span,
                }),
            },
            ty: cont_ty, span: for_span,
        })
    }

    // -----------------------------------------------------------------------
    // Expression lowering
    // -----------------------------------------------------------------------

    fn lower_expr(&mut self, expr: &Expr) -> Option<CoreExpr> {
        let ty = match self.type_map.get_expr_type(expr.id) {
            Some(t) => t.clone(),
            None => {
                self.errors.push(LowerError::InternalError {
                    message: format!("no type for expr {:?}", expr.id),
                    span: expr.span,
                });
                return None;
            }
        };

        let kind = self.lower_expr_kind(&expr.kind, &ty, expr.span)?;
        Some(CoreExpr { kind, ty, span: expr.span })
    }

    fn lower_expr_kind(
        &mut self,
        kind: &ExprKind,
        ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        match kind {
            // --- Literals ---
            ExprKind::Literal(lit) => Some(match lit {
                Literal::Int(n) => CoreExprKind::LitInt(*n),
                Literal::Float(f) => CoreExprKind::LitFloat(*f),
                Literal::Bool(b) => CoreExprKind::LitBool(*b),
                Literal::String(s) => CoreExprKind::LitStr(s.clone()),
            }),

            // --- Identifier ---
            ExprKind::Ident(name) => {
                if let Some(local_id) = self.local_allocator.lookup(name) {
                    Some(CoreExprKind::Local(local_id))
                } else if let Some(&func_id) = self.func_table.get(name.as_str()) {
                    Some(CoreExprKind::GlobalFunc(func_id))
                } else {
                    self.errors.push(LowerError::InternalError {
                        message: format!("unresolved name '{}' during lowering", name),
                        span,
                    });
                    None
                }
            }

            // --- Binary / Unary ---
            ExprKind::Binary { op, left, right } => {
                // Handle assignment expressions (e.g. as case arm bodies).
                if matches!(op, BinOp::Assign) {
                    // Simple ident rebinding: x = expr
                    if let ExprKind::Ident(name) = &left.kind {
                        if let Some(existing_local) = self.local_allocator.lookup(name) {
                            let value_expr = self.lower_expr(right)?;
                            return Some(CoreExprKind::Assign {
                                local: existing_local,
                                value: Box::new(value_expr),
                            });
                        }
                    }
                    // Complex lvalue: r.field = val, arr[i] = val, m[k] = val
                    let rhs_core = self.lower_expr(right)?;
                    if let Some((root_local, update_expr)) = self.lower_lvalue_chain(left, rhs_core) {
                        return Some(CoreExprKind::Assign {
                            local: root_local,
                            value: Box::new(update_expr),
                        });
                    }
                    return None;
                }

                let l = self.lower_expr(left)?;
                let r = self.lower_expr(right)?;
                Some(CoreExprKind::BinOp {
                    op: *op,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }

            ExprKind::Unary { op, expr: inner } => {
                let e = self.lower_expr(inner)?;
                Some(CoreExprKind::UnOp {
                    op: *op,
                    expr: Box::new(e),
                })
            }

            // --- Function call ---
            ExprKind::Call { callee, args } => self.lower_call(callee, args, ty, span),

            // --- Field access → RecordGet or method call ---
            ExprKind::FieldAccess { base, field } => {
                // TypeName.Variant (zero-arg variant construction)
                if let ExprKind::Ident(type_name) = &base.kind {
                    if self.local_allocator.lookup(type_name).is_none()
                        && !self.func_table.contains_key(type_name.as_str())
                    {
                        // Base is a type name — look for a variant
                        if let MonoType::Named { type_id, .. } = ty {
                            if let Some(variant_idx) = self.type_env.get_variant_index(*type_id, field) {
                                return Some(CoreExprKind::Variant {
                                    type_id: *type_id,
                                    variant: VariantId(variant_idx),
                                    args: vec![],
                                });
                            }
                        }
                    }
                }

                let base_expr = self.lower_expr(base)?;
                let base_ty = base_expr.ty.clone();

                match &base_ty {
                    MonoType::Named { type_id, .. } => {
                        if let Some(idx) = self.type_env.get_field_index(*type_id, field) {
                            Some(CoreExprKind::RecordGet {
                                target: Box::new(base_expr),
                                field: FieldId(idx),
                            })
                        } else {
                            self.errors.push(LowerError::UnknownField {
                                field: field.clone(),
                                type_name: format!("Type#{}", type_id.0),
                                span,
                            });
                            None
                        }
                    }
                    _ => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("field access on non-record type {:?}", base_ty),
                            span,
                        });
                        None
                    }
                }
            }

            // --- Index ---
            ExprKind::Index { base, index } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let base_expr = self.lower_expr(base)?;
                let index_expr = self.lower_expr(index)?;

                // Dict[key] lowers to dict_get(dict, key) → Option<V>
                if matches!(base_ty, Some(MonoType::Dict(_, _))) {
                    let func_ty = MonoType::Function {
                        params: vec![base_expr.ty.clone(), index_expr.ty.clone()],
                        ret: Box::new(base_expr.ty.clone()), // placeholder
                    };
                    let func_expr = CoreExpr {
                        kind: CoreExprKind::GlobalFunc(prelude::DICT_GET),
                        ty: func_ty,
                        span,
                    };
                    Some(CoreExprKind::Call {
                        callee: Box::new(func_expr),
                        args: vec![base_expr, index_expr],
                    })
                } else {
                    Some(CoreExprKind::Index {
                        base: Box::new(base_expr),
                        index: Box::new(index_expr),
                    })
                }
            }

            // --- Array literal ---
            ExprKind::Array { elements } => {
                let mut lowered = Vec::new();
                for e in elements {
                    lowered.push(self.lower_expr(e)?);
                }
                Some(CoreExprKind::ArrayLit { elements: lowered })
            }

            // --- If expression ---
            ExprKind::If { cond, then_branch, else_branch } => {
                let cond_expr = self.lower_expr(cond)?;
                let then_expr = self.lower_expr(then_branch)?;
                let else_expr = match else_branch {
                    Some(e) => self.lower_expr(e)?,
                    None => CoreExpr {
                        kind: CoreExprKind::LitVoid,
                        ty: MonoType::Void,
                        span,
                    },
                };
                Some(CoreExprKind::If {
                    cond: Box::new(cond_expr),
                    then_branch: Box::new(then_expr),
                    else_branch: Box::new(else_expr),
                })
            }

            // --- Block expression ---
            ExprKind::Block(block) => {
                self.local_allocator.push_scope();
                let result = self.lower_block(block)?;
                self.local_allocator.pop_scope();
                // Unwrap the inner block's kind; the ty is already set by lower_expr
                Some(result.kind)
            }

            // --- Record literal ---
            ExprKind::RecordLit { name: _, fields } => {
                let type_id = match ty {
                    MonoType::Named { type_id, .. } => *type_id,
                    _ => {
                        self.errors.push(LowerError::RecordNeedsTypeContext { span });
                        return None;
                    }
                };

                let mut lowered_fields = Vec::new();
                for (field_name, field_expr) in fields {
                    let idx = match self.type_env.get_field_index(type_id, field_name) {
                        Some(i) => i,
                        None => {
                            self.errors.push(LowerError::UnknownField {
                                field: field_name.clone(),
                                type_name: format!("Type#{}", type_id.0),
                                span,
                            });
                            return None;
                        }
                    };
                    let lowered_val = self.lower_expr(field_expr)?;
                    lowered_fields.push((FieldId(idx), lowered_val));
                }
                // Sort by FieldId for deterministic output
                lowered_fields.sort_by_key(|(fid, _)| *fid);
                Some(CoreExprKind::Record {
                    type_id,
                    fields: lowered_fields,
                })
            }

            // --- Variant literal ---
            ExprKind::VariantLit { name: variant_name, fields } => {
                let type_id = match ty {
                    MonoType::Named { type_id, .. } => *type_id,
                    _ => {
                        self.errors.push(LowerError::VariantNeedsTypeContext { span });
                        return None;
                    }
                };

                let variant_idx = match self.type_env.get_variant_index(type_id, variant_name) {
                    Some(i) => i,
                    None => {
                        self.errors.push(LowerError::UnknownVariant {
                            name: variant_name.clone(),
                            type_name: format!("Type#{}", type_id.0),
                            span,
                        });
                        return None;
                    }
                };

                let mut lowered_args = Vec::new();
                for f in fields {
                    lowered_args.push(self.lower_expr(f)?);
                }

                Some(CoreExprKind::Variant {
                    type_id,
                    variant: VariantId(variant_idx),
                    args: lowered_args,
                })
            }

            // --- Case expression → Match ---
            ExprKind::Case { scrutinee, arms } => {
                let scrut_expr = self.lower_expr(scrutinee)?;
                let scrut_ty = scrut_expr.ty.clone();
                let mut lowered_arms = Vec::new();
                for arm in arms {
                    if let Some(la) = self.lower_case_arm(arm, &scrut_ty) {
                        lowered_arms.push(la);
                    } else {
                        return None;
                    }
                }
                Some(CoreExprKind::Match {
                    scrutinee: Box::new(scrut_expr),
                    arms: lowered_arms,
                })
            }

            // --- String interpolation ---
            ExprKind::StringInterpolation { parts } => {
                self.lower_string_interpolation(parts, span)
            }

            // --- Collect expression ---
            ExprKind::Collect { pattern, iter, body } => {
                self.lower_collect(pattern, iter, body, ty, span)
            }

            // --- Lambda / closure ---
            ExprKind::Function(fe) => {
                // Bind lambda params in a new scope using the shared allocator
                // (so LocalIds are unique across the enclosing function and lambda).
                self.local_allocator.push_scope();
                let lambda_params: Vec<LocalId> = fe.params.iter()
                    .map(|p| self.local_allocator.alloc_and_bind(p.name.clone()))
                    .collect();

                let body = self.lower_expr(&fe.body)?;
                self.local_allocator.pop_scope();

                // Collect free variables: Local(id) refs not in lambda params
                let param_set: HashSet<LocalId> = lambda_params.iter().copied().collect();
                let free_vars = collect_local_refs(&body, &param_set);

                // Hoist to a new FunctionDef
                let func_id = self.alloc_hoisted_id();
                let return_ty = body.ty.clone();
                let hoisted = FunctionDef {
                    func_id,
                    name: format!("<lambda@{}>", span.start),
                    params: lambda_params,
                    body,
                    return_ty,
                };
                self.hoisted_functions.push(hoisted);

                Some(CoreExprKind::MakeClosure { func_id, free_vars })
            }

            // --- Try (deferred until generics) ---
            ExprKind::Try { expr: inner_expr } => {
                // try expr  desugas to:
                //   let tmp = expr
                //   match tmp {
                //     .Ok(v)  => v,
                //     .Err(e) => return .Err(e),
                //   }
                let inner = self.lower_expr(inner_expr)?;
                let inner_ty = inner.ty.clone();

                let tmp_local = self.local_allocator.alloc();
                let v_local   = self.local_allocator.alloc();
                let e_local   = self.local_allocator.alloc();

                // Determine the Ok payload type from the inner type
                let ok_ty = match &inner_ty {
                    MonoType::Named { args, .. } => args.first().cloned().unwrap_or(MonoType::Void),
                    _ => MonoType::Void,
                };
                let err_ty = match &inner_ty {
                    MonoType::Named { args, .. } => args.get(1).cloned().unwrap_or(MonoType::Void),
                    _ => MonoType::Void,
                };

                // Ok arm: bind v_local, return it
                let ok_pattern = CorePattern::Variant {
                    type_id: RESULT_TYPE_ID,
                    variant: VariantId(0),
                    fields: vec![CorePattern::Var(v_local)],
                };
                let ok_body = CoreExpr {
                    kind: CoreExprKind::Local(v_local),
                    ty: ok_ty.clone(),
                    span,
                };

                // Err arm: bind e_local, return Err(e_local)
                let err_payload = CoreExpr {
                    kind: CoreExprKind::Local(e_local),
                    ty: err_ty.clone(),
                    span,
                };
                let err_variant = CoreExpr {
                    kind: CoreExprKind::Variant {
                        type_id: RESULT_TYPE_ID,
                        variant: VariantId(1),
                        args: vec![err_payload],
                    },
                    ty: inner_ty.clone(),
                    span,
                };
                let err_body = CoreExpr {
                    kind: CoreExprKind::Return { value: Some(Box::new(err_variant)) },
                    ty: MonoType::Never,
                    span,
                };

                let err_pattern = CorePattern::Variant {
                    type_id: RESULT_TYPE_ID,
                    variant: VariantId(1),
                    fields: vec![CorePattern::Var(e_local)],
                };

                let match_expr = CoreExpr {
                    kind: CoreExprKind::Match {
                        scrutinee: Box::new(CoreExpr {
                            kind: CoreExprKind::Local(tmp_local),
                            ty: inner_ty.clone(),
                            span,
                        }),
                        arms: vec![
                            MatchArm { pattern: ok_pattern,  body: ok_body  },
                            MatchArm { pattern: err_pattern, body: err_body },
                        ],
                    },
                    ty: ok_ty,
                    span,
                };

                Some(CoreExprKind::Let {
                    local: tmp_local,
                    value: Box::new(inner),
                    body:  Box::new(match_expr),
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Call lowering
    // -----------------------------------------------------------------------

    fn lower_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ret_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        // Special case: len(x) is polymorphic — dispatch by arg type
        if let ExprKind::Ident(name) = &callee.kind {
            if name == "len" {
                return self.lower_len_call(args, span);
            }
        }

        // Field-access calls: module.func(args) or receiver.method(args)
        if let ExprKind::FieldAccess { base, field } = &callee.kind {
            // TypeName.Variant(args) — parameterized variant construction
            if let ExprKind::Ident(type_name) = &base.kind {
                if self.local_allocator.lookup(type_name).is_none()
                    && !self.func_table.contains_key(type_name.as_str())
                    && !self.module_aliases.contains(type_name.as_str())
                {
                    // Base is a type name — look for a variant with this name
                    if let MonoType::Named { type_id, .. } = ret_ty {
                        if let Some(variant_idx) = self.type_env.get_variant_index(*type_id, field) {
                            let mut lowered_args = Vec::new();
                            for a in args {
                                lowered_args.push(self.lower_expr(a)?);
                            }
                            return Some(CoreExprKind::Variant {
                                type_id: *type_id,
                                variant: VariantId(variant_idx),
                                args: lowered_args,
                            });
                        }
                    }
                }
            }

            // Module-qualified call: alias.func(args)
            if let ExprKind::Ident(alias) = &base.kind {
                if self.module_aliases.contains(alias.as_str()) {
                    let qualified = format!("{}.{}", alias, field);
                    if let Some(&func_id) = self.func_table.get(&qualified) {
                        let mut lowered_args = Vec::new();
                        for a in args {
                            lowered_args.push(self.lower_expr(a)?);
                        }
                        let func_ty = MonoType::Function {
                            params: lowered_args.iter().map(|a| a.ty.clone()).collect(),
                            ret: Box::new(ret_ty.clone()),
                        };
                        let func_expr = CoreExpr {
                            kind: CoreExprKind::GlobalFunc(func_id),
                            ty: func_ty,
                            span,
                        };
                        return Some(CoreExprKind::Call {
                            callee: Box::new(func_expr),
                            args: lowered_args,
                        });
                    } else {
                        self.errors.push(LowerError::InternalError {
                            message: format!("no FuncId for '{}'", qualified),
                            span,
                        });
                        return None;
                    }
                }
            }

            // Method call via FieldAccess: receiver.method(args)
            return self.lower_method_call(base, field, args, ret_ty, span);
        }

        // Regular call
        let callee_expr = self.lower_expr(callee)?;
        let mut lowered_args = Vec::new();
        for a in args {
            lowered_args.push(self.lower_expr(a)?);
        }
        Some(CoreExprKind::Call {
            callee: Box::new(callee_expr),
            args: lowered_args,
        })
    }

    fn lower_len_call(&mut self, args: &[Expr], span: Span) -> Option<CoreExprKind> {
        if args.len() != 1 {
            self.errors.push(LowerError::InternalError {
                message: "len() called with wrong number of arguments".to_string(),
                span,
            });
            return None;
        }
        let arg = &args[0];
        let arg_ty = self.type_map.get_expr_type(arg.id).cloned();
        let lowered_arg = self.lower_expr(arg)?;

        let func_id = match arg_ty {
            Some(MonoType::Array(_)) => prelude::ARRAY_LEN,
            Some(MonoType::String) => prelude::STRING_LEN,
            _ => {
                self.errors.push(LowerError::InternalError {
                    message: "len() called on unsupported type".to_string(),
                    span,
                });
                return None;
            }
        };

        let func_expr = CoreExpr {
            kind: CoreExprKind::GlobalFunc(func_id),
            ty: MonoType::Function {
                params: vec![lowered_arg.ty.clone()],
                ret: Box::new(MonoType::Int),
            },
            span,
        };

        Some(CoreExprKind::Call {
            callee: Box::new(func_expr),
            args: vec![lowered_arg],
        })
    }

    fn lower_method_call(
        &mut self,
        base: &Expr,
        method: &str,
        args: &[Expr],
        _ret_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let base_expr = self.lower_expr(base)?;
        let base_ty = base_expr.ty.clone();

        let mut all_args = vec![base_expr];
        for a in args {
            all_args.push(self.lower_expr(a)?);
        }

        let func_id = match (&base_ty, method) {
            (MonoType::Array(_), "len") => prelude::ARRAY_LEN,
            (MonoType::Array(_), "append") => prelude::ARRAY_APPEND,
            (MonoType::String, "len") => prelude::STRING_LEN,
            (MonoType::String, "concat") => prelude::STRING_CONCAT,
            (MonoType::Int, "to_string") => prelude::INT_TO_STRING,
            (MonoType::Float, "to_string") => prelude::FLOAT_TO_STRING,
            (MonoType::Bool, "to_string") => prelude::BOOL_TO_STRING,
            (MonoType::String, "to_string") => prelude::STRING_TO_STRING,
            (MonoType::Dict(_, _), "keys") => prelude::DICT_KEYS,
            (MonoType::Named { type_id, .. }, "get") if *type_id == crate::types::ty::CELL_TYPE_ID => prelude::CELL_GET,
            (MonoType::Named { type_id, .. }, "set") if *type_id == crate::types::ty::CELL_TYPE_ID => prelude::CELL_SET,
            (MonoType::Named { type_id, .. }, "update") if *type_id == crate::types::ty::CELL_TYPE_ID => prelude::CELL_UPDATE,
            (MonoType::Named { type_id, .. }, _) => {
                // User-defined inherent method: look up via TypeEnv → func_table
                let type_id = *type_id;
                if let Some(func_name) = self.type_env.get_method_function(type_id, method).cloned() {
                    if let Some(&func_id) = self.func_table.get(&func_name) {
                        func_id
                    } else {
                        self.errors.push(LowerError::InternalError {
                            message: format!(
                                "no FuncId for method '{}' (resolved to '{}')",
                                method, func_name
                            ),
                            span,
                        });
                        return None;
                    }
                } else {
                    self.errors.push(LowerError::InternalError {
                        message: format!(
                            "no inherent method '{}' for type Type#{}",
                            method, type_id.0
                        ),
                        span,
                    });
                    return None;
                }
            }
            _ => {
                self.errors.push(LowerError::InternalError {
                    message: format!(
                        "no inherent method '{}' for type {:?}",
                        method, base_ty
                    ),
                    span,
                });
                return None;
            }
        };

        let func_ty = MonoType::Function {
            params: all_args.iter().map(|a| a.ty.clone()).collect(),
            ret: Box::new(MonoType::Int), // placeholder; actual ty set by lower_expr wrapper
        };

        let func_expr = CoreExpr {
            kind: CoreExprKind::GlobalFunc(func_id),
            ty: func_ty,
            span,
        };

        Some(CoreExprKind::Call {
            callee: Box::new(func_expr),
            args: all_args,
        })
    }

    // -----------------------------------------------------------------------
    // Case arm / pattern lowering
    // -----------------------------------------------------------------------

    fn lower_case_arm(&mut self, arm: &CaseArm, scrut_ty: &MonoType) -> Option<MatchArm> {
        self.local_allocator.push_scope();
        let pattern = self.lower_pattern(&arm.pattern, Some(scrut_ty))?;
        let body = self.lower_expr(&arm.body)?;
        self.local_allocator.pop_scope();
        Some(MatchArm { pattern, body })
    }

    /// Lower a pattern, using `scrutinee_ty` (when known) to resolve variant TypeIds
    /// without an ambiguous name scan across all registered types.
    fn lower_pattern(&mut self, pattern: &Pattern, scrutinee_ty: Option<&MonoType>) -> Option<CorePattern> {
        match pattern {
            Pattern::Wildcard(_) => Some(CorePattern::Wildcard),

            Pattern::Ident(name, _) => {
                let local = self.local_allocator.alloc_and_bind(name.clone());
                Some(CorePattern::Var(local))
            }

            Pattern::Literal(lit, span) => Some(match lit {
                Literal::Int(n) => CorePattern::LitInt(*n),
                Literal::Bool(b) => CorePattern::LitBool(*b),
                Literal::String(s) => CorePattern::LitStr(s.clone()),
                Literal::Float(_) => {
                    self.errors.push(LowerError::UnsupportedFeature {
                        feature: "float pattern matching",
                        span: *span,
                    });
                    return None;
                }
            }),

            Pattern::Variant { name: variant_name, fields, span: pat_span } => {
                // Resolve TypeId from the scrutinee's declared type — avoids the
                // first-match scan that would pick the wrong type when variant names
                // collide across sum types (e.g., two types both have `None`).
                let (type_id, variant_idx) = match scrutinee_ty {
                    Some(MonoType::Named { type_id, .. }) => {
                        let type_id = *type_id;
                        match self.type_env.get_variant_index(type_id, variant_name) {
                            Some(idx) => (type_id, idx),
                            None => {
                                self.errors.push(LowerError::UnknownVariant {
                                    name: variant_name.to_string(),
                                    type_name: format!("Type#{}", type_id.0),
                                    span: *pat_span,
                                });
                                return None;
                            }
                        }
                    }
                    _ => {
                        // Fallback: scan all types (only reached if scrutinee type is unknown)
                        let mut found = None;
                        for i in 0..self.type_env.type_count() {
                            let tid = crate::types::ty::TypeId(i as u32);
                            if let Some(idx) = self.type_env.get_variant_index(tid, variant_name) {
                                found = Some((tid, idx));
                                break;
                            }
                        }
                        match found {
                            Some(pair) => pair,
                            None => {
                                self.errors.push(LowerError::UnknownVariant {
                                    name: variant_name.to_string(),
                                    type_name: "(unknown)".to_string(),
                                    span: *pat_span,
                                });
                                return None;
                            }
                        }
                    }
                };

                // Pass each field pattern the corresponding declared field type so
                // nested variant patterns (e.g. `.Some(.Ok(x))`) also resolve correctly.
                let field_types: Vec<MonoType> = self.type_env
                    .get_variants(type_id)
                    .and_then(|vs| vs.get(variant_idx))
                    .map(|v| v.fields.clone())
                    .unwrap_or_default();

                let mut lowered_fields = Vec::new();
                for (f, field_ty) in fields.iter().zip(&field_types) {
                    lowered_fields.push(self.lower_pattern(f, Some(field_ty))?);
                }
                // Safety net for fields beyond declared count (caught by type-checker)
                for f in fields.iter().skip(field_types.len()) {
                    lowered_fields.push(self.lower_pattern(f, None)?);
                }

                Some(CorePattern::Variant {
                    type_id,
                    variant: VariantId(variant_idx),
                    fields: lowered_fields,
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // String interpolation
    // -----------------------------------------------------------------------

    fn lower_string_interpolation(
        &mut self,
        parts: &[StringPart],
        span: Span,
    ) -> Option<CoreExprKind> {
        // Right-fold the parts into nested concat calls
        // "a${x}b" → concat("a", concat(to_string(x), "b"))
        let exprs: Vec<CoreExpr> = parts
            .iter()
            .map(|part| match part {
                StringPart::Literal(s) => Some(CoreExpr {
                    kind: CoreExprKind::LitStr(s.clone()),
                    ty: MonoType::String,
                    span,
                }),
                StringPart::Interpolation(e) => {
                    let inner = self.lower_expr(e)?;
                    let ty = inner.ty.clone();

                    let to_str_func_id = match &ty {
                        MonoType::String => return Some(inner),
                        MonoType::Int => prelude::INT_TO_STRING,
                        MonoType::Float => prelude::FLOAT_TO_STRING,
                        MonoType::Bool => prelude::BOOL_TO_STRING,
                        _ => {
                            self.errors.push(LowerError::UnsupportedFeature {
                                feature: "interpolation of non-primitive type",
                                span: e.span,
                            });
                            return None;
                        }
                    };

                    let func_expr = CoreExpr {
                        kind: CoreExprKind::GlobalFunc(to_str_func_id),
                        ty: MonoType::Function {
                            params: vec![ty],
                            ret: Box::new(MonoType::String),
                        },
                        span: e.span,
                    };

                    Some(CoreExpr {
                        kind: CoreExprKind::Call {
                            callee: Box::new(func_expr),
                            args: vec![inner],
                        },
                        ty: MonoType::String,
                        span: e.span,
                    })
                }
            })
            .collect::<Option<Vec<_>>>()?;

        if exprs.is_empty() {
            return Some(CoreExprKind::LitStr(String::new()));
        }

        // Right-fold with concat
        let result = exprs.into_iter().rev().reduce(|right, left| {
            let concat_func = CoreExpr {
                kind: CoreExprKind::GlobalFunc(prelude::STRING_CONCAT),
                ty: MonoType::Function {
                    params: vec![MonoType::String, MonoType::String],
                    ret: Box::new(MonoType::String),
                },
                span,
            };
            CoreExpr {
                kind: CoreExprKind::Call {
                    callee: Box::new(concat_func),
                    args: vec![left, right],
                },
                ty: MonoType::String,
                span,
            }
        })?;

        Some(result.kind)
    }

    // -----------------------------------------------------------------------
    // Collect expression desugaring
    // -----------------------------------------------------------------------

    fn lower_range_collect(
        &mut self,
        pattern: &Pattern,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        // collect x in range(n) { body }
        // →
        // Let(r, iter,
        //   Let(cur, r.start,
        //     Let(end, r.end,
        //       Let(step, r.step,
        //         Let(acc, [],
        //           Loop {
        //             If(cur >= end, Break(acc),
        //               Let(x, cur,
        //                 Let(_, cur += step, ← advance BEFORE body so continue skips append
        //                   Let(val, body,
        //                     Let(_, acc_assign,
        //                       Continue)))))
        //           })))))

        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;
        let range_named_ty = MonoType::named(RANGE_TYPE_ID);

        let r_tmp    = self.local_allocator.alloc_and_bind("__rc_r".to_string());
        let cur_tmp  = self.local_allocator.alloc_and_bind("__rc_cur".to_string());
        let end_tmp  = self.local_allocator.alloc_and_bind("__rc_end".to_string());
        let step_tmp = self.local_allocator.alloc_and_bind("__rc_step".to_string());
        let acc_local = self.local_allocator.alloc_and_bind("__rc_acc".to_string());

        // Field access: r.start (0), r.end (1), r.step (2)
        let start_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty.clone(), span: iter_span }),
                field: FieldId(0),
            },
            ty: MonoType::Int, span: iter_span,
        };
        let end_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty.clone(), span: iter_span }),
                field: FieldId(1),
            },
            ty: MonoType::Int, span: iter_span,
        };
        let step_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr { kind: CoreExprKind::Local(r_tmp), ty: range_named_ty, span: iter_span }),
                field: FieldId(2),
            },
            ty: MonoType::Int, span: iter_span,
        };

        let cur_expr  = CoreExpr { kind: CoreExprKind::Local(cur_tmp),  ty: MonoType::Int,     span: iter_span };
        let end_expr  = CoreExpr { kind: CoreExprKind::Local(end_tmp),  ty: MonoType::Int,     span: iter_span };
        let step_expr = CoreExpr { kind: CoreExprKind::Local(step_tmp), ty: MonoType::Int,     span: iter_span };
        let acc_expr  = CoreExpr { kind: CoreExprKind::Local(acc_local), ty: result_ty.clone(), span: iter_span };

        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };

        let body_val_local = self.local_allocator.alloc_and_bind("__rc_val".to_string());
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();

        self.local_allocator.pop_scope();

        // cur = cur + step
        let cur_plus_step = CoreExpr {
            kind: CoreExprKind::BinOp { op: BinOp::Add, left: Box::new(cur_expr.clone()), right: Box::new(step_expr) },
            ty: MonoType::Int, span: iter_span,
        };
        let cur_inc = CoreExpr {
            kind: CoreExprKind::Assign { local: cur_tmp, value: Box::new(cur_plus_step) },
            ty: MonoType::Void, span: iter_span,
        };

        // acc = array_append(acc, val)
        let append_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::ARRAY_APPEND),
            ty: MonoType::Function { params: vec![result_ty.clone(), body_ty.clone()], ret: Box::new(result_ty.clone()) },
            span: iter_span,
        };
        let acc_new_val = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(append_func),
                args: vec![
                    acc_expr.clone(),
                    CoreExpr { kind: CoreExprKind::Local(body_val_local), ty: body_ty, span: iter_span },
                ],
            },
            ty: result_ty.clone(), span: iter_span,
        };
        let acc_assign = CoreExpr {
            kind: CoreExprKind::Assign { local: acc_local, value: Box::new(acc_new_val) },
            ty: MonoType::Void, span: iter_span,
        };

        let continue_expr = CoreExpr { kind: CoreExprKind::Continue, ty: MonoType::Void, span: iter_span };

        // Build from inside out: Let(_, acc_assign, Continue)
        let tmp1 = self.local_allocator.alloc();
        let after_append = CoreExpr {
            kind: CoreExprKind::Let { local: tmp1, value: Box::new(acc_assign), body: Box::new(continue_expr) },
            ty: MonoType::Void, span: iter_span,
        };
        // Let(val, body, after_append)
        let with_val = CoreExpr {
            kind: CoreExprKind::Let { local: body_val_local, value: Box::new(body_expr), body: Box::new(after_append) },
            ty: MonoType::Void, span: iter_span,
        };
        // Let(_, cur_inc, with_val) — increment BEFORE body so continue advances
        let tmp2 = self.local_allocator.alloc();
        let with_inc = CoreExpr {
            kind: CoreExprKind::Let { local: tmp2, value: Box::new(cur_inc), body: Box::new(with_val) },
            ty: MonoType::Void, span: iter_span,
        };
        // Let(x, cur, with_inc)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let { local: elem_local, value: Box::new(cur_expr.clone()), body: Box::new(with_inc) },
            ty: MonoType::Void, span: iter_span,
        };

        // Break condition: cur >= end → Break(acc)
        let cond_ge = CoreExpr {
            kind: CoreExprKind::BinOp { op: BinOp::Ge, left: Box::new(cur_expr), right: Box::new(end_expr) },
            ty: MonoType::Bool, span: iter_span,
        };
        let break_acc = CoreExpr {
            kind: CoreExprKind::Break { value: Some(Box::new(acc_expr)) },
            ty: result_ty.clone(), span: iter_span,
        };
        let loop_if = CoreExpr {
            kind: CoreExprKind::If { cond: Box::new(cond_ge), then_branch: Box::new(break_acc), else_branch: Box::new(with_elem) },
            ty: result_ty.clone(), span: iter_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop { body: Box::new(loop_if) },
            ty: result_ty.clone(), span,
        };

        let empty_arr = CoreExpr { kind: CoreExprKind::ArrayLit { elements: vec![] }, ty: result_ty.clone(), span: iter_span };

        // Wrap: Let(r, iter, Let(cur, start_get, Let(end, end_get, Let(step, step_get, Let(acc, [], loop)))))
        Some(CoreExprKind::Let {
            local: r_tmp,
            value: Box::new(iter_expr),
            body: Box::new(CoreExpr {
                kind: CoreExprKind::Let {
                    local: cur_tmp,
                    value: Box::new(start_get),
                    body: Box::new(CoreExpr {
                        kind: CoreExprKind::Let {
                            local: end_tmp,
                            value: Box::new(end_get),
                            body: Box::new(CoreExpr {
                                kind: CoreExprKind::Let {
                                    local: step_tmp,
                                    value: Box::new(step_get),
                                    body: Box::new(CoreExpr {
                                        kind: CoreExprKind::Let {
                                            local: acc_local,
                                            value: Box::new(empty_arr),
                                            body: Box::new(loop_expr),
                                        },
                                        ty: result_ty.clone(), span,
                                    }),
                                },
                                ty: result_ty.clone(), span,
                            }),
                        },
                        ty: result_ty.clone(), span,
                    }),
                },
                ty: result_ty.clone(), span,
            }),
        })
    }

    fn lower_collect(
        &mut self,
        pattern: &Pattern,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let iter_ty = self.type_map.get_expr_type(iter.id).cloned();
        if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == RANGE_TYPE_ID) {
            return self.lower_range_collect(pattern, iter, body, result_ty, span);
        }

        // collect x in arr { body }
        // →
        // Let(arr_tmp, iter,
        //   Let(acc, [],
        //     Let(len, array_len(arr_tmp),
        //       Let(idx, 0,
        //         Loop {
        //           If(idx >= len, Break(acc),
        //             Let(x, arr_tmp[idx],
        //               Let(val, body,
        //                 Let(acc, append(acc, val),
        //                   Let(idx, idx+1, Continue)))))
        //         }))))

        let elem_ty = match iter_ty {
            Some(MonoType::Array(inner)) => *inner,
            _ => MonoType::Void,
        };

        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let arr_tmp = self.local_allocator.alloc_and_bind("__c_arr".to_string());
        let acc_local = self.local_allocator.alloc_and_bind("__c_acc".to_string());
        let len_local = self.local_allocator.alloc_and_bind("__c_len".to_string());
        let idx_local = self.local_allocator.alloc_and_bind("__c_idx".to_string());

        let arr_local_expr = CoreExpr {
            kind: CoreExprKind::Local(arr_tmp),
            ty: iter_expr.ty.clone(),
            span: iter_span,
        };

        // len = array_len(arr_tmp)
        let len_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::ARRAY_LEN),
            ty: MonoType::Function {
                params: vec![iter_expr.ty.clone()],
                ret: Box::new(MonoType::Int),
            },
            span: iter_span,
        };
        let len_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(len_func),
                args: vec![arr_local_expr.clone()],
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::Local(len_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        let acc_expr = CoreExpr {
            kind: CoreExprKind::Local(acc_local),
            ty: result_ty.clone(),
            span: iter_span,
        };

        // Loop body: bind element, lower body, append, inc idx
        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };

        let elem_value = CoreExpr {
            kind: CoreExprKind::Index {
                base: Box::new(arr_local_expr),
                index: Box::new(idx_expr.clone()),
            },
            ty: elem_ty,
            span: iter_span,
        };

        let body_val_local = self.local_allocator.alloc_and_bind("__c_val".to_string());
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();

        self.local_allocator.pop_scope();

        // acc = array_append(acc, val)  [mutation via Assign]
        let append_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::ARRAY_APPEND),
            ty: MonoType::Function {
                params: vec![result_ty.clone(), body_ty.clone()],
                ret: Box::new(result_ty.clone()),
            },
            span: iter_span,
        };
        let acc_new_val = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(append_func),
                args: vec![
                    acc_expr.clone(),
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: iter_span,
                    },
                ],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let acc_assign = CoreExpr {
            kind: CoreExprKind::Assign {
                local: acc_local,
                value: Box::new(acc_new_val),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // idx = idx + 1  [mutation via Assign]
        let one = CoreExpr {
            kind: CoreExprKind::LitInt(1),
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_plus_one = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(idx_expr.clone()),
                right: Box::new(one),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_assign = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_local,
                value: Box::new(idx_plus_one),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };

        // Build innermost: Assign(acc, append(acc, val)); Continue
        let tmp1 = self.local_allocator.alloc();
        let tail = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp1,
                value: Box::new(acc_assign),
                body: Box::new(continue_expr),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(val, body, tail)
        // If body produces Continue (e.g. `else { continue }`), the signal
        // propagates before acc is appended, skipping this element.
        let with_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(tail),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Assign(idx, idx+1); with_val
        // Increment BEFORE evaluating the body so that if body hits `continue`
        // the index is already advanced to the next element.
        let tmp2 = self.local_allocator.alloc();
        let with_idx = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp2,
                value: Box::new(idx_assign),
                body: Box::new(with_val),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(elem, arr[idx], with_idx)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(elem_value),
                body: Box::new(with_idx),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Break condition: idx >= len → Break(acc)
        let cond_ge = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(idx_expr),
                right: Box::new(len_expr),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let break_acc = CoreExpr {
            kind: CoreExprKind::Break {
                value: Some(Box::new(acc_expr)),
            },
            ty: result_ty.clone(),
            span: iter_span,
        };

        let loop_if = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(cond_ge),
                then_branch: Box::new(break_acc),
                else_branch: Box::new(with_elem),
            },
            ty: result_ty.clone(),
            span: iter_span,
        };

        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_if),
            },
            ty: result_ty.clone(),
            span,
        };

        // Wrap: Let(arr_tmp, iter, Let(acc, [], Let(len, len_call, Let(idx, 0, loop))))
        let zero = CoreExpr {
            kind: CoreExprKind::LitInt(0),
            ty: MonoType::Int,
            span: iter_span,
        };
        let empty_arr = CoreExpr {
            kind: CoreExprKind::ArrayLit { elements: vec![] },
            ty: result_ty.clone(),
            span: iter_span,
        };

        let with_idx = CoreExpr {
            kind: CoreExprKind::Let {
                local: idx_local,
                value: Box::new(zero),
                body: Box::new(loop_expr),
            },
            ty: result_ty.clone(),
            span,
        };
        let with_len = CoreExpr {
            kind: CoreExprKind::Let {
                local: len_local,
                value: Box::new(len_call),
                body: Box::new(with_idx),
            },
            ty: result_ty.clone(),
            span,
        };
        let with_acc_init = CoreExpr {
            kind: CoreExprKind::Let {
                local: acc_local,
                value: Box::new(empty_arr),
                body: Box::new(with_len),
            },
            ty: result_ty.clone(),
            span,
        };

        Some(CoreExprKind::Let {
            local: arr_tmp,
            value: Box::new(iter_expr),
            body: Box::new(with_acc_init),
        })
    }

    // -----------------------------------------------------------------------
    // Lvalue chain desugaring
    // -----------------------------------------------------------------------

    /// Lower a lvalue assignment chain, returning (root_local, new_value_for_root).
    ///
    /// Recursion builds up the update from innermost to outermost:
    ///   `a.b.c = x`  → (a_local, RecordUpdate(Local(a), b_id, RecordUpdate(RecordGet(Local(a), b_id), c_id, x)))
    ///   `arr[i] = x` → (arr_local, Call(ARRAY_SET, [Local(arr), lower(i), x]))
    ///   `m[k]   = x` → (m_local,   Call(DICT_SET,  [Local(m),   lower(k), x]))
    ///
    /// Returns None and records an error if the lvalue is invalid or unresolvable.
    fn lower_lvalue_chain(&mut self, lhs: &Expr, rhs_expr: CoreExpr) -> Option<(LocalId, CoreExpr)> {
        let span = lhs.span;
        match &lhs.kind {
            // Base case: simple identifier
            ExprKind::Ident(name) => {
                let local = match self.local_allocator.lookup(name) {
                    Some(l) => l,
                    None => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("unresolved lvalue name '{}' during lowering", name),
                            span,
                        });
                        return None;
                    }
                };
                Some((local, rhs_expr))
            }

            // Field write: base.field = rhs  →  RecordUpdate(lower(base), field_id, rhs)
            ExprKind::FieldAccess { base, field } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let type_id = match &base_ty {
                    Some(MonoType::Named { type_id, .. }) => *type_id,
                    _ => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("field write on non-record type {:?}", base_ty),
                            span,
                        });
                        return None;
                    }
                };
                let field_idx = match self.type_env.get_field_index(type_id, field) {
                    Some(i) => i,
                    None => {
                        self.errors.push(LowerError::UnknownField {
                            field: field.clone(),
                            type_name: format!("Type#{}", type_id.0),
                            span,
                        });
                        return None;
                    }
                };
                let base_core = self.lower_expr(base)?;
                let base_ty_clone = base_core.ty.clone();
                let update = CoreExpr {
                    kind: CoreExprKind::RecordUpdate {
                        base: Box::new(base_core),
                        field: FieldId(field_idx),
                        value: Box::new(rhs_expr),
                    },
                    ty: base_ty_clone,
                    span,
                };
                self.lower_lvalue_chain(base, update)
            }

            // Index write: base[index] = rhs  →  Call(ARRAY_SET or DICT_SET, [lower(base), lower(index), rhs])
            ExprKind::Index { base, index } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let func_id = match &base_ty {
                    Some(MonoType::Array(_)) => prelude::ARRAY_SET,
                    Some(MonoType::Dict(_, _)) => prelude::DICT_SET,
                    _ => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("index write on non-array/dict type {:?}", base_ty),
                            span,
                        });
                        return None;
                    }
                };
                let base_core = self.lower_expr(base)?;
                let idx_core = self.lower_expr(index)?;
                let base_ty_clone = base_core.ty.clone();
                let func_ty = MonoType::Function {
                    params: vec![base_core.ty.clone(), idx_core.ty.clone(), rhs_expr.ty.clone()],
                    ret: Box::new(base_ty_clone.clone()),
                };
                let func_expr = CoreExpr {
                    kind: CoreExprKind::GlobalFunc(func_id),
                    ty: func_ty,
                    span,
                };
                let update = CoreExpr {
                    kind: CoreExprKind::Call {
                        callee: Box::new(func_expr),
                        args: vec![base_core, idx_core, rhs_expr],
                    },
                    ty: base_ty_clone,
                    span,
                };
                self.lower_lvalue_chain(base, update)
            }

            _ => {
                self.errors.push(LowerError::InternalError {
                    message: "invalid lvalue in assignment".to_string(),
                    span,
                });
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// If `expr` is a simple assignment `name = value`, return `(name, value)`.
fn extract_simple_assign(expr: &Expr) -> Option<(&str, &Expr)> {
    if let ExprKind::Binary { op: BinOp::Assign, left, right } = &expr.kind {
        if let ExprKind::Ident(name) = &left.kind {
            return Some((name.as_str(), right));
        }
    }
    None
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Let { span, .. } => *span,
        Stmt::For { span, .. } => *span,
        Stmt::ForCond { span, .. } => *span,
        Stmt::Expr(e) => e.span,
        Stmt::Break { span, .. } => *span,
        Stmt::Continue { span } => *span,
        Stmt::Return { span, .. } => *span,
    }
}

/// Walk a Core IR expression tree and collect all `Local(id)` references
/// where `id` is NOT in the `exclude` set. Deduplicates while preserving
/// first-occurrence order.
fn collect_local_refs(expr: &CoreExpr, exclude: &HashSet<LocalId>) -> Vec<LocalId> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    collect_local_refs_inner(expr, exclude, &mut seen, &mut result);
    result
}

fn collect_local_refs_inner(
    expr: &CoreExpr,
    exclude: &HashSet<LocalId>,
    seen: &mut HashSet<LocalId>,
    result: &mut Vec<LocalId>,
) {
    use CoreExprKind::*;
    match &expr.kind {
        Local(id) => {
            if !exclude.contains(id) && seen.insert(*id) {
                result.push(*id);
            }
        }
        Let { local: _, value, body } => {
            collect_local_refs_inner(value, exclude, seen, result);
            collect_local_refs_inner(body, exclude, seen, result);
        }
        Assign { local: _, value } => {
            collect_local_refs_inner(value, exclude, seen, result);
        }
        BinOp { left, right, .. } => {
            collect_local_refs_inner(left, exclude, seen, result);
            collect_local_refs_inner(right, exclude, seen, result);
        }
        UnOp { expr: inner, .. } => {
            collect_local_refs_inner(inner, exclude, seen, result);
        }
        Call { callee, args } => {
            collect_local_refs_inner(callee, exclude, seen, result);
            for a in args {
                collect_local_refs_inner(a, exclude, seen, result);
            }
        }
        If { cond, then_branch, else_branch } => {
            collect_local_refs_inner(cond, exclude, seen, result);
            collect_local_refs_inner(then_branch, exclude, seen, result);
            collect_local_refs_inner(else_branch, exclude, seen, result);
        }
        Match { scrutinee, arms } => {
            collect_local_refs_inner(scrutinee, exclude, seen, result);
            for arm in arms {
                collect_local_refs_inner(&arm.body, exclude, seen, result);
            }
        }
        Loop { body } => {
            collect_local_refs_inner(body, exclude, seen, result);
        }
        Break { value: Some(v) } => {
            collect_local_refs_inner(v, exclude, seen, result);
        }
        Return { value: Some(v) } => {
            collect_local_refs_inner(v, exclude, seen, result);
        }
        Record { fields, .. } => {
            for (_, f) in fields {
                collect_local_refs_inner(f, exclude, seen, result);
            }
        }
        RecordGet { target, .. } => {
            collect_local_refs_inner(target, exclude, seen, result);
        }
        RecordUpdate { base, value, .. } => {
            collect_local_refs_inner(base, exclude, seen, result);
            collect_local_refs_inner(value, exclude, seen, result);
        }
        Variant { args, .. } => {
            for a in args {
                collect_local_refs_inner(a, exclude, seen, result);
            }
        }
        ArrayLit { elements } => {
            for e in elements {
                collect_local_refs_inner(e, exclude, seen, result);
            }
        }
        Index { base, index } => {
            collect_local_refs_inner(base, exclude, seen, result);
            collect_local_refs_inner(index, exclude, seen, result);
        }
        // MakeClosure's free_vars are LocalIds that must be visible to any wrapping lambda
        MakeClosure { free_vars, .. } => {
            for id in free_vars {
                if !exclude.contains(id) && seen.insert(*id) {
                    result.push(*id);
                }
            }
        }
        // These don't contain Local refs
        LitInt(_) | LitFloat(_) | LitBool(_) | LitStr(_) | LitVoid
        | GlobalFunc(_) | Break { value: None }
        | Return { value: None } | Continue => {}
    }
}
