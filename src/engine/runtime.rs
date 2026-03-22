use boa_engine::{
    builtins::array_buffer::SharedArrayBuffer,
    builtins::promise::PromiseState,
    gc::Tracer,
    js_string,
    module::SimpleModuleLoader,
    object::{
        builtins::{JsArrayBuffer, JsSharedArrayBuffer},
        FunctionObjectBuilder, JsObject, ObjectInitializer,
    },
    property::Attribute,
    realm::Realm,
    Context, Finalize, JsArgs, JsData, JsError, JsNativeError, JsResult, JsValue as BoaValue,
    Module, NativeFunction, Source, Trace,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

thread_local! {
    static PRINT_BUFFER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug)]
struct AgentRuntimeContext {
    runtime: Arc<AgentRuntime>,
}

#[derive(Debug)]
struct WorkerAgentContext {
    runtime: Arc<AgentRuntime>,
    mailbox: Arc<AgentMailbox>,
}

#[derive(Debug)]
struct AgentRuntime {
    started_at: Instant,
    next_agent_id: AtomicUsize,
    state: Mutex<AgentRuntimeState>,
}

#[derive(Debug, Default)]
struct AgentRuntimeState {
    agents: Vec<Arc<AgentMailbox>>,
    reports: VecDeque<String>,
}

#[derive(Debug)]
struct AgentMailbox {
    id: usize,
    state: Mutex<AgentMailboxState>,
    condvar: Condvar,
}

#[derive(Debug, Default)]
struct AgentMailboxState {
    pending: Option<BroadcastEnvelope>,
    closed: bool,
}

#[derive(Debug, Clone)]
struct BroadcastEnvelope {
    buffer: SharedArrayBuffer,
    completion: Arc<BroadcastCompletion>,
}

#[derive(Debug)]
struct BroadcastCompletion {
    remaining: Mutex<usize>,
    condvar: Condvar,
}

impl Finalize for AgentRuntimeContext {}

// SAFETY: Context data stores only Rust synchronization primitives and `Arc`s.
unsafe impl Trace for AgentRuntimeContext {
    unsafe fn trace(&self, _tracer: &mut Tracer) {}

    unsafe fn trace_non_roots(&self) {}

    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl JsData for AgentRuntimeContext {}

impl Finalize for WorkerAgentContext {}

// SAFETY: Context data stores only Rust synchronization primitives and `Arc`s.
unsafe impl Trace for WorkerAgentContext {
    unsafe fn trace(&self, _tracer: &mut Tracer) {}

    unsafe fn trace_non_roots(&self) {}

    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl JsData for WorkerAgentContext {}

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

        let mut context = Context::builder()
            .can_block(true)
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;
        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            ensure_agent_runtime(&mut context);
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
            .can_block(true)
            .module_loader(loader.clone())
            .build()
            .map_err(|err| convert_error(err, &mut Context::default()))?;

        if let Some(limit) = options.loop_iteration_limit {
            context.runtime_limits_mut().set_loop_iteration_limit(limit);
        }
        install_host_globals(&mut context).map_err(|err| convert_error(err, &mut context))?;

        if options.bootstrap_test262 {
            ensure_agent_runtime(&mut context);
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

impl AgentRuntime {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            next_agent_id: AtomicUsize::new(0),
            state: Mutex::new(AgentRuntimeState::default()),
        }
    }

    fn monotonic_now(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64() * 1000.0
    }

    fn sleep(&self, milliseconds: u64) {
        thread::sleep(Duration::from_millis(milliseconds));
    }

    fn start_worker(self: &Arc<Self>, source: String) -> JsResult<()> {
        let mailbox = Arc::new(AgentMailbox::new(
            self.next_agent_id.fetch_add(1, Ordering::SeqCst),
        ));
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .agents
            .push(mailbox.clone());

        let (started_tx, started_rx) = mpsc::channel();
        let runtime = Arc::clone(self);
        thread::spawn(move || run_worker_agent(runtime, mailbox, source, started_tx));

        match started_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(JsNativeError::typ().with_message(message).into()),
            Err(_) => Err(JsNativeError::typ()
                .with_message("failed to start test262 agent")
                .into()),
        }
    }

    fn broadcast(&self, buffer: SharedArrayBuffer) {
        let agents = self
            .state
            .lock()
            .expect("agent runtime mutex poisoned")
            .agents
            .clone();
        let completion = Arc::new(BroadcastCompletion::new(agents.len()));

        for agent in agents {
            agent.enqueue(BroadcastEnvelope {
                buffer: buffer.clone(),
                completion: completion.clone(),
            });
        }

        completion.wait();
    }

    fn push_report(&self, report: String) {
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .reports
            .push_back(report);
    }

    fn pop_report(&self) -> Option<String> {
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .reports
            .pop_front()
    }

    fn unregister_agent(&self, id: usize) {
        let mut state = self.state.lock().expect("agent runtime mutex poisoned");
        state.agents.retain(|agent| agent.id != id);
    }
}

impl AgentMailbox {
    fn new(id: usize) -> Self {
        Self {
            id,
            state: Mutex::new(AgentMailboxState::default()),
            condvar: Condvar::new(),
        }
    }

    fn enqueue(&self, envelope: BroadcastEnvelope) {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        if state.closed {
            drop(state);
            envelope.completion.acknowledge();
            return;
        }

        state.pending = Some(envelope);
        self.condvar.notify_all();
    }

