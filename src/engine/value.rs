use crate::engine::env::Environment;
use crate::engine::interpreter::{GeneratorState, Interpreter, RuntimeError};
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
    pub super_property_base: Option<JsValue>,
    pub home_object: Option<JsValue>,
    pub private_brand: Option<usize>,
    pub uses_lexical_this: bool,
    pub can_construct: bool,
    pub is_class_constructor: bool,
    pub is_derived_constructor: bool,
}

#[derive(Debug, Clone)]
pub enum PromiseState {
    Pending,
    Fulfilled(JsValue),
    Rejected(JsValue),
}

#[derive(Debug, Clone)]
pub enum PromiseReactionKind {
    Then {
        on_fulfilled: Option<JsValue>,
        on_rejected: Option<JsValue>,
    },
    Finally {
        on_finally: Option<JsValue>,
    },
    FinallyPassThrough {
        original_state: PromiseState,
    },
}

#[derive(Debug, Clone)]
pub struct PromiseReaction {
    pub kind: PromiseReactionKind,
    pub result_promise: Rc<RefCell<PromiseValue>>,
}

#[derive(Debug, Clone)]
pub struct PromiseValue {
    pub state: PromiseState,
    pub reactions: Vec<PromiseReaction>,
}

#[derive(Debug, Clone)]
pub enum BuiltinFunction {
    PromiseConstructor,
    PromiseResolve,
    PromiseReject,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    ModuleBindingGetter {
        env: Rc<RefCell<Environment>>,
        binding: String,
    },
    NamespaceBindingGetter {
        namespace: JsValue,
        export_name: String,
    },
    AsyncGeneratorResultMapper {
        done: bool,
    },
    PromiseResolver {
        promise: Rc<RefCell<PromiseValue>>,
        is_resolve: bool,
    },
}

/// A native (Rust) built-in function.
pub type NativeFn = fn(&mut Interpreter, &JsValue, &[JsValue]) -> Result<JsValue, RuntimeError>;

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

#[derive(Debug, Clone)]
pub enum PropertyValue {
    Data(JsValue),
    Accessor {
        getter: Option<JsValue>,
        setter: Option<JsValue>,
    },
}

pub type JsObjectMap = Rc<RefCell<HashMap<String, PropertyValue>>>;

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(i64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    Object(JsObjectMap),
    EnvironmentObject(Rc<RefCell<Environment>>),
    Promise(Rc<RefCell<PromiseValue>>),
    GeneratorState(Rc<RefCell<GeneratorState>>),
    ImportBinding {
        namespace: JsObjectMap,
        export_name: String,
    },
    Function(Rc<FunctionValue>),
    NativeFunction(Rc<NativeFunction>),
    BuiltinFunction(Rc<BuiltinFunction>),
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(l), JsValue::Boolean(r)) => l == r,
            (JsValue::Number(l), JsValue::Number(r)) => l == r,
            (JsValue::BigInt(l), JsValue::BigInt(r)) => l == r,
            (JsValue::String(l), JsValue::String(r)) => l == r,
            (JsValue::Array(l), JsValue::Array(r)) => Rc::ptr_eq(l, r),
            (JsValue::Object(l), JsValue::Object(r)) => Rc::ptr_eq(l, r),
            (JsValue::EnvironmentObject(l), JsValue::EnvironmentObject(r)) => Rc::ptr_eq(l, r),
            (JsValue::Promise(l), JsValue::Promise(r)) => Rc::ptr_eq(l, r),
            (JsValue::GeneratorState(l), JsValue::GeneratorState(r)) => Rc::ptr_eq(l, r),
            (
                JsValue::ImportBinding {
                    namespace: l_namespace,
                    export_name: l_export_name,
                },
                JsValue::ImportBinding {
                    namespace: r_namespace,
                    export_name: r_export_name,
                },
            ) => Rc::ptr_eq(l_namespace, r_namespace) && l_export_name == r_export_name,
            (JsValue::Function(l), JsValue::Function(r)) => Rc::ptr_eq(l, r),
            (JsValue::NativeFunction(l), JsValue::NativeFunction(r)) => Rc::ptr_eq(l, r),
            (JsValue::BuiltinFunction(l), JsValue::BuiltinFunction(r)) => Rc::ptr_eq(l, r),
            _ => false,
        }
    }
}

