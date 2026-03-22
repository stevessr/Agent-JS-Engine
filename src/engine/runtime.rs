use boa_engine::{
    builtins::promise::PromiseState,
    js_string,
    module::SimpleModuleLoader,
    object::{builtins::JsArrayBuffer, FunctionObjectBuilder, JsObject, ObjectInitializer},
    property::Attribute,
    realm::Realm,
    Context, JsArgs, JsError, JsResult, JsValue as BoaValue, Module, NativeFunction, Source,
};
use std::cell::RefCell;
use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::rc::Rc;

thread_local! {
    static PRINT_BUFFER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone, Default)]
pub struct EvalOptions {
    pub strict: bool,
    pub bootstrap_test262: bool,
    pub loop_iteration_limit: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct EvalOutput {
    pub value: Option<String>,
    pub printed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    pub name: String,
    pub message: String,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Default, Clone, Copy)]
pub struct JsEngine;

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

        let mut context = Context::default();
        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            install_test262_globals(&mut context)
                .map_err(|err| convert_error(err, &mut context))?;
        }

        let wrapped_source;
        let source = if options.strict {
            wrapped_source = format!("\"use strict\";\n{source}");
            wrapped_source.as_str()
        } else {
            source
        };

        let result = context
            .eval(Source::from_bytes(source))
            .map_err(|err| convert_error(err, &mut context))?;

        context
            .run_jobs()
            .map_err(|err| convert_error(err, &mut context))?;

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
            SimpleModuleLoader::new(module_root)
                .map_err(|err| convert_error(err, &mut Context::default()))?,
        );
        let mut context = Context::builder()
            .module_loader(loader.clone())
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;

        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            install_test262_globals(&mut context)
                .map_err(|err| convert_error(err, &mut context))?;
        }

        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let reader = Cursor::new(source.as_bytes());
        let parsed_source = Source::from_reader(reader, Some(canonical_path.as_path()));
        let module = Module::parse(parsed_source, None, &mut context)
            .map_err(|err| convert_error(err, &mut context))?;

        loader.insert(canonical_path, module.clone());

        let promise = module.load_link_evaluate(&mut context);
        context
            .run_jobs()
            .map_err(|err| convert_error(err, &mut context))?;

        match promise.state() {
            PromiseState::Fulfilled(value) => Ok(EvalOutput {
                value: display_value(&value, &mut context),
                printed: take_print_buffer(),
            }),
            PromiseState::Rejected(reason) => Err(convert_error(
                JsError::from_opaque(reason.clone()),
                &mut context,
            )),
            PromiseState::Pending => Err(EngineError {
                name: "ModuleError".to_string(),
                message: "module promise remained pending after job queue drain".to_string(),
            }),
        }
    }
}

fn install_host_globals(context: &mut Context) -> boa_engine::JsResult<()> {
    context.register_global_builtin_callable(
        js_string!("print"),
        1,
        NativeFunction::from_fn_ptr(host_print),
    )?;
    Ok(())
}

fn install_test262_globals(context: &mut Context) -> boa_engine::JsResult<()> {
    let test262 = build_test262_object(
        context.realm().clone(),
        context.global_object(),
        true,
        context,
    );
    context.register_global_property(js_string!("$262"), test262, Attribute::all())?;
    Ok(())
}

fn build_test262_object(
    target_realm: Realm,
    target_global: JsObject,
    expose_host_hooks: bool,
    context: &mut Context,
) -> JsObject {
    let eval_script = build_builtin_function(
        context,
        js_string!("evalScript"),
        1,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, target_realm, context| eval_script_in_realm(args, target_realm, context),
            target_realm,
        ),
    );
    let create_realm = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("createRealm"),
            0,
            NativeFunction::from_fn_ptr(host_create_realm),
        )
    });
    let detach_array_buffer = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("detachArrayBuffer"),
            1,
            NativeFunction::from_fn_ptr(host_detach_array_buffer),
        )
    });

    let mut object = ObjectInitializer::new(context);
    object.property(js_string!("global"), target_global, Attribute::all());
    object.property(js_string!("evalScript"), eval_script, Attribute::all());
    if let Some(create_realm) = create_realm {
        object.property(js_string!("createRealm"), create_realm, Attribute::all());
    }
    if let Some(detach_array_buffer) = detach_array_buffer {
        object.property(
            js_string!("detachArrayBuffer"),
            detach_array_buffer,
            Attribute::all(),
        );
    }
    object.build()
}

fn build_builtin_function(
    context: &mut Context,
    name: boa_engine::JsString,
    length: usize,
    body: NativeFunction,
) -> JsObject {
    FunctionObjectBuilder::new(context.realm(), body)
        .name(name)
        .length(length)
        .constructor(false)
        .build()
        .into()
}

fn eval_script_in_realm(
    args: &[BoaValue],
    target_realm: &Realm,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let source = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();

    with_realm(context, target_realm.clone(), |context| {
        let result = context.eval(Source::from_bytes(source.as_str()))?;
        context.run_jobs()?;
        Ok(result)
    })
}

fn host_create_realm(_: &BoaValue, _: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let new_realm = context.create_realm()?;
    let new_global = with_realm(context, new_realm.clone(), |context| {
        install_host_globals(context)?;
        install_test262_globals(context)?;
        Ok(context.global_object())
    })?;
    let wrapper = build_test262_object(new_realm, new_global, false, context);
    Ok(wrapper.into())
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

fn display_value(value: &BoaValue, context: &mut Context) -> Option<String> {
    if value.is_undefined() {
        None
    } else {
        value
            .to_string(context)
            .ok()
            .map(|text| text.to_std_string_escaped())
    }
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

    let name = native_name
        .clone()
        .unwrap_or_else(|| "ThrownValue".to_string());

    let message = if matches!(
        native_name.as_deref(),
        Some("RuntimeLimit") | Some("NoInstructionsRemain")
    ) {
        format!("{error}")
    } else {
        error
            .to_opaque(context)
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
