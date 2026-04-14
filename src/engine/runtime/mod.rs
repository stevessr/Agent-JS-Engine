use boa_engine::module::SyntheticModuleInitializer;
use boa_engine::{
    Context, Finalize, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString, JsSymbol,
    JsValue as BoaValue, Module, NativeFunction, Script, Source, Trace,
    builtins::{array_buffer::SharedArrayBuffer, object::OrdinaryObject, promise::PromiseState},
    gc::Tracer,
    js_string,
    module::{ModuleLoader, Referrer, resolve_module_specifier},
    object::{
        FunctionObjectBuilder, IntegrityLevel, JsObject, ObjectInitializer,
        builtins::{JsArray, JsArrayBuffer, JsPromise, JsProxy, JsSharedArrayBuffer, JsUint8Array},
    },
    property::{Attribute, PropertyDescriptor, PropertyKey},
    realm::Realm,
};
use regex::{Captures, Regex};
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

thread_local! {
    static PRINT_BUFFER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_LOAD_GUARD: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
    static DEFERRED_PREEVAL_GUARD: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

static PANIC_HOOK_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

const IMPORT_RESOURCE_MARKER: &str = "?__agentjs_type=";
const SOURCE_PHASE_UNAVAILABLE_MESSAGE: &str =
    "source phase imports are not supported for source text modules";
static STATIC_IMPORT_FROM_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)(from\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*type\s*:\s*(['"])(json|text|bytes)(['"])\s*\}"#,
    )
    .expect("static import-with regex must compile")
});
static STATIC_IMPORT_FROM_EMPTY_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)(from\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*\}"#)
        .expect("static import-with-empty regex must compile")
});
static STATIC_IMPORT_BARE_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)(import\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*type\s*:\s*(['"])(json|text|bytes)(['"])\s*\}"#,
    )
    .expect("bare import-with regex must compile")
});
static STATIC_IMPORT_BARE_EMPTY_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)(import\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*\}"#)
        .expect("bare import-with-empty regex must compile")
});
static STATIC_SOURCE_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)import\s+source\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+(['"])([^'"]+)(['"])\s*;?"#,
    )
    .expect("static source-phase import regex must compile")
});
static STATIC_DEFER_NAMESPACE_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)import\s+defer\s+\*\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+(['"])([^'"]+)(['"])(?:\s+with\s*\{\s*\})?\s*;?"#,
    )
    .expect("static import-defer namespace regex must compile")
});
static MODULE_IMPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:^|;)\s*import\s+(?:defer\s+\*\s+as\s+[A-Za-z_$][A-Za-z0-9_$]*|[^'";]+?)\s+from\s+(['"])([^'"]+)(['"])"#)
        .expect("module import-from regex must compile")
});
static MODULE_BARE_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:^|;)\s*import\s+(['"])([^'"]+)(['"])"#)
        .expect("module bare import regex must compile")
});
static MODULE_EXPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:^|;)\s*export\s+(?:\*|[^'";]+?)\s+from\s+(['"])([^'"]+)(['"])"#)
        .expect("module export-from regex must compile")
});
static MODULE_EXPORT_BINDING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)\bexport\s+(?:(?:default\s+)?async\s+function(?:\s*\*)?|default\s+function(?:\s*\*)?|function(?:\s*\*)?|default\s+class|class|const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)"#,
    )
    .expect("module export binding regex must compile")
});
static MODULE_EXPORT_LIST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)\bexport\s*\{([^}]*)\}"#).expect("module export list regex must compile")
});
static MODULE_EXPORT_NAMESPACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)\bexport\s+\*\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+(['"])([^'"]+)(['"])"#,
    )
    .expect("module export namespace regex must compile")
});
static USING_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?P<indent>\s*)(?P<await>await\s+)?using\s+(?P<name>[A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?P<expr>[\s\S]*?)\s*;\s*$"#,
    )
    .expect("using declaration regex must compile")
});
static TOP_LEVEL_USING_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^(?P<indent>\s*)(?P<await>await\s+)?using\s+[A-Za-z_$][A-Za-z0-9_$]*\s*="#)
        .expect("top-level using start regex must compile")
});
static FOR_AWAIT_USING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?P<indent>\s*)for\s*\(\s*(?P<await_kw>await\s+)?using\s+(?P<name>[A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?P<init>[\s\S]*?)\s*;\s*(?P<test>[\s\S]*?)\s*;\s*(?P<update>[\s\S]*?)\s*\)(?P<body>[\s\S]*)$"#,
    )
    .expect("for-await-using regex must compile")
});
static FOR_OF_AWAIT_USING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?P<indent>\s*)for\s*(?P<await_prefix>await\s+)?\(\s*(?P<await_kw>await\s+)?using\s+(?P<name>[A-Za-z_$][A-Za-z0-9_$]*)\s+of\s+(?P<iterable>[\s\S]*?)\s*\)(?P<body>[\s\S]*)$"#,
    )
    .expect("for-of await-using regex must compile")
});
static FOR_IN_AWAIT_USING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?P<indent>\s*)for\s*\(\s*(?P<await_kw>await\s+)?using\s+(?P<name>[A-Za-z_$][A-Za-z0-9_$]*)\s+in\s+(?P<iterable>[\s\S]*?)\s*\)(?P<body>[\s\S]*)$"#,
    )
    .expect("for-in await-using regex must compile")
});
static HTML_OPEN_COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(^|[\r\n])(?P<indent>[ \t]*)<!--(?P<body>.*)$"#)
        .expect("html open comment regex must compile")
});
static HTML_CLOSE_COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(^|[\r\n])(?P<prefix>(?:[ \t]|/\*.*?\*/)*?)-->(?P<body>.*)$"#)
        .expect("html close comment regex must compile")
});
static ANNEX_B_CALL_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^(?P<indent>[ \t]*)(?:(?P<prefix>\+\+|--)(?P<prefix_call>[A-Za-z_$][A-Za-z0-9_$]*\(\))|(?P<call>[A-Za-z_$][A-Za-z0-9_$]*\(\))\s*(?P<op>=|\+=|\+\+|--)(?P<suffix>[^\n;]*)?);\s*$"#)
        .expect("annex b call assignment regex must compile")
});
static ANNEX_B_FOR_IN_OF_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^(?P<indent>[ \t]*)for\s*\(\s*(?P<call>[A-Za-z_$][A-Za-z0-9_$]*\(\))\s+(?P<kind>in|of)\s+(?P<right>[^\)]*)\)\s*\{\s*\}\s*$"#)
        .expect("annex b for-in/of call regex must compile")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModuleResourceKind {
    JavaScript,
    Deferred,
    Json,
    Text,
    Bytes,
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

#[derive(Debug, Clone)]
struct DeferredNamespaceTarget {
    path: PathBuf,
    exports: Vec<String>,
}

impl JsData for DeferredNamespaceTarget {}
impl Finalize for DeferredNamespaceTarget {}

// Safety: the target metadata only stores Rust-owned strings and paths.
unsafe impl Trace for DeferredNamespaceTarget {
    boa_engine::gc::empty_trace!();
}

#[derive(Debug)]
struct DeferredLoadScope {
    path: PathBuf,
}

impl DeferredLoadScope {
    fn enter(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        DEFERRED_LOAD_GUARD.with(|guard| {
            let mut guard = guard.borrow_mut();
            if !guard.insert(path.clone()) {
                return None;
            }
            Some(Self { path })
        })
    }
}

impl Drop for DeferredLoadScope {
    fn drop(&mut self) {
        DEFERRED_LOAD_GUARD.with(|guard| {
            guard.borrow_mut().remove(&self.path);
        });
    }
}

#[derive(Debug)]
struct DeferredPreevalScope {
    path: PathBuf,
}

impl DeferredPreevalScope {
    fn enter(path: &Path) -> Option<Self> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        DEFERRED_PREEVAL_GUARD.with(|guard| {
            let mut guard = guard.borrow_mut();
            if !guard.insert(path.clone()) {
                return None;
            }
            Some(Self { path })
        })
    }
}

