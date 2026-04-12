fn load_module_from_path(
    path: &Path,
    kind: ModuleResourceKind,
    context: &mut Context,
) -> JsResult<Module> {
    match kind {
        ModuleResourceKind::JavaScript => {
            let source = std::fs::read_to_string(path).map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!("could not open file `{}`", path.display()))
                    .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
            })?;
            if let Some(module) = maybe_build_source_phase_module(path, &source, context)? {
                return Ok(module);
            }
            let source = preprocess_compat_source(&source, Some(path), true, true)
                .map_err(|error| JsNativeError::syntax().with_message(error.message))?;
            Module::parse(
                Source::from_reader(Cursor::new(source.as_bytes()), Some(path)),
                None,
                context,
            )
            .map_err(|error| {
                JsNativeError::syntax()
                    .with_message(format!("could not parse module `{}`", path.display()))
                    .with_cause(error)
                    .into()
            })
        }
        ModuleResourceKind::Deferred => load_deferred_namespace_module(path, context),
        ModuleResourceKind::Json => {
            let source = std::fs::read_to_string(path).map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!("could not open file `{}`", path.display()))
                    .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
            })?;
            Module::parse_json(JsString::from(source.as_str()), context).map_err(|error| {
                JsNativeError::syntax()
                    .with_message(format!("could not parse JSON module `{}`", path.display()))
                    .with_cause(error)
                    .into()
            })
        }
        ModuleResourceKind::Text => {
            let source = std::fs::read_to_string(path).map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!("could not open file `{}`", path.display()))
                    .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
            })?;
            Ok(Module::synthetic(
                &[js_string!("default")],
                SyntheticModuleInitializer::from_copy_closure_with_captures(
                    |module, value, _context| {
                        module.set_export(&js_string!("default"), value.clone())?;
                        Ok(())
                    },
                    BoaValue::from(JsString::from(source.as_str())),
                ),
                None,
                None,
                context,
            ))
        }
        ModuleResourceKind::Bytes => {
            let bytes = std::fs::read(path).map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!("could not open file `{}`", path.display()))
                    .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
            })?;
            let typed_array = JsUint8Array::from_iter(bytes, context)?;
            let buffer = typed_array
                .get(js_string!("buffer"), context)?
                .as_object()
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("bytes module default export did not expose an ArrayBuffer")
                })?;
            mark_array_buffer_immutable(&buffer, context)?;

            Ok(Module::synthetic(
                &[js_string!("default")],
                SyntheticModuleInitializer::from_copy_closure_with_captures(
                    |module, value, _context| {
                        module.set_export(&js_string!("default"), value.clone())?;
                        Ok(())
                    },
                    BoaValue::from(typed_array),
                ),
                None,
                None,
                context,
            ))
        }
    }
}

fn load_deferred_namespace_module(path: &Path, context: &mut Context) -> JsResult<Module> {
    let proxy = build_deferred_namespace_proxy(path, context)?;

    let captures = JsObject::with_object_proto(context.intrinsics());
    captures.set(js_string!("proxy"), BoaValue::from(proxy), false, context)?;
    captures.set(
        js_string!("path"),
        js_string!(path.to_string_lossy()),
        false,
        context,
    )?;

    Ok(Module::synthetic(
        &[js_string!("default")],
        SyntheticModuleInitializer::from_copy_closure_with_captures(
            |module, captures, context| {
                let captures = captures.as_object().ok_or_else(|| {
                    JsNativeError::typ().with_message("deferred namespace captures missing")
                })?;
                let proxy = captures.get(js_string!("proxy"), context)?;
                let path_str = captures
                    .get(js_string!("path"), context)?
                    .to_string(context)?
                    .to_std_string_lossy();
                let path = Path::new(&path_str);

                module.set_export(&js_string!("default"), proxy)?;
                preevaluate_async_deferred_dependencies(path, context)?;
                Ok(())
            },
            BoaValue::from(captures),
        ),
        Some(path.to_path_buf()),
        None,
        context,
    ))
}

fn ensure_deferred_module_loaded_and_linked(
    path: &Path,
    context: &mut Context,
) -> JsResult<Module> {
    let Some(loader) = context.downcast_module_loader::<CompatModuleLoader>() else {
        return Err(JsNativeError::typ()
            .with_message("deferred namespace imports require a module loader")
            .into());
    };

    if let Some(module) = loader.get(path, ModuleResourceKind::JavaScript) {
        return Ok(module);
    }

    let module = load_module_from_path(path, ModuleResourceKind::JavaScript, context)?;
    loader.insert(
        path.to_path_buf(),
        ModuleResourceKind::JavaScript,
        module.clone(),
    );

    let Some(_scope) = DeferredLoadScope::enter(path) else {
        return Ok(module);
    };

    let promise = module.load(context);
    context.run_jobs()?;
    match promise.state() {
        PromiseState::Fulfilled(_) => {
            module.link(context)?;
        }
        PromiseState::Rejected(reason) => return Err(JsError::from_opaque(reason.clone())),
        PromiseState::Pending => {
            return Err(JsNativeError::typ()
                .with_message("deferred namespace module remained pending during load")
                .into());
        }
    }

    Ok(module)
}