    fn receive(&self) -> Option<BroadcastEnvelope> {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        loop {
            if let Some(envelope) = state.pending.take() {
                return Some(envelope);
            }
            if state.closed {
                return None;
            }
            state = self
                .condvar
                .wait(state)
                .expect("agent mailbox mutex poisoned");
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        state.closed = true;
        if let Some(envelope) = state.pending.take() {
            drop(state);
            envelope.completion.acknowledge();
            self.condvar.notify_all();
            return;
        }
        self.condvar.notify_all();
    }
}

impl BroadcastCompletion {
    fn new(remaining: usize) -> Self {
        Self {
            remaining: Mutex::new(remaining),
            condvar: Condvar::new(),
        }
    }

    fn acknowledge(&self) {
        let mut remaining = self
            .remaining
            .lock()
            .expect("broadcast completion mutex poisoned");
        if *remaining == 0 {
            return;
        }
        *remaining -= 1;
        if *remaining == 0 {
            self.condvar.notify_all();
        }
    }

    fn wait(&self) {
        let mut remaining = self
            .remaining
            .lock()
            .expect("broadcast completion mutex poisoned");
        while *remaining != 0 {
            remaining = self
                .condvar
                .wait(remaining)
                .expect("broadcast completion mutex poisoned");
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
    let agent = build_agent_object(context);

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
    if let Some(agent) = agent {
        object.property(js_string!("agent"), agent, Attribute::all());
    }
    object.build()
}

fn build_agent_object(context: &mut Context) -> Option<JsObject> {
    if context.has_data::<WorkerAgentContext>() {
        Some(build_worker_agent_object(context))
    } else if context.has_data::<AgentRuntimeContext>() {
        Some(build_main_agent_object(context))
    } else {
        None
    }
}

fn build_main_agent_object(context: &mut Context) -> JsObject {
    let start = build_builtin_function(
        context,
        js_string!("start"),
        1,
        NativeFunction::from_fn_ptr(host_agent_start),
    );
    let broadcast = build_builtin_function(
        context,
        js_string!("broadcast"),
        1,
        NativeFunction::from_fn_ptr(host_agent_broadcast),
    );
    let get_report = build_builtin_function(
        context,
        js_string!("getReport"),
        0,
        NativeFunction::from_fn_ptr(host_agent_get_report),
    );
    let sleep = build_builtin_function(
        context,
        js_string!("sleep"),
        1,
        NativeFunction::from_fn_ptr(host_agent_sleep),
    );
    let monotonic_now = build_builtin_function(
        context,
        js_string!("monotonicNow"),
        0,
        NativeFunction::from_fn_ptr(host_agent_monotonic_now),
    );

    let mut object = ObjectInitializer::new(context);
    object.property(js_string!("start"), start, Attribute::all());
    object.property(js_string!("broadcast"), broadcast, Attribute::all());
    object.property(js_string!("getReport"), get_report, Attribute::all());
    object.property(js_string!("sleep"), sleep, Attribute::all());
    object.property(js_string!("monotonicNow"), monotonic_now, Attribute::all());
    object.build()
}

fn build_worker_agent_object(context: &mut Context) -> JsObject {
    let receive_broadcast = build_builtin_function(
        context,
        js_string!("receiveBroadcast"),
        1,
        NativeFunction::from_fn_ptr(host_worker_receive_broadcast),
    );
    let report = build_builtin_function(
        context,
        js_string!("report"),
        1,
        NativeFunction::from_fn_ptr(host_worker_report),
    );
    let sleep = build_builtin_function(
        context,
        js_string!("sleep"),
        1,
        NativeFunction::from_fn_ptr(host_agent_sleep),
    );
    let leaving = build_builtin_function(
        context,
        js_string!("leaving"),
        0,
        NativeFunction::from_fn_ptr(host_worker_leaving),
    );
    let monotonic_now = build_builtin_function(
        context,
        js_string!("monotonicNow"),
        0,
        NativeFunction::from_fn_ptr(host_agent_monotonic_now),
    );

    let mut object = ObjectInitializer::new(context);
    object.property(
        js_string!("receiveBroadcast"),
        receive_broadcast,
        Attribute::all(),
    );
    object.property(js_string!("report"), report, Attribute::all());
    object.property(js_string!("sleep"), sleep, Attribute::all());
    object.property(js_string!("leaving"), leaving, Attribute::all());
    object.property(js_string!("monotonicNow"), monotonic_now, Attribute::all());
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
        install_host_globals(&mut context).and_then(|_| install_test262_globals(&mut context))
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

fn host_agent_start(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let source = script_source_from_args(args, context)?;
    agent_runtime(context)?.start_worker(source)?;
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
    context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(agent_runtime(context)?.monotonic_now().into())
}

fn host_worker_receive_broadcast(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let callback = args
        .get_or_undefined(0)
        .as_callable()
        .ok_or_else(|| JsNativeError::typ().with_message("receiveBroadcast requires a callback"))?;
    let envelope = worker_mailbox(context)?.receive().ok_or_else(|| {
        JsNativeError::typ().with_message("test262 worker agent mailbox is closed")
    })?;
    let sab = JsSharedArrayBuffer::from_buffer(envelope.buffer.clone(), context);
    envelope.completion.acknowledge();
    let result = callback.call(&BoaValue::undefined(), &[sab.into()], context);
    result?;
    context.run_jobs()?;
    Ok(BoaValue::undefined())
}

fn host_worker_report(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let report = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_lossy();
    agent_runtime(context)?.push_report(report);
    Ok(BoaValue::undefined())
}

fn host_worker_leaving(_: &BoaValue, _: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
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
