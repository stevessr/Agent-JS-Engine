#[derive(Debug, Default, Clone, Copy)]
pub struct JsEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportCallSyntaxKind {
    Dynamic,
    SingleArgument,
}

impl JsEngine {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn eval(&self, source: &str) -> Result<EvalOutput, EngineError> {
        self.eval_with_options(source, &EvalOptions::default())
    }

    pub fn eval_with_options(
        &self,
        source: &str,
        options: &EvalOptions,
    ) -> Result<EvalOutput, EngineError> {
        reset_print_buffer();

        let mut context = Context::builder()
            .can_block(options.can_block)
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;
        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            ensure_agent_runtime(&mut context);
            install_test262_globals(&mut context, true)
                .map_err(|err| convert_error(err, &mut context))?;
        }

        let source = finalize_script_source(source, options.strict, None)?;
        let result = context
            .eval(Source::from_bytes(source.as_str()))
            .map_err(|err| convert_error(err, &mut context))?;

        drain_jobs_for_eval(&mut context)?;

        Ok(EvalOutput {
            value: display_value(&result, &mut context),
            printed: take_print_buffer(),
        })
    }

    pub fn eval_script_with_options(
        &self,
        source: &str,
        path: &Path,
        module_root: &Path,
        options: &EvalOptions,
    ) -> Result<EvalOutput, EngineError> {
        reset_print_buffer();

        let loader = Rc::new(
            CompatModuleLoader::new(module_root)
                .map_err(|err| convert_error(err, &mut Context::default()))?,
        );
        let mut context = Context::builder()
            .can_block(options.can_block)
            .module_loader(loader)
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;

        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            ensure_agent_runtime(&mut context);
            install_test262_globals(&mut context, true)
                .map_err(|err| convert_error(err, &mut context))?;
        }

        let canonical_path = normalize_source_path(path);
        let source =
            finalize_script_source(source, options.strict, Some(canonical_path.as_path()))?;
        let result = context
            .eval(Source::from_reader(
                Cursor::new(source.as_bytes()),
                Some(canonical_path.as_path()),
            ))
            .map_err(|err| convert_error(err, &mut context))?;

        drain_jobs_for_eval(&mut context)?;

        Ok(EvalOutput {
            value: display_value(&result, &mut context),
            printed: take_print_buffer(),
        })
    }

    pub fn eval_module_with_options(
        &self,
        source: &str,
        path: &Path,
        module_root: &Path,
        options: &EvalOptions,
    ) -> Result<EvalOutput, EngineError> {
        reset_print_buffer();

        let loader = Rc::new(
            CompatModuleLoader::new(module_root)
                .map_err(|err| convert_error(err, &mut Context::default()))?,
        );
        let mut context = Context::builder()
            .can_block(options.can_block)
            .module_loader(loader.clone())
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;

        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            ensure_agent_runtime(&mut context);
            install_test262_globals(&mut context, true)
                .map_err(|err| convert_error(err, &mut context))?;
        }

        let canonical_path = normalize_source_path(path);
        let prepared_source =
            preprocess_compat_source(source, Some(canonical_path.as_path()), true)?;
        let reader = Cursor::new(prepared_source.as_bytes());
        let parsed_source = Source::from_reader(reader, Some(canonical_path.as_path()));
        let module = Module::parse(parsed_source, None, &mut context)
            .map_err(|err| convert_error(err, &mut context))?;

        loader.insert(
            canonical_path,
            ModuleResourceKind::JavaScript,
            module.clone(),
        );

        let promise = module.load_link_evaluate(&mut context);
        let value = settle_promise_for_eval(
            &promise,
            &mut context,
            "module promise remained pending after job queue drain",
        )?;
        drain_jobs_for_eval(&mut context)?;

        Ok(EvalOutput {
            value: display_value(&value, &mut context),
            printed: take_print_buffer(),
        })
    }
}

fn drain_jobs_for_eval(context: &mut Context) -> Result<(), EngineError> {
    context
        .run_jobs()
        .map_err(|err| convert_error(err, context))
}

fn settle_promise_for_eval(
    promise: &JsPromise,
    context: &mut Context,
    pending_message: &str,
) -> Result<BoaValue, EngineError> {
    drain_jobs_for_eval(context)?;
    match promise.state() {
        PromiseState::Fulfilled(value) => Ok(value),
        PromiseState::Rejected(reason) => {
            Err(convert_error(JsError::from_opaque(reason.clone()), context))
        }
        PromiseState::Pending => Err(EngineError {
            name: "ModuleError".to_string(),
            message: pending_message.to_string(),
        }),
    }
}

