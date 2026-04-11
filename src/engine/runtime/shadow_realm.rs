fn host_create_realm(_: &BoaValue, _: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let new_realm = context.create_realm()?;
    let new_global = with_realm(context, new_realm.clone(), |context| {
        install_child_realm_host_globals(context)?;
        install_test262_globals(context, true)?;
        Ok(context.global_object())
    })?;
    let wrapper = build_test262_object(new_realm, new_global, false, context);
    Ok(wrapper.into())
}

fn register_shadow_realm_callable(callable: BoaValue, context: &mut Context) -> JsResult<u64> {
    if callable.as_callable().is_none() {
        return Err(JsNativeError::typ()
            .with_message("ShadowRealm bridge can only register callable values")
            .into());
    }

    let host = host_hooks_context(context)?;
    let id = host.shadow_realm_next_callable_id.get();
    host.shadow_realm_next_callable_id.set(id + 1);
    host.shadow_realm_callables
        .borrow_mut()
        .insert(id, callable);
    Ok(id)
}

fn get_shadow_realm_registered_callable(id: u64, context: &mut Context) -> JsResult<BoaValue> {
    host_hooks_context(context)?
        .shadow_realm_callables
        .borrow()
        .get(&id)
        .cloned()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ShadowRealm bridge callable does not exist")
                .into()
        })
}

fn shadow_realm_target_length_and_name(
    callable: &BoaValue,
    context: &mut Context,
) -> JsResult<(f64, JsString)> {
    let object = callable.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ShadowRealm wrapped value must be callable")
    })?;

    let mut length = 0.0;
    if object.has_own_property(js_string!("length"), context)? {
        let target_length = object.get(js_string!("length"), context)?;
        if let Some(number) = target_length.as_number() {
            if number == f64::INFINITY {
                length = f64::INFINITY;
            } else if number == f64::NEG_INFINITY {
                length = 0.0;
            } else if number.is_finite() {
                length = number.trunc().max(0.0);
            }
        }
    }

    let name = object
        .get(js_string!("name"), context)?
        .as_string()
        .unwrap_or_else(|| js_string!(""));

    Ok((length, name))
}

fn shadow_realm_wrap_value_for_realm(
    value: BoaValue,
    wrapper_realm: Realm,
    context: &mut Context,
) -> JsResult<BoaValue> {
    if value.as_callable().is_some() {
        return create_shadow_realm_wrapped_function_for_realm(value, wrapper_realm, context);
    }

    if value.is_object() {
        return Err(JsNativeError::typ()
            .with_message("ShadowRealm values must be primitive or callable")
            .into());
    }

    Ok(value)
}

fn shadow_realm_convert_args_for_realm(
    args: &[BoaValue],
    target_realm: Realm,
    context: &mut Context,
) -> JsResult<Vec<BoaValue>> {
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        if arg.as_callable().is_some() {
            converted.push(create_shadow_realm_wrapped_function_for_realm(
                arg.clone(),
                target_realm.clone(),
                context,
            )?);
            continue;
        }
        if arg.is_object() {
            return Err(JsNativeError::typ()
                .with_message("ShadowRealm wrapped functions only accept primitives or callables")
                .into());
        }
        converted.push(arg.clone());
    }
    Ok(converted)
}

