fn ensure_agent_runtime(context: &mut Context) -> Arc<AgentRuntime> {
    if let Some(data) = context.get_data::<AgentRuntimeContext>() {
        return data.runtime.clone();
    }

    let runtime = Arc::new(AgentRuntime::new());
    context.insert_data(AgentRuntimeContext {
        runtime: runtime.clone(),
    });
    runtime
}

fn ensure_host_hooks_context(context: &mut Context) {
    if context.has_data::<HostHooksContext>() {
        return;
    }
    context.insert_data(HostHooksContext::new());
}

fn host_hooks_context(context: &Context) -> JsResult<&HostHooksContext> {
    context.get_data::<HostHooksContext>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("host hooks context is unavailable")
            .into()
    })
}

fn normalize_module_tracking_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn push_active_module_evaluation(path: &Path, context: &Context) -> JsResult<()> {
    host_hooks_context(context)?
        .active_module_evaluations
        .borrow_mut()
        .push(normalize_module_tracking_path(path));
    Ok(())
}

fn pop_active_module_evaluation(path: &Path, context: &Context) -> JsResult<()> {
    let normalized = normalize_module_tracking_path(path);
    let mut active = host_hooks_context(context)?
        .active_module_evaluations
        .borrow_mut();
    if let Some(index) = active
        .iter()
        .rposition(|candidate| candidate == &normalized)
    {
        active.remove(index);
    }
    Ok(())
}

fn tracked_active_module_paths(context: &Context) -> JsResult<HashSet<PathBuf>> {
    Ok(host_hooks_context(context)?
        .active_module_evaluations
        .borrow()
        .iter()
        .cloned()
        .collect())
}

fn array_buffer_original_symbol(
    context: &Context,
    method_name: &'static str,
) -> JsResult<JsSymbol> {
    host_hooks_context(context)?
        .array_buffer_originals
        .get(method_name)
        .cloned()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message(format!(
                    "missing hidden ArrayBuffer method for `{method_name}`"
                ))
                .into()
        })
}

fn data_view_original_symbol(context: &Context, method_name: &'static str) -> JsResult<JsSymbol> {
    host_hooks_context(context)?
        .data_view_originals
        .get(method_name)
        .cloned()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message(format!(
                    "missing hidden DataView method for `{method_name}`"
                ))
                .into()
        })
}

fn immutable_marker_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.immutable_marker.clone())
}

fn promise_then_original_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.promise_then_original.clone())
}

fn array_flat_original_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.array_flat_original.clone())
}

fn with_original_promise_then<T>(
    context: &mut Context,
    callback: impl FnOnce(&mut Context) -> JsResult<T>,
) -> JsResult<T> {
    let prototype = context.intrinsics().constructors().promise().prototype();
    let original = prototype.get(promise_then_original_symbol(context)?, context)?;
    let current = prototype.get(js_string!("then"), context)?;

    prototype.define_property_or_throw(
        js_string!("then"),
        PropertyDescriptor::builder()
            .value(original)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let result = callback(context);

    prototype.define_property_or_throw(
        js_string!("then"),
        PropertyDescriptor::builder()
            .value(current)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    result
}

fn mark_array_buffer_immutable(buffer: &JsObject, context: &mut Context) -> JsResult<()> {
    buffer.define_property_or_throw(
        immutable_marker_symbol(context)?,
        PropertyDescriptor::builder()
            .value(true)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    Ok(())
}

fn is_marked_immutable_array_buffer(buffer: &JsObject, context: &mut Context) -> JsResult<bool> {
    Ok(buffer
        .get(immutable_marker_symbol(context)?, context)?
        .to_boolean())
}

fn array_buffer_from_this(this: &BoaValue) -> JsResult<JsArrayBuffer> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ArrayBuffer method called with invalid `this` value")
    })?;
    Ok(JsArrayBuffer::from_object(object.clone()).map_err(|_| {
        JsNativeError::typ().with_message("ArrayBuffer method called with invalid `this` value")
    })?)
}

fn array_buffer_is_immutable(this: &BoaValue, context: &mut Context) -> JsResult<bool> {
    let buffer = array_buffer_from_this(this)?;
    is_marked_immutable_array_buffer(&buffer.clone().into(), context)
}

fn call_hidden_method(
    prototype: &JsObject,
    hidden_symbol: JsSymbol,
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let original = prototype.get(hidden_symbol, context)?;
    let callable = original.as_callable().ok_or_else(|| {
        JsNativeError::typ().with_message(format!("missing original method for `{method_name}`"))
    })?;
    callable.call(this, args, context)
}

fn call_hidden_array_buffer_method(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();
    call_hidden_method(
        &prototype,
        array_buffer_original_symbol(context, method_name)?,
        method_name,
        this,
        args,
        context,
    )
}

