use boa_engine::{
    builtins::array_buffer::SharedArrayBuffer,
    builtins::promise::PromiseState,
    gc::Tracer,
    js_string,
    module::{ModuleLoader, Referrer, resolve_module_specifier},
    object::{
        builtins::{JsArrayBuffer, JsSharedArrayBuffer},
        FunctionObjectBuilder, JsObject, ObjectInitializer,
    },
    property::{Attribute, PropertyDescriptor},
    realm::Realm,
    Context, Finalize, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString,
    JsValue as BoaValue, Module, NativeFunction, Source, Trace,
};
use boa_engine::module::SyntheticModuleInitializer;
use regex::{Captures, Regex};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

thread_local! {
    static PRINT_BUFFER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

const IMPORT_RESOURCE_MARKER: &str = "?__agentjs_type=";

static STATIC_IMPORT_FROM_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)(from\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*type\s*:\s*(['"])(json|text)(['"])\s*\}"#,
    )
    .expect("static import-with regex must compile")
});
static STATIC_IMPORT_BARE_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)(import\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*type\s*:\s*(['"])(json|text)(['"])\s*\}"#,
    )
    .expect("bare import-with regex must compile")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModuleResourceKind {
    JavaScript,
    Json,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModuleCacheKey {
    path: PathBuf,
    kind: ModuleResourceKind,
}

#[derive(Debug)]
struct CompatModuleLoader {
    root: PathBuf,
    module_map: RefCell<HashMap<ModuleCacheKey, Module>>,
}

impl CompatModuleLoader {
    fn new<P: AsRef<Path>>(root: P) -> JsResult<Self> {
        let root = root.as_ref();
        let absolute = root.canonicalize().map_err(|error| {
            JsNativeError::typ()
                .with_message(format!("could not set module root `{}`", root.display()))
                .with_cause(JsError::from_opaque(js_string!(error.to_string()).into()))
        })?;
        Ok(Self {
            root: absolute,
            module_map: RefCell::default(),
        })
    }

    fn insert(&self, path: PathBuf, kind: ModuleResourceKind, module: Module) {
        self.module_map
            .borrow_mut()
            .insert(ModuleCacheKey { path, kind }, module);
    }

    fn get(&self, path: &Path, kind: ModuleResourceKind) -> Option<Module> {
        self.module_map
            .borrow()
            .get(&ModuleCacheKey {
                path: path.to_path_buf(),
                kind,
            })
            .cloned()
    }
}

impl ModuleLoader for CompatModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let (specifier, kind) = decode_import_resource_kind(&specifier);
        let path = resolve_module_specifier(
            Some(&self.root),
            &specifier,
            referrer.path(),
            &mut context.borrow_mut(),
        )?;

        if let Some(module) = self.get(&path, kind) {
            return Ok(module);
        }

        let module = load_module_from_path(&path, kind, &mut context.borrow_mut())?;
        self.insert(path, kind, module.clone());
        Ok(module)
    }
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

        let result = context
            .eval(Source::from_bytes(
                finalize_script_source(source, options.strict, None).as_str(),
            ))
            .map_err(|err| convert_error(err, &mut context))?;

        context
            .run_jobs()
            .map_err(|err| convert_error(err, &mut context))?;

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
            .can_block(true)
            .module_loader(loader)
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

        let canonical_path = normalize_source_path(path);
        let source = finalize_script_source(source, options.strict, Some(canonical_path.as_path()));
        let result = context
            .eval(Source::from_reader(
                Cursor::new(source.as_bytes()),
                Some(canonical_path.as_path()),
            ))
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
            CompatModuleLoader::new(module_root)
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

        let canonical_path = normalize_source_path(path);
        let prepared_source = preprocess_compat_source(source, Some(canonical_path.as_path()));
        let reader = Cursor::new(prepared_source.as_bytes());
        let parsed_source = Source::from_reader(reader, Some(canonical_path.as_path()));
        let module = Module::parse(parsed_source, None, &mut context)
            .map_err(|err| convert_error(err, &mut context))?;

        loader.insert(canonical_path, ModuleResourceKind::JavaScript, module.clone());

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

fn normalize_source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn finalize_script_source(source: &str, strict: bool, source_path: Option<&Path>) -> String {
    let prepared = preprocess_compat_source(source, source_path);
    if strict {
        format!("\"use strict\";\n{prepared}")
    } else {
        prepared
    }
}

fn preprocess_compat_source(source: &str, source_path: Option<&Path>) -> String {
    let source = rewrite_static_import_attributes(source);
    let (source, rewrote_dynamic_imports) = rewrite_dynamic_import_calls(&source);
    if rewrote_dynamic_imports {
        format!("{}\n{source}", build_import_compat_helper(source_path))
    } else {
        source
    }
}

fn build_import_compat_helper(source_path: Option<&Path>) -> String {
    let referrer_literal = source_path
        .map(|path| format!("{:?}", path.to_string_lossy()))
        .unwrap_or_else(|| "\"\"".to_string());
    format!(
        r#"
const __agentjs_referrer__ = {referrer_literal};
const __agentjs_import__ = function(specifier, options) {{
  try {{
    let resourceType = "";
    if (arguments.length > 1 && options !== undefined) {{
      if ((typeof options !== "object" && typeof options !== "function") || options === null) {{
        return Promise.reject(new TypeError("The second argument to import() must be an object"));
      }}

      const attributes = options.with;
      if (attributes !== undefined) {{
        if ((typeof attributes !== "object" && typeof attributes !== "function") || attributes === null) {{
          return Promise.reject(new TypeError("The `with` import option must be an object"));
        }}

        for (const key of Object.keys(attributes)) {{
          const value = attributes[key];
          if (typeof value !== "string") {{
            return Promise.reject(new TypeError("Import attribute values must be strings"));
          }}
          if (key === "type" && (value === "json" || value === "text")) {{
            resourceType = value;
          }}
        }}
      }}
    }}

    if (resourceType) {{
      const cachedNamespace = globalThis.__agentjs_get_cached_import__(
        specifier,
        resourceType,
        __agentjs_referrer__
      );
      if (cachedNamespace !== undefined) {{
        return Promise.resolve(cachedNamespace);
      }}
      specifier = String(specifier) + "{IMPORT_RESOURCE_MARKER}" + resourceType;
    }}

    return import(specifier);
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
"#
    )
}

fn rewrite_static_import_attributes(source: &str) -> String {
    let source = STATIC_IMPORT_FROM_WITH_RE.replace_all(source, rewrite_import_attribute_match);
    STATIC_IMPORT_BARE_WITH_RE
        .replace_all(source.as_ref(), rewrite_import_attribute_match)
        .into_owned()
}

fn rewrite_import_attribute_match(captures: &Captures<'_>) -> String {
    let prefix = captures.get(1).expect("prefix capture is required").as_str();
    let quote = captures.get(2).expect("quote capture is required").as_str();
    let specifier = captures
        .get(3)
        .expect("specifier capture is required")
        .as_str();
    let resource_type = captures
        .get(6)
        .expect("resource type capture is required")
        .as_str();
    let rewritten = encode_import_resource_kind(specifier, resource_type);
    format!("{prefix}{quote}{rewritten}{quote}")
}

fn rewrite_dynamic_import_calls(source: &str) -> (String, bool) {
    let bytes = source.as_bytes();
    let mut rewritten = String::with_capacity(source.len());
    let mut changed = false;
    let mut i = 0;
    let mut last = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                i = skip_js_string(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            b'i' if matches_dynamic_import(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import__");
                i += "import".len();
                last = i;
                changed = true;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !changed {
        return (source.to_string(), false);
    }

    rewritten.push_str(&source[last..]);
    (rewritten, true)
}

fn matches_dynamic_import(bytes: &[u8], start: usize) -> bool {
    const IMPORT: &[u8] = b"import";
    if bytes.len() < start + IMPORT.len() || &bytes[start..start + IMPORT.len()] != IMPORT {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if start + IMPORT.len() < bytes.len() && is_identifier_byte(bytes[start + IMPORT.len()]) {
        return false;
    }

    let mut cursor = start + IMPORT.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    bytes.get(cursor) == Some(&b'(')
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

fn skip_js_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i = (i + 2).min(bytes.len());
            }
            current if current == quote => {
                return i + 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    bytes.len()
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn encode_import_resource_kind(specifier: &str, resource_type: &str) -> String {
    format!("{specifier}{IMPORT_RESOURCE_MARKER}{resource_type}")
}

fn decode_import_resource_kind(specifier: &JsString) -> (JsString, ModuleResourceKind) {
    let raw = specifier.to_std_string_escaped();
    let Some((path, resource_type)) = raw.rsplit_once(IMPORT_RESOURCE_MARKER) else {
        return (specifier.clone(), ModuleResourceKind::JavaScript);
    };

    let kind = match resource_type {
        "json" => ModuleResourceKind::Json,
        "text" => ModuleResourceKind::Text,
        _ => ModuleResourceKind::JavaScript,
    };
    (JsString::from(path), kind)
}

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
            let source = preprocess_compat_source(&source, Some(path));
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
    context.register_global_builtin_callable(
        js_string!("__agentjs_get_cached_import__"),
        3,
        NativeFunction::from_fn_ptr(host_get_cached_import),
    )?;
    install_array_buffer_detached_getter(context)?;
    Ok(())
}

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
        "json" => ModuleResourceKind::Json,
        "text" => ModuleResourceKind::Text,
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

    Ok(loader
        .get(&path, kind)
        .map(|module| module.namespace(context).into())
        .unwrap_or_default())
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