pub fn new_object_map() -> JsObjectMap {
    Rc::new(RefCell::new(HashMap::new()))
}

pub fn object_with_proto(proto: JsValue) -> JsValue {
    let map = new_object_map();
    map.borrow_mut()
        .insert("__proto__".to_string(), PropertyValue::Data(proto));
    JsValue::Object(map)
}

/// Create a plain JS object from key-value pairs.
pub fn make_object(pairs: impl IntoIterator<Item = (&'static str, JsValue)>) -> JsValue {
    let map = new_object_map();
    for (k, v) in pairs {
        map.borrow_mut()
            .insert(k.to_string(), PropertyValue::Data(v));
    }
    JsValue::Object(map)
}

/// Create a JS Error object with `name` and `message`.
pub fn make_error(name: &str, message: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "name".to_string(),
        PropertyValue::Data(JsValue::String(name.to_string())),
    );
    map.insert(
        "message".to_string(),
        PropertyValue::Data(JsValue::String(message.to_string())),
    );
    map.insert(
        "stack".to_string(),
        PropertyValue::Data(JsValue::String(format!("{name}: {message}"))),
    );
    JsValue::Object(Rc::new(RefCell::new(map)))
}

/// Wrap a native function as a JsValue.
pub fn native_fn(name: &'static str, func: NativeFn) -> JsValue {
    JsValue::NativeFunction(Rc::new(NativeFunction { name, func }))
}