fn has_hidden_array_buffer_method(
    context: &mut Context,
    method_name: &'static str,
) -> JsResult<bool> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();
    let symbol = array_buffer_original_symbol(context, method_name)?;
    prototype.has_own_property(symbol, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_annex_b_html_open_and_close_comments() {
        let source = "<!-- open\ncode();\n--> close\nnext();\n";
        let (rewritten, changed) = rewrite_annex_b_html_comments(source);
        assert!(changed);
        assert!(rewritten.contains("// open"));
        assert!(rewritten.contains("// close"));
    }

    #[test]
    fn rewrites_annex_b_call_assignment_targets() {
        let source = "f() = g();\n  for (f() in [1]) {}\nf()++;\nasync() = 1;\n";
        let (rewritten, _) = rewrite_annex_b_call_assignment_targets(source);
        assert!(
            rewritten.contains("throw new ReferenceError('Invalid left-hand side in assignment');")
        );
        assert!(!rewritten.contains("async() = 1;"));
    }

    #[test]
    fn rewrites_top_level_await_using_after_frontmatter_comment() {
        let source = r#"/*---
flags: [module]
---*/

await using x = {
  [Symbol.asyncDispose]() {}
};
"#;

        let rewritten = preprocess_compat_source(source, None, true, true).unwrap();
        assert!(rewritten.contains("/*---\nflags: [module]\n---*/"));
        assert!(rewritten.contains("const __agentjs_using_stack__ = new AsyncDisposableStack();"));
        assert!(rewritten.contains("const x = {\n  [Symbol.asyncDispose]() {}\n};"));
        assert!(rewritten.contains(
            "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
        ));
        assert!(!rewritten.contains("await using x ="));
    }

    #[test]
    fn preprocess_does_not_treat_for_inside_comments_as_for_statement() {
        let source = r#"/*---
description: returns data desc for functions on built-ins
---*/

verifyProperty(Date.prototype, "getYear", {
  enumerable: false,
  writable: true,
  configurable: true
});
"#;

        let rewritten = preprocess_compat_source(source, None, false, false).unwrap();
        assert!(rewritten.contains("returns data desc for functions on built-ins"));
        assert!(!rewritten.contains("__agentjs_using_stack__"));
    }

    #[test]
    fn preprocess_does_not_treat_identifier_name_for_as_for_statement() {
        let source = r#"
const obj = { for: 1, using: 2 };
obj.for + obj.using;
"#;

        let rewritten = preprocess_compat_source(source, None, false, false).unwrap();
        assert!(rewritten.contains("{ for: 1, using: 2 }"));
        assert!(!rewritten.contains("__agentjs_using_value__"));
    }
}

fn call_hidden_data_view_method(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let prototype = context.intrinsics().constructors().data_view().prototype();
    call_hidden_method(
        &prototype,
        data_view_original_symbol(context, method_name)?,
        method_name,
        this,
        args,
        context,
    )
}

fn immutable_array_buffer_error(operation: &'static str) -> JsError {
    JsNativeError::typ()
        .with_message(format!(
            "cannot perform `{operation}` on an immutable ArrayBuffer"
        ))
        .into()
}

fn read_transfer_length_argument(args: &[BoaValue], context: &mut Context) -> JsResult<()> {
    if let Some(new_length) = args.first().filter(|value| !value.is_undefined()) {
        let _ = new_length.to_index(context)?;
    }
    Ok(())
}

fn array_buffer_transfer_copy_and_detach(
    this: &BoaValue,
    args: &[BoaValue],
    preserve_resizability: bool,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    let buffer_object: JsObject = buffer.clone().into();
    let current_bytes = buffer.data().map(|bytes| bytes.to_vec()).ok_or_else(|| {
        JsNativeError::typ().with_message("cannot transfer a detached ArrayBuffer")
    })?;
    let is_resizable = preserve_resizability
        && buffer_object
            .get(js_string!("resizable"), context)?
            .to_boolean();
    let max_byte_length = if is_resizable {
        usize::try_from(
            buffer_object
                .get(js_string!("maxByteLength"), context)?
                .to_index(context)?,
        )
        .map_err(|_| {
            JsNativeError::range().with_message("ArrayBuffer length exceeds supported range")
        })?
    } else {
        current_bytes.len()
    };
    let target_length = if args.first().is_none_or(BoaValue::is_undefined) {
        current_bytes.len()
    } else {
        usize::try_from(args[0].to_index(context)?).map_err(|_| {
            JsNativeError::range().with_message("ArrayBuffer length exceeds supported range")
        })?
    };

    if is_resizable && target_length > max_byte_length {
        return Err(JsNativeError::range()
            .with_message("new ArrayBuffer length exceeds maxByteLength")
            .into());
    }

    let next_buffer = if is_resizable {
        JsArrayBuffer::new(target_length, context)?.with_max_byte_length(max_byte_length as u64)
    } else {
        JsArrayBuffer::new(target_length, context)?
    };
    if let Some(mut bytes) = next_buffer.data_mut() {
        let copy_length = current_bytes.len().min(target_length);
        bytes[..copy_length].copy_from_slice(&current_bytes[..copy_length]);
    }

    let _ = buffer.detach(&BoaValue::undefined())?;
    Ok(next_buffer.into())
}

