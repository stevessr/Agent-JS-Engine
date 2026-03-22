use boa_engine::{Context, JsError, JsValue as BoaValue, NativeFunction, Source, js_string};
use std::cell::RefCell;
use std::fmt;

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
            context
                .runtime_limits_mut()
                .set_loop_iteration_limit(limit);
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
    context.eval(Source::from_bytes(
        r#"
        globalThis.$262 = {
            global: globalThis,
            evalScript: function(sourceText) {
                return eval(sourceText);
            }
        };
        "#,
    ))?;
    Ok(())
}

fn host_print(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> boa_engine::JsResult<BoaValue> {
    let message = args
        .iter()
        .map(|value| value.to_string(context).map(|text| text.to_std_string_escaped()))
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
        .or_else(|| error.try_native(context).ok().map(|native| native.kind.to_string()));

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
