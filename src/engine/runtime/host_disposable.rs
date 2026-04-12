fn host_async_disposable_stack_constructor(
    new_target: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("Constructor AsyncDisposableStack requires new")
            .into());
    }

    let prototype = get_prototype_from_custom_constructor(
        new_target,
        "AsyncDisposableStack",
        context,
    )?;

    let instance = JsObject::from_proto_and_data(Some(prototype), AsyncDisposableStackData::new());

    Ok(instance.into())
}

fn host_async_disposable_stack_use(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.use requires an object receiver")
    })?;
    let data = obj.downcast_ref::<AsyncDisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for AsyncDisposableStack.prototype.use")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("AsyncDisposableStack is disposed")
            .into());
    }

    let value = args.get_or_undefined(0);
    if value.is_null_or_undefined() {
        data.stack.borrow_mut().push(AsyncDisposableResource {
            value: BoaValue::undefined(),
            method: BoaValue::undefined(),
            needs_await: true,
        });
        return Ok(value.clone());
    }

    let value_obj = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.use requires an object value")
    })?;

    let symbol_ctor = context.intrinsics().constructors().symbol().constructor();
    let for_method = symbol_ctor.get(js_string!("for"), context)?;
    let async_dispose_symbol = for_method
        .as_callable()
        .unwrap()
        .call(
            &symbol_ctor.clone().into(),
            &[js_string!("Symbol.asyncDispose").into()],
            context,
        )?;
    let mut method = value_obj.get(async_dispose_symbol.to_property_key(context)?, context)?;
    let mut needs_await = true;

    if method.is_undefined() {
        let dispose_symbol = for_method
            .as_callable()
            .unwrap()
            .call(
                &symbol_ctor.into(),
                &[js_string!("Symbol.dispose").into()],
                context,
            )?;
        method = value_obj.get(dispose_symbol.to_property_key(context)?, context)?;
        needs_await = false;
    }

    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message(
                "Disposable value must have a callable Symbol.asyncDispose or Symbol.dispose",
            )
            .into());
    }

    data.stack.borrow_mut().push(AsyncDisposableResource {
        value: value.clone(),
        method,
        needs_await,
    });

    Ok(value.clone())
}

fn host_async_disposable_stack_adopt(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.adopt requires an object receiver")
    })?;
    let data = obj.downcast_ref::<AsyncDisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for AsyncDisposableStack.prototype.adopt")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("AsyncDisposableStack is disposed")
            .into());
    }

    let value = args.get_or_undefined(0);
    let on_dispose_async = args.get_or_undefined(1);
    if !on_dispose_async.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("onDisposeAsync must be callable")
            .into());
    }

    let value_capture = value.clone();
    let on_dispose_async_capture = on_dispose_async.clone();
    let method = NativeFunction::from_copy_closure_with_captures(
        move |_this, _args, (v, f), context| {
            let f_obj = f.as_object().expect("onDisposeAsync must be an object");
            f_obj.call(&BoaValue::undefined(), &[v.clone()], context)
        },
        (value_capture, on_dispose_async_capture),
    );

    data.stack.borrow_mut().push(AsyncDisposableResource {
        value: BoaValue::undefined(),
        method: method.to_js_function(context.realm()).into(),
        needs_await: true,
    });

    Ok(value.clone())
}

fn host_async_disposable_stack_defer(
    this: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.defer requires an object receiver")
    })?;
    let data = obj.downcast_ref::<AsyncDisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for AsyncDisposableStack.prototype.defer")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("AsyncDisposableStack is disposed")
            .into());
    }

    let on_dispose_async = args.get_or_undefined(0);
    if !on_dispose_async.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("onDisposeAsync must be callable")
            .into());
    }

    data.stack.borrow_mut().push(AsyncDisposableResource {
        value: BoaValue::undefined(),
        method: on_dispose_async.clone(),
        needs_await: true,
    });

    Ok(BoaValue::undefined())
}

fn host_async_disposable_stack_move(
    this: &BoaValue,
    _args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.move requires an object receiver")
    })?;
    let data = obj.downcast_ref::<AsyncDisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for AsyncDisposableStack.prototype.move")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("AsyncDisposableStack is disposed")
            .into());
    }

    let prototype = get_custom_intrinsic_prototype("AsyncDisposableStack", _context)?;
    let instance = JsObject::from_proto_and_data(Some(prototype), AsyncDisposableStackData::new());

    {
        let next_data = instance
            .downcast_ref::<AsyncDisposableStackData>()
            .unwrap();
        *next_data.stack.borrow_mut() = std::mem::take(&mut *data.stack.borrow_mut());
    }
    data.status.set(DisposableStackStatus::Disposed);

    Ok(instance.into())
}

