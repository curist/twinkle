use std::collections::HashMap;

use crate::syntax::ast::{
    BinOp, Block, CaseArm, Expr, ExprKind, FunctionDecl, Item, Literal, Pattern, SourceFile,
    Stmt, StringPart,
};
use crate::syntax::span::Span;
use crate::types::env::TypeEnv;
use crate::types::ty::MonoType;
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

    // User functions start here
    pub const USER_FUNC_START: u32 = 15;
}

// ---------------------------------------------------------------------------
// Lowerer
// ---------------------------------------------------------------------------

pub struct Lowerer {
    type_map: TypeMap,
    type_env: TypeEnv,
    /// Map from function name to its assigned FuncId
    func_table: HashMap<String, FuncId>,
    errors: Vec<LowerError>,
    /// Per-function local variable allocator (reset for each function)
    local_allocator: LocalAllocator,
}

impl Lowerer {
    pub fn new(type_map: TypeMap, type_env: TypeEnv) -> Self {
        let mut func_table = HashMap::new();

        // Register prelude functions
        func_table.insert("print".to_string(), prelude::PRINT);
        func_table.insert("println".to_string(), prelude::PRINTLN);
        func_table.insert("error".to_string(), prelude::ERROR);

        // len is polymorphic and handled specially in lower_expr_call

        Self {
            type_map,
            type_env,
            func_table,
            errors: Vec::new(),
            local_allocator: LocalAllocator::new(),
        }
    }

    /// Lower a complete source file to Core IR
    pub fn lower_module(mut self, ast: &SourceFile) -> Result<CoreModule, Vec<LowerError>> {
        // First pass: assign FuncIds to all user functions (source order)
        let mut next_func_id = prelude::USER_FUNC_START;
        for item in &ast.items {
            if let Item::Function(decl) = item {
                let func_id = FuncId(next_func_id);
                next_func_id += 1;
                self.func_table.insert(decl.name.clone(), func_id);
            }
        }

        // Second pass: lower each function
        let mut functions = Vec::new();
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if let Some(func_def) = self.lower_function(decl) {
                    functions.push(func_def);
                }
            }
        }

        let main_func_id = self.func_table.get("main").copied();

        if self.errors.is_empty() {
            Ok(CoreModule {
                functions,
                type_env: self.type_env,
                main_func_id,
            })
        } else {
            Err(self.errors)
        }
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

                // Optionally bind user-visible index
                let loop_tail = if let Some(user_idx) = idx_user {
                    CoreExpr {
                        kind: CoreExprKind::Let {
                            local: user_idx,
                            value: Box::new(idx_local_expr.clone()),
                            body: Box::new(idx_rebind),
                        },
                        ty: MonoType::Void,
                        span: iter_span,
                    }
                } else {
                    idx_rebind
                };

                // body_then_tail: the body followed by loop_tail
                let body_then_tail = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(body_expr),
                        body: Box::new(loop_tail),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Let(elem, elem_value, body_then_tail)
                let elem_let = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: elem_local,
                        value: Box::new(elem_value),
                        body: Box::new(body_then_tail),
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
                // Handle rebinding assignments: produce Void
                if matches!(op, BinOp::Assign) {
                    // These are handled at the Stmt::Expr level as rebindings.
                    // If they appear as expressions (not statements), produce Void.
                    let _ = self.lower_expr(right)?;
                    return Some(CoreExprKind::LitVoid);
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
                let base_expr = self.lower_expr(base)?;
                let index_expr = self.lower_expr(index)?;
                Some(CoreExprKind::Index {
                    base: Box::new(base_expr),
                    index: Box::new(index_expr),
                })
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
                let mut lowered_arms = Vec::new();
                for arm in arms {
                    if let Some(la) = self.lower_case_arm(arm, &scrutinee.span) {
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

            // --- Lambda (non-capturing only, deferred) ---
            ExprKind::Function(_) => {
                self.errors.push(LowerError::UnsupportedFeature {
                    feature: "lambda expressions",
                    span,
                });
                None
            }

            // --- Try (deferred until generics) ---
            ExprKind::Try { .. } => {
                self.errors.push(LowerError::UnsupportedFeature {
                    feature: "try expressions",
                    span,
                });
                None
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

        // Method call via FieldAccess: x.method(args)
        // e.g. arr.append(v) → Call(GlobalFunc(ARRAY_APPEND), [arr, v])
        if let ExprKind::FieldAccess { base, field } = &callee.kind {
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

    fn lower_case_arm(&mut self, arm: &CaseArm, scrut_span: &Span) -> Option<MatchArm> {
        self.local_allocator.push_scope();
        let pattern = self.lower_pattern(&arm.pattern, scrut_span)?;
        let body = self.lower_expr(&arm.body)?;
        self.local_allocator.pop_scope();
        Some(MatchArm { pattern, body })
    }

    fn lower_pattern(&mut self, pattern: &Pattern, span: &Span) -> Option<CorePattern> {
        match pattern {
            Pattern::Wildcard(_) => Some(CorePattern::Wildcard),

            Pattern::Ident(name, _) => {
                let local = self.local_allocator.alloc_and_bind(name.clone());
                Some(CorePattern::Var(local))
            }

            Pattern::Literal(lit, _) => Some(match lit {
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
                // We need the type_id from context. For now, scan all types to find the variant.
                // A more robust approach would pass the scrutinee type through.
                let (type_id, variant_idx) = self.resolve_variant_in_any_type(variant_name, *pat_span)?;

                let mut lowered_fields = Vec::new();
                for f in fields {
                    lowered_fields.push(self.lower_pattern(f, pat_span)?);
                }

                Some(CorePattern::Variant {
                    type_id,
                    variant: VariantId(variant_idx),
                    fields: lowered_fields,
                })
            }
        }
    }

    /// Find a variant by name across all known sum types.
    /// Used when we don't have explicit type context in a pattern.
    fn resolve_variant_in_any_type(
        &mut self,
        variant_name: &str,
        span: Span,
    ) -> Option<(crate::types::ty::TypeId, usize)> {
        // Iterate over all registered types
        for i in 0..self.type_env.type_count() {
            let type_id = crate::types::ty::TypeId(i as u32);
            if let Some(idx) = self.type_env.get_variant_index(type_id, variant_name) {
                return Some((type_id, idx));
            }
        }
        self.errors.push(LowerError::UnknownVariant {
            name: variant_name.to_string(),
            type_name: "(unknown)".to_string(),
            span,
        });
        None
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

    fn lower_collect(
        &mut self,
        pattern: &Pattern,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
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

        let elem_ty = match self.type_map.get_expr_type(iter.id).cloned() {
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

        // Build from innermost: Assign(idx, idx+1); Continue
        let tmp1 = self.local_allocator.alloc();
        let tail = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp1,
                value: Box::new(idx_assign),
                body: Box::new(continue_expr),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Assign(acc, append(acc, val)); tail
        let tmp2 = self.local_allocator.alloc();
        let with_acc = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp2,
                value: Box::new(acc_assign),
                body: Box::new(tail),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(val, body, with_acc)
        let with_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(with_acc),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(elem, arr[idx], with_val)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(elem_value),
                body: Box::new(with_val),
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
