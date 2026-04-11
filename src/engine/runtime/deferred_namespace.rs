fn host_get_cached_import(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let specifier = args.get_or_undefined(0).to_string(context)?;
    let resource_type = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_lossy();
    let referrer_path = args
        .get_or_undefined(2)
        .to_string(context)?
        .to_std_string_lossy();

    let kind = match resource_type.as_str() {
        "" => ModuleResourceKind::JavaScript,
        "defer" => ModuleResourceKind::Deferred,
        "json" => ModuleResourceKind::Json,
        "text" => ModuleResourceKind::Text,
        "bytes" => ModuleResourceKind::Bytes,
        _ => return Ok(BoaValue::undefined()),
    };

    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Ok(BoaValue::undefined());
    };
    if referrer_path.is_empty() {
        return Ok(BoaValue::undefined());
    }

    let path = resolve_module_specifier(
        Some(&loader.root),
        &specifier,
        Some(Path::new(&referrer_path)),
        context,
    )?;

    let Some(module) = loader.get(&path, kind) else {
        return Ok(BoaValue::undefined());
    };

    if kind == ModuleResourceKind::Deferred {
        return module
            .namespace(context)
            .get(js_string!("default"), context);
    }

    Ok(module.namespace(context).into())
}

fn host_assert_import_source(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let specifier = args.get_or_undefined(0).to_string(context)?;
    let referrer_path = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_lossy();

    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Ok(BoaValue::undefined());
    };

    let referrer = if referrer_path.is_empty() {
        None
    } else {
        Some(Path::new(&referrer_path))
    };
    let _ = resolve_module_specifier(Some(&loader.root), &specifier, referrer, context)?;

    Err(JsNativeError::syntax()
        .with_message(SOURCE_PHASE_UNAVAILABLE_MESSAGE)
        .into())
}

fn host_dynamic_import_defer(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let specifier = args.get_or_undefined(0).to_string(context)?;
    let referrer_path = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_lossy();

    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Ok(JsPromise::resolve(BoaValue::undefined(), context).into());
    };
    if referrer_path.is_empty() {
        return Ok(JsPromise::resolve(BoaValue::undefined(), context).into());
    }

    let path = resolve_module_specifier(
        Some(&loader.root),
        &specifier,
        Some(Path::new(&referrer_path)),
        context,
    )?;

    with_original_promise_then(context, |context| {
        let deferred_module = if let Some(module) = loader.get(&path, ModuleResourceKind::Deferred)
        {
            module
        } else {
            let deferred_module = load_deferred_namespace_module(&path, context)?;
            loader.insert(
                path.clone(),
                ModuleResourceKind::Deferred,
                deferred_module.clone(),
            );
            deferred_module
        };

        let deferred_namespace_promise = deferred_module.load_link_evaluate(context);
        context.run_jobs()?;
        match deferred_namespace_promise.state() {
            PromiseState::Fulfilled(_) => {}
            PromiseState::Rejected(reason) => {
                return Ok(JsPromise::reject(JsError::from_opaque(reason.clone()), context).into());
            }
            PromiseState::Pending => {
                return Ok(JsPromise::reject(
                    JsNativeError::typ().with_message(
                        "deferred namespace promise remained pending during initialization",
                    ),
                    context,
                )
                .into());
            }
        }

        let namespace = deferred_module
            .namespace(context)
            .get(js_string!("default"), context)?;

        if let Err(error) = ensure_deferred_module_loaded_and_linked(&path, context) {
            return Ok(JsPromise::reject(error, context).into());
        }
        if let Err(error) = preevaluate_async_deferred_dependencies(&path, context) {
            return Ok(JsPromise::reject(error, context).into());
        }

        Ok(JsPromise::resolve(namespace, context).into())
    })
}

fn host_deferred_namespace_get(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;
    if is_symbol_like_deferred_namespace_key(&key) {
        return deferred_namespace_ordinary_get(&metadata, &key);
    }

    let module = evaluate_deferred_namespace_module(&metadata.path, context)?;
    if !deferred_namespace_exports_include(&metadata, &key) {
        return Ok(BoaValue::undefined());
    }

    module.namespace(context).get(key, context)
}

fn host_deferred_namespace_get_own_property_descriptor(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if is_symbol_like_deferred_namespace_key(&key) {
        return deferred_namespace_ordinary_get_own_property_descriptor(&metadata, &key, context);
    }

    let module = evaluate_deferred_namespace_module(&metadata.path, context)?;
    if !deferred_namespace_exports_include(&metadata, &key) {
        return Ok(BoaValue::undefined());
    }

    build_data_descriptor_object(
        module.namespace(context).get(key, context)?,
        true,
        true,
        false,
        context,
    )
}