fn create_shadow_realm_wrapped_function_for_realm(
    callable: BoaValue,
    wrapper_realm: Realm,
    context: &mut Context,
) -> JsResult<BoaValue> {
    if callable.as_callable().is_none() {
        return Err(JsNativeError::typ()
            .with_message("ShadowRealm wrapped value must be callable")
            .into());
    }

    let callable_object = callable.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ShadowRealm wrapped value must be callable")
    })?;
    let foreign_realm = callable_object.get_function_realm(context)?;
    let callable_id = register_shadow_realm_callable(callable.clone(), context)?;
    let (length, name) = shadow_realm_target_length_and_name(&callable, context)
        .map_err(|_| JsNativeError::typ().with_message("WrappedFunctionCreate failed"))?;

    let wrapper: JsObject = FunctionObjectBuilder::new(
        &wrapper_realm,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, capture, context| {
                let wrapper_realm = capture.wrapper_realm.clone();
                let converted_args = shadow_realm_convert_args_for_realm(
                    args,
                    capture.foreign_realm.clone(),
                    context,
                )
                .map_err(|_| {
                    JsNativeError::typ()
                        .with_message("Wrapped function invocation failed")
                        .with_realm(wrapper_realm.clone())
                })?;
                let callable = get_shadow_realm_registered_callable(capture.callable_id, context)?;
                let function = callable.as_callable().ok_or_else(|| {
                    JsNativeError::typ().with_message("ShadowRealm bridge callable is not callable")
                })?;

                let result = function
                    .call(&BoaValue::undefined(), &converted_args, context)
                    .map_err(|_| {
                        JsNativeError::typ()
                            .with_message("Wrapped function invocation failed")
                            .with_realm(wrapper_realm.clone())
                    })?;

                shadow_realm_wrap_value_for_realm(result, wrapper_realm.clone(), context).map_err(|_| {
                    JsNativeError::typ()
                        .with_message("ShadowRealm values must be primitive or callable")
                        .with_realm(wrapper_realm)
                        .into()
                })
            },
            ShadowRealmWrappedFunctionCapture {
                callable_id,
                foreign_realm,
                wrapper_realm: wrapper_realm.clone(),
            },
        ),
    )
    .name(js_string!(""))
    .length(0)
    .constructor(false)
    .build()
    .into();

    wrapper.define_property_or_throw(
        js_string!("length"),
        PropertyDescriptor::builder()
            .value(length)
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    wrapper.define_property_or_throw(
        js_string!("name"),
        PropertyDescriptor::builder()
            .value(name.clone())
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    Ok(wrapper.into())
}

fn host_shadow_realm_register_callable(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let callable = args.get_or_undefined(0).clone();
    let id = register_shadow_realm_callable(callable, context)?;
    Ok((id as f64).into())
}

fn host_shadow_realm_invoke(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let id = args.get_or_undefined(0).to_number(context)?;
    if !id.is_finite() || id < 0.0 || id.fract() != 0.0 {
        return Err(JsNativeError::typ()
            .with_message("ShadowRealm bridge id must be a non-negative integer")
            .into());
    }
    let id = id as u64;

    let callable = get_shadow_realm_registered_callable(id, context)?;
    let function = callable.as_callable().ok_or_else(|| {
        JsNativeError::typ().with_message("ShadowRealm bridge callable is not callable")
    })?;
    function.call(&BoaValue::undefined(), &args[1..], context)
}

fn host_shadow_realm_wrap_callable(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let (wrapper_realm, callable) = if args.len() >= 2 {
        let carrier = args.get_or_undefined(0);
        let carrier_object = carrier.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ShadowRealm wrapper realm carrier must be callable")
        })?;
        let wrapper_realm = carrier_object.get_function_realm(context)?;
        (wrapper_realm, args.get_or_undefined(1).clone())
    } else {
        (context.realm().clone(), args.get_or_undefined(0).clone())
    };

    create_shadow_realm_wrapped_function_for_realm(callable, wrapper_realm, context)
}

fn host_shadow_realm_dynamic_import(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let specifier = args.get_or_undefined(0).to_string(context)?;
    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Ok(JsPromise::reject(
            JsNativeError::typ().with_message("dynamic import is unavailable"),
            context,
        )
        .into());
    };

    let referrer = context.stack_trace().find_map(|frame| {
        let rendered = frame.position().path.to_string();
        if rendered.is_empty() {
            None
        } else {
            let candidate = PathBuf::from(rendered);
            if candidate.exists() {
                Some(candidate.canonicalize().unwrap_or(candidate))
            } else {
                None
            }
        }
    });

    let path =
        resolve_module_specifier(Some(&loader.root), &specifier, referrer.as_deref(), context)?;

    let module = if let Some(module) = loader.get(&path, ModuleResourceKind::JavaScript) {
        module
    } else {
        let module = load_module_from_path(&path, ModuleResourceKind::JavaScript, context)?;
        loader.insert(path.clone(), ModuleResourceKind::JavaScript, module.clone());
        module
    };

    let promise = module.load_link_evaluate(context);
    for _ in 0..16 {
        context.run_jobs()?;
        if !matches!(promise.state(), PromiseState::Pending) {
            break;
        }
    }
    match promise.state() {
        PromiseState::Fulfilled(_) => {
            Ok(JsPromise::resolve(module.namespace(context), context).into())
        }
        PromiseState::Rejected(reason) => {
            Ok(JsPromise::reject(JsError::from_opaque(reason.clone()), context).into())
        }
        PromiseState::Pending => Ok(promise.into()),
    }
}

fn host_shadow_realm_can_parse_script(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let source = script_source_from_args(args, context)?;
    Ok(
        Script::parse(Source::from_bytes(source.as_str()), None, context)
            .is_ok()
            .into(),
    )
}

