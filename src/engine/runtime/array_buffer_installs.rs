fn install_array_buffer_detached_getter(context: &mut Context) -> boa_engine::JsResult<()> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();
    if prototype.has_own_property(js_string!("detached"), context)? {
        return Ok(());
    }

    let getter = build_builtin_function(
        context,
        js_string!("get detached"),
        0,
        NativeFunction::from_fn_ptr(host_array_buffer_detached_getter),
    );
    prototype.define_property_or_throw(
        js_string!("detached"),
        PropertyDescriptor::builder()
            .get(getter)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    Ok(())
}

fn install_array_buffer_immutable_hooks(context: &mut Context) -> boa_engine::JsResult<()> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();

    if !prototype.has_own_property(js_string!("immutable"), context)? {
        let getter = build_builtin_function(
            context,
            js_string!("get immutable"),
            0,
            NativeFunction::from_fn_ptr(host_array_buffer_immutable_getter),
        );
        prototype.define_property_or_throw(
            js_string!("immutable"),
            PropertyDescriptor::builder()
                .get(getter)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    install_prototype_method_wrapper(
        &prototype,
        "resize",
        1,
        array_buffer_original_symbol(context, "resize")?,
        NativeFunction::from_fn_ptr(host_array_buffer_resize_wrapper),
        context,
    )?;
    install_prototype_method_wrapper(
        &prototype,
        "transfer",
        0,
        array_buffer_original_symbol(context, "transfer")?,
        NativeFunction::from_fn_ptr(host_array_buffer_transfer_wrapper),
        context,
    )?;
    install_prototype_method_wrapper(
        &prototype,
        "transferToFixedLength",
        0,
        array_buffer_original_symbol(context, "transferToFixedLength")?,
        NativeFunction::from_fn_ptr(host_array_buffer_transfer_to_fixed_length_wrapper),
        context,
    )?;
    install_prototype_method_wrapper(
        &prototype,
        "slice",
        2,
        array_buffer_original_symbol(context, "slice")?,
        NativeFunction::from_fn_ptr(host_array_buffer_slice_wrapper),
        context,
    )?;
    install_prototype_method_if_missing(
        &prototype,
        "transfer",
        0,
        NativeFunction::from_fn_ptr(host_array_buffer_transfer_wrapper),
        context,
    )?;
    install_prototype_method_if_missing(
        &prototype,
        "transferToFixedLength",
        0,
        NativeFunction::from_fn_ptr(host_array_buffer_transfer_to_fixed_length_wrapper),
        context,
    )?;

    if !prototype.has_own_property(js_string!("transferToImmutable"), context)? {
        let transfer = prototype.get(js_string!("transfer"), context)?;
        if transfer.as_callable().is_some() {
            let method = build_builtin_function(
                context,
                js_string!("transferToImmutable"),
                0,
                NativeFunction::from_fn_ptr(host_array_buffer_transfer_to_immutable),
            );
            prototype.define_property_or_throw(
                js_string!("transferToImmutable"),
                PropertyDescriptor::builder()
                    .value(method)
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }

    if !prototype.has_own_property(js_string!("sliceToImmutable"), context)? {
        let slice = prototype.get(js_string!("slice"), context)?;
        if slice.as_callable().is_some() {
            let method = build_builtin_function(
                context,
                js_string!("sliceToImmutable"),
                2,
                NativeFunction::from_fn_ptr(host_array_buffer_slice_to_immutable),
            );
            prototype.define_property_or_throw(
                js_string!("sliceToImmutable"),
                PropertyDescriptor::builder()
                    .value(method)
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }

    Ok(())
}

fn install_data_view_immutable_hooks(context: &mut Context) -> boa_engine::JsResult<()> {
    let prototype = context.intrinsics().constructors().data_view().prototype();

    for (name, length, function) in [
        (
            "setBigInt64",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_big_int64_wrapper),
        ),
        (
            "setBigUint64",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_big_uint64_wrapper),
        ),
        (
            "setFloat16",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_float16_wrapper),
        ),
        (
            "setFloat32",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_float32_wrapper),
        ),
        (
            "setFloat64",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_float64_wrapper),
        ),
        (
            "setInt16",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_int16_wrapper),
        ),
        (
            "setInt32",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_int32_wrapper),
        ),
        (
            "setInt8",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_int8_wrapper),
        ),
        (
            "setUint16",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_uint16_wrapper),
        ),
        (
            "setUint32",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_uint32_wrapper),
        ),
        (
            "setUint8",
            2,
            NativeFunction::from_fn_ptr(host_data_view_set_uint8_wrapper),
        ),
    ] {
        install_prototype_method_wrapper(
            &prototype,
            name,
            length,
            data_view_original_symbol(context, name)?,
            function,
            context,
        )?;
    }

    Ok(())
}

fn install_prototype_method_wrapper(
    prototype: &JsObject,
    method_name: &'static str,
    length: usize,
    original_symbol: JsSymbol,
    body: NativeFunction,
    context: &mut Context,
) -> boa_engine::JsResult<()> {
    if prototype.has_own_property(original_symbol.clone(), context)? {
        return Ok(());
    }

    let original = prototype.get(js_string!(method_name), context)?;
    if original.as_callable().is_none() {
        return Ok(());
    }

    prototype.define_property_or_throw(
        original_symbol,
        PropertyDescriptor::builder()
            .value(original)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;

    let wrapper = build_builtin_function(context, js_string!(method_name), length, body);
    prototype.define_property_or_throw(
        js_string!(method_name),
        PropertyDescriptor::builder()
            .value(wrapper)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(())
}

fn install_prototype_method_if_missing(
    prototype: &JsObject,
    method_name: &'static str,
    length: usize,
    body: NativeFunction,
    context: &mut Context,
) -> boa_engine::JsResult<()> {
    if prototype.has_own_property(js_string!(method_name), context)? {
        return Ok(());
    }

    let method = build_builtin_function(context, js_string!(method_name), length, body);
    prototype.define_property_or_throw(
        js_string!(method_name),
        PropertyDescriptor::builder()
            .value(method)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    Ok(())
}

