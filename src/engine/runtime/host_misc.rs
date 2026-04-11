fn host_gc(_: &BoaValue, _: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
    Ok(BoaValue::undefined())
}

fn host_detach_array_buffer(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = args.get_or_undefined(0).as_object().ok_or_else(|| {
        boa_engine::JsNativeError::typ().with_message("detachArrayBuffer requires an ArrayBuffer")
    })?;
    let buffer = JsArrayBuffer::from_object(buffer)?;
    buffer.detach(&BoaValue::undefined())?;
    Ok(BoaValue::undefined())
}

fn host_array_buffer_detached_getter(
    this: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("get ArrayBuffer.prototype.detached called with invalid `this`")
    })?;
    let buffer = JsArrayBuffer::from_object(object).map_err(|_| {
        JsNativeError::typ()
            .with_message("get ArrayBuffer.prototype.detached called with invalid `this`")
    })?;
    Ok(buffer.data().is_none().into())
}

fn host_array_buffer_immutable_getter(
    this: &BoaValue,
    _: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    Ok(is_marked_immutable_array_buffer(&buffer.clone().into(), context)?.into())
}

fn host_array_buffer_resize_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        return Err(immutable_array_buffer_error("resize"));
    }
    call_hidden_array_buffer_method("resize", this, args, context)
}

fn host_array_buffer_transfer_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transfer"));
    }
    if has_hidden_array_buffer_method(context, "transfer")? {
        call_hidden_array_buffer_method("transfer", this, args, context)
    } else {
        array_buffer_transfer_copy_and_detach(this, args, true, context)
    }
}

fn host_array_buffer_transfer_to_fixed_length_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transferToFixedLength"));
    }
    if has_hidden_array_buffer_method(context, "transferToFixedLength")? {
        call_hidden_array_buffer_method("transferToFixedLength", this, args, context)
    } else {
        array_buffer_transfer_copy_and_detach(this, args, false, context)
    }
}

fn host_array_buffer_slice_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let result = call_hidden_array_buffer_method("slice", this, args, context)?;
    let Some(object) = result.as_object() else {
        return Ok(result);
    };
    if JsArrayBuffer::from_object(object.clone()).is_ok()
        && is_marked_immutable_array_buffer(&object, context)?
    {
        return Err(immutable_array_buffer_error("slice"));
    }
    Ok(result)
}

fn host_array_buffer_transfer_to_immutable(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transferToImmutable"));
    }
    let result = if has_hidden_array_buffer_method(context, "transfer")? {
        call_hidden_array_buffer_method("transfer", this, args, context)?
    } else {
        array_buffer_transfer_copy_and_detach(this, args, false, context)?
    };
    mark_array_buffer_result_immutable(result, context)
}

fn host_array_buffer_slice_to_immutable(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let result = array_buffer_slice_copy(this, args, context)?;
    mark_array_buffer_result_immutable(result, context)
}

fn host_abstract_module_source_constructor(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Err(JsNativeError::typ()
        .with_message("%AbstractModuleSource% constructor cannot be invoked directly")
        .into())
}

fn host_abstract_module_source_to_string_tag(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(BoaValue::undefined())
}

fn host_agent_start(_: &BoaValue, args: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
    let source = script_source_from_args(args, _context)?;
    agent_runtime(_context)?.start_worker(source)?;
    Ok(BoaValue::undefined())
}

fn host_agent_broadcast(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = args.get_or_undefined(0).as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("broadcast requires a SharedArrayBuffer")
    })?;
    let buffer = JsSharedArrayBuffer::from_object(buffer)?.inner();
    agent_runtime(context)?.broadcast(buffer);
    Ok(BoaValue::undefined())
}

fn host_agent_get_report(
    _: &BoaValue,
    _: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(agent_runtime(context)?
        .pop_report()
        .map_or_else(BoaValue::null, |report| {
            BoaValue::from(boa_engine::JsString::from(report.as_str()))
        }))
}

fn host_agent_sleep(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    agent_runtime(context)?.sleep(sleep_duration_from_args(args, context)?);
    Ok(BoaValue::undefined())
}

fn host_agent_monotonic_now(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(agent_runtime(_context)?
.monotonic_now().into())
}

fn host_worker_receive_broadcast(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let callback = args
        .get_or_undefined(0)
        .as_callable()
        .ok_or_else(|| JsNativeError::typ().with_message("receiveBroadcast requires a callback"))?;
    let envelope = worker_mailbox(_context)?.receive().ok_or_else(|| {
        JsNativeError::typ().with_message("test262 worker agent mailbox is closed")
    })?;
    let sab = JsSharedArrayBuffer::from_buffer(envelope.buffer.clone(), _context);
    envelope.completion.acknowledge();
    let result = callback.call(&BoaValue::undefined(), &[sab.into()], _context);
    result?;
    _context.run_jobs()?;
    Ok(BoaValue::undefined())
}

fn host_worker_report(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let report = args
        .get_or_undefined(0)
        .to_string(_context)?
        .to_std_string_lossy();
    agent_runtime(_context)?.push_report(report);
    Ok(BoaValue::undefined())
}

fn get_custom_intrinsic_prototype(name: &str, context: &mut Context) -> JsResult<JsObject> {
    let ctor: BoaValue = context.global_object().get(js_string!(name), context)?;
    if let Some(ctor_obj) = ctor.as_object() {
        let prototype: BoaValue = ctor_obj.get(js_string!("prototype"), context)?;
        if let Some(proto_obj) = prototype.as_object() {
            return Ok(proto_obj.clone());
        }
    }
    Ok(context.intrinsics().constructors().object().prototype())
}

fn get_prototype_from_custom_constructor(
    new_target: &BoaValue,
    intrinsic_name: &str,
    context: &mut Context,
) -> JsResult<JsObject> {
    if let Some(new_target_obj) = new_target.as_object() {
        let prototype: BoaValue = new_target_obj.get(js_string!("prototype"), context)?;
        if let Some(proto_obj) = prototype.as_object() {
            return Ok(proto_obj.clone());
        }
    }

    let realm = new_target
        .as_object()
        .expect("new_target must be an object")
        .get_function_realm(context)?;
    let global: JsObject = realm.global_object();
    let ctor: BoaValue = global.get(js_string!(intrinsic_name), context)?;
    if let Some(ctor_obj) = ctor.as_object() {
        let prototype: BoaValue = ctor_obj.get(js_string!("prototype"), context)?;
        if let Some(proto_obj) = prototype.as_object() {
            return Ok(proto_obj.clone());
        }
    }
    Ok(realm.intrinsics().constructors().object().prototype())
}

