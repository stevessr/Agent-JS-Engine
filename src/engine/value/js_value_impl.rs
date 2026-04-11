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

