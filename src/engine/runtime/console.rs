fn install_console_object(context: &mut Context) -> JsResult<()> {
    let console = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("log"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("info"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("debug"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_warn),
            js_string!("warn"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_error),
            js_string!("error"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_dir),
            js_string!("dir"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_assert),
            js_string!("assert"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_clear),
            js_string!("clear"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_count),
            js_string!("count"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_count_reset),
            js_string!("countReset"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group),
            js_string!("group"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group),
            js_string!("groupCollapsed"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group_end),
            js_string!("groupEnd"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_table),
            js_string!("table"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time),
            js_string!("time"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time_log),
            js_string!("timeLog"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time_end),
            js_string!("timeEnd"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_trace),
            js_string!("trace"),
            0,
        )
        .build();

    context
        .global_object()
        .set(js_string!("console"), console, true, context)?;
    Ok(())
}

thread_local! {
    static CONSOLE_COUNTERS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    static CONSOLE_TIMERS: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    static CONSOLE_GROUP_DEPTH: RefCell<usize> = const { RefCell::new(0) };
}

fn console_format_args(args: &[BoaValue], context: &mut Context) -> JsResult<String> {
    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    let message = args
        .iter()
        .map(|value| {
            value
                .to_string(context)
                .map(|text| text.to_std_string_escaped())
        })
        .collect::<JsResult<Vec<_>>>()?
        .join(" ");
    Ok(format!("{indent}{message}"))
}

fn console_log(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_warn(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("[WARN] {message}")));
    Ok(BoaValue::undefined())
}

fn console_error(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("[ERROR] {message}")));
    Ok(BoaValue::undefined())
}

fn console_dir(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_assert(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let condition = args.get_or_undefined(0).to_boolean();
    if !condition {
        let msg_args = if args.len() > 1 { &args[1..] } else { &[] };
        let message = if msg_args.is_empty() {
            "Assertion failed".to_string()
        } else {
            format!(
                "Assertion failed: {}",
                console_format_args(msg_args, context)?
            )
        };
        PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    }
    Ok(BoaValue::undefined())
}

fn console_clear(_: &BoaValue, _: &[BoaValue], _: &mut Context) -> JsResult<BoaValue> {
    // Just log a clear marker
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push("\x1b[2J\x1b[H".to_string()));
    Ok(BoaValue::undefined())
}

fn console_count(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let count = CONSOLE_COUNTERS.with(|counters| {
        let mut counters = counters.borrow_mut();
        let count = counters.entry(label.clone()).or_insert(0);
        *count += 1;
        *count
    });

    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    PRINT_BUFFER.with(|buffer| {
        buffer
            .borrow_mut()
            .push(format!("{indent}{label}: {count}"))
    });
    Ok(BoaValue::undefined())
}

fn console_count_reset(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    CONSOLE_COUNTERS.with(|counters| {
        counters.borrow_mut().remove(&label);
    });
    Ok(BoaValue::undefined())
}

fn console_group(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    if !args.is_empty() {
        let message = console_format_args(args, context)?;
        PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    }
    CONSOLE_GROUP_DEPTH.with(|d| *d.borrow_mut() += 1);
    Ok(BoaValue::undefined())
}

fn console_group_end(_: &BoaValue, _: &[BoaValue], _: &mut Context) -> JsResult<BoaValue> {
    CONSOLE_GROUP_DEPTH.with(|d| {
        let mut depth = d.borrow_mut();
        if *depth > 0 {
            *depth -= 1;
        }
    });
    Ok(BoaValue::undefined())
}

fn console_table(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    // Simple implementation: just log the value
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_time(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    CONSOLE_TIMERS.with(|timers| {
        timers.borrow_mut().insert(label, Instant::now());
    });
    Ok(BoaValue::undefined())
}

fn console_time_log(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let elapsed =
        CONSOLE_TIMERS.with(|timers| timers.borrow().get(&label).map(|start| start.elapsed()));

    if let Some(elapsed) = elapsed {
        let extra = if args.len() > 1 {
            format!(" {}", console_format_args(&args[1..], context)?)
        } else {
            String::new()
        };
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer.borrow_mut().push(format!(
                "{indent}{label}: {:.3}ms{extra}",
                elapsed.as_secs_f64() * 1000.0
            ))
        });
    } else {
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer
                .borrow_mut()
                .push(format!("{indent}Timer '{label}' does not exist"))
        });
    }
    Ok(BoaValue::undefined())
}

fn console_time_end(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let elapsed = CONSOLE_TIMERS.with(|timers| timers.borrow_mut().remove(&label));

    if let Some(start) = elapsed {
        let elapsed = start.elapsed();
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer.borrow_mut().push(format!(
                "{indent}{label}: {:.3}ms",
                elapsed.as_secs_f64() * 1000.0
            ))
        });
    } else {
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer
                .borrow_mut()
                .push(format!("{indent}Timer '{label}' does not exist"))
        });
    }
    Ok(BoaValue::undefined())
}

fn console_trace(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = if args.is_empty() {
        "Trace".to_string()
    } else {
        format!("Trace: {}", console_format_args(args, context)?)
    };
    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("{indent}{message}")));
    Ok(BoaValue::undefined())
}