fn host_async_disposable_stack_dispose_async(
    this: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = match this.as_object() {
        Some(obj) => obj,
        None => {
            return Ok(JsPromise::reject(
                JsNativeError::typ()
                    .with_message("AsyncDisposableStack.prototype.disposeAsync requires an object receiver"),
                context,
            ).into());
        }
    };
    let data = match obj.downcast_ref::<AsyncDisposableStackData>() {
        Some(data) => data,
        None => {
            return Ok(JsPromise::reject(
                JsNativeError::typ()
                    .with_message("Incompatible receiver for AsyncDisposableStack.prototype.disposeAsync"),
                context,
            ).into());
        }
    };

    if data.status.get() == DisposableStackStatus::Disposed {
        return Ok(JsPromise::resolve(BoaValue::undefined(), context).into());
    }

    data.status.set(DisposableStackStatus::Disposed);

    let resources = std::mem::take(&mut *data.stack.borrow_mut());
    if resources.is_empty() {
        return Ok(JsPromise::resolve(BoaValue::undefined(), context).into());
    }

    let mut js_resources = Vec::with_capacity(resources.len());
    for res in resources {
        let res_obj = ObjectInitializer::new(context)
            .property(js_string!("value"), res.value, Attribute::all())
            .property(js_string!("method"), res.method, Attribute::all())
            .property(js_string!("needsAwait"), res.needs_await, Attribute::all())
            .build();
        js_resources.push(res_obj.into());
    }
    let resources_array = JsArray::from_iter(js_resources, context);

    let loop_fn = context.eval(Source::from_bytes(
        r#"
        (async function(resources) {
            let hasCompletion = false;
            let completion;
            while (resources.length > 0) {
                const resource = resources.pop();
                try {
                    if (resource.method !== undefined) {
                        if (resource.needsAwait) {
                            await resource.method.call(resource.value);
                        } else {
                            resource.method.call(resource.value);
                        }
                    } else if (resource.needsAwait) {
                        await undefined;
                    }
                } catch (error) {
                    if (!hasCompletion) {
                        completion = error;
                        hasCompletion = true;
                    } else {
                        completion = new SuppressedError(error, completion, undefined);
                    }
                }
            }
            if (hasCompletion) {
                throw completion;
            }
        })
        "#,
    ))?;

    if let Some(callable) = loop_fn.as_object() {
        callable.call(&BoaValue::undefined(), &[resources_array.into()], context)
    } else {
        Err(JsNativeError::typ()
            .with_message("Internal error: loop_fn is not callable")
            .into())
    }
}

fn host_async_disposable_stack_disposed_getter(
    this: &BoaValue,
    _args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("AsyncDisposableStack.prototype.disposed requires an object receiver")
    })?;
    let data = obj.downcast_ref::<AsyncDisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for AsyncDisposableStack.prototype.disposed")
    })?;

    Ok((data.status.get() == DisposableStackStatus::Disposed).into())
}

fn host_dispose_sync_using(
    _this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let stack = args.get_or_undefined(0);
    let has_body_error = args.get_or_undefined(1).to_boolean();
    let body_error = args.get_or_undefined(2).clone();

    let stack_obj = stack.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("__agentjsDisposeSyncUsing__ requires an object stack")
    })?;

    let dispose_method = stack_obj.get(js_string!("dispose"), context)?;
    let result = if let Some(callable) = dispose_method.as_object() {
        callable.call(stack, &[], context)
    } else {
        return Err(JsNativeError::typ().with_message("dispose is not callable").into());
    };
    if let Err(dispose_error) = result {
        if has_body_error {
            let suppressed_error_ctor = context
                .global_object()
                .get(js_string!("SuppressedError"), context)?;
            if let Some(ctor) = suppressed_error_ctor.as_object() {
                let err = ctor.call(
                    &BoaValue::undefined(),
                    &[dispose_error.to_opaque(context).into(), body_error, BoaValue::undefined()],
                    context,
                )?;
                return Err(JsError::from_opaque(err));
            }
        }
        return Err(dispose_error);
    }

    Ok(BoaValue::undefined())
}

