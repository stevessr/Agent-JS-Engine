use crate::engine::env::Environment;
use crate::engine::interpreter::RuntimeError;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct FunctionValue {
    pub id: usize,
    pub env: Rc<RefCell<Environment>>,
    pub prototype: JsValue,
    pub properties: JsObjectMap,
    pub super_binding: Option<JsValue>,
    pub is_class_constructor: bool,
    pub is_derived_constructor: bool,
}

/// A native (Rust) built-in function.
pub type NativeFn = fn(&JsValue, &[JsValue]) -> Result<JsValue, RuntimeError>;

#[derive(Clone)]
pub struct NativeFunction {
    pub name: &'static str,
    pub func: NativeFn,
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[native function {}]", self.name)
    }
}

pub type JsObjectMap = Rc<RefCell<HashMap<String, JsValue>>>;

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    Object(JsObjectMap),
    Function(Rc<FunctionValue>),
    NativeFunction(Rc<NativeFunction>),
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(l), JsValue::Boolean(r)) => l == r,
            (JsValue::Number(l), JsValue::Number(r)) => l == r,
            (JsValue::String(l), JsValue::String(r)) => l == r,
            (JsValue::Array(l), JsValue::Array(r)) => Rc::ptr_eq(l, r),
            (JsValue::Object(l), JsValue::Object(r)) => Rc::ptr_eq(l, r),
            (JsValue::Function(l), JsValue::Function(r)) => Rc::ptr_eq(l, r),
            (JsValue::NativeFunction(l), JsValue::NativeFunction(r)) => Rc::ptr_eq(l, r),
            _ => false,
        }
    }
}

pub fn new_object_map() -> JsObjectMap {
    Rc::new(RefCell::new(HashMap::new()))
}

pub fn object_with_proto(proto: JsValue) -> JsValue {
    let map = new_object_map();
    map.borrow_mut().insert("__proto__".to_string(), proto);
    JsValue::Object(map)
}

/// Create a plain JS object from key-value pairs.
pub fn make_object(pairs: impl IntoIterator<Item = (&'static str, JsValue)>) -> JsValue {
    let map = new_object_map();
    for (k, v) in pairs {
        map.borrow_mut().insert(k.to_string(), v);
    }
    JsValue::Object(map)
}

/// Create a JS Error object with `name` and `message`.
pub fn make_error(name: &str, message: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert("name".to_string(), JsValue::String(name.to_string()));
    map.insert("message".to_string(), JsValue::String(message.to_string()));
    map.insert(
        "stack".to_string(),
        JsValue::String(format!("{name}: {message}")),
    );
    JsValue::Object(Rc::new(RefCell::new(map)))
}

/// Wrap a native function as a JsValue.
pub fn native_fn(name: &'static str, func: NativeFn) -> JsValue {
    JsValue::NativeFunction(Rc::new(NativeFunction { name, func }))
}

impl JsValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Boolean(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::String(s) => !s.is_empty(),
            JsValue::Array(_)
            | JsValue::Object(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_) => true,
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
            JsValue::Function(_) | JsValue::NativeFunction(_) => "function".to_string(),
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            JsValue::Number(n) => *n,
            JsValue::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse().unwrap_or(f64::NAN)
                }
            }
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
            JsValue::Object(_) | JsValue::Function(_) | JsValue::NativeFunction(_) => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => format_number(*n),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Array(values) => values
                .borrow()
                .iter()
                .map(|v| match v {
                    JsValue::Undefined | JsValue::Null => String::new(),
                    _ => v.as_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            JsValue::Object(map) => {
                // Error-like objects stringify as "Name: message"
                let map = map.borrow();
                if let (Some(JsValue::String(name)), Some(JsValue::String(msg))) =
                    (map.get("name"), map.get("message"))
                {
                    if matches!(
                        name.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                    ) {
                        return if msg.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}: {msg}")
                        };
                    }
                }
                "[object Object]".to_string()
            }
            JsValue::Function(_) | JsValue::NativeFunction(_) => {
                "function () { [native code] }".to_string()
            }
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
            // Objects coerce to string for +
            (JsValue::Object(_), _) | (_, JsValue::Object(_)) => {
                let s1 = self.as_string();
                let s2 = other.as_string();
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

    /// Get a named property (used by member-expression evaluation).
    pub fn get_property(&self, key: &str) -> JsValue {
        match self {
            JsValue::Object(map) => get_object_property(map, key),
            JsValue::Array(arr) => {
                let arr = arr.borrow();
                if key == "length" {
                    return JsValue::Number(arr.len() as f64);
                }
                if let Ok(idx) = key.parse::<usize>() {
                    arr.get(idx).cloned().unwrap_or(JsValue::Undefined)
                } else {
                    JsValue::Undefined
                }
            }
            JsValue::String(s) => {
                if key == "length" {
                    return JsValue::Number(s.chars().count() as f64);
                }
                if let Ok(idx) = key.parse::<usize>() {
                    s.chars()
                        .nth(idx)
                        .map(|c| JsValue::String(c.to_string()))
                        .unwrap_or(JsValue::Undefined)
                } else {
                    JsValue::Undefined
                }
            }
            JsValue::Function(function) => get_object_property(&function.properties, key),
            _ => JsValue::Undefined,
        }
    }
}

pub fn get_object_property(map: &JsObjectMap, key: &str) -> JsValue {
    if let Some(value) = map.borrow().get(key).cloned() {
        return value;
    }

    let proto = map.borrow().get("__proto__").cloned();
    match proto {
        Some(JsValue::Object(proto_map)) => get_object_property(&proto_map, key),
        Some(JsValue::Function(function)) => get_object_property(&function.properties, key),
        _ => JsValue::Undefined,
    }
}

pub fn has_object_property(map: &JsObjectMap, key: &str) -> bool {
    if map.borrow().contains_key(key) {
        return true;
    }

    let proto = map.borrow().get("__proto__").cloned();
    match proto {
        Some(JsValue::Object(proto_map)) => has_object_property(&proto_map, key),
        Some(JsValue::Function(function)) => has_object_property(&function.properties, key),
        _ => false,
    }
}

/// Format a number the way JS does.
pub fn format_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == 0.0 {
        return "0".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return format!("{:.0}", n);
    }
    format!("{}", n)
}