impl JsValue {
    fn bigint_binary_operands(&self, other: &JsValue) -> Result<Option<(i64, i64)>, RuntimeError> {
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::BigInt(left), JsValue::BigInt(right)) => Ok(Some((*left, *right))),
            (JsValue::BigInt(_), _) | (_, JsValue::BigInt(_)) => Err(RuntimeError::TypeError(
                "cannot mix BigInt and other types".into(),
            )),
            _ => Ok(None),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Boolean(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::BigInt(n) => *n != 0,
            JsValue::String(s) => !s.is_empty(),
            JsValue::ImportBinding {
                namespace,
                export_name,
            } => resolve_namespace_export(namespace, export_name).is_truthy(),
            JsValue::Array(_)
            | JsValue::Object(_)
            | JsValue::EnvironmentObject(_)
            | JsValue::Promise(_)
            | JsValue::GeneratorState(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_)
            | JsValue::BuiltinFunction(_) => true,
        }
    }

    pub fn type_of(&self) -> String {
        match self {
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Null => "object".to_string(),
            JsValue::Boolean(_) => "boolean".to_string(),
            JsValue::Number(_) => "number".to_string(),
            JsValue::BigInt(_) => "bigint".to_string(),
            JsValue::String(_) => "string".to_string(),
            JsValue::ImportBinding {
                namespace,
                export_name,
            } => resolve_namespace_export(namespace, export_name).type_of(),
            JsValue::Array(_)
            | JsValue::Object(_)
            | JsValue::EnvironmentObject(_)
            | JsValue::Promise(_)
            | JsValue::GeneratorState(_) => "object".to_string(),
            JsValue::Function(_) | JsValue::NativeFunction(_) | JsValue::BuiltinFunction(_) => {
                "function".to_string()
            }
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            JsValue::Number(n) => *n,
            JsValue::BigInt(n) => *n as f64,
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
            JsValue::ImportBinding {
                namespace,
                export_name,
            } => resolve_namespace_export(namespace, export_name).as_number(),
            JsValue::GeneratorState(_) => f64::NAN,
            JsValue::Array(arr) => {
                let arr = arr.borrow();
                match arr.len() {
                    0 => 0.0,
                    1 => arr[0].as_number(),
                    _ => f64::NAN,
                }
            }
            JsValue::Object(_)
            | JsValue::EnvironmentObject(_)
            | JsValue::Promise(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_)
            | JsValue::BuiltinFunction(_) => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => format_number(*n),
            JsValue::BigInt(n) => format!("{n}n"),
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
                if let (
                    Some(PropertyValue::Data(JsValue::String(name))),
                    Some(PropertyValue::Data(JsValue::String(msg))),
                ) = (map.get("name"), map.get("message"))
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
            JsValue::EnvironmentObject(_) => "[object Object]".to_string(),
            JsValue::Promise(_) => "[object Promise]".to_string(),
            JsValue::GeneratorState(_) => "[object Generator]".to_string(),
            JsValue::ImportBinding {
                namespace,
                export_name,
            } => resolve_namespace_export(namespace, export_name).as_string(),
            JsValue::Function(_) | JsValue::NativeFunction(_) | JsValue::BuiltinFunction(_) => {
                "function () { [native code] }".to_string()
            }
        }
    }

    pub fn add(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::BigInt(left + right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::String(s1), _) => {
                let s2 = right.as_string();
                if s1.len() + s2.len() > 500_000 {
                    return Err(RuntimeError::ReferenceError("OOM Limit".into()));
                }
                Ok(JsValue::String(s1.clone() + &s2))
            }
            (_, JsValue::String(s2)) => {
                let s1 = left.as_string();
                if s1.len() + s2.len() > 500_000 {
                    return Err(RuntimeError::ReferenceError("OOM Limit".into()));
                }
                Ok(JsValue::String(s1 + s2))
            }
            // Objects coerce to string for +
            (JsValue::Object(_), _)
            | (_, JsValue::Object(_))
            | (JsValue::EnvironmentObject(_), _)
            | (_, JsValue::EnvironmentObject(_))
            | (JsValue::Promise(_), _)
            | (_, JsValue::Promise(_)) => {
                let s1 = left.as_string();
                let s2 = right.as_string();
                Ok(JsValue::String(s1 + &s2))
            }
            _ => Ok(JsValue::Number(left.as_number() + right.as_number())),
        }
    }

    pub fn sub(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::BigInt(left - right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        Ok(JsValue::Number(left.as_number() - right.as_number()))
    }

    pub fn mul(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::BigInt(left * right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        Ok(JsValue::Number(left.as_number() * right.as_number()))
    }

    pub fn div(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            if right == 0 {
                return Err(RuntimeError::RangeError("Division by zero".into()));
            }
            return Ok(JsValue::BigInt(left / right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        Ok(JsValue::Number(left.as_number() / right.as_number()))
    }

    pub fn lt(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::Boolean(left < right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 < s2)),
            _ => Ok(JsValue::Boolean(left.as_number() < right.as_number())),
        }
    }

    pub fn le(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::Boolean(left <= right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 <= s2)),
            _ => Ok(JsValue::Boolean(left.as_number() <= right.as_number())),
        }
    }

    pub fn gt(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::Boolean(left > right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 > s2)),
            _ => Ok(JsValue::Boolean(left.as_number() > right.as_number())),
        }
    }

    pub fn ge(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        if let Some((left, right)) = self.bigint_binary_operands(other)? {
            return Ok(JsValue::Boolean(left >= right));
        }
        let left = resolve_indirect_value(self);
        let right = resolve_indirect_value(other);
        match (&left, &right) {
            (JsValue::String(s1), JsValue::String(s2)) => Ok(JsValue::Boolean(s1 >= s2)),
            _ => Ok(JsValue::Boolean(left.as_number() >= right.as_number())),
        }
    }

    /// Get a named property (used by member-expression evaluation).
    pub fn get_property(&self, key: &str) -> JsValue {
        match self {
            JsValue::Object(map) => get_object_property(map, key),
            JsValue::EnvironmentObject(env) => env.borrow().get(key).unwrap_or(JsValue::Undefined),
            JsValue::Promise(_) => match key {
                "then" => JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseThen)),
                "catch" => JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseCatch)),
                "finally" => JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseFinally)),
                _ => JsValue::Undefined,
            },
            JsValue::GeneratorState(_) => JsValue::Undefined,
            JsValue::ImportBinding {
                namespace,
                export_name,
            } => resolve_namespace_export(namespace, export_name).get_property(key),
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
            JsValue::BuiltinFunction(function) => match function.as_ref() {
                BuiltinFunction::PromiseConstructor => match key {
                    "resolve" => JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseResolve)),
                    "reject" => JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseReject)),
                    _ => JsValue::Undefined,
                },
                _ => JsValue::Undefined,
            },
            _ => JsValue::Undefined,
        }
    }
}

