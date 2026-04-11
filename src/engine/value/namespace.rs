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
