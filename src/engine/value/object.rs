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
