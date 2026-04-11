fn catch_silent_panic<F, T>(f: F) -> std::thread::Result<T>
where
    F: FnOnce() -> T,
{
    let _guard = PANIC_HOOK_LOCK
        .lock()
        .expect("panic hook mutex must not be poisoned");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    result
}

fn host_worker_leaving(_: &BoaValue, _: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
    Ok(BoaValue::undefined())
}

fn host_data_view_set_big_int64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setBigInt64", this, args, context)
}

fn host_data_view_set_big_uint64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setBigUint64", this, args, context)
}

fn host_data_view_set_float16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat16", this, args, context)
}

fn host_data_view_set_float32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat32", this, args, context)
}

fn host_data_view_set_float64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat64", this, args, context)
}

fn host_data_view_set_int16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt16", this, args, context)
}

fn host_data_view_set_int32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt32", this, args, context)
}

fn host_data_view_set_int8_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt8", this, args, context)
}

fn host_data_view_set_uint16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint16", this, args, context)
}

fn host_data_view_set_uint32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint32", this, args, context)
}

fn host_data_view_set_uint8_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint8", this, args, context)
}

fn host_data_view_set_wrapper(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if data_view_buffer_is_immutable(this, context)? {
        return Err(immutable_array_buffer_error(method_name));
    }
    call_hidden_data_view_method(method_name, this, args, context)
}

fn with_realm<T, F>(context: &mut Context, realm: Realm, operation: F) -> JsResult<T>
where
    F: FnOnce(&mut Context) -> JsResult<T>,
{
    let previous_realm = context.enter_realm(realm);
    let result = operation(context);
    context.enter_realm(previous_realm);
    result
}

fn host_print(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> boa_engine::JsResult<BoaValue> {
    let message = args
        .iter()
        .map(|value| {
            value
                .to_string(context)
                .map(|text| text.to_std_string_escaped())
        })
        .collect::<boa_engine::JsResult<Vec<_>>>()?
        .join(" ");

    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn host_error_is_error(
    _: &BoaValue,
    args: &[BoaValue],
    _: &mut Context,
) -> boa_engine::JsResult<BoaValue> {
    Ok(args
        .get_or_undefined(0)
        .as_object()
        .is_some_and(|value| value.is::<BoaBuiltinError>())
        .into())
}

fn display_value(value: &BoaValue, context: &mut Context) -> Option<String> {
    if value.is_undefined() {
        None
    } else {
        Some(inspect_value(value, context, 0, &mut HashSet::new()))
    }
}

/// Format a JS value for REPL display, similar to Node.js util.inspect
fn inspect_value(
    value: &BoaValue,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
) -> String {
    const MAX_DEPTH: usize = 4;
    const MAX_ARRAY_ITEMS: usize = 100;
    const MAX_STRING_LEN: usize = 100;

    if value.is_undefined() {
        "undefined".to_string()
    } else if value.is_null() {
        "null".to_string()
    } else if let Some(b) = value.as_boolean() {
        b.to_string()
    } else if let Some(n) = value.as_number() {
        if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else if n == 0.0 && n.is_sign_negative() {
            "-0".to_string()
        } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
            // Integer-like number
            format!("{}", n as i64)
        } else {
            format!("{n}")
        }
    } else if let Some(s) = value.as_string() {
        let s_str = s.to_std_string_escaped();
        let escaped = escape_string(&s_str);
        if escaped.len() > MAX_STRING_LEN {
            format!(
                "'{}'... ({} more characters)",
                &escaped[..MAX_STRING_LEN],
                escaped.len() - MAX_STRING_LEN
            )
        } else {
            format!("'{escaped}'")
        }
    } else if let Some(sym) = value.as_symbol() {
        let desc = sym
            .description()
            .map(|d| d.to_std_string_escaped())
            .unwrap_or_default();
        if desc.is_empty() {
            "Symbol()".to_string()
        } else {
            format!("Symbol({desc})")
        }
    } else if let Some(n) = value.as_bigint() {
        format!("{n}n")
    } else if let Some(obj) = value.as_object() {
        // Circular reference check
        let ptr = obj.as_ref() as *const _ as usize;
        if seen.contains(&ptr) {
            return "[Circular]".to_string();
        }
        seen.insert(ptr);

        let result = inspect_object(&obj, context, depth, seen, MAX_DEPTH, MAX_ARRAY_ITEMS);

        seen.remove(&ptr);
        result
    } else {
        // Fallback
        value
            .to_string(context)
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|_| "[unknown]".to_string())
    }
}

fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\'' => result.push_str("\\'"),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => result.push_str(&format!("\\x{:02x}", c as u32)),
            c => result.push(c),
        }
    }
    result
}

fn inspect_object(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
    max_array_items: usize,
) -> String {
    // Check if it's a function
    if obj.is_callable() {
        let name = obj
            .get(js_string!("name"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_default();

        // Check if it's an async function or generator
        let to_string = obj
            .get(PropertyKey::from(JsSymbol::to_string_tag()), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()));

        let func_type = match to_string.as_deref() {
            Some("AsyncFunction") => "AsyncFunction",
            Some("GeneratorFunction") => "GeneratorFunction",
            Some("AsyncGeneratorFunction") => "AsyncGeneratorFunction",
            _ => "Function",
        };

        if name.is_empty() {
            format!("[{func_type} (anonymous)]")
        } else {
            format!("[{func_type}: {name}]")
        }
    }
    // Check for Promise
    else if let Ok(promise) = JsPromise::from_object(obj.clone()) {
        let state = promise.state();
        match state {
            PromiseState::Pending => "Promise { <pending> }".to_string(),
            PromiseState::Fulfilled(v) => {
                if depth >= max_depth {
                    "Promise { ... }".to_string()
                } else {
                    let inner = inspect_value(&v, context, depth + 1, seen);
                    format!("Promise {{ {inner} }}")
                }
            }
            PromiseState::Rejected(e) => {
                if depth >= max_depth {
                    "Promise { <rejected> ... }".to_string()
                } else {
                    let inner = inspect_value(&e, context, depth + 1, seen);
                    format!("Promise {{ <rejected> {inner} }}")
                }
            }
        }
    }
    // Check for Array
    else if obj.is_array() {
        inspect_array(obj, context, depth, seen, max_depth, max_array_items)
    }
    // Check for TypedArray
    else if let Ok(arr) = JsUint8Array::from_object(obj.clone()) {
        let len = arr.length(context).unwrap_or(0);
        format!("Uint8Array({len}) [ ... ]")
    }
    // Check for ArrayBuffer
    else if JsArrayBuffer::from_object(obj.clone()).is_ok() {
        let len = obj
            .get(js_string!("byteLength"), context)
            .ok()
            .and_then(|v| v.to_u32(context).ok())
            .unwrap_or(0);
        format!("ArrayBuffer {{ byteLength: {len} }}")
    }
    // Check for Date
    else if is_date_object(obj, context) {
        // Try toISOString first
        if let Ok(to_iso) = obj.get(js_string!("toISOString"), context) {
            if let Some(func) = to_iso.as_object().filter(|o| o.is_callable()) {
                if let Ok(result) = func.call(&BoaValue::from(obj.clone()), &[], context) {
                    if let Ok(s) = result.to_string(context) {
                        return s.to_std_string_escaped();
                    }
                }
            }
        }
        // Fallback: try toString
        if let Ok(to_str) = obj.get(js_string!("toString"), context) {
            if let Some(func) = to_str.as_object().filter(|o| o.is_callable()) {
                if let Ok(result) = func.call(&BoaValue::from(obj.clone()), &[], context) {
                    if let Ok(s) = result.to_string(context) {
                        return s.to_std_string_escaped();
                    }
                }
            }
        }
        "[Date]".to_string()
    }
    // Check for RegExp
    else if is_regexp_object(obj, context) {
        // Get source and flags
        let source = obj
            .get(js_string!("source"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_else(|| "".to_string());
        let flags = obj
            .get(js_string!("flags"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_default();
        format!("/{source}/{flags}")
    }
    // Check for Error
    else if is_error_object(obj, context) {
        let name = obj
            .get(js_string!("name"), context)
            .ok()
            .and_then(|v| v.to_string(context).ok())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|| "Error".to_string());
        let message = obj
            .get(js_string!("message"), context)
            .ok()
            .and_then(|v| v.to_string(context).ok())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        if message.is_empty() {
            format!("[{name}]")
        } else {
            format!("[{name}: {message}]")
        }
    }
    // Check for Map
    else if is_map_object(obj, context) {
        inspect_map(obj, context, depth, seen, max_depth)
    }
    // Check for Set
    else if is_set_object(obj, context) {
        inspect_set(obj, context, depth, seen, max_depth)
    }
    // Generic object
    else {
        inspect_plain_object(obj, context, depth, seen, max_depth)
    }
}

fn inspect_array(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
    max_items: usize,
) -> String {
    let length = obj
        .get(js_string!("length"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0) as usize;

    if depth >= max_depth {
        return format!("[Array({length})]");
    }

    if length == 0 {
        return "[]".to_string();
    }

    let mut items = Vec::new();
    let show_count = length.min(max_items);

    for i in 0..show_count {
        let idx = js_string!(i.to_string());
        match obj.get(PropertyKey::from(idx), context) {
            Ok(v) => items.push(inspect_value(&v, context, depth + 1, seen)),
            Err(_) => items.push("<error>".to_string()),
        }
    }

    let remaining = if length > max_items {
        format!(" ... {} more items", length - max_items)
    } else {
        String::new()
    };

    // Format based on complexity
    let single_line = format!("[ {}{remaining} ]", items.join(", "));
    if single_line.len() <= 80 && !single_line.contains('\n') {
        single_line
    } else {
        let indent = "  ".repeat(depth + 1);
        let inner = items
            .iter()
            .map(|s| format!("{indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("[\n{inner}{remaining}\n{}]", "  ".repeat(depth))
    }
}

fn inspect_plain_object(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "[Object]".to_string();
    }

    // Get own property keys
    let keys = match obj.own_property_keys(context) {
        Ok(keys) => keys,
        Err(_) => return "[Object]".to_string(),
    };

    if keys.is_empty() {
        return "{}".to_string();
    }

    let mut entries = Vec::new();
    for key in keys.iter().take(20) {
        let key_str = match key {
            PropertyKey::String(s) => s.to_std_string_escaped(),
            PropertyKey::Symbol(sym) => {
                let desc = sym
                    .description()
                    .map(|d| d.to_std_string_escaped())
                    .unwrap_or_default();
                format!("[Symbol({desc})]")
            }
            PropertyKey::Index(i) => i.get().to_string(),
        };

        // Skip internal properties
        if key_str.starts_with("__") {
            continue;
        }

        match obj.get(key.clone(), context) {
            Ok(v) => {
                let val_str = inspect_value(&v, context, depth + 1, seen);
                // Quote keys if needed
                if needs_quotes(&key_str) {
                    entries.push(format!("'{key_str}': {val_str}"));
                } else {
                    entries.push(format!("{key_str}: {val_str}"));
                }
            }
            Err(_) => continue,
        }
    }

    let remaining = if keys.len() > 20 {
        format!(" ... {} more properties", keys.len() - 20)
    } else {
        String::new()
    };

    // Check for custom constructor name
    let constructor_name = obj
        .get(js_string!("constructor"), context)
        .ok()
        .and_then(|c| c.as_object())
        .and_then(|c| c.get(js_string!("name"), context).ok())
        .and_then(|n| n.as_string().map(|s| s.to_std_string_escaped()))
        .filter(|n| n != "Object" && !n.is_empty());

    let prefix = constructor_name
        .map(|n| format!("{n} "))
        .unwrap_or_default();

    // Format based on complexity
    let single_line = format!("{prefix}{{ {}{remaining} }}", entries.join(", "));
    if single_line.len() <= 80 && !single_line.contains('\n') {
        single_line
    } else {
        let indent = "  ".repeat(depth + 1);
        let inner = entries
            .iter()
            .map(|s| format!("{indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("{prefix}{{\n{inner}{remaining}\n{}}}", "  ".repeat(depth))
    }
}

fn inspect_map(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "Map { ... }".to_string();
    }

    // Get entries via forEach or iteration
    let size = obj
        .get(js_string!("size"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0);

    if size == 0 {
        return "Map(0) {}".to_string();
    }

    // Try to iterate using entries()
    let entries_fn = match obj.get(js_string!("entries"), context) {
        Ok(v) if v.is_callable() => v,
        _ => return format!("Map({size}) {{ ... }}"),
    };

    let iterator = match entries_fn
        .as_object()
        .and_then(|f| f.call(&BoaValue::from(obj.clone()), &[], context).ok())
    {
        Some(it) => it,
        None => return format!("Map({size}) {{ ... }}"),
    };

    let mut items = Vec::new();
    for _ in 0..size.min(10) {
        let next_fn = match iterator
            .as_object()
            .and_then(|it| it.get(js_string!("next"), context).ok())
        {
            Some(f) if f.is_callable() => f,
            _ => break,
        };

        let result = match next_fn
            .as_object()
            .and_then(|f| f.call(&iterator, &[], context).ok())
        {
            Some(r) => r,
            None => break,
        };

        let done = result
            .as_object()
            .and_then(|r| r.get(js_string!("done"), context).ok())
            .map(|v| v.to_boolean());

        if done == Some(true) {
            break;
        }

        if let Some(value) = result
            .as_object()
            .and_then(|r| r.get(js_string!("value"), context).ok())
        {
            if let Some(pair) = value.as_object() {
                let key = pair
                    .get(PropertyKey::from(js_string!("0")), context)
                    .unwrap_or(BoaValue::undefined());
                let val = pair
                    .get(PropertyKey::from(js_string!("1")), context)
                    .unwrap_or(BoaValue::undefined());
                let k_str = inspect_value(&key, context, depth + 1, seen);
                let v_str = inspect_value(&val, context, depth + 1, seen);
                items.push(format!("{k_str} => {v_str}"));
            }
        }
    }

    let remaining = if size > 10 {
        format!(", ... {} more", size - 10)
    } else {
        String::new()
    };

    format!("Map({size}) {{ {}{remaining} }}", items.join(", "))
}

fn inspect_set(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "Set { ... }".to_string();
    }

    let size = obj
        .get(js_string!("size"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0);

    if size == 0 {
        return "Set(0) {}".to_string();
    }

    // Try to iterate using values()
    let values_fn = match obj.get(js_string!("values"), context) {
        Ok(v) if v.is_callable() => v,
        _ => return format!("Set({size}) {{ ... }}"),
    };

    let iterator = match values_fn
        .as_object()
        .and_then(|f| f.call(&BoaValue::from(obj.clone()), &[], context).ok())
    {
        Some(it) => it,
        None => return format!("Set({size}) {{ ... }}"),
    };

    let mut items = Vec::new();
    for _ in 0..size.min(10) {
        let next_fn = match iterator
            .as_object()
            .and_then(|it| it.get(js_string!("next"), context).ok())
        {
            Some(f) if f.is_callable() => f,
            _ => break,
        };

        let result = match next_fn
            .as_object()
            .and_then(|f| f.call(&iterator, &[], context).ok())
        {
            Some(r) => r,
            None => break,
        };

        let done = result
            .as_object()
            .and_then(|r| r.get(js_string!("done"), context).ok())
            .map(|v| v.to_boolean());

        if done == Some(true) {
            break;
        }

        if let Some(value) = result
            .as_object()
            .and_then(|r| r.get(js_string!("value"), context).ok())
        {
            items.push(inspect_value(&value, context, depth + 1, seen));
        }
    }

    let remaining = if size > 10 {
        format!(", ... {} more", size - 10)
    } else {
        String::new()
    };

    format!("Set({size}) {{ {}{remaining} }}", items.join(", "))
}

fn is_date_object(obj: &JsObject, context: &mut Context) -> bool {
    // Check if getTime exists and is callable
    obj.get(js_string!("getTime"), context)
        .ok()
        .map(|v| v.is_callable())
        .unwrap_or(false)
        && obj
            .get(js_string!("toISOString"), context)
            .ok()
            .map(|v| v.is_callable())
            .unwrap_or(false)
}

fn is_regexp_object(obj: &JsObject, context: &mut Context) -> bool {
    // Check Symbol.toStringTag first
    if obj
        .get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "RegExp")
        .unwrap_or(false)
    {
        return true;
    }
    // Fallback: check for source property (not has_own_property, as it's on prototype)
    obj.get(js_string!("source"), context).is_ok()
        && obj.get(js_string!("flags"), context).is_ok()
        && obj
            .get(js_string!("test"), context)
            .ok()
            .map(|v| v.is_callable())
            .unwrap_or(false)
}

fn is_error_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(js_string!("name"), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s.ends_with("Error"))
        .unwrap_or(false)
        || obj
            .has_own_property(js_string!("stack"), context)
            .unwrap_or(false)
}

fn is_map_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "Map")
        .unwrap_or(false)
}

fn is_set_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "Set")
        .unwrap_or(false)
}

fn needs_quotes(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let first = key.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return true;
    }
    key.chars()
        .any(|c| !c.is_alphanumeric() && c != '_' && c != '$')
}

fn convert_error(error: JsError, context: &mut Context) -> EngineError {
    let native_name = error
        .as_native()
        .map(|native| native.kind.to_string())
        .or_else(|| {
            error
                .try_native(context)
                .ok()
                .map(|native| native.kind.to_string())
        });

    let opaque = error.to_opaque(context);
    let thrown_name = opaque.as_object().and_then(|object| {
        object
            .get(js_string!("name"), context)
            .ok()
            .filter(|value| !value.is_undefined() && !value.is_null())
            .and_then(|value| value.to_string(context).ok())
            .map(|text| text.to_std_string_escaped())
            .filter(|text| !text.is_empty())
            .or_else(|| {
                object
                    .get(js_string!("constructor"), context)
                    .ok()
                    .and_then(|value| value.as_object())
                    .and_then(|constructor| constructor.get(js_string!("name"), context).ok())
                    .filter(|value| !value.is_undefined() && !value.is_null())
                    .and_then(|value| value.to_string(context).ok())
                    .map(|text| text.to_std_string_escaped())
                    .filter(|text| !text.is_empty())
            })
    });

    let name = native_name
        .clone()
        .or(thrown_name)
        .unwrap_or_else(|| "ThrownValue".to_string());

    let message = if matches!(
        native_name.as_deref(),
        Some("RuntimeLimit") | Some("NoInstructionsRemain")
    ) {
        format!("{error}")
    } else {
        opaque
            .to_string(context)
            .ok()
            .map(|text| text.to_std_string_escaped())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| format!("{error}"))
    };

    EngineError { name, message }
}

fn reset_print_buffer() {
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().clear());
}

fn take_print_buffer() -> Vec<String> {
    PRINT_BUFFER.with(|buffer| std::mem::take(&mut *buffer.borrow_mut()))
}
