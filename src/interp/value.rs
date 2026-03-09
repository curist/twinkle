use crate::ir::core::{FuncId, LocalId};
use crate::types::ty::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Iterator state: a seed value plus a step closure.
/// The step closure takes the seed and returns an UnfoldStep<T,S> variant.
#[derive(Debug, Clone, PartialEq)]
pub struct IteratorState {
    pub seed: Box<Value>,
    pub step: Box<Value>, // must be a Closure
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Byte(u8),
    Str(String),
    Vec(std::vec::Vec<Value>),
    /// Linear-scan dict; key equality via PartialEq
    Dict(Vec<(Value, Value)>),
    /// Record fields in FieldId order (index = FieldId.0)
    Record(TypeId, Vec<Value>),
    /// (type_id, variant_index, payload)
    Variant(TypeId, usize, Vec<Value>),
    /// Closure: func_id + captured free-variable bindings
    Closure(FuncId, HashMap<LocalId, Value>),
    /// Mutable cell (interior mutability via Rc<RefCell<...>>)
    Cell(Rc<RefCell<Value>>),
    /// Lazy iterator: seed + step closure (persistent/value semantics)
    Iterator(Rc<IteratorState>),
    Void,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                // Format without unnecessary trailing zeros but always show decimal
                let s = format!("{}", n);
                if s.contains('.') {
                    write!(f, "{}", s)
                } else {
                    write!(f, "{}.0", s)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Byte(b) => write!(f, "{}", b),
            Value::Str(s) => write!(f, "{}", s),
            Value::Void => write!(f, "()"),
            Value::Vec(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            Value::Dict(pairs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Record(_, fields) => {
                write!(f, ".{{")?;
                for (i, v) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Value::Variant(_, idx, args) => {
                write!(f, "Variant#{}", idx)?;
                if !args.is_empty() {
                    write!(f, "(")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", a)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            Value::Closure(id, _) => write!(f, "<closure FuncId({})>", id.0),
            Value::Cell(inner) => write!(f, "Cell({})", inner.borrow()),
            Value::Iterator(_) => write!(f, "<iterator>"),
        }
    }
}

impl Value {
    /// Deep-clone this value, copying the contents of any `Cell` rather than
    /// sharing the `Rc<RefCell<...>>` pointer.  All other container types
    /// (`Arr`, `Dict`, `Record`, `Variant`, `Closure`) are recursed so that
    /// nested `Cell` values inside them are also copied.
    ///
    /// Used by the interpreter when capturing a frame snapshot for `defer`
    /// to implement capture-by-value semantics.
    pub fn deep_clone(&self) -> Value {
        match self {
            Value::Cell(rc) => Value::Cell(Rc::new(RefCell::new(rc.borrow().deep_clone()))),
            Value::Vec(elems) => Value::Vec(elems.iter().map(|v| v.deep_clone()).collect()),
            Value::Dict(kvs) => Value::Dict(
                kvs.iter()
                    .map(|(k, v)| (k.deep_clone(), v.deep_clone()))
                    .collect(),
            ),
            Value::Record(tid, fields) => {
                Value::Record(*tid, fields.iter().map(|v| v.deep_clone()).collect())
            }
            Value::Variant(tid, vidx, args) => {
                Value::Variant(*tid, *vidx, args.iter().map(|v| v.deep_clone()).collect())
            }
            Value::Closure(fid, free_vars) => Value::Closure(
                *fid,
                free_vars
                    .iter()
                    .map(|(k, v)| (*k, v.deep_clone()))
                    .collect(),
            ),
            // Primitives and Iterator are value types; Clone is sufficient.
            other => other.clone(),
        }
    }
}