fn array_buffer_slice_copy(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    let current_bytes = buffer
        .data()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| JsNativeError::typ().with_message("cannot slice a detached ArrayBuffer"))?;
    let len = i64::try_from(current_bytes.len())
        .expect("slice length should always fit into i64 on supported targets");
    let first = resolve_slice_index(args.first(), 0, len, context)?;
    let final_index = resolve_slice_index(args.get(1), len, len, context)?;
    let first = usize::try_from(first).expect("slice start should be non-negative");
    let final_index =
        usize::try_from(final_index.max(first as i64)).expect("slice end should be non-negative");

    let next_buffer = JsArrayBuffer::new(final_index.saturating_sub(first), context)?;
    if let Some(mut bytes) = next_buffer.data_mut() {
        bytes.copy_from_slice(&current_bytes[first..final_index]);
    }

    Ok(next_buffer.into())
}

fn resolve_slice_index(
    value: Option<&BoaValue>,
    default: i64,
    len: i64,
    context: &mut Context,
) -> JsResult<i64> {
    let Some(value) = value.filter(|value| !value.is_undefined()) else {
        return Ok(default);
    };

    Ok(match value.to_integer_or_infinity(context)? {
        boa_engine::value::IntegerOrInfinity::NegativeInfinity => 0,
        boa_engine::value::IntegerOrInfinity::PositiveInfinity => len,
        boa_engine::value::IntegerOrInfinity::Integer(integer) if integer < 0 => {
            (len + integer).max(0)
        }
        boa_engine::value::IntegerOrInfinity::Integer(integer) => integer.min(len),
    })
}

fn mark_array_buffer_result_immutable(
    value: BoaValue,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("expected ArrayBuffer result from immutable helper")
    })?;
    let _ = JsArrayBuffer::from_object(object.clone()).map_err(|_| {
        JsNativeError::typ().with_message("expected ArrayBuffer result from immutable helper")
    })?;
    mark_array_buffer_immutable(&object, context)?;
    Ok(value)
}

fn data_view_buffer_is_immutable(this: &BoaValue, context: &mut Context) -> JsResult<bool> {
    let view = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DataView method called with invalid `this` value")
    })?;
    let buffer = view
        .get(js_string!("buffer"), context)?
        .as_object()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("DataView method called with invalid `this` value")
        })?;
    is_marked_immutable_array_buffer(&buffer, context)
}

fn agent_runtime(context: &Context) -> JsResult<Arc<AgentRuntime>> {
    if let Some(data) = context.get_data::<WorkerAgentContext>() {
        return Ok(data.runtime.clone());
    }
    if let Some(data) = context.get_data::<AgentRuntimeContext>() {
        return Ok(data.runtime.clone());
    }
    Err(JsNativeError::typ()
        .with_message("test262 agent runtime is unavailable")
        .into())
}

fn worker_mailbox(context: &Context) -> JsResult<Arc<AgentMailbox>> {
    context
        .get_data::<WorkerAgentContext>()
        .map(|data| data.mailbox.clone())
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("test262 worker agent context is unavailable")
                .into()
        })
}

fn script_source_from_args(args: &[BoaValue], context: &mut Context) -> JsResult<String> {
    Ok(args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_lossy())
}

fn sleep_duration_from_args(args: &[BoaValue], context: &mut Context) -> JsResult<u64> {
    let milliseconds = args.get_or_undefined(0).to_number(context)?;
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return Ok(0);
    }

    let clamped = milliseconds.min(u64::MAX as f64);
    Ok(clamped.ceil() as u64)
}

fn run_worker_agent(
    runtime: Arc<AgentRuntime>,
    mailbox: Arc<AgentMailbox>,
    source: String,
    started_tx: mpsc::Sender<Result<(), String>>,
) {
    let mut context = match Context::builder().can_block(true).build() {
        Ok(context) => context,
        Err(error) => {
            let _ = started_tx.send(Err(error.to_string()));
            runtime.unregister_agent(mailbox.id);
            return;
        }
    };

    context.insert_data(WorkerAgentContext {
        runtime: runtime.clone(),
        mailbox: mailbox.clone(),
    });

    if let Err(error) =
        install_host_globals(&mut context).and_then(|_| install_test262_globals(&mut context, true))
    {
        let _ = started_tx.send(Err(error.to_string()));
        mailbox.close();
        runtime.unregister_agent(mailbox.id);
        return;
    }

    let _ = started_tx.send(Ok(()));

    let result = context
        .eval(Source::from_bytes(source.as_str()))
        .and_then(|_| context.run_jobs());
    if let Err(error) = result {
        let error = convert_error(error, &mut context);
        runtime.push_report(format!("worker-error: {}", error));
    }

    mailbox.close();
    runtime.unregister_agent(mailbox.id);
}

fn eval_script_in_realm(
    args: &[BoaValue],
    target_realm: &Realm,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let source = script_source_from_args(args, context)?;
    let source = finalize_script_source(&source, false, None)
        .map_err(|err| JsNativeError::syntax().with_message(err.message))?;
    let result = with_realm(context, target_realm.clone(), |context| {
        let result = context.eval(Source::from_bytes(source.as_str()));
        if result.is_ok() {
            context.poison_global_environment();
        }
        result
    })?;
    context.run_jobs()?;
    Ok(result)
}
