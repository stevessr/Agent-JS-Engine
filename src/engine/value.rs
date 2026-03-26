use crate::engine::interpreter::RuntimeError;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    Object(Rc<RefCell<HashMap<String, JsValue>>>),
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
            (JsValue::Number(a), JsValue::Number(b)) => a == b,
            (JsValue::String(a), JsValue::String(b)) => a == b,
            // Objects and arrays use reference equality (JS semantics)
            (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
            (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
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
            JsValue::Array(_) | JsValue::Object(_) => true,
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
            JsValue::Array(arr) => {
                let arr = arr.borrow();
                match arr.len() {
                    0 => 0.0,
                    1 => arr[0].as_number(),
                    _ => f64::NAN,
                }
            }
            JsValue::Object(_) => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 && !n.is_nan() && !n.is_infinite() {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Array(arr) => {
                let arr = arr.borrow();
                arr.iter()
                    .map(|v| match v {
                        JsValue::Null | JsValue::Undefined => String::new(),
                        _ => v.as_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            }
            JsValue::Object(_) => "[object Object]".to_string(),
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
            (JsValue::Array(_), _)
            | (_, JsValue::Array(_))
            | (JsValue::Object(_), _)
            | (_, JsValue::Object(_)) => {
                let s1 = self.as_string();
                let s2 = other.as_string();
                if s1.len() + s2.len() > 500_000 {
                    return Err(RuntimeError::ReferenceError("OOM Limit".into()));
                }
                Ok(JsValue::String(s1 + &s2))
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