fn host_deferred_namespace_get_prototype_of(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(BoaValue::null())
}

fn host_deferred_namespace_has(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(_context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if is_symbol_like_deferred_namespace_key(&key) {
        return Ok(deferred_namespace_ordinary_has(&metadata, &key).into());
    }

    let _ = evaluate_deferred_namespace_module(&metadata.path, _context)?;
    Ok(deferred_namespace_exports_include(&metadata, &key).into())
}

fn host_deferred_namespace_is_extensible(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(false.into())
}

fn host_deferred_namespace_own_keys(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    let _ = evaluate_deferred_namespace_module(&metadata.path, _context)?;
    let keys = metadata
        .exports
        .iter()
        .cloned()
        .map(JsString::from)
        .map(PropertyKey::from)
        .map(BoaValue::from)
        .chain(std::iter::once(BoaValue::from(PropertyKey::from(
            JsSymbol::to_string_tag(),
        ))));
    Ok(JsArray::from_iter(keys, _context).into())
}

fn host_deferred_namespace_prevent_extensions(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(true.into())
}

fn host_deferred_namespace_define_property(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(_context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if !is_symbol_like_deferred_namespace_key(&key) {
        let _ = evaluate_deferred_namespace_module(&metadata.path, _context)?;
    }

    Ok(false.into())
}

fn host_deferred_namespace_delete_property(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(_context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if is_symbol_like_deferred_namespace_key(&key) {
        return Ok(deferred_namespace_delete_symbol_like_key(&metadata, &key).into());
    }

    let _ = evaluate_deferred_namespace_module(&metadata.path, _context)?;
    Ok(false.into())
}

fn host_deferred_namespace_set(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(false.into())
}

fn host_deferred_namespace_set_prototype_of(
    _: &BoaValue,
    args: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(args.get_or_undefined(1).is_null().into())
}

fn deferred_namespace_target_metadata(target: &JsObject) -> JsResult<DeferredNamespaceTarget> {
    target
        .downcast_ref::<DeferredNamespaceTarget>()
        .map(|metadata| metadata.clone())
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("deferred namespace target missing")
                .into()
        })
}

fn deferred_namespace_ordinary_get(
    metadata: &DeferredNamespaceTarget,
    key: &PropertyKey,
) -> JsResult<BoaValue> {
    match key {
        PropertyKey::Symbol(symbol) if symbol == &JsSymbol::to_string_tag() => {
            Ok(js_string!("Deferred Module").into())
        }
        _ if deferred_namespace_exports_include(metadata, key) => Ok(BoaValue::undefined()),
        _ => Ok(BoaValue::undefined()),
    }
}

fn deferred_namespace_ordinary_get_own_property_descriptor(
    metadata: &DeferredNamespaceTarget,
    key: &PropertyKey,
    context: &mut Context,
) -> JsResult<BoaValue> {
    match key {
        PropertyKey::Symbol(symbol) if symbol == &JsSymbol::to_string_tag() => {
            build_data_descriptor_object(
                js_string!("Deferred Module").into(),
                false,
                false,
                false,
                context,
            )
        }
        _ if deferred_namespace_exports_include(metadata, key) => {
            build_data_descriptor_object(BoaValue::undefined(), true, true, false, context)
        }
        _ => Ok(BoaValue::undefined()),
    }
}

fn deferred_namespace_ordinary_has(metadata: &DeferredNamespaceTarget, key: &PropertyKey) -> bool {
    match key {
        PropertyKey::Symbol(symbol) => symbol == &JsSymbol::to_string_tag(),
        _ => deferred_namespace_exports_include(metadata, key),
    }
}

fn deferred_namespace_delete_symbol_like_key(
    metadata: &DeferredNamespaceTarget,
    key: &PropertyKey,
) -> bool {
    match key {
        PropertyKey::Symbol(symbol) => symbol != &JsSymbol::to_string_tag(),
        _ => !deferred_namespace_exports_include(metadata, key),
    }
}

fn build_data_descriptor_object(
    value: BoaValue,
    writable: bool,
    enumerable: bool,
    configurable: bool,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let descriptor = ObjectInitializer::new(context)
        .property(js_string!("value"), value, Attribute::all())
        .property(js_string!("writable"), writable, Attribute::all())
        .property(js_string!("enumerable"), enumerable, Attribute::all())
        .property(js_string!("configurable"), configurable, Attribute::all())
        .build();
    Ok(descriptor.into())
}

fn is_symbol_like_deferred_namespace_key(key: &PropertyKey) -> bool {
    match key {
        PropertyKey::Symbol(_) => true,
        PropertyKey::String(string) => string.to_std_string_lossy() == "then",
        PropertyKey::Index(_) => false,
    }
}

fn deferred_namespace_exports_include(
    metadata: &DeferredNamespaceTarget,
    key: &PropertyKey,
) -> bool {
    let Some(name) = deferred_namespace_export_name(key) else {
        return false;
    };
    metadata.exports.iter().any(|export| export == &name)
}

fn deferred_namespace_export_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::String(string) => Some(string.to_std_string_lossy()),
        PropertyKey::Index(index) => Some(index.get().to_string()),
        PropertyKey::Symbol(_) => None,
    }
}