fn host_dispose_async_using(
    _this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let stack = args.get_or_undefined(0).clone();
    let has_body_error = args.get_or_undefined(1).to_boolean();
    let body_error = args.get_or_undefined(2).clone();

    let stack_obj = stack.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("__agentjsDisposeAsyncUsing__ requires an object stack")
    })?;

    let dispose_async_method = stack_obj.get(js_string!("disposeAsync"), context)?;
    let result = if let Some(callable) = dispose_async_method.as_object() {
        callable.call(&stack, &[], context)?
    } else {
        return Err(JsNativeError::typ().with_message("disposeAsync is not callable").into());
    };
    let promise_obj = result.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("disposeAsync must return an object")
    })?;
    let promise = JsPromise::from_object(promise_obj.clone())?;

    if has_body_error {
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            move |_this, args, captures, context| {
                let dispose_error = args.get_or_undefined(0).clone();
                let body_error = captures.clone();
                let suppressed_error_ctor = context
                    .global_object()
                    .get(js_string!("SuppressedError"), context)?;
                if let Some(ctor) = suppressed_error_ctor.as_object() {
                    let err = ctor.call(
                        &BoaValue::undefined(),
                        &[dispose_error, body_error, BoaValue::undefined()],
                        context,
                    )?;
                    return Err(JsError::from_opaque(err));
                }
                Err(JsError::from_opaque(dispose_error))
            },
            body_error,
        );
        Ok(promise
            .then(
                None,
                Some(on_rejected.to_js_function(context.realm())),
                context,
            )
            .into())
    } else {
        Ok(promise.into())
    }
}

fn host_disposable_stack_constructor(
    new_target: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("Constructor DisposableStack requires new")
            .into());
    }

    let prototype = get_prototype_from_custom_constructor(
        new_target,
        "DisposableStack",
        context,
    )?;

    let instance = JsObject::from_proto_and_data(Some(prototype), DisposableStackData::new());

    Ok(instance.into())
}

fn host_disposable_stack_use(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DisposableStack.prototype.use requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ().with_message("Incompatible receiver for DisposableStack.prototype.use")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("DisposableStack is disposed")
            .into());
    }

    let value = args.get_or_undefined(0);
    if value.is_null_or_undefined() {
        return Ok(value.clone());
    }

    let value_obj = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.use requires an object value")
    })?;

    let symbol_ctor = context.intrinsics().constructors().symbol().constructor();
    let for_method = symbol_ctor.get(js_string!("for"), context)?;
    let dispose_symbol = for_method
        .as_callable()
        .unwrap()
        .call(
            &symbol_ctor.into(),
            &[js_string!("Symbol.dispose").into()],
            context,
        )?;
    let method = value_obj.get(dispose_symbol.to_property_key(context)?, context)?;
    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("Disposable value must have a callable Symbol.dispose")
            .into());
    }

    data.stack.borrow_mut().push(DisposableResource {
        value: value.clone(),
        method,
    });

    Ok(value.clone())
}

fn host_disposable_stack_adopt(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.adopt requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for DisposableStack.prototype.adopt")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("DisposableStack is disposed")
            .into());
    }

    let value = args.get_or_undefined(0);
    let on_dispose = args.get_or_undefined(1);
    if !on_dispose.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("onDispose must be callable")
            .into());
    }

    let value_capture = value.clone();
    let on_dispose_capture = on_dispose.clone();
    let method = NativeFunction::from_copy_closure_with_captures(
        move |_this, _args, (v, f), context| {
            let f_obj = f.as_object().expect("onDispose must be an object");
            f_obj.call(&BoaValue::undefined(), &[v.clone()], context)
        },
        (value_capture, on_dispose_capture),
    );

    data.stack.borrow_mut().push(DisposableResource {
        value: BoaValue::undefined(),
        method: method.to_js_function(context.realm()).into(),
    });

    Ok(value.clone())
}

fn host_disposable_stack_defer(
    this: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.defer requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for DisposableStack.prototype.defer")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("DisposableStack is disposed")
            .into());
    }

    let on_dispose = args.get_or_undefined(0);
    if !on_dispose.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("onDispose must be callable")
            .into());
    }

    data.stack.borrow_mut().push(DisposableResource {
        value: BoaValue::undefined(),
        method: on_dispose.clone(),
    });

    Ok(BoaValue::undefined())
}

fn host_disposable_stack_move(
    this: &BoaValue,
    _args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.move requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ().with_message("Incompatible receiver for DisposableStack.prototype.move")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Err(JsNativeError::reference()
            .with_message("DisposableStack is disposed")
            .into());
    }

    let prototype = get_custom_intrinsic_prototype("DisposableStack", _context)?;
    let instance = JsObject::from_proto_and_data(Some(prototype), DisposableStackData::new());

    {
        let next_data = instance.downcast_ref::<DisposableStackData>().unwrap();
        *next_data.stack.borrow_mut() = std::mem::take(&mut *data.stack.borrow_mut());
    }
    data.status.set(DisposableStackStatus::Disposed);

    Ok(instance.into())
}