impl Drop for DeferredPreevalScope {
    fn drop(&mut self) {
        DEFERRED_PREEVAL_GUARD.with(|guard| {
            guard.borrow_mut().remove(&self.path);
        });
    }
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
        let path = path.canonicalize().unwrap_or(path);

        if let Some(module) = self.get(&path, kind) {
            return Ok(module);
        }

        if kind == ModuleResourceKind::Deferred {
            let module = load_deferred_namespace_module(&path, &mut context.borrow_mut())?;
            self.insert(path.clone(), kind, module.clone());
            ensure_deferred_module_loaded_and_linked(&path, &mut context.borrow_mut())?;
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

#[derive(Debug, Clone)]
struct HostHooksContext {
    immutable_marker: JsSymbol,
    array_buffer_originals: HashMap<&'static str, JsSymbol>,
    data_view_originals: HashMap<&'static str, JsSymbol>,
    promise_then_original: JsSymbol,
    array_flat_original: JsSymbol,
    active_module_evaluations: RefCell<Vec<PathBuf>>,
    shadow_realm_next_callable_id: Cell<u64>,
    shadow_realm_callables: RefCell<HashMap<u64, BoaValue>>,
    dispose_symbol: JsSymbol,
    async_dispose_symbol: JsSymbol,
}

impl HostHooksContext {
    fn new() -> Self {
        let hidden_symbol = |name: &'static str| {
            JsSymbol::new(Some(js_string!(name))).expect("host hook symbol allocation must succeed")
        };

        Self {
            immutable_marker: hidden_symbol("agentjs.immutableArrayBuffer"),
            array_buffer_originals: HashMap::from([
                (
                    "resize",
                    hidden_symbol("agentjs.original.ArrayBuffer.resize"),
                ),
                (
                    "transfer",
                    hidden_symbol("agentjs.original.ArrayBuffer.transfer"),
                ),
                (
                    "transferToFixedLength",
                    hidden_symbol("agentjs.original.ArrayBuffer.transferToFixedLength"),
                ),
                ("slice", hidden_symbol("agentjs.original.ArrayBuffer.slice")),
            ]),
            data_view_originals: HashMap::from([
                (
                    "setBigInt64",
                    hidden_symbol("agentjs.original.DataView.setBigInt64"),
                ),
                (
                    "setBigUint64",
                    hidden_symbol("agentjs.original.DataView.setBigUint64"),
                ),
                (
                    "setFloat16",
                    hidden_symbol("agentjs.original.DataView.setFloat16"),
                ),
                (
                    "setFloat32",
                    hidden_symbol("agentjs.original.DataView.setFloat32"),
                ),
                (
                    "setFloat64",
                    hidden_symbol("agentjs.original.DataView.setFloat64"),
                ),
                (
                    "setInt16",
                    hidden_symbol("agentjs.original.DataView.setInt16"),
                ),
                (
                    "setInt32",
                    hidden_symbol("agentjs.original.DataView.setInt32"),
                ),
                (
                    "setInt8",
                    hidden_symbol("agentjs.original.DataView.setInt8"),
                ),
                (
                    "setUint16",
                    hidden_symbol("agentjs.original.DataView.setUint16"),
                ),
                (
                    "setUint32",
                    hidden_symbol("agentjs.original.DataView.setUint32"),
                ),
                (
                    "setUint8",
                    hidden_symbol("agentjs.original.DataView.setUint8"),
                ),
            ]),
            promise_then_original: hidden_symbol("agentjs.original.Promise.then"),
            array_flat_original: hidden_symbol("agentjs.original.Array.prototype.flat"),
            active_module_evaluations: RefCell::new(Vec::new()),
            shadow_realm_next_callable_id: Cell::new(0),
            shadow_realm_callables: RefCell::new(HashMap::new()),
            dispose_symbol: hidden_symbol("Symbol.dispose"),
            async_dispose_symbol: hidden_symbol("Symbol.asyncDispose"),
        }
    }
}

impl Finalize for HostHooksContext {}

// SAFETY: Context data stores `JsSymbol`s and registered `JsValue`s rooted by the context data.
unsafe impl Trace for HostHooksContext {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        for value in self.shadow_realm_callables.borrow().values() {
            // SAFETY: values stored in the host registry must remain reachable.
            unsafe { value.trace(tracer) };
        }
    }