fn parse_deferred_module_export_names(path: &Path) -> JsResult<Vec<String>> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        JsNativeError::typ()
            .with_message(format!("could not open file `{}`", path.display()))
            .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
    })?;

    let mut exports = HashSet::new();

    for captures in MODULE_EXPORT_BINDING_RE.captures_iter(&source) {
        exports.insert(
            captures
                .get(1)
                .expect("binding capture is required")
                .as_str()
                .to_string(),
        );
    }

    if source.contains("export default") {
        exports.insert("default".to_string());
    }

    for captures in MODULE_EXPORT_LIST_RE.captures_iter(&source) {
        let bindings = captures
            .get(1)
            .expect("export list capture is required")
            .as_str();
        for binding in bindings
            .split(',')
            .map(str::trim)
            .filter(|binding| !binding.is_empty())
        {
            let Some(name) = binding.split_whitespace().last() else {
                continue;
            };
            exports.insert(name.to_string());
        }
    }

    for captures in MODULE_EXPORT_NAMESPACE_RE.captures_iter(&source) {
        exports.insert(
            captures
                .get(1)
                .expect("namespace capture is required")
                .as_str()
                .to_string(),
        );
    }

    let mut exports = exports.into_iter().collect::<Vec<_>>();
    exports.sort();
    Ok(exports)
}

fn evaluate_deferred_namespace_module(path: &Path, context: &mut Context) -> JsResult<Module> {
    let active_paths = current_executing_module_paths(context);
    if !is_ready_for_sync_execution(path, &active_paths, &mut HashSet::new(), context)? {
        return Err(JsNativeError::typ()
            .with_message("deferred namespace is not ready for synchronous evaluation")
            .into());
    }

    let module = ensure_deferred_module_loaded_and_linked(path, context)?;
    let promise = catch_silent_panic(|| module.evaluate(context)).map_err(|_| {
        JsNativeError::typ()
            .with_message("deferred namespace is not ready for synchronous evaluation")
    })?;
    catch_silent_panic(|| context.run_jobs())
        .map_err(|_| {
            JsNativeError::typ()
                .with_message("deferred namespace evaluation panicked while running jobs")
        })?
        .map_err(|error| error)?;
    match promise.state() {
        PromiseState::Fulfilled(_) => Ok(module),
        PromiseState::Rejected(reason) => Err(JsError::from_opaque(reason.clone())),
        PromiseState::Pending => Err(JsNativeError::typ()
            .with_message("deferred namespace module remained pending during evaluation")
            .into()),
    }
}

fn current_executing_module_paths(context: &Context) -> HashSet<PathBuf> {
    context
        .stack_trace()
        .filter_map(|frame| {
            let rendered = frame.position().path.to_string();
            let candidate = PathBuf::from(rendered);
            if candidate.exists() {
                Some(candidate.canonicalize().unwrap_or(candidate))
            } else {
                None
            }
        })
        .collect()
}

fn requested_module_paths(path: &Path, context: &mut Context) -> JsResult<Vec<PathBuf>> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        JsNativeError::typ()
            .with_message(format!("could not open file `{}`", path.display()))
            .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
    })?;

    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Ok(Vec::new());
    };

    let mut paths = HashSet::new();
    for regex in [
        &*MODULE_IMPORT_FROM_RE,
        &*MODULE_BARE_IMPORT_RE,
        &*MODULE_EXPORT_FROM_RE,
    ] {
        for captures in regex.captures_iter(&source) {
            let specifier = captures
                .get(2)
                .expect("specifier capture is required")
                .as_str();
            let resolved = resolve_module_specifier(
                Some(&loader.root),
                &JsString::from(specifier),
                Some(path),
                context,
            )?;
            paths.insert(resolved);
        }
    }

    Ok(paths.into_iter().collect())
}

fn is_ready_for_sync_execution(
    path: &Path,
    active_paths: &HashSet<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    context: &mut Context,
) -> JsResult<bool> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(path.clone()) {
        return Ok(true);
    }
    if active_paths.contains(&path) {
        return Ok(false);
    }

    for dependency in requested_module_paths(&path, context)? {
        if !is_ready_for_sync_execution(&dependency, active_paths, seen, context)? {
            return Ok(false);
        }
    }

    Ok(true)
}