fn host_disposable_stack_dispose(
    this: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.dispose requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for DisposableStack.prototype.dispose")
    })?;

    if data.status.get() == DisposableStackStatus::Disposed {
        return Ok(BoaValue::undefined());
    }

    data.status.set(DisposableStackStatus::Disposed);

    let mut resources = std::mem::take(&mut *data.stack.borrow_mut());
    let mut errors = Vec::new();

    while let Some(resource) = resources.pop() {
        let method_obj = resource.method.as_object().expect("method must be an object");
        if let Err(e) = method_obj.call(&resource.value, &[], context) {
            errors.push(e.to_opaque(context));
        }
    }

    if errors.is_empty() {
        Ok(BoaValue::undefined())
    } else {
        let mut completion = errors.remove(0);
        let suppressed_error_ctor = context
            .global_object()
            .get(js_string!("SuppressedError"), context)?;

        if let Some(ctor) = suppressed_error_ctor.as_object() {
            for error in errors {
                completion = ctor.call(
                    &BoaValue::undefined(),
                    &[error, completion, BoaValue::undefined()],
                    context,
                )?;
            }
        }
        Err(JsError::from_opaque(completion))
    }
}

fn host_disposable_stack_disposed_getter(
    this: &BoaValue,
    _args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DisposableStack.prototype.disposed requires an object receiver")
    })?;
    let data = obj.downcast_ref::<DisposableStackData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Incompatible receiver for DisposableStack.prototype.disposed")
    })?;

    Ok((data.status.get() == DisposableStackStatus::Disposed).into())
}

fn host_async_iterator_dispose(
    this: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = match this.to_object(context) {
        Ok(obj) => obj,
        Err(e) => return Ok(JsPromise::reject(e, context).into()),
    };
    let return_method = match obj.get(js_string!("return"), context) {
        Ok(v) => v,
        Err(e) => return Ok(JsPromise::reject(e, context).into()),
    };
    if let Some(callable) = return_method.as_object() {
        match callable.call(this, &[], context) {
            Ok(v) => Ok(v),
            Err(e) => Ok(JsPromise::reject(e, context).into()),
        }
    } else {
        Ok(JsPromise::resolve(BoaValue::undefined(), context).into())
    }
}

fn host_iterator_dispose(
    this: &BoaValue,
    _args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let return_method = obj.get(js_string!("return"), context)?;
    if let Some(callable) = return_method.as_object() {
        callable.call(this, &[], context)
    } else {
        Ok(BoaValue::undefined())
    }
}

fn host_suppressed_error_constructor(
    new_target: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let error = args.get_or_undefined(0).clone();
    let suppressed = args.get_or_undefined(1).clone();
    let message = args.get_or_undefined(2);

    let new_target = if new_target.is_undefined() {
        context
            .global_object()
            .get(js_string!("SuppressedError"), context)
            .unwrap_or_else(|_| BoaValue::undefined())
    } else {
        new_target.clone()
    };

    let prototype = get_prototype_from_custom_constructor(
        &new_target,
        "SuppressedError",
        context,
    )?;

    let error_constructor = context.intrinsics().constructors().error().constructor();

    let message_args = if message.is_undefined() {
        vec![]
    } else {
        vec![message.clone()]
    };

    let instance = if error_constructor.is_callable() {
        error_constructor.call(&BoaValue::undefined(), &message_args, context)?
    } else {
        return Err(JsNativeError::typ()
            .with_message("Error constructor is not callable")
            .into());
    };
    let instance_obj = instance
        .as_object()
        .expect("Error constructor must return an object");

    instance_obj.set_prototype(Some(prototype));

    instance_obj.define_property_or_throw(
        js_string!("error"),
        PropertyDescriptor::builder()
            .value(error)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    instance_obj.define_property_or_throw(
        js_string!("suppressed"),
        PropertyDescriptor::builder()
            .value(suppressed)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(instance)
}

fn host_promise_all_keyed(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let promise_constructor = if let Some(obj) = this.as_object() {
        if obj.is_callable() {
            obj.clone()
        } else {
            context.intrinsics().constructors().promise().constructor()
        }
    } else {
        context.intrinsics().constructors().promise().constructor()
    };

    let items = args.get_or_undefined(0);
    if items.is_null() || items.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("Promise keyed methods require an object argument")
            .into());
    }
    let dictionary = items.to_object(context)?;
    let object_ctor = context.intrinsics().constructors().object().constructor();
    let keys_method = object_ctor.get(js_string!("keys"), context)?;
    let keys_val = if let Some(callable) = keys_method.as_object() {
        callable.call(&object_ctor.into(), &[items.clone()], context)?
    } else {
        return Err(JsNativeError::typ().with_message("Object.keys is not callable").into());
    };
    let keys_obj = keys_val.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Object.keys must return an object")
    })?;
    let keys_array = JsArray::from_object(keys_obj.clone())?;
    let keys_len = keys_array.length(context)?;

    let mut values = Vec::with_capacity(keys_len as usize);
    let mut keys_values = Vec::with_capacity(keys_len as usize);
    for i in 0..keys_len {
        let key_val = keys_array.at(i as i64, context)?;
        let key = key_val.to_property_key(context)?;
        keys_values.push(key_val.clone());
        values.push(dictionary.get(key, context)?);
    }

    let values_array = JsArray::from_iter(values, context);

    let all_method = promise_constructor.get(js_string!("all"), context)?;
    let promise_value = if let Some(callable) = all_method.as_object() {
        callable.call(
            &promise_constructor.clone().into(),
            &[values_array.into()],
            context,
        )?
    } else {
        return Err(JsNativeError::typ().with_message("Promise.all is not callable").into());
    };
    let promise_obj = promise_value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Promise.all must return an object")
    })?;
    let promise = JsPromise::from_object(promise_obj.clone())?;

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, captures, context| {
            let results = args.get_or_undefined(0);
            let results_obj = results.as_object().ok_or_else(|| {
                JsNativeError::typ().with_message("Promise.all result must be an object")
            })?;
            let results_array = JsArray::from_object(results_obj.clone())?;
            let output = JsObject::with_null_proto();
            for (i, key_value) in captures.iter().enumerate() {
                let result = results_array.at(i as i64, context)?;
                let key = key_value.to_property_key(context)?;
                output.create_data_property_or_throw(key, result, context)?;
            }
            Ok(output.into())
        },
        keys_values,
    );

    Ok(promise
        .then(
            Some(on_fulfilled.to_js_function(context.realm())),
            None,
            context,
        )
        .into())
}