    unsafe fn trace_non_roots(&self) {}

    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl JsData for HostHooksContext {}

#[derive(Debug, Clone)]
struct ShadowRealmWrappedFunctionCapture {
    callable_id: u64,
    foreign_realm: Realm,
    wrapper_realm: Realm,
}

impl Finalize for ShadowRealmWrappedFunctionCapture {}

// SAFETY: capture stores traced realms plus a plain integer id.
unsafe impl Trace for ShadowRealmWrappedFunctionCapture {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        // SAFETY: the captured realms must stay reachable while the wrapper exists.
        unsafe { self.foreign_realm.trace(tracer) };
        unsafe { self.wrapper_realm.trace(tracer) };
    }

    unsafe fn trace_non_roots(&self) {}

    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl JsData for ShadowRealmWrappedFunctionCapture {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisposableStackStatus {
    Pending,
    Disposed,
}

#[derive(Debug)]
struct DisposableResource {
    value: BoaValue,
    method: BoaValue,
}

impl Finalize for DisposableResource {}

unsafe impl Trace for DisposableResource {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            self.value.trace(tracer);
            self.method.trace(tracer);
        }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {
        self.finalize();
    }
}

#[derive(Debug, JsData)]
struct DisposableStackData {
    status: Cell<DisposableStackStatus>,
    stack: RefCell<Vec<DisposableResource>>,
}

impl Finalize for DisposableStackData {}

unsafe impl Trace for DisposableStackData {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        for resource in self.stack.borrow().iter() {
            unsafe {
                resource.trace(tracer);
            }
        }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl DisposableStackData {
    fn new() -> Self {
        Self {
            status: Cell::new(DisposableStackStatus::Pending),
            stack: RefCell::new(Vec::new()),
        }
    }
}

#[derive(Debug)]
struct AsyncDisposableResource {
    value: BoaValue,
    method: BoaValue,
    needs_await: bool,
}

impl Finalize for AsyncDisposableResource {}

unsafe impl Trace for AsyncDisposableResource {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            self.value.trace(tracer);
            self.method.trace(tracer);
        }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {
        self.finalize();
    }
}

#[derive(Debug, JsData)]
struct AsyncDisposableStackData {
    status: Cell<DisposableStackStatus>,
    stack: RefCell<Vec<AsyncDisposableResource>>,
}

impl Finalize for AsyncDisposableStackData {}

unsafe impl Trace for AsyncDisposableStackData {
    unsafe fn trace(&self, tracer: &mut Tracer) {
        for resource in self.stack.borrow().iter() {
            unsafe {
                resource.trace(tracer);
            }
        }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl AsyncDisposableStackData {
    fn new() -> Self {
        Self {
            status: Cell::new(DisposableStackStatus::Pending),
            stack: RefCell::new(Vec::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalOptions {
    pub strict: bool,
    pub bootstrap_test262: bool,
    pub loop_iteration_limit: Option<u64>,
    pub can_block: bool,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            strict: false,
            bootstrap_test262: false,
            loop_iteration_limit: None,
            can_block: true,
        }
    }
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

/// A REPL session that maintains JavaScript context across evaluations.
pub struct ReplSession {
    context: Context,
    strict: bool,
}

impl ReplSession {
    /// Create a new REPL session with the given working directory and options.
    pub fn new(cwd: &Path, options: &EvalOptions) -> Result<Self, EngineError> {
        let loader = Rc::new(
            CompatModuleLoader::new(cwd)
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

        Ok(Self {
            context,
            strict: options.strict,
        })
    }

    /// Evaluate JavaScript code in the REPL session, preserving state.
    pub fn eval(&mut self, source: &str) -> Result<EvalOutput, EngineError> {
        reset_print_buffer();

        let source = finalize_script_source(source, self.strict, None)?;
        let result = self
            .context
            .eval(Source::from_bytes(source.as_str()))
            .map_err(|err| convert_error(err, &mut self.context))?;

        drain_jobs_for_eval(&mut self.context)?;

        Ok(EvalOutput {
            value: display_value(&result, &mut self.context),
            printed: take_print_buffer(),
        })
    }
}

include!("engine.rs");
include!("rewrite.rs");
include!("module.rs");
include!("agent.rs");
include!("console.rs");
include!("builtins.rs");
include!("intl_date_time.rs");
include!("iterator.rs");
include!("string_regexp.rs");
include!("array_buffer_installs.rs");
include!("test262.rs");
include!("host_context.rs");
include!("shadow_realm.rs");
include!("host_misc.rs");
include!("host_disposable.rs");
include!("deferred_namespace.rs");
include!("inspect.rs");
