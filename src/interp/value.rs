use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crate::ir::core::{FuncId, LocalId};
use crate::types::ty::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Arr(Vec<Value>),
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
            Value::Str(s) => write!(f, "{}", s),
            Value::Void => write!(f, "()"),
            Value::Arr(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            Value::Dict(pairs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Record(_, fields) => {
                write!(f, ".{{")?;
                for (i, v) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Value::Variant(_, idx, args) => {
                write!(f, "Variant#{}", idx)?;
                if !args.is_empty() {
                    write!(f, "(")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", a)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            Value::Closure(id, _) => write!(f, "<closure FuncId({})>", id.0),
            Value::Cell(inner) => write!(f, "Cell({})", inner.borrow()),
        }
    }
}
