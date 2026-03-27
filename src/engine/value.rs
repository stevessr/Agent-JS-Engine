use crate::engine::env::Environment;
use crate::engine::interpreter::RuntimeError;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct FunctionValue {
    pub id: usize,
    pub env: Rc<RefCell<Environment>>,
}

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    Object(Rc<RefCell<HashMap<String, JsValue>>>),
    Function(Rc<FunctionValue>),
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(left), JsValue::Boolean(right)) => left == right,
            (JsValue::Number(left), JsValue::Number(right)) => left == right,
            (JsValue::String(left), JsValue::String(right)) => left == right,
            (JsValue::Array(left), JsValue::Array(right)) => {
                let left = left.borrow();
                let right = right.borrow();
                *left == *right
            }
            (JsValue::Object(left), JsValue::Object(right)) => {
                let left = left.borrow();
                let right = right.borrow();
                *left == *right
            }
            (JsValue::Function(left), JsValue::Function(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl JsValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Boolean(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::String(s) => !s.is_empty(),
            JsValue::Array(_) | JsValue::Object(_) | JsValue::Function(_) => true,
        }
    }

    pub fn type_of(&self) -> String {
        match self {
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Null => "object".to_string(),
            JsValue::Boolean(_) => "boolean".to_string(),
            JsValue::Number(_) => "number".to_string(),
            JsValue::String(_) => "string".to_string(),
            JsValue::Array(_) | JsValue::Object(_) => "object".to_string(),
            JsValue::Function(_) => "function".to_string(),
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            JsValue::Number(n) => *n,
            JsValue::String(s) => s.parse().unwrap_or(f64::NAN),
            JsValue::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            JsValue::Null => 0.0,
            JsValue::Undefined => f64::NAN,
            JsValue::Array(_) | JsValue::Object(_) | JsValue::Function(_) => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => n.to_string(),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Array(values) => values
                .borrow()
                .iter()
                .map(|value| match value {
                    JsValue::Undefined | JsValue::Null => String::new(),
                    _ => value.as_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            JsValue::Object(_) => "[object Object]".to_string(),
            JsValue::Function(_) => "function () { [native code] }".to_string(),
        }
    }

    pub fn add(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), _) => {
                let s2 = other.as_string();
                if s1.len() + s2.len() > 500_000 {
                    return Err(RuntimeError::ReferenceError("OOM Limit".into()));
                }
                Ok(JsValue::String(s1.clone() + &s2))
            }
            (_, JsValue::String(s2)) => {
                let s1 = self.as_string();
                if s1.len() + s2.len() > 500_000 {
                    return Err(RuntimeError::ReferenceError("OOM Limit".into()));
                }
                Ok(JsValue::String(s1 + s2))
            }
            _ => Ok(JsValue::Number(self.as_number() + other.as_number())),
        }
    }

    pub fn sub(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        Ok(JsValue::Number(self.as_number() - other.as_number()))
    }

    pub fn mul(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        Ok(JsValue::Number(self.as_number() * other.as_number()))
    }

    pub fn div(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        Ok(JsValue::Number(self.as_number() / other.as_number()))
    }

    pub fn lt(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 < s2)),
            _ => Ok(JsValue::Boolean(self.as_number() < other.as_number())),
        }
    }

    pub fn le(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 <= s2)),
            _ => Ok(JsValue::Boolean(self.as_number() <= other.as_number())),
        }
    }

    pub fn gt(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 > s2)),
            _ => Ok(JsValue::Boolean(self.as_number() > other.as_number())),
        }
    }

    pub fn ge(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 >= s2)),
            _ => Ok(JsValue::Boolean(self.as_number() >= other.as_number())),
        }
    }
}