fn host_promise_all_settled_keyed(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let promise_constructor = if let Some(obj) = this.as_object() {
        if obj.is_callable() {
            obj.clone()
        } else {
            context.intrinsics().constructors().promise().constructor()
        }
    } else {
        context.intrinsics().constructors().promise().constructor()
    };

    let items = args.get_or_undefined(0);
    if items.is_null() || items.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("Promise keyed methods require an object argument")
            .into());
    }
    let dictionary = items.to_object(context)?;
    let object_ctor = context.intrinsics().constructors().object().constructor();
    let keys_method = object_ctor.get(js_string!("keys"), context)?;
    let keys_val = if let Some(callable) = keys_method.as_object() {
        callable.call(&object_ctor.into(), &[items.clone()], context)?
    } else {
        return Err(JsNativeError::typ().with_message("Object.keys is not callable").into());
    };
    let keys_obj = keys_val.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Object.keys must return an object")
    })?;
    let keys_array = JsArray::from_object(keys_obj.clone())?;
    let keys_len = keys_array.length(context)?;

    let mut values = Vec::with_capacity(keys_len as usize);
    let mut keys_values = Vec::with_capacity(keys_len as usize);
    for i in 0..keys_len {
        let key_val = keys_array.at(i as i64, context)?;
        let key = key_val.to_property_key(context)?;
        keys_values.push(key_val.clone());
        values.push(dictionary.get(key, context)?);
    }

    let values_array = JsArray::from_iter(values, context);

    let all_settled_method = promise_constructor.get(js_string!("allSettled"), context)?;
    let promise_value = if let Some(callable) = all_settled_method.as_object() {
        callable.call(
            &promise_constructor.clone().into(),
            &[values_array.into()],
            context,
        )?
    } else {
        return Err(JsNativeError::typ().with_message("Promise.allSettled is not callable").into());
    };
    let promise_obj = promise_value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Promise.allSettled must return an object")
    })?;
    let promise = JsPromise::from_object(promise_obj.clone())?;

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, captures, context| {
            let results = args.get_or_undefined(0);
            let results_obj = results.as_object().ok_or_else(|| {
                JsNativeError::typ().with_message("Promise.allSettled result must be an object")
            })?;
            let results_array = JsArray::from_object(results_obj.clone())?;
            let output = JsObject::with_null_proto();
            for (i, key_value) in captures.iter().enumerate() {
                let result = results_array.at(i as i64, context)?;
                let key = key_value.to_property_key(context)?;
                output.create_data_property_or_throw(key, result, context)?;
            }
            Ok(output.into())
        },
        keys_values,
    );

    Ok(promise
        .then(
            Some(on_fulfilled.to_js_function(context.realm())),
            None,
            context,
        )
        .into())
}