fn preevaluate_async_deferred_dependencies(path: &Path, context: &mut Context) -> JsResult<()> {
    preevaluate_async_deferred_dependencies_inner(path, &mut HashSet::new(), context)
}

fn preevaluate_async_deferred_dependencies_inner(
    path: &Path,
    seen: &mut HashSet<PathBuf>,
    context: &mut Context,
) -> JsResult<()> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(path.clone()) {
        return Ok(());
    }

    let module = ensure_deferred_module_loaded_and_linked(&path, context)?;
    let source = std::fs::read_to_string(&path).unwrap_or_default();
    if is_async_module_source(&source) {
        let evaluate_promise = module.evaluate(context);
        context.run_jobs()?;
        match evaluate_promise.state() {
            PromiseState::Fulfilled(_) => return Ok(()),
            PromiseState::Rejected(reason) => return Err(JsError::from_opaque(reason.clone())),
            PromiseState::Pending => {
                return Err(JsNativeError::typ()
                    .with_message("deferred namespace module remained pending during evaluation")
                    .into());
            }
        }
    }

    for dependency in requested_module_paths(&path, context)? {
        preevaluate_async_deferred_dependencies_inner(&dependency, seen, context)?;
    }

    Ok(())
}

fn build_deferred_namespace_proxy(path: &Path, context: &mut Context) -> JsResult<JsProxy> {
    let exports = parse_deferred_module_export_names(path)?;
    let target = JsObject::from_proto_and_data(
        None,
        DeferredNamespaceTarget {
            path: path.to_path_buf(),
            exports: exports.clone(),
        },
    );
    for export in exports {
        target.define_property_or_throw(
            JsString::from(export.as_str()),
            PropertyDescriptor::builder()
                .value(BoaValue::undefined())
                .writable(true)
                .enumerable(true)
                .configurable(true),
            context,
        )?;
    }
    target.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("Deferred Module"))
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    target.set_integrity_level(IntegrityLevel::Sealed, context)?;

    Ok(JsProxy::builder(target)
        .get(host_deferred_namespace_get)
        .get_own_property_descriptor(host_deferred_namespace_get_own_property_descriptor)
        .get_prototype_of(host_deferred_namespace_get_prototype_of)
        .has(host_deferred_namespace_has)
        .is_extensible(host_deferred_namespace_is_extensible)
        .own_keys(host_deferred_namespace_own_keys)
        .prevent_extensions(host_deferred_namespace_prevent_extensions)
        .define_property(host_deferred_namespace_define_property)
        .delete_property(host_deferred_namespace_delete_property)
        .set(host_deferred_namespace_set)
        .set_prototype_of(host_deferred_namespace_set_prototype_of)
        .build(context))
}

fn maybe_build_source_phase_module(
    path: &Path,
    source: &str,
    context: &mut Context,
) -> JsResult<Option<Module>> {
    let specifiers = STATIC_SOURCE_IMPORT_RE
        .captures_iter(source)
        .map(|captures| {
            captures
                .get(3)
                .expect("specifier capture is required")
                .as_str()
                .to_string()
        })
        .collect::<Vec<_>>();

    if specifiers.is_empty() {
        return Ok(None);
    }

    let root = context
        .downcast_module_loader::<CompatModuleLoader>()
        .map(|loader| loader.root.clone());

    for specifier in specifiers {
        let resolved = resolve_module_specifier(
            root.as_deref(),
            &JsString::from(specifier.as_str()),
            Some(path),
            context,
        )?;
        std::fs::metadata(&resolved).map_err(|error| {
            JsNativeError::typ()
                .with_message(format!("could not open file `{}`", resolved.display()))
                .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
        })?;
    }

    Ok(Some(Module::synthetic(
        &[],
        SyntheticModuleInitializer::from_copy_closure_with_captures(
            |_, _, _| {
                Err(JsNativeError::syntax()
                    .with_message(SOURCE_PHASE_UNAVAILABLE_MESSAGE)
                    .into())
            },
            (),
        ),
        None,
        None,
        context,
    )))
}