pub fn resolve_indirect_value(value: &JsValue) -> JsValue {
    match value {
        JsValue::ImportBinding {
            namespace,
            export_name,
        } => resolve_namespace_export(namespace, export_name),
        _ => value.clone(),
    }
}

pub fn get_property_value(map: &JsObjectMap, key: &str) -> Option<PropertyValue> {
    if let Some(value) = map.borrow().get(key).cloned() {
        return Some(value);
    }

    let proto = map.borrow().get("__proto__").cloned();
    match proto {
        Some(PropertyValue::Data(JsValue::Object(proto_map))) => {
            get_property_value(&proto_map, key)
        }
        Some(PropertyValue::Data(JsValue::Function(function))) => {
            get_property_value(&function.properties, key)
        }
        _ => None,
    }
}

pub fn get_object_property(map: &JsObjectMap, key: &str) -> JsValue {
    match get_property_value(map, key) {
        Some(PropertyValue::Data(value)) => value,
        Some(PropertyValue::Accessor {
            getter: Some(getter),
            ..
        }) => getter,
        Some(PropertyValue::Accessor { getter: None, .. }) => JsValue::Undefined,
        None => JsValue::Undefined,
    }
}

fn resolve_namespace_accessor(getter: &JsValue) -> JsValue {
    match getter {
        JsValue::BuiltinFunction(function) => match function.as_ref() {
            BuiltinFunction::ModuleBindingGetter { env, binding } => {
                env.borrow().get(binding).unwrap_or(JsValue::Undefined)
            }
            BuiltinFunction::NamespaceBindingGetter {
                namespace,
                export_name,
            } => resolve_namespace_export_value(namespace, export_name),
            _ => getter.clone(),
        },
        _ => getter.clone(),
    }
}

pub fn resolve_namespace_export(namespace: &JsObjectMap, export_name: &str) -> JsValue {
    match get_property_value(namespace, export_name) {
        Some(PropertyValue::Data(value)) => value,
        Some(PropertyValue::Accessor {
            getter: Some(getter),
            ..
        }) => resolve_namespace_accessor(&getter),
        Some(PropertyValue::Accessor { getter: None, .. }) | None => JsValue::Undefined,
    }
}

pub fn resolve_namespace_export_value(namespace: &JsValue, export_name: &str) -> JsValue {
    match namespace {
        JsValue::Object(map) => resolve_namespace_export(map, export_name),
        _ => JsValue::Undefined,
    }
}

pub fn set_namespace_export(
    namespace: &JsObjectMap,
    export_name: &str,
    value: JsValue,
) -> Result<(), String> {
    match get_property_value(namespace, export_name) {
        Some(PropertyValue::Data(_)) => {
            namespace
                .borrow_mut()
                .insert(export_name.to_string(), PropertyValue::Data(value));
            Ok(())
        }
        Some(PropertyValue::Accessor {
            getter: Some(getter),
            ..
        }) => match getter {
            JsValue::BuiltinFunction(function) => match function.as_ref() {
                BuiltinFunction::ModuleBindingGetter { env, binding } => {
                    env.borrow_mut().set(binding, value)
                }
                BuiltinFunction::NamespaceBindingGetter {
                    namespace,
                    export_name,
                } => match namespace {
                    JsValue::Object(map) => set_namespace_export(map, export_name, value),
                    _ => Err("module namespace is not an object".to_string()),
                },
                _ => Err("module export is read-only".to_string()),
            },
            _ => Err("module export is read-only".to_string()),
        },
        Some(PropertyValue::Accessor { getter: None, .. }) | None => {
            Err(format!("ReferenceError: {} is not defined", export_name))
        }
    }
}

pub fn has_object_property(map: &JsObjectMap, key: &str) -> bool {
    get_property_value(map, key).is_some()
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
