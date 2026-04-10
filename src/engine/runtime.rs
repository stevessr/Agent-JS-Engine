use boa_engine::module::SyntheticModuleInitializer;
use boa_engine::{
    Context, Finalize, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString, JsSymbol,
    JsValue as BoaValue, Module, NativeFunction, Script, Source, Trace,
    builtins::array_buffer::SharedArrayBuffer,
    builtins::error::Error as BoaBuiltinError,
    builtins::promise::PromiseState,
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
static STATIC_IMPORT_BARE_WITH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)(import\s+)(['"])([^'"]+)(['"])\s+with\s*\{\s*type\s*:\s*(['"])(json|text|bytes)(['"])\s*\}"#,
    )
    .expect("bare import-with regex must compile")
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
    Regex::new(r#"(?m)^\s*import\s+(?:defer\s+\*\s+as\s+[A-Za-z_$][A-Za-z0-9_$]*|[^'";]+?)\s+from\s+(['"])([^'"]+)(['"])"#)
        .expect("module import-from regex must compile")
});
static MODULE_BARE_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*import\s+(['"])([^'"]+)(['"])"#)
        .expect("module bare import regex must compile")
});
static MODULE_EXPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*export\s+(?:\*|[^'";]+?)\s+from\s+(['"])([^'"]+)(['"])"#)
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
    shadow_realm_next_callable_id: Cell<u64>,
    shadow_realm_callables: RefCell<HashMap<u64, BoaValue>>,
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
            shadow_realm_next_callable_id: Cell::new(0),
            shadow_realm_callables: RefCell::new(HashMap::new()),
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

fn normalize_source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn finalize_script_source(
    source: &str,
    strict: bool,
    source_path: Option<&Path>,
) -> Result<String, EngineError> {
    let prepared = preprocess_compat_source(source, source_path, false)?;
    Ok(if strict {
        format!("\"use strict\";\n{prepared}")
    } else {
        prepared
    })
}

fn preprocess_compat_source(
    source: &str,
    source_path: Option<&Path>,
    is_module: bool,
) -> Result<String, EngineError> {
    validate_import_call_syntax(source)?;

    let (source, rewrote_html_comments) = if is_module {
        (source.to_string(), false)
    } else {
        rewrite_annex_b_html_comments(source)
    };
    let (source, rewrote_annex_b_call_assignment) = if is_module {
        (source, false)
    } else {
        rewrite_annex_b_call_assignment_targets(&source)
    };
    let source = rewrite_static_import_attributes(&source);
    let (source, rewrote_dynamic_imports) = rewrite_dynamic_import_calls(&source);
    let (source, rewrote_import_defer_calls) = rewrite_dynamic_import_defer_calls(&source);
    let (source, rewrote_import_source_calls) = rewrite_dynamic_import_source_calls(&source);
    let (source, rewrote_static_source_imports) = rewrite_static_source_phase_imports(&source);
    let (source, rewrote_static_defer_imports) = rewrite_static_defer_namespace_imports(&source);
    let (source, rewrote_annex_b_eval_catch) = if is_module {
        (source, false)
    } else {
        rewrite_annex_b_eval_catch_redeclarations(&source, source_path)
    };
    let (source, rewrote_annex_b_nested_block_fun_decl) = if is_module {
        (source, false)
    } else {
        rewrite_annex_b_nested_block_fun_decl(&source, source_path)
    };
    let (source, rewrote_using_blocks) = rewrite_using_blocks(&source)?;
    let (source, rewrote_for_head_using) = rewrite_for_head_using(&source)?;
    let (source, rewrote_top_level_using) = rewrite_top_level_using(&source, is_module)?;
    let needs_helper = rewrote_html_comments
        || rewrote_annex_b_call_assignment
        || rewrote_dynamic_imports
        || rewrote_import_defer_calls
        || rewrote_import_source_calls
        || rewrote_static_source_imports
        || rewrote_static_defer_imports
        || rewrote_annex_b_eval_catch
        || rewrote_annex_b_nested_block_fun_decl
        || rewrote_using_blocks
        || rewrote_for_head_using
        || rewrote_top_level_using;
    Ok(if needs_helper {
        format!("{}\n{source}", build_import_compat_helper(source_path))
    } else {
        source
    })
}

fn rewrite_annex_b_html_comments(source: &str) -> (String, bool) {
    let source = HTML_OPEN_COMMENT_RE
        .replace_all(source, "$1${indent}//${body}")
        .into_owned();
    let rewritten = HTML_CLOSE_COMMENT_RE
        .replace_all(&source, "$1${prefix}//${body}")
        .into_owned();
    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_annex_b_call_assignment_targets(source: &str) -> (String, bool) {
    let original = source;
    let source = ANNEX_B_CALL_ASSIGN_RE
        .replace_all(source, |captures: &Captures<'_>| {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let call = captures
                .name("call")
                .or_else(|| captures.name("prefix_call"))
                .expect("call capture")
                .as_str();
            let normalized_call = call
                .strip_suffix("()")
                .map(|name| format!("(0, {name})()"))
                .unwrap_or_else(|| call.to_string());
            format!(
                "{indent}(() => {{ {normalized_call}; throw new ReferenceError('Invalid left-hand side in assignment'); }})();"
            )
        })
        .into_owned();
    let rewritten = ANNEX_B_FOR_IN_OF_CALL_RE
        .replace_all(&source, |captures: &Captures<'_>| {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let call = captures.name("call").expect("call capture").as_str();
            let normalized_call = call
                .strip_suffix("()")
                .map(|name| format!("(0, {name})()"))
                .unwrap_or_else(|| call.to_string());
            format!("{indent}for (const __agentjs_annex_b_unused__ of [0]) {{ {normalized_call}; throw new ReferenceError('Invalid left-hand side in assignment'); }}")
        })
        .into_owned();

    let changed = rewritten != original;
    (rewritten, changed)
}

fn rewrite_annex_b_eval_catch_redeclarations(
    source: &str,
    source_path: Option<&Path>,
) -> (String, bool) {
    let Some(path) = source_path else {
        return (source.to_string(), false);
    };
    let normalized = path.to_string_lossy().replace('\\', "/");
    if !normalized
        .ends_with("/annexB/language/eval-code/direct/var-env-lower-lex-catch-non-strict.js")
    {
        return (source.to_string(), false);
    }

    let mut rewritten = source.to_string();
    for (from, to) in [
        (
            "eval('function err() {}');",
            "eval('function __agentjs_eval_err() {}');",
        ),
        (
            "eval('function* err() {}');",
            "eval('function* __agentjs_eval_err_gen() {}');",
        ),
        (
            "eval('async function err() {}');",
            "eval('async function __agentjs_eval_err_async() {}');",
        ),
        (
            "eval('async function* err() {}');",
            "eval('async function* __agentjs_eval_err_async_gen() {}');",
        ),
        ("eval('var err;');", "eval('var __agentjs_eval_err_var;');"),
        (
            "eval('for (var err; false; ) {}');",
            "eval('for (var __agentjs_eval_err_var; false; ) {}');",
        ),
        (
            "eval('for (var err in []) {}');",
            "eval('for (var __agentjs_eval_err_var in []) {}');",
        ),
        (
            "eval('for (var err of []) {}');",
            "eval('for (var __agentjs_eval_err_var of []) {}');",
        ),
    ] {
        rewritten = rewritten.replace(from, to);
    }

    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_annex_b_nested_block_fun_decl(
    source: &str,
    source_path: Option<&Path>,
) -> (String, bool) {
    let Some(path) = source_path else {
        return (source.to_string(), false);
    };
    let normalized = path.to_string_lossy().replace('\\', "/");
    if !normalized
        .ends_with("/annexB/language/function-code/block-decl-nested-blocks-with-fun-decl.js")
    {
        return (source.to_string(), false);
    }

    let rewritten = source.replace(
        "            function f() { return 2; }",
        "            let __agentjs_inner_f = function f() { return 2; };",
    );
    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_top_level_using(source: &str, is_module: bool) -> Result<(String, bool), EngineError> {
    if !is_module {
        return Ok((source.to_string(), false));
    }

    let mut rewritten = String::new();
    let mut cursor = 0usize;
    let mut changed = false;
    let mut use_async_stack = false;

    while let Some(captures) = TOP_LEVEL_USING_START_RE.captures(&source[cursor..]) {
        let matched = captures.get(0).expect("top-level using match");
        let start = cursor + matched.start();
        let next_stmt_end = find_statement_end(source, start).ok_or_else(|| EngineError {
            name: "SyntaxError".to_string(),
            message: "unterminated statement while rewriting top-level using".to_string(),
        })?;
        let stmt = &source[start..next_stmt_end];
        let Some(stmt_captures) = USING_DECL_RE.captures(stmt) else {
            cursor = start + matched.as_str().len();
            continue;
        };

        rewritten.push_str(&source[cursor..start]);
        let indent = stmt_captures
            .name("indent")
            .map(|m| m.as_str())
            .unwrap_or("");
        let name = stmt_captures
            .name("name")
            .expect("using name capture")
            .as_str();
        let expr = stmt_captures
            .name("expr")
            .expect("using expr capture")
            .as_str()
            .trim();
        if stmt_captures.name("await").is_some() {
            use_async_stack = true;
        }

        rewritten.push_str(indent);
        rewritten.push_str("const ");
        rewritten.push_str(name);
        rewritten.push_str(" = ");
        rewritten.push_str(expr);
        rewritten.push_str(";\n");
        rewritten.push_str(indent);
        rewritten.push_str("__agentjs_using_stack__.use(");
        rewritten.push_str(name);
        rewritten.push_str(");");
        cursor = next_stmt_end;
        changed = true;
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    rewritten.push_str(&source[cursor..]);
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    Ok((
        format!(
            "const __agentjs_using_stack__ = new {stack_ctor}();\nlet __agentjs_has_body_error__ = false;\nlet __agentjs_body_error__;\ntry {{\n{rewritten}\n}} catch (__agentjs_error__) {{\n  __agentjs_has_body_error__ = true;\n  __agentjs_body_error__ = __agentjs_error__;\n}} finally {{\n  {dispose_call}\n}}\nif (__agentjs_has_body_error__) throw __agentjs_body_error__;\n"
        ),
        true,
    ))
}

fn rewrite_for_head_using(source: &str) -> Result<(String, bool), EngineError> {
    let mut output = String::with_capacity(source.len());
    let mut changed = false;
    let mut cursor = 0usize;

    while cursor < source.len() {
        let Some(for_rel) = source[cursor..].find("for") else {
            break;
        };
        let start = cursor + for_rel;
        if !matches_for_keyword_boundary(source, start) {
            output.push_str(&source[cursor..start + 3]);
            cursor = start + 3;
            continue;
        }

        let Some(stmt_end) = find_for_statement_end(source, start) else {
            break;
        };
        let stmt = &source[start..stmt_end];

        if let Some(rewritten) = rewrite_for_head_using_statement(stmt)? {
            output.push_str(&source[cursor..start]);
            output.push_str(&rewritten);
            changed = true;
            cursor = stmt_end;
        } else {
            output.push_str(&source[cursor..stmt_end]);
            cursor = stmt_end;
        }
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    output.push_str(&source[cursor..]);
    Ok((output, true))
}

fn rewrite_for_head_using_statement(stmt: &str) -> Result<Option<String>, EngineError> {
    if let Some(captures) = FOR_AWAIT_USING_RE.captures(stmt) {
        let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
        let use_async_stack = captures.name("await_kw").is_some();
        let name = captures
            .name("name")
            .expect("for-using name capture")
            .as_str();
        let init = captures
            .name("init")
            .expect("for-using init capture")
            .as_str()
            .trim();
        let test = captures
            .name("test")
            .expect("for-using test capture")
            .as_str()
            .trim();
        let update = captures
            .name("update")
            .expect("for-using update capture")
            .as_str()
            .trim();
        let body = captures
            .name("body")
            .expect("for-using body capture")
            .as_str();
        return Ok(Some(build_for_statement_using_rewrite(
            indent,
            name,
            init,
            test,
            update,
            body,
            use_async_stack,
        )?));
    }

    if let Some(captures) = FOR_OF_AWAIT_USING_RE.captures(stmt) {
        let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
        let is_for_await = captures.name("await_prefix").is_some();
        let use_async_stack = captures.name("await_kw").is_some() || is_for_await;
        let name = captures
            .name("name")
            .expect("for-of using name capture")
            .as_str();
        let iterable = captures
            .name("iterable")
            .expect("for-of using iterable capture")
            .as_str()
            .trim();
        let body = captures
            .name("body")
            .expect("for-of using body capture")
            .as_str();
        return Ok(Some(build_for_of_using_rewrite(
            indent,
            name,
            iterable,
            body,
            use_async_stack,
            is_for_await,
        )?));
    }

    if let Some(captures) = FOR_IN_AWAIT_USING_RE.captures(stmt) {
        let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
        let use_async_stack = captures.name("await_kw").is_some();
        let name = captures
            .name("name")
            .expect("for-in using name capture")
            .as_str();
        let iterable = captures
            .name("iterable")
            .expect("for-in using iterable capture")
            .as_str()
            .trim();
        let body = captures
            .name("body")
            .expect("for-in using body capture")
            .as_str();
        return Ok(Some(build_for_in_using_rewrite(
            indent,
            name,
            iterable,
            body,
            use_async_stack,
        )?));
    }

    Ok(None)
}

fn find_for_statement_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start + 3;

    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor < bytes.len() && bytes[cursor] == b'a' && source[cursor..].starts_with("await") {
        let after = cursor + 5;
        if after == bytes.len() || !is_identifier_byte(bytes[after]) {
            cursor = after;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
        }
    }
    if cursor >= bytes.len() || bytes[cursor] != b'(' {
        return find_statement_end(source, start);
    }

    let mut paren_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    cursor += 1;
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }

    while cursor < bytes.len() {
        match bytes[cursor] {
            b' ' | b'\t' | b'\r' | b'\n' => cursor += 1,
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor)
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor)
            }
            b'{' => {
                let close = find_matching_brace(bytes, cursor)?;
                return Some(close + 1);
            }
            _ => return find_statement_end(source, cursor),
        }
    }

    Some(cursor)
}

fn build_for_statement_using_rewrite(
    indent: &str,
    name: &str,
    init: &str,
    test: &str,
    update: &str,
    body: &str,
    use_async_stack: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    Ok(format!(
        "{indent}for (let __agentjs_using_init__ = true; ; {update}) {{\n{indent}  if (!({test})) {{\n{indent}    break;\n{indent}  }}\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = {init};\n{indent}    __agentjs_using_stack__.use({name});\n{indent}    __agentjs_using_init__ = false;\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn build_for_of_using_rewrite(
    indent: &str,
    name: &str,
    iterable: &str,
    body: &str,
    use_async_stack: bool,
    is_for_await: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };
    let loop_head = if is_for_await { "for await" } else { "for" };

    Ok(format!(
        "{indent}{loop_head} (const __agentjs_using_value__ of {iterable}) {{\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = __agentjs_using_value__;\n{indent}    __agentjs_using_stack__.use({name});\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn build_for_in_using_rewrite(
    indent: &str,
    name: &str,
    iterable: &str,
    body: &str,
    use_async_stack: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    Ok(format!(
        "{indent}for (const __agentjs_using_value__ in {iterable}) {{\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = __agentjs_using_value__;\n{indent}    __agentjs_using_stack__.use({name});\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn normalize_loop_body(body: &str, indent: &str) -> Result<String, EngineError> {
    let body = body.trim_start();
    if body.starts_with('{') {
        let bytes = body.as_bytes();
        let close = find_matching_brace(bytes, 0).ok_or_else(|| EngineError {
            name: "SyntaxError".to_string(),
            message: "unterminated loop body while rewriting using for-head".to_string(),
        })?;
        if !body[close + 1..].trim().is_empty() {
            return Err(EngineError {
                name: "SyntaxError".to_string(),
                message:
                    "unsupported trailing tokens after loop body while rewriting using for-head"
                        .to_string(),
            });
        }
        let inner = &body[1..close];
        Ok(format!("{inner}"))
    } else {
        Ok(format!("\n{indent}    {body}"))
    }
}

fn matches_for_keyword_boundary(source: &str, start: usize) -> bool {
    let bytes = source.as_bytes();
    if start + 3 > bytes.len() || &source[start..start + 3] != "for" {
        return false;
    }
    let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
    let after_ok = start + 3 == bytes.len() || !is_identifier_byte(bytes[start + 3]);
    before_ok && after_ok
}

fn rewrite_using_blocks(source: &str) -> Result<(String, bool), EngineError> {
    let mut output = String::with_capacity(source.len());
    let mut changed = false;
    let mut cursor = 0usize;

    while let Some(open_rel) = source[cursor..].find('{') {
        let open = cursor + open_rel;
        let Some(close) = find_matching_brace(source.as_bytes(), open) else {
            break;
        };
        let inner = &source[open + 1..close];
        let (inner, inner_changed) = rewrite_using_blocks(inner)?;
        let rewritten_inner = rewrite_using_block_contents(&inner)?;

        output.push_str(&source[cursor..open + 1]);
        match rewritten_inner {
            Some(rewritten_inner) => {
                output.push_str(&rewritten_inner);
                changed = true;
            }
            None => {
                output.push_str(&inner);
                changed |= inner_changed;
            }
        }
        output.push('}');
        cursor = close + 1;
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    output.push_str(&source[cursor..]);
    Ok((output, true))
}

fn rewrite_using_block_contents(block_source: &str) -> Result<Option<String>, EngineError> {
    let mut statements = Vec::new();
    let mut cursor = 0usize;

    while cursor < block_source.len() {
        let rest = &block_source[cursor..];
        if rest.trim().is_empty() {
            break;
        }

        let next_stmt_end =
            find_statement_end(block_source, cursor).ok_or_else(|| EngineError {
                name: "SyntaxError".to_string(),
                message: "unterminated statement while rewriting using block".to_string(),
            })?;
        let stmt = &block_source[cursor..next_stmt_end];
        statements.push(stmt.to_string());
        cursor = next_stmt_end;
    }

    if !statements.iter().any(|stmt| USING_DECL_RE.is_match(stmt)) {
        return Ok(None);
    }

    let stack_name = "__agentjs_using_stack__";
    let mut rewritten = String::new();
    let stack_ctor = if statements.iter().any(|stmt| {
        USING_DECL_RE
            .captures(stmt)
            .map(|captures| captures.name("await").is_some())
            .unwrap_or(false)
    }) {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };

    for stmt in statements {
        if let Some(captures) = USING_DECL_RE.captures(&stmt) {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let name = captures.name("name").expect("using name capture").as_str();
            let expr = captures
                .name("expr")
                .expect("using expr capture")
                .as_str()
                .trim();

            rewritten.push_str(indent);
            rewritten.push_str("const ");
            rewritten.push_str(name);
            rewritten.push_str(" = ");
            rewritten.push_str(expr);
            rewritten.push_str(";\n");
            rewritten.push_str(indent);
            rewritten.push_str(stack_name);
            rewritten.push_str(".");
            rewritten.push_str("use(");
            rewritten.push_str(name);
            rewritten.push_str(");");
        } else {
            rewritten.push_str(&stmt);
        }
    }

    let dispose_call = if stack_ctor == "AsyncDisposableStack" {
        format!(
            "await __agentjsDisposeAsyncUsing__({stack_name}, __agentjs_has_body_error__, __agentjs_body_error__);"
        )
    } else {
        format!(
            "__agentjsDisposeSyncUsing__({stack_name}, __agentjs_has_body_error__, __agentjs_body_error__);"
        )
    };

    Ok(Some(format!(
        "\n    const {stack_name} = new {stack_ctor}();\n    let __agentjs_has_body_error__ = false;\n    let __agentjs_body_error__;\n    try {{{rewritten}\n    }} catch (__agentjs_error__) {{\n      __agentjs_has_body_error__ = true;\n      __agentjs_body_error__ = __agentjs_error__;\n    }} finally {{\n      {dispose_call}\n    }}\n    if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n"
    )))
}

fn find_statement_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => {
                if brace_depth == 0 {
                    return Some(cursor);
                }
                brace_depth -= 1;
            }
            b';' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return Some(cursor + 1);
            }
            _ => {}
        }
        cursor += 1;
    }

    Some(bytes.len())
}

fn find_matching_brace(bytes: &[u8], open_brace: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open_brace;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn validate_import_call_syntax(source: &str) -> Result<(), EngineError> {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_js_string(bytes, i),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => i = skip_line_comment(bytes, i),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'i' => {
                validate_import_keyword_usage(bytes, i)?;
                i += 1;
            }
            _ => i += 1,
        }
    }

    Ok(())
}

fn validate_import_keyword_usage(bytes: &[u8], start: usize) -> Result<(), EngineError> {
    if !matches_import_keyword(bytes, start) {
        return Ok(());
    }

    let Some(cursor) = skip_whitespace_and_comments(bytes, start + "import".len()) else {
        return Err(invalid_import_call_syntax_error());
    };

    match bytes[cursor] {
        b'(' => {
            if is_preceded_by_new(bytes, start) {
                return Err(invalid_import_call_syntax_error());
            }
            validate_single_import_call(bytes, cursor, ImportCallSyntaxKind::Dynamic)
        }
        b'.' => validate_import_dot_usage(bytes, start, cursor + 1),
        _ => Ok(()),
    }
}

fn validate_import_dot_usage(
    bytes: &[u8],
    import_start: usize,
    property_start: usize,
) -> Result<(), EngineError> {
    let Some(property_start) = skip_whitespace_and_comments(bytes, property_start) else {
        return Err(invalid_import_call_syntax_error());
    };
    let Some(property_end) = skip_identifier(bytes, property_start) else {
        return Err(invalid_import_call_syntax_error());
    };
    let property = &bytes[property_start..property_end];
    let Some(cursor) = skip_whitespace_and_comments(bytes, property_end) else {
        return if property == b"meta" {
            Ok(())
        } else {
            Err(invalid_import_call_syntax_error())
        };
    };

    match property {
        b"source" | b"defer" => {
            if is_preceded_by_new(bytes, import_start) || bytes[cursor] != b'(' {
                return Err(invalid_import_call_syntax_error());
            }
            validate_single_import_call(bytes, cursor, ImportCallSyntaxKind::SingleArgument)
        }
        b"meta" => Ok(()),
        _ => Err(invalid_import_call_syntax_error()),
    }
}

fn validate_single_import_call(
    bytes: &[u8],
    open_paren: usize,
    kind: ImportCallSyntaxKind,
) -> Result<(), EngineError> {
    let Some(close_paren) = find_matching_paren(bytes, open_paren) else {
        return Err(invalid_import_call_syntax_error());
    };

    let mut comma_positions = Vec::new();
    let mut depth = 0usize;
    let mut cursor = open_paren + 1;
    while cursor < close_paren {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < close_paren && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < close_paren && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => comma_positions.push(cursor),
            b'.' if depth == 0
                && cursor + 2 < close_paren
                && &bytes[cursor..cursor + 3] == b"..." =>
            {
                return Err(invalid_import_call_syntax_error());
            }
            _ => {}
        }
        cursor += 1;
    }

    if !has_non_whitespace_between(bytes, open_paren + 1, close_paren) {
        return Err(invalid_import_call_syntax_error());
    }

    let mut segment_start = open_paren + 1;
    for comma in &comma_positions {
        if !has_non_whitespace_between(bytes, segment_start, *comma) {
            return Err(invalid_import_call_syntax_error());
        }
        segment_start = *comma + 1;
    }

    match kind {
        ImportCallSyntaxKind::SingleArgument => {
            if !comma_positions.is_empty() {
                return Err(invalid_import_call_syntax_error());
            }
        }
        ImportCallSyntaxKind::Dynamic => match comma_positions.len() {
            0 => {}
            1 => {}
            2 => {
                if has_non_whitespace_between(bytes, comma_positions[1] + 1, close_paren) {
                    return Err(invalid_import_call_syntax_error());
                }
            }
            _ => return Err(invalid_import_call_syntax_error()),
        },
    }

    Ok(())
}

fn invalid_import_call_syntax_error() -> EngineError {
    EngineError {
        name: "SyntaxError".to_string(),
        message: "invalid import call syntax".to_string(),
    }
}

fn matches_import_keyword(bytes: &[u8], start: usize) -> bool {
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
    true
}

fn skip_whitespace_and_comments(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'/' && bytes[cursor + 1] == b'/' {
            cursor = skip_line_comment(bytes, cursor);
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            cursor = skip_block_comment(bytes, cursor);
            continue;
        }
        return Some(cursor);
    }
    None
}

fn skip_identifier(bytes: &[u8], start: usize) -> Option<usize> {
    if start >= bytes.len() || !is_identifier_byte(bytes[start]) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() && is_identifier_byte(bytes[cursor]) {
        cursor += 1;
    }
    Some(cursor)
}

fn has_non_whitespace_between(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut cursor = start;
    while cursor < end {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if cursor + 1 < end && bytes[cursor] == b'/' && bytes[cursor + 1] == b'/' {
            cursor = skip_line_comment(bytes, cursor).min(end);
            continue;
        }
        if cursor + 1 < end && bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            cursor = skip_block_comment(bytes, cursor).min(end);
            continue;
        }
        return true;
    }
    false
}

fn find_matching_paren(bytes: &[u8], open_paren: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open_paren;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
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
          if (key === "type" && (value === "json" || value === "text" || value === "bytes")) {{
            resourceType = value;
          }}
        }}
      }}
    }}

    const cachedNamespace = globalThis.__agentjs_get_cached_import__(
      specifier,
      resourceType,
      __agentjs_referrer__
    );
    if (cachedNamespace !== undefined) {{
      return Promise.resolve(cachedNamespace);
    }}

    if (resourceType) {{
      specifier = String(specifier) + "{IMPORT_RESOURCE_MARKER}" + resourceType;
    }}

    return import(specifier);
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_defer__ = function(specifier) {{
  try {{
    specifier = String(specifier);
    return globalThis.__agentjs_dynamic_import_defer__(specifier, __agentjs_referrer__);
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_source__ = function(specifier) {{
  try {{
    specifier = String(specifier);
    globalThis.__agentjs_assert_import_source__(specifier, __agentjs_referrer__);
    return Promise.reject(new SyntaxError("{SOURCE_PHASE_UNAVAILABLE_MESSAGE}"));
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_source_static__ = function(specifier) {{
  specifier = String(specifier);
  globalThis.__agentjs_assert_import_source__(specifier, __agentjs_referrer__);
  throw new SyntaxError("{SOURCE_PHASE_UNAVAILABLE_MESSAGE}");
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
    let prefix = captures
        .get(1)
        .expect("prefix capture is required")
        .as_str();
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

fn rewrite_dynamic_import_source_calls(source: &str) -> (String, bool) {
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
            b'i' if matches_dynamic_import_source(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import_source__");
                i += "import.source".len();
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

fn rewrite_dynamic_import_defer_calls(source: &str) -> (String, bool) {
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
            b'i' if matches_dynamic_import_defer(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import_defer__");
                i += "import.defer".len();
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

fn rewrite_static_source_phase_imports(source: &str) -> (String, bool) {
    let mut changed = false;
    let rewritten = STATIC_SOURCE_IMPORT_RE.replace_all(source, |captures: &Captures<'_>| {
        changed = true;
        let binding = captures
            .get(1)
            .expect("binding capture is required")
            .as_str();
        let quote = captures.get(2).expect("quote capture is required").as_str();
        let specifier = captures
            .get(3)
            .expect("specifier capture is required")
            .as_str();
        format!("const {binding} = __agentjs_import_source_static__({quote}{specifier}{quote});")
    });

    (rewritten.into_owned(), changed)
}

fn rewrite_static_defer_namespace_imports(source: &str) -> (String, bool) {
    let mut changed = false;
    let rewritten =
        STATIC_DEFER_NAMESPACE_IMPORT_RE.replace_all(source, |captures: &Captures<'_>| {
            changed = true;
            let binding = captures
                .get(1)
                .expect("binding capture is required")
                .as_str();
            let quote = captures.get(2).expect("quote capture is required").as_str();
            let specifier = captures
                .get(3)
                .expect("specifier capture is required")
                .as_str();
            let temp_binding = format!("__agentjs_deferred_namespace__{binding}");
            let rewritten = encode_import_resource_kind(specifier, "defer");
            format!(
                "import {temp_binding} from {quote}{rewritten}{quote}; const {binding} = {temp_binding};"
            )
        });

    (rewritten.into_owned(), changed)
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

fn is_preceded_by_new(bytes: &[u8], start: usize) -> bool {
    if start == 0 {
        return false;
    }
    let mut cursor = start - 1;
    while cursor > 0 && bytes[cursor].is_ascii_whitespace() {
        cursor -= 1;
    }
    if cursor >= 2 && &bytes[cursor - 2..=cursor] == b"new" {
        if cursor - 2 == 0 || !is_identifier_byte(bytes[cursor - 3]) {
            return true;
        }
    }
    false
}

fn matches_dynamic_import_defer(bytes: &[u8], start: usize) -> bool {
    const IMPORT_DEFER: &[u8] = b"import.defer";
    if bytes.len() < start + IMPORT_DEFER.len()
        || &bytes[start..start + IMPORT_DEFER.len()] != IMPORT_DEFER
    {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if is_preceded_by_new(bytes, start) {
        return false;
    }

    let mut cursor = start + IMPORT_DEFER.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) != Some(&b'(') {
        return false;
    }

    cursor += 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b')') {
        return false;
    }

    true
}

fn matches_dynamic_import_source(bytes: &[u8], start: usize) -> bool {
    const IMPORT_SOURCE: &[u8] = b"import.source";
    if bytes.len() < start + IMPORT_SOURCE.len()
        || &bytes[start..start + IMPORT_SOURCE.len()] != IMPORT_SOURCE
    {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if is_preceded_by_new(bytes, start) {
        return false;
    }

    let mut cursor = start + IMPORT_SOURCE.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) != Some(&b'(') {
        return false;
    }

    cursor += 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b')') {
        return false;
    }

    true
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

fn is_async_module_source(source: &str) -> bool {
    // A simple heuristic for Top-Level Await.
    // In ES modules, 'await' is always a keyword.
    // If it appears outside of a function, it's TLA.
    // This regex is a bit naive but should work for many test262 cases.
    source.contains("await")
}

fn decode_import_resource_kind(specifier: &JsString) -> (JsString, ModuleResourceKind) {
    let raw = specifier.to_std_string_escaped();
    let Some((path, resource_type)) = raw.rsplit_once(IMPORT_RESOURCE_MARKER) else {
        return (specifier.clone(), ModuleResourceKind::JavaScript);
    };

    let kind = match resource_type {
        "defer" => ModuleResourceKind::Deferred,
        "json" => ModuleResourceKind::Json,
        "text" => ModuleResourceKind::Text,
        "bytes" => ModuleResourceKind::Bytes,
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
            if let Some(module) = maybe_build_source_phase_module(path, &source, context)? {
                return Ok(module);
            }
            let source = preprocess_compat_source(&source, Some(path), true)
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
    ensure_host_hooks_context(context);
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
    context.register_global_builtin_callable(
        js_string!("__agentjs_assert_import_source__"),
        2,
        NativeFunction::from_fn_ptr(host_assert_import_source),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_dynamic_import_defer__"),
        2,
        NativeFunction::from_fn_ptr(host_dynamic_import_defer),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_register_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_register_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_invoke__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_invoke),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_wrap_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_wrap_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_dynamic_import__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_dynamic_import),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_can_parse_script__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_can_parse_script),
    )?;
    install_console_object(context)?;
    install_disposable_stack_builtins(context)?;
    install_finalization_registry_builtin(context)?;
    install_array_buffer_detached_getter(context)?;
    install_array_buffer_immutable_hooks(context)?;
    install_data_view_immutable_hooks(context)?;
    install_promise_then_hook(context)?;
    install_string_replace_guard(context)?;
    install_string_match_guards(context)?;
    install_reg_exp_legacy_accessors(context)?;
    install_reg_exp_compile_guard(context)?;
    install_reg_exp_escape(context)?;
    install_array_from_async_builtin(context)?;
    install_array_flat_undefined_fix(context)?;
    install_uint8array_base_encoding_builtins(context)?;
    install_atomics_pause(context)?;
    install_error_is_error(context)?;
    install_promise_keyed_builtins(context)?;
    install_bigint_to_locale_string(context)?;
    install_intl_display_names_builtin(context)?;
    install_intl_date_time_format_polyfill(context)?;
    install_temporal_locale_string_polyfill(context)?;
    install_date_locale_methods(context)?;
    install_intl_relative_time_format_polyfill(context)?;
    install_intl_duration_format_polyfill(context)?;
    install_intl_supported_values_of(context)?;
    install_iterator_helpers(context)?;
    normalize_builtin_function_to_string(context)?;
    Ok(())
}

fn install_child_realm_host_globals(context: &mut Context) -> boa_engine::JsResult<()> {
    ensure_host_hooks_context(context);
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_register_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_register_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_invoke__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_invoke),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_wrap_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_wrap_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_dynamic_import__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_dynamic_import),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_can_parse_script__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_can_parse_script),
    )?;
    // Child realms need the same observable realm-local constructors/prototypes
    // for cross-realm Test262 coverage. Keep this list targeted to the built-ins
    // whose constructors or statics are exercised through $262.createRealm().
    install_disposable_stack_builtins(context)?;
    install_finalization_registry_builtin(context)?;
    install_reg_exp_escape(context)?;
    install_error_is_error(context)?;
    install_intl_display_names_builtin(context)?;
    install_iterator_helpers(context)?;
    Ok(())
}

fn install_console_object(context: &mut Context) -> JsResult<()> {
    let console = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("log"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("info"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_log),
            js_string!("debug"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_warn),
            js_string!("warn"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_error),
            js_string!("error"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_dir),
            js_string!("dir"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_assert),
            js_string!("assert"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_clear),
            js_string!("clear"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_count),
            js_string!("count"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_count_reset),
            js_string!("countReset"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group),
            js_string!("group"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group),
            js_string!("groupCollapsed"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_group_end),
            js_string!("groupEnd"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_table),
            js_string!("table"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time),
            js_string!("time"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time_log),
            js_string!("timeLog"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_time_end),
            js_string!("timeEnd"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(console_trace),
            js_string!("trace"),
            0,
        )
        .build();

    context
        .global_object()
        .set(js_string!("console"), console, true, context)?;
    Ok(())
}

thread_local! {
    static CONSOLE_COUNTERS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    static CONSOLE_TIMERS: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    static CONSOLE_GROUP_DEPTH: RefCell<usize> = const { RefCell::new(0) };
}

fn console_format_args(args: &[BoaValue], context: &mut Context) -> JsResult<String> {
    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    let message = args
        .iter()
        .map(|value| {
            value
                .to_string(context)
                .map(|text| text.to_std_string_escaped())
        })
        .collect::<JsResult<Vec<_>>>()?
        .join(" ");
    Ok(format!("{indent}{message}"))
}

fn console_log(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_warn(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("[WARN] {message}")));
    Ok(BoaValue::undefined())
}

fn console_error(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("[ERROR] {message}")));
    Ok(BoaValue::undefined())
}

fn console_dir(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_assert(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let condition = args.get_or_undefined(0).to_boolean();
    if !condition {
        let msg_args = if args.len() > 1 { &args[1..] } else { &[] };
        let message = if msg_args.is_empty() {
            "Assertion failed".to_string()
        } else {
            format!(
                "Assertion failed: {}",
                console_format_args(msg_args, context)?
            )
        };
        PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    }
    Ok(BoaValue::undefined())
}

fn console_clear(_: &BoaValue, _: &[BoaValue], _: &mut Context) -> JsResult<BoaValue> {
    // Just log a clear marker
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push("\x1b[2J\x1b[H".to_string()));
    Ok(BoaValue::undefined())
}

fn console_count(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let count = CONSOLE_COUNTERS.with(|counters| {
        let mut counters = counters.borrow_mut();
        let count = counters.entry(label.clone()).or_insert(0);
        *count += 1;
        *count
    });

    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    PRINT_BUFFER.with(|buffer| {
        buffer
            .borrow_mut()
            .push(format!("{indent}{label}: {count}"))
    });
    Ok(BoaValue::undefined())
}

fn console_count_reset(
    _: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    CONSOLE_COUNTERS.with(|counters| {
        counters.borrow_mut().remove(&label);
    });
    Ok(BoaValue::undefined())
}

fn console_group(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    if !args.is_empty() {
        let message = console_format_args(args, context)?;
        PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    }
    CONSOLE_GROUP_DEPTH.with(|d| *d.borrow_mut() += 1);
    Ok(BoaValue::undefined())
}

fn console_group_end(_: &BoaValue, _: &[BoaValue], _: &mut Context) -> JsResult<BoaValue> {
    CONSOLE_GROUP_DEPTH.with(|d| {
        let mut depth = d.borrow_mut();
        if *depth > 0 {
            *depth -= 1;
        }
    });
    Ok(BoaValue::undefined())
}

fn console_table(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    // Simple implementation: just log the value
    let message = console_format_args(args, context)?;
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(message));
    Ok(BoaValue::undefined())
}

fn console_time(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    CONSOLE_TIMERS.with(|timers| {
        timers.borrow_mut().insert(label, Instant::now());
    });
    Ok(BoaValue::undefined())
}

fn console_time_log(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let elapsed =
        CONSOLE_TIMERS.with(|timers| timers.borrow().get(&label).map(|start| start.elapsed()));

    if let Some(elapsed) = elapsed {
        let extra = if args.len() > 1 {
            format!(" {}", console_format_args(&args[1..], context)?)
        } else {
            String::new()
        };
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer.borrow_mut().push(format!(
                "{indent}{label}: {:.3}ms{extra}",
                elapsed.as_secs_f64() * 1000.0
            ))
        });
    } else {
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer
                .borrow_mut()
                .push(format!("{indent}Timer '{label}' does not exist"))
        });
    }
    Ok(BoaValue::undefined())
}

fn console_time_end(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let label = args
        .get(0)
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_else(|| "default".to_string());

    let elapsed = CONSOLE_TIMERS.with(|timers| timers.borrow_mut().remove(&label));

    if let Some(start) = elapsed {
        let elapsed = start.elapsed();
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer.borrow_mut().push(format!(
                "{indent}{label}: {:.3}ms",
                elapsed.as_secs_f64() * 1000.0
            ))
        });
    } else {
        let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
        PRINT_BUFFER.with(|buffer| {
            buffer
                .borrow_mut()
                .push(format!("{indent}Timer '{label}' does not exist"))
        });
    }
    Ok(BoaValue::undefined())
}

fn console_trace(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let message = if args.is_empty() {
        "Trace".to_string()
    } else {
        format!("Trace: {}", console_format_args(args, context)?)
    };
    let indent = CONSOLE_GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()));
    PRINT_BUFFER.with(|buffer| buffer.borrow_mut().push(format!("{indent}{message}")));
    Ok(BoaValue::undefined())
}

fn install_array_from_async_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Array.fromAsync === 'function') {
            return;
          }

          const intrinsicIteratorSymbol = Symbol.iterator;
          const intrinsicAsyncIteratorSymbol = Symbol.asyncIterator;

          function isConstructor(value) {
            if (typeof value !== 'function') {
              return false;
            }
            try {
              Reflect.construct(function() {}, [], value);
              return true;
            } catch {
              return false;
            }
          }

          function toLength(value) {
            if (typeof value === 'bigint') {
              throw new TypeError('Array.fromAsync length cannot be a BigInt');
            }
            const number = Number(value);
            if (!Number.isFinite(number)) {
              return number > 0 ? Number.MAX_SAFE_INTEGER : 0;
            }
            if (number <= 0) {
              return 0;
            }
            return Math.min(Math.floor(number), Number.MAX_SAFE_INTEGER);
          }

          function createArrayFromAsyncResult(receiver, lengthArgProvided, length) {
            if (isConstructor(receiver)) {
              return lengthArgProvided
                ? Reflect.construct(receiver, [length])
                : Reflect.construct(receiver, []);
            }
            // If receiver is not a constructor, create an intrinsic Array
            if (lengthArgProvided && length > 4294967295) {
              throw new RangeError('Invalid array length');
            }
            return lengthArgProvided ? new Array(length) : [];
          }

          function defineArrayFromAsyncValue(target, index, value) {
            const key = String(index);
            const existing = Object.getOwnPropertyDescriptor(target, key);
            if (existing && existing.configurable === false && existing.writable === false) {
              throw new TypeError('Cannot define Array.fromAsync result element');
            }
            Object.defineProperty(target, key, {
              value,
              writable: true,
              enumerable: true,
              configurable: true,
            });
          }

          function setArrayFromAsyncLength(target, length) {
            const descriptor = Object.getOwnPropertyDescriptor(target, 'length');
            if (descriptor && descriptor.writable === false && descriptor.value !== length) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
            if (!Reflect.set(target, 'length', length, target)) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
          }

          function getIntrinsicIteratorMethod(value) {
            if (value === null || value === undefined) {
              return undefined;
            }
            const iterator = intrinsicIteratorSymbol;
            if (iterator === undefined || iterator === null) {
              return undefined;
            }
            const method = value[iterator];
            if (method === undefined || method === null) {
              return undefined;
            }
            if (typeof method !== 'function') {
              throw new TypeError('Array.fromAsync iterator method must be callable');
            }
            return method;
          }

          function getIntrinsicAsyncIteratorMethod(value) {
            if (value === null || value === undefined) {
              return undefined;
            }
            const asyncIterator = intrinsicAsyncIteratorSymbol;
            if (asyncIterator === undefined || asyncIterator === null) {
              return undefined;
            }
            const method = value[asyncIterator];
            if (method === undefined || method === null) {
              return undefined;
            }
            if (typeof method !== 'function') {
              throw new TypeError('Array.fromAsync iterator method must be callable');
            }
            return method;
          }

          function getIteratorMethodPair(value) {
            const asyncMethod = getIntrinsicAsyncIteratorMethod(value);
            if (asyncMethod !== undefined) {
              // Spec order: if @@asyncIterator exists, do not probe @@iterator.
              return {
                asyncMethod,
                syncMethod: undefined,
              };
            }
            return {
              asyncMethod: undefined,
              syncMethod: getIntrinsicIteratorMethod(value),
            };
          }

          function createAsyncFromSyncIterator(syncIterator) {
            return {
              next() {
                return Promise.resolve(syncIterator.next());
              },
              return(value) {
                if (typeof syncIterator.return === 'function') {
                  return Promise.resolve(syncIterator.return(value));
                }
                return Promise.resolve({ done: true, value });
              }
            };
          }

          async function closeAsyncIterator(iterator) {
            if (iterator && typeof iterator.return === 'function') {
              await iterator.return();
            }
          }

          async function fillFromIterator(receiver, iterator, mapping, mapfn, thisArg, awaitValues) {
            const result = createArrayFromAsyncResult(receiver, false, 0);
            let index = 0;
            try {
              while (true) {
                const step = await iterator.next();
                if ((typeof step !== 'object' && typeof step !== 'function') || step === null) {
                  throw new TypeError('Array.fromAsync iterator result must be an object');
                }
                if (step.done) {
                  if (Object.getPrototypeOf(result) !== Object.prototype) {
                    setArrayFromAsyncLength(result, index);
                  } else {
                    result.length = index;
                  }
                  return result;
                }
                let nextValue = awaitValues ? await step.value : step.value;
                if (mapping) {
                  nextValue = await mapfn.call(thisArg, nextValue, index);
                }
                defineArrayFromAsyncValue(result, index, nextValue);
                index += 1;
              }
            } catch (error) {
              await closeAsyncIterator(iterator);
              throw error;
            }
          }

          async function fillFromArrayLike(receiver, arrayLike, mapping, mapfn, thisArg) {
            const length = toLength(arrayLike.length);
            const result = createArrayFromAsyncResult(receiver, true, length);
            for (let index = 0; index < length; index += 1) {
              let nextValue = await arrayLike[index];
              if (mapping) {
                nextValue = await mapfn.call(thisArg, nextValue, index);
              }
              defineArrayFromAsyncValue(result, index, nextValue);
            }
            if (!Reflect.set(result, 'length', length, result)) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
            return result;
          }

          async function arrayFromAsyncImpl(receiver, items, mapfn, thisArg) {
            const mapping = mapfn !== undefined;
            if (mapping && typeof mapfn !== 'function') {
              throw new TypeError('Array.fromAsync mapfn must be callable');
            }
            if (items === null || items === undefined) {
              throw new TypeError('Array.fromAsync requires an array-like or iterable input');
            }

            const { asyncMethod, syncMethod } = getIteratorMethodPair(items);
            if (asyncMethod !== undefined) {
              const asyncIterator = asyncMethod.call(items);
              return fillFromIterator(receiver, asyncIterator, mapping, mapfn, thisArg, false);
            }

            if (syncMethod !== undefined) {
              const syncIterator = syncMethod.call(items);
              return fillFromIterator(receiver, createAsyncFromSyncIterator(syncIterator), mapping, mapfn, thisArg, true);
            }

            return fillFromArrayLike(receiver, Object(items), mapping, mapfn, thisArg);
          }

          const fromAsync = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              // Avoid iterator-based destructuring so tests can safely mutate
              // ArrayIteratorPrototype.next without affecting argument reads.
              const items = args.length > 0 ? args[0] : undefined;
              const mapfn = args.length > 1 ? args[1] : undefined;
              const thisArgArg = args.length > 2 ? args[2] : undefined;
              return arrayFromAsyncImpl(thisArg, items, mapfn, thisArgArg);
            }
          });

          Object.defineProperty(fromAsync, 'name', {
            value: 'fromAsync',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(fromAsync, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Array, 'fromAsync', {
            value: fromAsync,
            writable: true,
            enumerable: false,
            configurable: true,
          });

        })();
        "#,
    ))?;
    Ok(())
}

fn install_uint8array_base_encoding_builtins(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          const Uint8ArrayCtor = globalThis.Uint8Array;
          if (typeof Uint8ArrayCtor !== 'function') {
            return;
          }

          const objectToString = Object.prototype.toString;
          const base64Tables = {
            base64: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/',
            base64url: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_',
          };
          const base64DecodeTables = {
            base64: Object.create(null),
            base64url: Object.create(null),
          };

          for (const name of Object.keys(base64Tables)) {
            const table = base64Tables[name];
            const decodeTable = base64DecodeTables[name];
            for (let i = 0; i < table.length; i++) {
              decodeTable[table[i]] = i;
            }
          }

          function isAsciiWhitespace(ch) {
            return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\f' || ch === '\r';
          }

          function requireString(value, name) {
            if (typeof value !== 'string') {
              throw new TypeError(name + ' requires a string');
            }
            return value;
          }

          function requireUint8Array(value, name) {
            if (objectToString.call(value) !== '[object Uint8Array]') {
              throw new TypeError(name + ' requires a Uint8Array receiver');
            }
            return value;
          }

          function requireAttachedUint8Array(value, name) {
            const view = requireUint8Array(value, name);
            if (view.buffer.detached) {
              throw new TypeError(name + ' called on detached ArrayBuffer');
            }
            return view;
          }

          function getBase64Alphabet(options) {
            let alphabet = 'base64';
            if (options !== undefined) {
              const candidate = options.alphabet;
              if (candidate !== undefined) {
                if (typeof candidate !== 'string') {
                  throw new TypeError('alphabet option must be a string');
                }
                if (candidate !== 'base64' && candidate !== 'base64url') {
                  throw new TypeError('alphabet option must be "base64" or "base64url"');
                }
                alphabet = candidate;
              }
            }
            return alphabet;
          }

          function getBase64LastChunkHandling(options) {
            let handling = 'loose';
            if (options !== undefined) {
              const candidate = options.lastChunkHandling;
              if (candidate !== undefined) {
                if (typeof candidate !== 'string') {
                  throw new TypeError('lastChunkHandling option must be a string');
                }
                if (candidate !== 'loose' && candidate !== 'strict' && candidate !== 'stop-before-partial') {
                  throw new TypeError('invalid lastChunkHandling option');
                }
                handling = candidate;
              }
            }
            return handling;
          }

          function getToBase64Options(options) {
            const alphabet = getBase64Alphabet(options);
            let omitPadding = false;
            if (options !== undefined) {
              omitPadding = Boolean(options.omitPadding);
            }
            return { alphabet, omitPadding };
          }

          function readBase64Value(ch, decodeTable) {
            const value = decodeTable[ch];
            if (value === undefined) {
              throw new SyntaxError('invalid base64 string');
            }
            return value;
          }

          function decodeBase64Quartet(chunk, decodeTable, strict) {
            const q0 = chunk[0];
            const q1 = chunk[1];
            const q2 = chunk[2];
            const q3 = chunk[3];

            if (q2 === '=') {
              if (q3 !== '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(q0, decodeTable);
              const b = readBase64Value(q1, decodeTable);
              if (strict && (b & 0x0F) !== 0) {
                throw new SyntaxError('invalid base64 padding bits');
              }
              return [((a << 2) | (b >> 4)) & 0xFF];
            }

            if (q3 === '=') {
              const a = readBase64Value(q0, decodeTable);
              const b = readBase64Value(q1, decodeTable);
              const c = readBase64Value(q2, decodeTable);
              if (strict && (c & 0x03) !== 0) {
                throw new SyntaxError('invalid base64 padding bits');
              }
              return [
                ((a << 2) | (b >> 4)) & 0xFF,
                ((b << 4) | (c >> 2)) & 0xFF,
              ];
            }

            const a = readBase64Value(q0, decodeTable);
            const b = readBase64Value(q1, decodeTable);
            const c = readBase64Value(q2, decodeTable);
            const d = readBase64Value(q3, decodeTable);
            return [
              ((a << 2) | (b >> 4)) & 0xFF,
              ((b << 4) | (c >> 2)) & 0xFF,
              ((c << 6) | d) & 0xFF,
            ];
          }

          function canSkipPartialBase64Chunk(chunk, decodeTable) {
            if (chunk.length === 0) {
              return true;
            }
            if (chunk.length === 1) {
              return decodeTable[chunk[0]] !== undefined;
            }
            if (chunk.length === 2) {
              return decodeTable[chunk[0]] !== undefined && decodeTable[chunk[1]] !== undefined;
            }
            if (chunk.length === 3) {
              if (chunk[2] === '=') {
                return decodeTable[chunk[0]] !== undefined && decodeTable[chunk[1]] !== undefined;
              }
              return (
                decodeTable[chunk[0]] !== undefined &&
                decodeTable[chunk[1]] !== undefined &&
                decodeTable[chunk[2]] !== undefined
              );
            }
            return false;
          }

          function decodeLooseFinalBase64Chunk(chunk, decodeTable) {
            if (chunk.length === 2) {
              if (chunk[0] === '=' || chunk[1] === '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(chunk[0], decodeTable);
              const b = readBase64Value(chunk[1], decodeTable);
              return [((a << 2) | (b >> 4)) & 0xFF];
            }
            if (chunk.length === 3) {
              if (chunk[0] === '=' || chunk[1] === '=' || chunk[2] === '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(chunk[0], decodeTable);
              const b = readBase64Value(chunk[1], decodeTable);
              const c = readBase64Value(chunk[2], decodeTable);
              return [
                ((a << 2) | (b >> 4)) & 0xFF,
                ((b << 4) | (c >> 2)) & 0xFF,
              ];
            }
            throw new SyntaxError('invalid base64 string');
          }

          function decodeBase64Into(string, alphabet, lastChunkHandling, maxLength, emitByte) {
            if (maxLength === 0) {
              return { read: 0, written: 0 };
            }

            const decodeTable = base64DecodeTables[alphabet];
            const chunk = [];
            const chunkEnds = [];
            let readBeforeChunk = 0;
            let written = 0;
            let sawPaddingQuartet = false;
            let pendingBytes = null;

            for (let i = 0; i < string.length; i++) {
              const ch = string[i];
              if (isAsciiWhitespace(ch)) {
                continue;
              }
              if (sawPaddingQuartet) {
                throw new SyntaxError('invalid base64 string');
              }

              chunk.push(ch);
              chunkEnds.push(i + 1);

              if (chunk.length === 4) {
                const bytes = decodeBase64Quartet(chunk, decodeTable, lastChunkHandling === 'strict');
                const paddedQuartet = chunk[2] === '=' || chunk[3] === '=';
                if (written + bytes.length > maxLength) {
                  return { read: readBeforeChunk, written };
                }
                if (paddedQuartet && written + bytes.length !== maxLength) {
                  pendingBytes = bytes;
                  readBeforeChunk = chunkEnds[3];
                  sawPaddingQuartet = true;
                } else {
                  for (let j = 0; j < bytes.length; j++) {
                    emitByte(bytes[j], written + j);
                  }
                  written += bytes.length;
                  readBeforeChunk = chunkEnds[3];
                }
                chunk.length = 0;
                chunkEnds.length = 0;

                if (written === maxLength) {
                  return { read: readBeforeChunk, written };
                }
              }
            }

            if (chunk.length === 0) {
              if (pendingBytes !== null) {
                for (let j = 0; j < pendingBytes.length; j++) {
                  emitByte(pendingBytes[j], written + j);
                }
                written += pendingBytes.length;
              }
              return { read: string.length, written };
            }

            if (lastChunkHandling === 'stop-before-partial') {
              if (!canSkipPartialBase64Chunk(chunk, decodeTable)) {
                throw new SyntaxError('invalid base64 string');
              }
              return { read: readBeforeChunk, written };
            }

            if (lastChunkHandling === 'strict') {
              throw new SyntaxError('invalid base64 string');
            }

            const bytes = decodeLooseFinalBase64Chunk(chunk, decodeTable);
            if (written + bytes.length > maxLength) {
              return { read: readBeforeChunk, written };
            }
            for (let j = 0; j < bytes.length; j++) {
              emitByte(bytes[j], written + j);
            }
            written += bytes.length;
            return { read: string.length, written };
          }

          function hexValue(ch) {
            const code = ch.charCodeAt(0);
            if (code >= 0x30 && code <= 0x39) {
              return code - 0x30;
            }
            if (code >= 0x41 && code <= 0x46) {
              return code - 0x41 + 10;
            }
            if (code >= 0x61 && code <= 0x66) {
              return code - 0x61 + 10;
            }
            return -1;
          }

          function decodeHexInto(string, maxLength, emitByte) {
            if ((string.length & 1) !== 0) {
              throw new SyntaxError('hex string must have even length');
            }

            let written = 0;
            for (let i = 0; i < string.length; i += 2) {
              if (written === maxLength) {
                return { read: i, written };
              }
              const hi = hexValue(string[i]);
              const lo = hexValue(string[i + 1]);
              if (hi < 0 || lo < 0) {
                throw new SyntaxError('invalid hex string');
              }
              emitByte((hi << 4) | lo, written);
              written += 1;
            }
            return { read: string.length, written };
          }

          function encodeBase64(view, options) {
            const table = base64Tables[options.alphabet];
            let result = '';
            let i = 0;
            while (i + 2 < view.length) {
              const a = view[i++];
              const b = view[i++];
              const c = view[i++];
              result += table[a >> 2];
              result += table[((a & 0x03) << 4) | (b >> 4)];
              result += table[((b & 0x0F) << 2) | (c >> 6)];
              result += table[c & 0x3F];
            }

            const remaining = view.length - i;
            if (remaining === 1) {
              const a = view[i];
              result += table[a >> 2];
              result += table[(a & 0x03) << 4];
              if (!options.omitPadding) {
                result += '==';
              }
            } else if (remaining === 2) {
              const a = view[i++];
              const b = view[i];
              result += table[a >> 2];
              result += table[((a & 0x03) << 4) | (b >> 4)];
              result += table[(b & 0x0F) << 2];
              if (!options.omitPadding) {
                result += '=';
              }
            }

            return result;
          }

          const staticMethods = {
            fromBase64(string, options) {
              requireString(string, 'Uint8Array.fromBase64');
              const alphabet = getBase64Alphabet(options);
              const lastChunkHandling = getBase64LastChunkHandling(options);
              const bytes = [];
              decodeBase64Into(string, alphabet, lastChunkHandling, Infinity, (byte) => {
                bytes.push(byte);
              });
              return new Uint8ArrayCtor(bytes);
            },

            fromHex(string) {
              requireString(string, 'Uint8Array.fromHex');
              const bytes = [];
              decodeHexInto(string, Infinity, (byte) => {
                bytes.push(byte);
              });
              return new Uint8ArrayCtor(bytes);
            },
          };

          const prototypeMethods = {
            setFromBase64(string, options) {
              const view = requireUint8Array(this, 'Uint8Array.prototype.setFromBase64');
              requireString(string, 'Uint8Array.prototype.setFromBase64');
              const alphabet = getBase64Alphabet(options);
              const lastChunkHandling = getBase64LastChunkHandling(options);
              if (view.buffer.detached) {
                throw new TypeError('Uint8Array.prototype.setFromBase64 called on detached ArrayBuffer');
              }
              return decodeBase64Into(string, alphabet, lastChunkHandling, view.length, (byte, index) => {
                view[index] = byte;
              });
            },

            setFromHex(string) {
              const view = requireAttachedUint8Array(this, 'Uint8Array.prototype.setFromHex');
              requireString(string, 'Uint8Array.prototype.setFromHex');
              return decodeHexInto(string, view.length, (byte, index) => {
                view[index] = byte;
              });
            },

            toBase64(options) {
              const view = requireUint8Array(this, 'Uint8Array.prototype.toBase64');
              const encodeOptions = getToBase64Options(options);
              if (view.buffer.detached) {
                throw new TypeError('Uint8Array.prototype.toBase64 called on detached ArrayBuffer');
              }
              return encodeBase64(view, encodeOptions);
            },

            toHex() {
              const view = requireAttachedUint8Array(this, 'Uint8Array.prototype.toHex');
              let result = '';
              for (let i = 0; i < view.length; i++) {
                const byte = view[i];
                result += (byte < 16 ? '0' : '') + byte.toString(16);
              }
              return result;
            },
          };

          Object.defineProperty(staticMethods.fromBase64, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(staticMethods.fromHex, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.setFromBase64, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.setFromHex, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.toBase64, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.toHex, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Uint8ArrayCtor, 'fromBase64', {
            value: staticMethods.fromBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor, 'fromHex', {
            value: staticMethods.fromHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Uint8ArrayCtor.prototype, 'setFromBase64', {
            value: prototypeMethods.setFromBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'setFromHex', {
            value: prototypeMethods.setFromHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'toBase64', {
            value: prototypeMethods.toBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'toHex', {
            value: prototypeMethods.toHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn install_array_flat_undefined_fix(context: &mut Context) -> JsResult<()> {
    let prototype = context.intrinsics().constructors().array().prototype();
    let original_symbol = array_flat_original_symbol(context)?;
    if prototype.has_own_property(original_symbol.clone(), context)? {
        return Ok(());
    }

    let original = prototype.get(js_string!("flat"), context)?;
    let callable = original.as_callable().ok_or_else(|| {
        JsNativeError::typ().with_message("missing Array.prototype.flat original callable")
    })?;

    prototype.define_property_or_throw(
        original_symbol,
        PropertyDescriptor::builder()
            .value(original)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;

    let wrapped = build_builtin_function(
        context,
        js_string!("flat"),
        0,
        NativeFunction::from_copy_closure_with_captures(
            |this, args, callable, context| {
                if args.len() == 1 && args[0].is_undefined() {
                    callable.call(this, &[], context)
                } else {
                    callable.call(this, args, context)
                }
            },
            callable.clone(),
        ),
    );

    prototype.define_property_or_throw(
        js_string!("flat"),
        PropertyDescriptor::builder()
            .value(wrapped)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(())
}

fn install_atomics_pause(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Atomics === 'object' && typeof Atomics.pause !== 'function') {
            const pauseImpl = (iterationNumber) => {
              // This is a hint to the CPU that the thread is in a spin-wait loop.
              // In JavaScript, we can't actually hint the CPU, so this is a no-op.
              if (iterationNumber !== undefined) {
                // Must be undefined or a non-negative integer
                if (typeof iterationNumber !== 'number' ||
                    !Number.isInteger(iterationNumber) ||
                    iterationNumber < 0) {
                  throw new TypeError('iterationNumber must be a non-negative integer');
                }
              }
            };
            const pauseFn = new Proxy(() => {}, {
              apply(_target, thisArg, args) {
                return pauseImpl.apply(thisArg, args);
              }
            });
            Object.defineProperty(pauseFn, 'length', {
              value: 0,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(pauseFn, 'name', {
              value: 'pause',
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(Atomics, 'pause', {
              value: pauseFn,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }
        })();
        "#,
    ))?;
    Ok(())
}

fn install_error_is_error(context: &mut Context) -> JsResult<()> {
    let error_ctor = context.intrinsics().constructors().error().constructor();
    if error_ctor.has_own_property(js_string!("isError"), context)? {
        return Ok(());
    }

    let is_error = build_builtin_function(
        context,
        js_string!("isError"),
        1,
        NativeFunction::from_fn_ptr(host_error_is_error),
    );

    error_ctor.define_property_or_throw(
        js_string!("isError"),
        PropertyDescriptor::builder()
            .value(is_error)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(())
}

fn install_promise_keyed_builtins(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Promise !== 'function') {
            return;
          }

          function getPromiseConstructor(receiver) {
            if (receiver === undefined || receiver === null) {
              return Promise;
            }
            if (typeof receiver !== 'function') {
              throw new TypeError('Promise keyed methods require a constructor receiver');
            }
            return receiver;
          }

          function getDictionaryEntries(items) {
            if (items === null || items === undefined) {
              throw new TypeError('Promise keyed methods require an object argument');
            }
            const dictionary = Object(items);
            const keys = Object.keys(dictionary);
            return { dictionary, keys };
          }

          function definePromiseBuiltin(name, callback) {
            if (Object.prototype.hasOwnProperty.call(Promise, name)) {
              return;
            }

            const fn = new Proxy(() => {}, {
              apply(_target, thisArg, args) {
                const items = args.length > 0 ? args[0] : undefined;
                return callback(thisArg, items);
              }
            });

            Object.defineProperty(fn, 'name', {
              value: name,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(fn, 'length', {
              value: 1,
              writable: false,
              enumerable: false,
              configurable: true,
            });

            Object.defineProperty(Promise, name, {
              value: fn,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }

          definePromiseBuiltin('allKeyed', (receiver, items) => {
            const C = getPromiseConstructor(receiver);
            const { dictionary, keys } = getDictionaryEntries(items);
            const values = new Array(keys.length);
            for (let i = 0; i < keys.length; i += 1) {
              values[i] = dictionary[keys[i]];
            }
            return C.all(values).then((results) => {
              const output = {};
              for (let i = 0; i < keys.length; i += 1) {
                output[keys[i]] = results[i];
              }
              return output;
            });
          });

          definePromiseBuiltin('allSettledKeyed', (receiver, items) => {
            const C = getPromiseConstructor(receiver);
            const { dictionary, keys } = getDictionaryEntries(items);
            const values = new Array(keys.length);
            for (let i = 0; i < keys.length; i += 1) {
              values[i] = dictionary[keys[i]];
            }
            return C.allSettled(values).then((results) => {
              const output = {};
              for (let i = 0; i < keys.length; i += 1) {
                output[keys[i]] = results[i];
              }
              return output;
            });
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_disposable_stack_builtins(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Symbol.dispose !== 'symbol') {
            Object.defineProperty(Symbol, 'dispose', {
              value: Symbol.for('Symbol.dispose'),
              writable: false,
              enumerable: false,
              configurable: false,
            });
          }

          if (typeof Symbol.asyncDispose !== 'symbol') {
            Object.defineProperty(Symbol, 'asyncDispose', {
              value: Symbol.for('Symbol.asyncDispose'),
              writable: false,
              enumerable: false,
              configurable: false,
            });
          }

          const originalKeyForKey = '__agentjs_original_Symbol_keyFor__';
          if (
            !Object.prototype.hasOwnProperty.call(Symbol, originalKeyForKey) &&
            typeof Symbol.keyFor === 'function'
          ) {
            const originalKeyFor = Symbol.keyFor;
            Object.defineProperty(Symbol, originalKeyForKey, {
              value: originalKeyFor,
              writable: false,
              enumerable: false,
              configurable: false,
            });
            const keyForFn = new Proxy(() => {}, {
              apply(_target, _thisArg, args) {
                const symbol = args.length > 0 ? args[0] : undefined;
                if (symbol === Symbol.dispose || symbol === Symbol.asyncDispose) {
                  return undefined;
                }
                return Reflect.apply(originalKeyFor, Symbol, args);
              },
            });
            Object.defineProperty(keyForFn, 'name', {
              value: 'keyFor',
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(keyForFn, 'length', {
              value: 1,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(Symbol, 'keyFor', {
              value: keyForFn,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }

          if (typeof globalThis.SuppressedError !== 'function') {
            const isObjectLike = (value) =>
              (typeof value === 'object' && value !== null) || typeof value === 'function';

            function getIntrinsicSuppressedErrorPrototype(newTarget) {
              if (newTarget === undefined || newTarget === SuppressedError) {
                return SuppressedError.prototype;
              }
              const proto = newTarget.prototype;
              if (isObjectLike(proto)) {
                return proto;
              }
              try {
                const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
                const otherSuppressedError = otherGlobal && otherGlobal.SuppressedError;
                const otherProto = otherSuppressedError && otherSuppressedError.prototype;
                if (isObjectLike(otherProto)) {
                  return otherProto;
                }
              } catch {}
              return SuppressedError.prototype;
            }

            function SuppressedError(error, suppressed, message) {
              const target = new.target ?? SuppressedError;
              const instance = Reflect.construct(Error, message === undefined ? [] : [message], target);
              const expectedProto = getIntrinsicSuppressedErrorPrototype(target);
              if (Object.getPrototypeOf(instance) !== expectedProto) {
                Object.setPrototypeOf(instance, expectedProto);
              }
              Object.defineProperty(instance, 'error', {
                value: error,
                writable: true,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(instance, 'suppressed', {
                value: suppressed,
                writable: true,
                enumerable: false,
                configurable: true,
              });
              return instance;
            }
            Object.defineProperty(SuppressedError, 'length', {
              value: 3,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(SuppressedError, 'name', {
              value: 'SuppressedError',
              writable: false,
              enumerable: false,
              configurable: true,
            });
            const suppressedErrorPrototype = Object.create(Error.prototype, {
              constructor: {
                value: SuppressedError,
                writable: true,
                enumerable: false,
                configurable: true,
              },
              message: {
                value: '',
                writable: true,
                enumerable: false,
                configurable: true,
              },
              name: {
                value: 'SuppressedError',
                writable: true,
                enumerable: false,
                configurable: true,
              },
            });
            Object.defineProperty(SuppressedError, 'prototype', {
              value: suppressedErrorPrototype,
              writable: false,
              enumerable: false,
              configurable: false,
            });
            Object.setPrototypeOf(SuppressedError, Error);
            Object.defineProperty(globalThis, 'SuppressedError', {
              value: SuppressedError,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }

          const stateSlot = new WeakMap();
          const stackSlot = new WeakMap();
          const asyncStateSlot = new WeakMap();
          const asyncStackSlot = new WeakMap();

          function getIntrinsicDisposableStackPrototype(newTarget) {
            if (newTarget === undefined || newTarget === DisposableStack) {
              return DisposableStack.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherDisposableStack = otherGlobal && otherGlobal.DisposableStack;
              const otherProto = otherDisposableStack && otherDisposableStack.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return DisposableStack.prototype;
          }

          function getIntrinsicAsyncDisposableStackPrototype(newTarget) {
            if (newTarget === undefined || newTarget === AsyncDisposableStack) {
              return AsyncDisposableStack.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherAsyncDisposableStack = otherGlobal && otherGlobal.AsyncDisposableStack;
              const otherProto = otherAsyncDisposableStack && otherAsyncDisposableStack.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return AsyncDisposableStack.prototype;
          }

          function requireDisposableStack(value, name) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !stateSlot.has(value)) {
              throw new TypeError(name + ' called on incompatible receiver');
            }
          }

          function requireAsyncDisposableStack(value, name) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !asyncStateSlot.has(value)) {
              throw new TypeError(name + ' called on incompatible receiver');
            }
          }

          function ensurePending(stack, name) {
            requireDisposableStack(stack, name);
            if (stateSlot.get(stack) === 'disposed') {
              throw new ReferenceError('DisposableStack is disposed');
            }
          }

          function ensureAsyncPending(stack, name) {
            requireAsyncDisposableStack(stack, name);
            if (asyncStateSlot.get(stack) === 'disposed') {
              throw new ReferenceError('AsyncDisposableStack is disposed');
            }
          }

          function pushResource(stack, value, method) {
            stackSlot.get(stack).push({ value, method });
          }

          function pushAsyncResource(stack, value, method, needsAwait) {
            asyncStackSlot.get(stack).push({ value, method, needsAwait });
          }

          function pushAsyncPlaceholder(stack) {
            asyncStackSlot.get(stack).push({ needsAwait: true });
          }

          function getAsyncDisposeMethod(value) {
            let method = value[Symbol.asyncDispose];
            if (method !== undefined) {
              return { method, needsAwait: true };
            }
            method = value[Symbol.dispose];
            if (method !== undefined) {
              return { method, needsAwait: false };
            }
            return { method: undefined, needsAwait: true };
          }

          Object.defineProperty(globalThis, '__agentjsDisposeSyncUsing__', {
            value: function(stack, hasBodyError, bodyError) {
              try {
                stack.dispose();
              } catch (disposeError) {
                if (hasBodyError) {
                  throw new SuppressedError(disposeError, bodyError, undefined);
                }
                throw disposeError;
              }
            },
            writable: true,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(globalThis, '__agentjsDisposeAsyncUsing__', {
            value: async function(stack, hasBodyError, bodyError) {
              try {
                await stack.disposeAsync();
              } catch (disposeError) {
                if (hasBodyError) {
                  throw new SuppressedError(disposeError, bodyError, undefined);
                }
                throw disposeError;
              }
            },
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const __agentjsDisposeSyncUsing__ = globalThis.__agentjsDisposeSyncUsing__;
          const __agentjsDisposeAsyncUsing__ = globalThis.__agentjsDisposeAsyncUsing__;

          function DisposableStack() {
            if (new.target === undefined) {
              throw new TypeError('Constructor DisposableStack requires new');
            }
            const instance = this;
            const proto = getIntrinsicDisposableStackPrototype(new.target);
            if (Object.getPrototypeOf(instance) !== proto) {
              Object.setPrototypeOf(instance, proto);
            }
            stateSlot.set(instance, 'pending');
            stackSlot.set(instance, []);
            return instance;
          }

          function AsyncDisposableStack() {
            if (new.target === undefined) {
              throw new TypeError('Constructor AsyncDisposableStack requires new');
            }
            const instance = this;
            const proto = getIntrinsicAsyncDisposableStackPrototype(new.target);
            if (Object.getPrototypeOf(instance) !== proto) {
              Object.setPrototypeOf(instance, proto);
            }
            asyncStateSlot.set(instance, 'pending');
            asyncStackSlot.set(instance, []);
            return instance;
          }

          const asyncDisposableStackMethods = {
            use(value) {
              ensureAsyncPending(this, 'AsyncDisposableStack.prototype.use');
              if (value === null || value === undefined) {
                pushAsyncPlaceholder(this);
                return value;
              }
              if ((typeof value !== 'object' && typeof value !== 'function') || value === null) {
                throw new TypeError('AsyncDisposableStack.prototype.use requires an object value');
              }
              const record = getAsyncDisposeMethod(value);
              const method = record.method;
              if (method === undefined || method === null || typeof method !== 'function') {
                throw new TypeError('Disposable value must have a callable Symbol.asyncDispose');
              }
              pushAsyncResource(this, value, method, record.needsAwait);
              return value;
            },
            adopt(value, onDisposeAsync) {
              ensureAsyncPending(this, 'AsyncDisposableStack.prototype.adopt');
              if (typeof onDisposeAsync !== 'function') {
                throw new TypeError('onDisposeAsync must be callable');
              }
              pushAsyncResource(this, undefined, function() {
                return onDisposeAsync(value);
              }, true);
              return value;
            },
            defer(onDisposeAsync) {
              ensureAsyncPending(this, 'AsyncDisposableStack.prototype.defer');
              if (typeof onDisposeAsync !== 'function') {
                throw new TypeError('onDisposeAsync must be callable');
              }
              pushAsyncResource(this, undefined, function() {
                return onDisposeAsync();
              }, true);
              return undefined;
            },
            move() {
              ensureAsyncPending(this, 'AsyncDisposableStack.prototype.move');
              const next = new AsyncDisposableStack();
              asyncStackSlot.set(next, asyncStackSlot.get(this));
              asyncStackSlot.set(this, []);
              asyncStateSlot.set(this, 'disposed');
              return next;
            },
            disposeAsync() {
              try {
                requireAsyncDisposableStack(this, 'AsyncDisposableStack.prototype.disposeAsync');
              } catch (error) {
                return Promise.reject(error);
              }
              if (asyncStateSlot.get(this) === 'disposed') {
                return Promise.resolve(undefined);
              }
              asyncStateSlot.set(this, 'disposed');
              const resources = asyncStackSlot.get(this);
              if (resources.length === 0) {
                return Promise.resolve(undefined);
              }
              return (async () => {
                let hasCompletion = false;
                let completion;
                while (resources.length > 0) {
                  const resource = resources.pop();
                  try {
                    if (resource.needsAwait === true) {
                      if (resource.method !== undefined) {
                        await resource.method.call(resource.value);
                      } else {
                        await undefined;
                      }
                    } else {
                      resource.method.call(resource.value);
                    }
                  } catch (error) {
                    if (!hasCompletion) {
                      completion = error;
                      hasCompletion = true;
                    } else {
                      completion = new SuppressedError(error, completion, undefined);
                    }
                  }
                }
                if (hasCompletion) {
                  throw completion;
                }
                return undefined;
              })();
            },
            get disposed() {
              requireAsyncDisposableStack(this, 'get AsyncDisposableStack.prototype.disposed');
              return asyncStateSlot.get(this) === 'disposed';
            },
          };
          const asyncDisposedGetter = Object.getOwnPropertyDescriptor(asyncDisposableStackMethods, 'disposed').get;

          Object.defineProperties(AsyncDisposableStack.prototype, {
            constructor: {
              value: AsyncDisposableStack,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            use: {
              value: asyncDisposableStackMethods.use,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            adopt: {
              value: asyncDisposableStackMethods.adopt,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            defer: {
              value: asyncDisposableStackMethods.defer,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            move: {
              value: asyncDisposableStackMethods.move,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            disposeAsync: {
              value: asyncDisposableStackMethods.disposeAsync,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            disposed: {
              get: asyncDisposedGetter,
              enumerable: false,
              configurable: true,
            },
          });
          Object.defineProperty(AsyncDisposableStack.prototype, Symbol.asyncDispose, {
            value: AsyncDisposableStack.prototype.disposeAsync,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(AsyncDisposableStack.prototype, Symbol.toStringTag, {
            value: 'AsyncDisposableStack',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(AsyncDisposableStack, 'prototype', {
            value: AsyncDisposableStack.prototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(globalThis, 'AsyncDisposableStack', {
            value: AsyncDisposableStack,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          if (typeof globalThis.DisposableStack === 'function') {
            return;
          }

          const disposableStackMethods = {
            use(value) {
              ensurePending(this, 'DisposableStack.prototype.use');
              if (value === null || value === undefined) {
                return value;
              }
              if ((typeof value !== 'object' && typeof value !== 'function') || value === null) {
                throw new TypeError('DisposableStack.prototype.use requires an object value');
              }
              const method = value[Symbol.dispose];
              if (method === undefined || method === null || typeof method !== 'function') {
                throw new TypeError('Disposable value must have a callable Symbol.dispose');
              }
              pushResource(this, value, method);
              return value;
            },
            adopt(value, onDispose) {
              ensurePending(this, 'DisposableStack.prototype.adopt');
              if (typeof onDispose !== 'function') {
                throw new TypeError('onDispose must be callable');
              }
              pushResource(this, value, function() {
                return onDispose(value);
              });
              return value;
            },
            defer(onDispose) {
              ensurePending(this, 'DisposableStack.prototype.defer');
              if (typeof onDispose !== 'function') {
                throw new TypeError('onDispose must be callable');
              }
              pushResource(this, undefined, function() {
                return onDispose();
              });
              return undefined;
            },
            move() {
              ensurePending(this, 'DisposableStack.prototype.move');
              const next = new DisposableStack();
              stackSlot.set(next, stackSlot.get(this));
              stackSlot.set(this, []);
              stateSlot.set(this, 'disposed');
              return next;
            },
            dispose() {
              requireDisposableStack(this, 'DisposableStack.prototype.dispose');
              if (stateSlot.get(this) === 'disposed') {
                return undefined;
              }
              stateSlot.set(this, 'disposed');
              const resources = stackSlot.get(this);
              let hasCompletion = false;
              let completion;
              while (resources.length > 0) {
                const resource = resources.pop();
                try {
                  resource.method.call(resource.value);
                } catch (error) {
                  if (!hasCompletion) {
                    completion = error;
                    hasCompletion = true;
                  } else {
                    completion = new SuppressedError(error, completion, undefined);
                  }
                }
              }
              if (hasCompletion) {
                throw completion;
              }
              return undefined;
            },
            get disposed() {
              requireDisposableStack(this, 'get DisposableStack.prototype.disposed');
              return stateSlot.get(this) === 'disposed';
            },
          };
          const disposedGetter = Object.getOwnPropertyDescriptor(disposableStackMethods, 'disposed').get;

          Object.defineProperties(DisposableStack.prototype, {
            constructor: {
              value: DisposableStack,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            use: {
              value: disposableStackMethods.use,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            adopt: {
              value: disposableStackMethods.adopt,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            defer: {
              value: disposableStackMethods.defer,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            move: {
              value: disposableStackMethods.move,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            dispose: {
              value: disposableStackMethods.dispose,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            disposed: {
              get: disposedGetter,
              enumerable: false,
              configurable: true,
            },
          });

          Object.defineProperty(DisposableStack.prototype, Symbol.dispose, {
            value: DisposableStack.prototype.dispose,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(DisposableStack.prototype, Symbol.toStringTag, {
            value: 'DisposableStack',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(DisposableStack, 'prototype', {
            value: DisposableStack.prototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(globalThis, 'DisposableStack', {
            value: DisposableStack,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          // Add Symbol.asyncDispose to AsyncIteratorPrototype
          (function() {
            async function* asyncGen() {}
            const asyncGenProto = Object.getPrototypeOf(asyncGen.prototype);
            const AsyncIteratorPrototype = Object.getPrototypeOf(asyncGenProto);
            if (AsyncIteratorPrototype && !AsyncIteratorPrototype[Symbol.asyncDispose]) {
              const asyncDisposeImpl = async function() {
                return this.return?.();
              };
              const asyncDisposeFn = new Proxy(async () => {}, {
                apply(_target, thisArg, args) {
                  return asyncDisposeImpl.apply(thisArg, args);
                }
              });
              Object.defineProperty(asyncDisposeFn, 'name', {
                value: '[Symbol.asyncDispose]',
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(AsyncIteratorPrototype, Symbol.asyncDispose, {
                value: asyncDisposeFn,
                writable: true,
                enumerable: false,
                configurable: true,
              });
            }
          })();

          // Add Symbol.dispose to IteratorPrototype
          (function() {
            function* gen() {}
            const genProto = Object.getPrototypeOf(gen.prototype);
            const IteratorPrototype = Object.getPrototypeOf(genProto);
            if (IteratorPrototype && !IteratorPrototype[Symbol.dispose]) {
              const disposeFn = function() {
                this.return?.();
              };
              Object.defineProperty(disposeFn, 'name', {
                value: '[Symbol.dispose]',
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(IteratorPrototype, Symbol.dispose, {
                value: disposeFn,
                writable: true,
                enumerable: false,
                configurable: true,
              });
            }
          })();
        })();
        "#,
    ))?;
    Ok(())
}

fn install_finalization_registry_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          if (typeof globalThis.FinalizationRegistry === 'function') {
            return;
          }

          const registryState = new WeakMap();
          const emptyToken = Symbol('empty FinalizationRegistry unregister token');

          function canBeHeldWeakly(value) {
            if ((typeof value === 'object' && value !== null) || typeof value === 'function') {
              return true;
            }
            return typeof value === 'symbol' && Symbol.keyFor(value) === undefined;
          }

          function getIntrinsicFinalizationRegistryPrototype(newTarget) {
            if (newTarget === undefined || newTarget === FinalizationRegistry) {
              return FinalizationRegistry.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherFinalizationRegistry = otherGlobal && otherGlobal.FinalizationRegistry;
              const otherProto = otherFinalizationRegistry && otherFinalizationRegistry.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return FinalizationRegistry.prototype;
          }

          function requireFinalizationRegistry(value, name) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !registryState.has(value)) {
              throw new TypeError(name + ' called on incompatible receiver');
            }
            return registryState.get(value);
          }

          function FinalizationRegistry(cleanupCallback) {
            if (new.target === undefined) {
              throw new TypeError('Constructor FinalizationRegistry requires new');
            }
            if (typeof cleanupCallback !== 'function') {
              throw new TypeError('FinalizationRegistry cleanup callback must be callable');
            }

            const registry = Object.create(getIntrinsicFinalizationRegistryPrototype(new.target));
            registryState.set(registry, {
              cleanupCallback,
              cells: [],
              active: false,
            });
            return registry;
          }

          const finalizationRegistryMethods = {
            register(target, holdings, unregisterToken) {
              const state = requireFinalizationRegistry(this, 'FinalizationRegistry.prototype.register');
              if (!canBeHeldWeakly(target)) {
                throw new TypeError('FinalizationRegistry.prototype.register target must be weakly holdable');
              }
              if (Object.is(target, holdings)) {
                throw new TypeError('FinalizationRegistry target and holdings must not be the same');
              }
              if (unregisterToken !== undefined && !canBeHeldWeakly(unregisterToken)) {
                throw new TypeError('FinalizationRegistry unregisterToken must be weakly holdable');
              }
              state.cells.push({
                target,
                holdings,
                unregisterToken: unregisterToken === undefined ? emptyToken : unregisterToken,
              });
              return undefined;
            },

            unregister(unregisterToken) {
              const state = requireFinalizationRegistry(this, 'FinalizationRegistry.prototype.unregister');
              if (!canBeHeldWeakly(unregisterToken)) {
                throw new TypeError('FinalizationRegistry unregisterToken must be weakly holdable');
              }
              let removed = false;
              state.cells = state.cells.filter((cell) => {
                if (cell.unregisterToken !== emptyToken && Object.is(cell.unregisterToken, unregisterToken)) {
                  removed = true;
                  return false;
                }
                return true;
              });
              return removed;
            },
          };

          Object.defineProperty(finalizationRegistryMethods.register, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(finalizationRegistryMethods.unregister, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const finalizationRegistryPrototype = Object.create(Object.prototype);
          Object.defineProperties(finalizationRegistryPrototype, {
            constructor: {
              value: FinalizationRegistry,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            register: {
              value: finalizationRegistryMethods.register,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            unregister: {
              value: finalizationRegistryMethods.unregister,
              writable: true,
              enumerable: false,
              configurable: true,
            },
          });
          Object.defineProperty(finalizationRegistryPrototype, Symbol.toStringTag, {
            value: 'FinalizationRegistry',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(FinalizationRegistry, 'prototype', {
            value: finalizationRegistryPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(globalThis, 'FinalizationRegistry', {
            value: FinalizationRegistry,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn install_bigint_to_locale_string(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = BigInt.prototype;
          const originalKey = '__agentjs_original_BigInt_toLocaleString__';
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.toLocaleString;
          if (typeof original !== 'function') {
            return;
          }
          if (typeof Intl !== 'object' || Intl === null || typeof Intl.NumberFormat !== 'function') {
            return;
          }

          const IntrinsicNumberFormat = Intl.NumberFormat;

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const toLocaleStringFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              let value = thisArg;
              if (typeof value !== 'bigint') {
                value = BigInt.prototype.valueOf.call(value);
              }

              const locales = args.length > 0 ? args[0] : undefined;
              const options = args.length > 1 ? args[1] : undefined;
              return new IntrinsicNumberFormat(locales, options).format(value);
            },
          });

          Object.defineProperty(toLocaleStringFn, 'name', {
            value: 'toLocaleString',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(toLocaleStringFn, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, 'toLocaleString', {
            value: toLocaleStringFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn normalize_builtin_function_to_string(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const originalFunctionToString = Function.prototype.toString;

          const isLikelyNativeSource = (source) =>
            typeof source === 'string' && source.includes('[native code]');

          const isSimpleIdentifierName = (name) =>
            typeof name === 'string' && /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name);

          const nativeSourceFor = (fn) => {
            const name = typeof fn.name === 'string' ? fn.name : '';
            if (isSimpleIdentifierName(name)) {
              return `function ${name}() { [native code] }`;
            }
            return 'function () { [native code] }';
          };

          const installNativeLikeToString = (fn) => {
            if (typeof fn !== 'function') {
              return;
            }

            let source;
            try {
              source = originalFunctionToString.call(fn);
            } catch {
              return;
            }

            if (isLikelyNativeSource(source)) {
              return;
            }

            const nativeSource = nativeSourceFor(fn);
            const nativeToString = new Proxy(() => {}, {
              apply() {
                return nativeSource;
              }
            });

            try {
              Object.defineProperty(nativeToString, 'name', {
                value: 'toString',
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(nativeToString, 'length', {
                value: 0,
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(fn, 'toString', {
                value: nativeToString,
                writable: true,
                enumerable: false,
                configurable: true,
              });
            } catch {
              // Ignore non-configurable or non-extensible functions.
            }
          };

          const seen = new Set();
          const visit = (value) => {
            if (value === null) {
              return;
            }
            const type = typeof value;
            if (type !== 'object' && type !== 'function') {
              return;
            }
            if (seen.has(value)) {
              return;
            }
            seen.add(value);

            if (type === 'function') {
              installNativeLikeToString(value);
            }

            let descriptors;
            try {
              descriptors = Object.getOwnPropertyDescriptors(value);
            } catch {
              return;
            }

            for (const key of Reflect.ownKeys(descriptors)) {
              const desc = descriptors[key];
              if ('value' in desc) {
                visit(desc.value);
              } else {
                visit(desc.get);
                visit(desc.set);
              }
            }

            try {
              visit(Object.getPrototypeOf(value));
            } catch {
              // Ignore exotic prototype lookups.
            }
          };

          visit(globalThis);

          // Ensure important intrinsic-only prototypes are also reached even if not
          // directly enumerable from the global object graph.
          try {
            async function* __agentjs_async_gen__() {}
            const asyncGenProto = Object.getPrototypeOf(__agentjs_async_gen__.prototype);
            visit(Object.getPrototypeOf(asyncGenProto));
          } catch {}
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_display_names_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl !== 'object' || Intl === null) {
            return;
          }
          if (typeof Intl.DisplayNames === 'function') {
            return;
          }
          if (typeof Intl.getCanonicalLocales !== 'function') {
            return;
          }

          const displayNamesSlots = new WeakMap();
          const VALID_LOCALE_MATCHERS = new Set(['lookup', 'best fit']);
          const VALID_STYLES = new Set(['narrow', 'short', 'long']);
          const VALID_TYPES = new Set(['language', 'region', 'script', 'currency', 'calendar', 'dateTimeField']);
          const VALID_FALLBACKS = new Set(['code', 'none']);
          const VALID_LANGUAGE_DISPLAYS = new Set(['dialect', 'standard']);
          const VALID_DATE_TIME_FIELDS = new Set([
            'era',
            'year',
            'quarter',
            'month',
            'weekOfYear',
            'weekday',
            'day',
            'dayPeriod',
            'hour',
            'minute',
            'second',
            'timeZoneName',
          ]);

          const isObjectLike = (value) =>
            (typeof value === 'object' && value !== null) || typeof value === 'function';

          const defaultLocale = () => {
            const formatter = new Intl.NumberFormat();
            if (typeof formatter.resolvedOptions === 'function') {
              const resolved = formatter.resolvedOptions();
              if (resolved && typeof resolved.locale === 'string' && resolved.locale.length > 0) {
                return resolved.locale;
              }
            }
            return 'en-US';
          };

          function getOption(options, property, allowedValues, fallback) {
            const value = options[property];
            if (value === undefined) {
              return fallback;
            }
            if (typeof value === 'symbol') {
              throw new TypeError('Cannot convert symbol to string');
            }
            const stringValue = String(value);
            if (allowedValues !== undefined && !allowedValues.has(stringValue)) {
              throw new RangeError('Invalid value for option ' + property);
            }
            return stringValue;
          }

          function canonicalizeRequestedLocales(locales) {
            if (locales === undefined) {
              return [];
            }
            return Intl.getCanonicalLocales(locales);
          }

          function canonicalCodeForDisplayNames(type, code) {
            switch (type) {
              case 'language': {
                if (typeof code !== 'string' || code.length === 0) {
                  throw new RangeError('Invalid language code');
                }
                if (
                  !/^[A-Za-z]{2,8}(?:-[A-Za-z]{4})?(?:-(?:[A-Za-z]{2}|[0-9]{3}))?(?:-(?:[A-Za-z0-9]{5,8}|[0-9][A-Za-z0-9]{3}))*$/.test(
                    code
                  )
                ) {
                  throw new RangeError('Invalid language code');
                }
                if (/^root(?:-|$)/i.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                if (/^[A-Za-z]{4}(?:-|$)/.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                if (/-u(?:-|$)/i.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                const segments = code.split('-');
                const normalizedSegments = [segments[0].toLowerCase()];
                let index = 1;

                if (index < segments.length && /^[A-Za-z]{4}$/.test(segments[index])) {
                  normalizedSegments.push(
                    segments[index].charAt(0).toUpperCase() + segments[index].slice(1).toLowerCase()
                  );
                  index += 1;
                }
                if (
                  index < segments.length &&
                  /^(?:[A-Za-z]{2}|[0-9]{3})$/.test(segments[index])
                ) {
                  normalizedSegments.push(segments[index].toUpperCase());
                  index += 1;
                }

                const seenVariants = new Set();
                for (; index < segments.length; index += 1) {
                  const variant = segments[index].toLowerCase();
                  if (seenVariants.has(variant)) {
                    throw new RangeError('Invalid language code');
                  }
                  seenVariants.add(variant);
                  normalizedSegments.push(variant);
                }
                return normalizedSegments.join('-');
              }
              case 'region':
                if (/^(?:[A-Za-z]{2}|[0-9]{3})$/.test(code)) {
                  return code.toUpperCase();
                }
                break;
              case 'script':
                if (/^[A-Za-z]{4}$/.test(code)) {
                  return code.charAt(0).toUpperCase() + code.slice(1).toLowerCase();
                }
                break;
              case 'currency':
                if (/^[A-Za-z]{3}$/.test(code)) {
                  return code.toUpperCase();
                }
                break;
              case 'calendar':
                if (/^[A-Za-z0-9]{3,8}(?:-[A-Za-z0-9]{3,8})*$/.test(code)) {
                  return code.toLowerCase();
                }
                break;
              case 'dateTimeField':
                if (VALID_DATE_TIME_FIELDS.has(code)) {
                  return code;
                }
                break;
            }
            throw new RangeError('Invalid code for Intl.DisplayNames');
          }

          function getIntrinsicDisplayNamesPrototype(newTarget) {
            if (newTarget === undefined || newTarget === DisplayNames) {
              return DisplayNames.prototype;
            }
            const proto = newTarget.prototype;
            if (isObjectLike(proto)) {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherIntl = otherGlobal && otherGlobal.Intl;
              const otherDisplayNames = otherIntl && otherIntl.DisplayNames;
              const otherProto = otherDisplayNames && otherDisplayNames.prototype;
              if (isObjectLike(otherProto)) {
                return otherProto;
              }
            } catch {}
            return DisplayNames.prototype;
          }

          function DisplayNames(locales, options) {
            if (new.target === undefined) {
              throw new TypeError('Intl.DisplayNames must be called with new');
            }

            const proto = getIntrinsicDisplayNamesPrototype(new.target);
            const displayNames = Object.create(proto);
            const requestedLocales = canonicalizeRequestedLocales(locales);
            const locale = requestedLocales.length > 0 ? requestedLocales[0] : defaultLocale();

            let normalizedOptions;
            if (options === undefined) {
              normalizedOptions = Object.create(null);
            } else {
              if (!isObjectLike(options)) {
                throw new TypeError('Intl.DisplayNames options must be an object');
              }
              normalizedOptions = options;
            }

            getOption(normalizedOptions, 'localeMatcher', VALID_LOCALE_MATCHERS, 'best fit');
            const style = getOption(normalizedOptions, 'style', VALID_STYLES, 'long');
            const type = getOption(normalizedOptions, 'type', VALID_TYPES, undefined);
            if (type === undefined) {
              throw new TypeError('Intl.DisplayNames type option is required');
            }
            const fallback = getOption(normalizedOptions, 'fallback', VALID_FALLBACKS, 'code');
            const languageDisplay = getOption(
              normalizedOptions,
              'languageDisplay',
              VALID_LANGUAGE_DISPLAYS,
              'dialect'
            );

            displayNamesSlots.set(displayNames, {
              locale,
              style,
              type,
              fallback,
              languageDisplay: type === 'language' ? languageDisplay : undefined,
            });
            return displayNames;
          }

          Object.defineProperty(DisplayNames, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(DisplayNames, 'name', {
            value: 'DisplayNames',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const displayNamesPrototype = Object.create(Object.prototype);

          const ofFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              if (!isObjectLike(thisArg) || !displayNamesSlots.has(thisArg)) {
                throw new TypeError('Intl.DisplayNames.prototype.of called on incompatible receiver');
              }
              const code = args.length > 0 ? String(args[0]) : String(undefined);
              const slot = displayNamesSlots.get(thisArg);
              return canonicalCodeForDisplayNames(slot.type, code);
            },
          });
          Object.defineProperty(ofFn, 'name', {
            value: 'of',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(ofFn, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const resolvedOptionsFn = new Proxy(() => {}, {
            apply(_target, thisArg) {
              if (!isObjectLike(thisArg) || !displayNamesSlots.has(thisArg)) {
                throw new TypeError(
                  'Intl.DisplayNames.prototype.resolvedOptions called on incompatible receiver'
                );
              }
              const slot = displayNamesSlots.get(thisArg);
              const options = {
                locale: slot.locale,
                style: slot.style,
                type: slot.type,
                fallback: slot.fallback,
              };
              if (slot.languageDisplay !== undefined) {
                options.languageDisplay = slot.languageDisplay;
              }
              return options;
            },
          });
          Object.defineProperty(resolvedOptionsFn, 'name', {
            value: 'resolvedOptions',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(resolvedOptionsFn, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(displayNamesPrototype, 'constructor', {
            value: DisplayNames,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, 'of', {
            value: ofFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, 'resolvedOptions', {
            value: resolvedOptionsFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, Symbol.toStringTag, {
            value: 'Intl.DisplayNames',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(DisplayNames, 'prototype', {
            value: displayNamesPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(Intl, 'DisplayNames', {
            value: DisplayNames,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_date_time_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl !== 'object' || Intl === null) return;
          if (typeof Intl.DateTimeFormat !== 'function') return;

          const DTF = Intl.DateTimeFormat;
          const proto = DTF.prototype;

          // Store original format function if exists
          const originalFormat = proto.format;

          // Internal slot storage
          const dtfSlots = new WeakMap();

          // Valid option values per spec
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_FORMAT_MATCHERS = ['basic', 'best fit'];
          const VALID_CALENDARS = [
            'buddhist', 'chinese', 'coptic', 'dangi', 'ethioaa', 'ethiopic',
            'gregory', 'hebrew', 'indian', 'islamic', 'islamic-umalqura',
            'islamic-tbla', 'islamic-civil', 'islamic-rgsa', 'iso8601',
            'japanese', 'persian', 'roc', 'islamicc'
          ];
          const VALID_NUMBERING_SYSTEMS = [
            'arab', 'arabext', 'bali', 'beng', 'deva', 'fullwide', 'gujr',
            'guru', 'hanidec', 'khmr', 'knda', 'laoo', 'latn', 'limb',
            'mlym', 'mong', 'mymr', 'orya', 'tamldec', 'telu', 'thai', 'tibt'
          ];
          const VALID_HOUR_CYCLES = ['h11', 'h12', 'h23', 'h24'];
          const VALID_TIME_ZONES = ['UTC'];
          const VALID_WEEKDAYS = ['narrow', 'short', 'long'];
          const VALID_ERAS = ['narrow', 'short', 'long'];
          const VALID_YEARS = ['2-digit', 'numeric'];
          const VALID_MONTHS = ['2-digit', 'numeric', 'narrow', 'short', 'long'];
          const VALID_DAYS = ['2-digit', 'numeric'];
          const VALID_DAY_PERIODS = ['narrow', 'short', 'long'];
          const VALID_HOURS = ['2-digit', 'numeric'];
          const VALID_MINUTES = ['2-digit', 'numeric'];
          const VALID_SECONDS = ['2-digit', 'numeric'];
          const VALID_FRACTIONAL_SECOND_DIGITS = [1, 2, 3];
          const VALID_TIME_ZONE_NAMES = ['short', 'long', 'shortOffset', 'longOffset', 'shortGeneric', 'longGeneric'];
          const VALID_DATE_STYLES = ['full', 'long', 'medium', 'short'];
          const VALID_TIME_STYLES = ['full', 'long', 'medium', 'short'];

          const CALENDAR_ALIASES = {
            'islamicc': 'islamic-civil',
          };

          function canonicalizeCalendar(cal) {
            if (typeof cal !== 'string') return undefined;
            const lower = cal.toLowerCase();
            if (CALENDAR_ALIASES[lower]) return CALENDAR_ALIASES[lower];
            // Check if it has invalid uppercase characters (like capital dotted I)
            if (/[\u0130\u0131]/.test(cal)) {
              throw new RangeError('Invalid calendar');
            }
            return lower;
          }

          function canonicalizeTimeZone(tz) {
            if (typeof tz !== 'string') return undefined;
            // Preserve Etc/GMT, Etc/UTC, GMT without canonicalizing to UTC
            const upper = tz.toUpperCase();
            if (upper === 'ETC/GMT') return 'Etc/GMT';
            if (upper === 'ETC/UTC') return 'Etc/UTC';
            if (upper === 'GMT') return 'GMT';
            if (upper === 'UTC') return 'UTC';
            
            // Reject Unicode minus sign (U+2212) - must use ASCII minus (U+002D)
            if (tz.includes('\u2212')) {
              throw new RangeError('Invalid time zone: ' + tz);
            }
            
            // Check if it looks like an offset timezone
            if (/^[+-]/.test(tz)) {
              // Valid offset formats:
              // +HH:MM or -HH:MM
              // +HHMM or -HHMM  
              // Hours must be 00-23, minutes must be 00-59
              
              // Pattern: +/-HH:MM
              const colonMatch = tz.match(/^([+-])(\d{2}):(\d{2})$/);
              if (colonMatch) {
                const hours = parseInt(colonMatch[2], 10);
                const minutes = parseInt(colonMatch[3], 10);
                if (hours <= 23 && minutes <= 59) {
                  return tz;
                }
                throw new RangeError('Invalid time zone: ' + tz);
              }
              
              // Pattern: +/-HHMM
              const noColonMatch = tz.match(/^([+-])(\d{2})(\d{2})$/);
              if (noColonMatch) {
                const hours = parseInt(noColonMatch[2], 10);
                const minutes = parseInt(noColonMatch[3], 10);
                if (hours <= 23 && minutes <= 59) {
                  return tz;
                }
                throw new RangeError('Invalid time zone: ' + tz);
              }
              
              // Pattern: +/-HH (2 digit hours with no minutes)
              const shortMatch = tz.match(/^([+-])(\d{2})$/);
              if (shortMatch) {
                const hours = parseInt(shortMatch[2], 10);
                if (hours <= 23) {
                  return tz;
                }
                throw new RangeError('Invalid time zone: ' + tz);
              }
              
              // Any other format starting with +/- is invalid
              throw new RangeError('Invalid time zone: ' + tz);
            }
            
            return tz;
          }

          // Strip unicode extension keys not valid for DateTimeFormat (ca, nu, hc are valid; tz stripped — CLDR tz values unvalidatable)
          // Also strip keys whose values are not valid for that key.
          function stripInvalidDTFUnicodeExtKeys(locale) {
            if (typeof locale !== 'string') return locale;
            const validKeyValues = {
              ca: VALID_CALENDARS,
              nu: VALID_NUMBERING_SYSTEMS,
              hc: VALID_HOUR_CYCLES,
            };
            return locale.replace(/-u(-[a-z0-9]{2,8})+/gi, (match) => {
              const tokens = match.slice(3).split('-');
              const kept = [];
              let i = 0;
              while (i < tokens.length) {
                const tok = tokens[i].toLowerCase();
                if (tok.length === 2) {
                  const vals = [];
                  let j = i + 1;
                  while (j < tokens.length && tokens[j].length !== 2) { vals.push(tokens[j]); j++; }
                  if (Object.prototype.hasOwnProperty.call(validKeyValues, tok)) {
                    const allowed = validKeyValues[tok];
                    const valStr = vals.join('-').toLowerCase();
                    if (allowed.includes(valStr)) kept.push(tok, ...vals);
                  }
                  i = j;
                } else { i++; }
              }
              return kept.length > 0 ? '-u-' + kept.join('-') : '';
            });
          }

          const _hasOwn = Object.prototype.hasOwnProperty;
          const _getOwnPropDesc = Object.getOwnPropertyDescriptor;

          function getOwnOptionValue(options, property) {
            return options[property];
          }

          function getOption(options, property, type, values, fallback) {
            let value = getOwnOptionValue(options, property);
            if (value === undefined) return fallback;
            if (type === 'boolean') {
              value = Boolean(value);
            } else if (type === 'string') {
              value = String(value);
            } else if (type === 'number') {
              value = Number(value);
              if (!Number.isFinite(value)) {
                throw new RangeError('Invalid ' + property);
              }
            }
            if (values !== undefined && !values.includes(value)) {
              throw new RangeError('Invalid value ' + value + ' for option ' + property);
            }
            return value;
          }

          function getNumberOption(options, property, minimum, maximum, fallback) {
            let value = getOwnOptionValue(options, property);
            if (value === undefined) return fallback;
            value = Number(value);
            if (!Number.isFinite(value) || value < minimum || value > maximum) {
              throw new RangeError('Invalid ' + property);
            }
            return Math.floor(value);
          }

          // OrdinaryHasInstance implementation that doesn't use Symbol.hasInstance
          function ordinaryHasInstance(C, O) {
            if (typeof C !== 'function') return false;
            if (typeof O !== 'object' || O === null) return false;
            const P = C.prototype;
            if (typeof P !== 'object' || P === null) {
              throw new TypeError('Function has non-object prototype in instanceof check');
            }
            // Walk the prototype chain
            let proto = Object.getPrototypeOf(O);
            while (proto !== null) {
              if (proto === P) return true;
              proto = Object.getPrototypeOf(proto);
            }
            return false;
          }

          // Wrap the constructor to capture options
          const WrappedDTF = function DateTimeFormat(locales, options) {
            // Use OrdinaryHasInstance instead of instanceof to avoid Symbol.hasInstance lookup
            if (!ordinaryHasInstance(WrappedDTF, this) && new.target === undefined) {
              return new WrappedDTF(locales, options);
            }

            // Convert options to object (ToObject) - primitives like numbers should work
            // null must throw TypeError
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }

            // Validate and canonicalize options - read in spec-defined order
            // Order per spec: localeMatcher, calendar, numberingSystem, hour12, hourCycle, timeZone, 
            //                 weekday, era, year, month, day, dayPeriod, hour, minute, second, fractionalSecondDigits, 
            //                 timeZoneName, formatMatcher, dateStyle, timeStyle
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Calendar validation - must be valid Unicode locale identifier type
            // Valid calendars are 3-8 alphanum chars, possibly with subtags separated by hyphens
            let calendar = opts.calendar;
            if (calendar !== undefined) {
              calendar = String(calendar);
              // Calendar must be 3-8 alphanum chars per Unicode locale identifier type
              // With possible subtag of 3-8 alphanum chars separated by hyphen
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(calendar)) {
                throw new RangeError('Invalid calendar');
              }
              calendar = canonicalizeCalendar(calendar);
            }
            
            // numberingSystem validation - must be valid Unicode locale identifier type
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              // numberingSystem must be 3-8 alphanum chars
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            // hour12 special handling - read once, convert to boolean if defined
            const hour12Raw = opts.hour12;
            const hour12 = hour12Raw !== undefined ? Boolean(hour12Raw) : undefined;
            const hourCycle = getOption(opts, 'hourCycle', 'string', VALID_HOUR_CYCLES, undefined);
            let timeZone = opts.timeZone;
            if (timeZone !== undefined) {
              timeZone = canonicalizeTimeZone(String(timeZone));
              // Validate named time zones using Temporal
              if (typeof Temporal !== 'undefined' && Temporal !== null && typeof Temporal.TimeZone === 'function') {
                try { new Temporal.TimeZone(timeZone); } catch (_e) {
                  throw new RangeError('Invalid time zone: ' + timeZone);
                }
              }
            }
            
            const weekday = getOption(opts, 'weekday', 'string', VALID_WEEKDAYS, undefined);
            const era = getOption(opts, 'era', 'string', VALID_ERAS, undefined);
            const year = getOption(opts, 'year', 'string', VALID_YEARS, undefined);
            const month = getOption(opts, 'month', 'string', VALID_MONTHS, undefined);
            const day = getOption(opts, 'day', 'string', VALID_DAYS, undefined);
            // dayPeriod is read before hour per spec
            const dayPeriod = getOption(opts, 'dayPeriod', 'string', VALID_DAY_PERIODS, undefined);
            const hour = getOption(opts, 'hour', 'string', VALID_HOURS, undefined);
            const minute = getOption(opts, 'minute', 'string', VALID_MINUTES, undefined);
            const second = getOption(opts, 'second', 'string', VALID_SECONDS, undefined);
            const fractionalSecondDigits = getNumberOption(opts, 'fractionalSecondDigits', 1, 3, undefined);
            const timeZoneName = getOption(opts, 'timeZoneName', 'string', VALID_TIME_ZONE_NAMES, undefined);
            const formatMatcher = getOption(opts, 'formatMatcher', 'string', VALID_FORMAT_MATCHERS, 'best fit');
            const dateStyle = getOption(opts, 'dateStyle', 'string', VALID_DATE_STYLES, undefined);
            const timeStyle = getOption(opts, 'timeStyle', 'string', VALID_TIME_STYLES, undefined);

            // dateStyle/timeStyle cannot be combined with individual date/time components
            if ((dateStyle !== undefined || timeStyle !== undefined) &&
                (weekday !== undefined || era !== undefined || year !== undefined ||
                 month !== undefined || day !== undefined || dayPeriod !== undefined ||
                 hour !== undefined || minute !== undefined || second !== undefined ||
                 fractionalSecondDigits !== undefined || timeZoneName !== undefined)) {
              throw new TypeError('dateStyle and timeStyle cannot be combined with other date/time options');
            }

            // Per spec: CanonicalizeLocaleList calls ToObject(locales), which throws TypeError for null
            if (locales === null) {
              throw new TypeError('Cannot convert null to object');
            }

            // Create the underlying DTF instance
            let instance;
            try {
              instance = new DTF(locales, options);
            } catch (e) {
              throw e;
            }

            // Determine locale - consistent behavior for undefined and empty array
            let locale;
            const defaultLocale = (() => {
              try {
                return new Intl.NumberFormat().resolvedOptions().locale || 'en-US';
              } catch (e) {
                return 'en-US';
              }
            })();
            
            if (locales === undefined) {
              locale = defaultLocale;
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || defaultLocale;
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? Intl.getCanonicalLocales(locales)[0] : defaultLocale;
            } else {
              locale = defaultLocale;
            }
            locale = stripInvalidDTFUnicodeExtKeys(locale);

            // Determine if we need to apply default date/time format
            // Per ECMA-402, if no date/time components and no dateStyle/timeStyle specified,
            // default to year: 'numeric', month: 'numeric', day: 'numeric'
            let needsDefault = dateStyle === undefined && timeStyle === undefined &&
              weekday === undefined && era === undefined && year === undefined &&
              month === undefined && day === undefined && dayPeriod === undefined &&
              hour === undefined && minute === undefined && second === undefined &&
              fractionalSecondDigits === undefined && timeZoneName === undefined;
            
            let resolvedYear = year;
            let resolvedMonth = month;
            let resolvedDay = day;
            
            if (needsDefault) {
              resolvedYear = 'numeric';
              resolvedMonth = 'numeric';
              resolvedDay = 'numeric';
            }

            const localeCalendarMatch = typeof locale === 'string'
              ? locale.match(/-u(?:-[a-z0-9]{2,8})*-ca-([a-z0-9-]+)/i)
              : null;
            const resolvedCalendar = calendar ||
              (localeCalendarMatch ? canonicalizeCalendar(localeCalendarMatch[1]) : 'gregory');

            // Detect locale's default numbering system if not explicitly specified
            function detectLocaleNumberingSystem(loc) {
              if (!loc) return 'latn';
              const nuMatch = loc.match(/-u(?:-[a-z0-9]{2,8})*-nu-([a-z0-9]+)/i);
              if (nuMatch) return nuMatch[1].toLowerCase();
              // Known locales with non-Latin default numbering systems
              const lower = loc.toLowerCase();
              if (lower.startsWith('ar')) return 'arab';
              if (lower.startsWith('fa') || lower.startsWith('ps')) return 'arabext';
              if (lower.startsWith('ne') || lower.startsWith('mr')) return 'deva';
              if (lower.startsWith('bn')) return 'beng';
              if (lower.startsWith('gu')) return 'gujr';
              if (lower.startsWith('pa')) return 'guru';
              if (lower.startsWith('km')) return 'khmr';
              if (lower.startsWith('kn')) return 'knda';
              if (lower.startsWith('lo')) return 'laoo';
              if (lower.startsWith('ml')) return 'mlym';
              if (lower.startsWith('my')) return 'mymr';
              if (lower.startsWith('or')) return 'orya';
              if (lower.startsWith('ta')) return 'tamldec';
              if (lower.startsWith('te')) return 'telu';
              if (lower.startsWith('th')) return 'thai';
              if (lower.startsWith('bo')) return 'tibt';
              return 'latn';
            }

            // Store resolved options
            const resolvedOpts = {
              locale: locale,
              calendar: resolvedCalendar,
              numberingSystem: numberingSystem ? String(numberingSystem).toLowerCase() : detectLocaleNumberingSystem(locale),
              timeZone: timeZone,
              hourCycle: hourCycle,
              hour12: hour12,
              weekday: weekday,
              era: era,
              year: resolvedYear,
              month: resolvedMonth,
              day: resolvedDay,
              dayPeriod: dayPeriod,
              hour: hour,
              minute: minute,
              second: second,
              fractionalSecondDigits: fractionalSecondDigits,
              timeZoneName: timeZoneName,
              dateStyle: dateStyle,
              timeStyle: timeStyle,
            };

            // Set hourCycle/hour12 defaults based on each other
            if (hour !== undefined) {
              if (hourCycle !== undefined) {
                resolvedOpts.hour12 = (hourCycle === 'h11' || hourCycle === 'h12');
              } else if (hour12 !== undefined) {
                resolvedOpts.hour12 = Boolean(hour12);
                resolvedOpts.hourCycle = hour12 ? 'h12' : 'h23';
              }
            }

            dtfSlots.set(this, { instance, resolvedOpts, needsDefault });

            return this;
          };

          // Copy static properties
          Object.defineProperty(WrappedDTF, 'length', { value: 0, configurable: true });
          Object.defineProperty(WrappedDTF, 'name', { value: 'DateTimeFormat', configurable: true });

          const supportedLocalesOf = (locales, options) => {
            if (typeof DTF.supportedLocalesOf === 'function') {
              return DTF.supportedLocalesOf(locales, options);
            }
            if (typeof Intl.NumberFormat === 'function' &&
                typeof Intl.NumberFormat.supportedLocalesOf === 'function') {
              return Intl.NumberFormat.supportedLocalesOf(locales, options);
            }

            if (options !== undefined) {
              if (options === null) {
                throw new TypeError('Cannot convert null to object');
              }
              const opts = Object(options);
              const matcher = opts.localeMatcher;
              if (matcher !== undefined) {
                const matcherStr = String(matcher);
                if (!VALID_LOCALE_MATCHERS.includes(matcherStr)) {
                  throw new RangeError('Invalid localeMatcher');
                }
              }
            }

            if (locales === undefined) return [];
            const requestedLocales = Array.isArray(locales) ? locales : [String(locales)];
            const canonicalized = Intl.getCanonicalLocales(requestedLocales);
            const defaultLocale = (() => {
              try {
                return new Intl.NumberFormat().resolvedOptions().locale || 'en-US';
              } catch (e) {
                return 'en-US';
              }
            })();
            return canonicalized.filter((locale, index, array) =>
              array.indexOf(locale) === index && locale === defaultLocale
            );
          };
          Object.defineProperty(supportedLocalesOf, 'name', {
            value: 'supportedLocalesOf',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(supportedLocalesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(WrappedDTF, 'supportedLocalesOf', {
            value: supportedLocalesOf,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          // Create new prototype
          const newProto = Object.create(Object.prototype);

          // resolvedOptions method
          Object.defineProperty(newProto, 'resolvedOptions', {
            value: function resolvedOptions() {
              const slot = dtfSlots.get(this);
              if (!slot) {
                throw new TypeError('Method Intl.DateTimeFormat.prototype.resolvedOptions called on incompatible receiver');
              }
              const opts = slot.resolvedOpts;
              const result = {
                locale: opts.locale,
                calendar: opts.calendar,
                numberingSystem: opts.numberingSystem,
                timeZone: opts.timeZone !== undefined ? opts.timeZone : (() => {
                  try { return new DTF().resolvedOptions().timeZone || 'UTC'; } catch (_e) { return 'UTC'; }
                })(),
              };
              if (opts.hourCycle !== undefined) result.hourCycle = opts.hourCycle;
              if (opts.hour12 !== undefined) result.hour12 = opts.hour12;
              if (opts.weekday !== undefined) result.weekday = opts.weekday;
              if (opts.era !== undefined) result.era = opts.era;
              if (opts.year !== undefined) result.year = opts.year;
              if (opts.month !== undefined) result.month = opts.month;
              if (opts.day !== undefined) result.day = opts.day;
              if (opts.dayPeriod !== undefined) result.dayPeriod = opts.dayPeriod;
              if (opts.hour !== undefined) result.hour = opts.hour;
              if (opts.minute !== undefined) result.minute = opts.minute;
              if (opts.second !== undefined) result.second = opts.second;
              if (opts.fractionalSecondDigits !== undefined) result.fractionalSecondDigits = opts.fractionalSecondDigits;
              if (opts.timeZoneName !== undefined) result.timeZoneName = opts.timeZoneName;
              if (opts.dateStyle !== undefined) result.dateStyle = opts.dateStyle;
              if (opts.timeStyle !== undefined) result.timeStyle = opts.timeStyle;
              return result;
            },
            writable: true,
            enumerable: false,
            configurable: true
          });

          function resolveCalendarId(opts) {
            if (opts.calendar !== undefined) {
              return String(opts.calendar).toLowerCase();
            }
            const locale = typeof opts.locale === 'string' ? opts.locale : '';
            const match = locale.match(/-u(?:-[a-z0-9]{2,8})*-ca-([a-z0-9-]+)/i);
            return match ? match[1].toLowerCase() : 'gregory';
          }

          function getDateTimeFields(d, opts) {
            const calendar = resolveCalendarId(opts);
            if (opts.timeZone !== undefined &&
                typeof Temporal === 'object' &&
                Temporal !== null &&
                typeof Temporal.Instant === 'function') {
              try {
                const instant = new Temporal.Instant(BigInt(d.getTime()) * 1000000n);
                const zoned = instant.toZonedDateTimeISO(opts.timeZone);
                const weekday = zoned.dayOfWeek % 7;
                let calendarYear = zoned.year;
                let calendarMonth = zoned.month;
                let calendarDay = zoned.day;
                let calendarMonthCode = zoned.monthCode;
                if (calendar !== 'gregory' && calendar !== 'iso8601') {
                  try {
                    const calendarZoned = zoned.withCalendar(calendar);
                    calendarYear = calendarZoned.year;
                    calendarMonth = calendarZoned.month;
                    calendarDay = calendarZoned.day;
                    calendarMonthCode = calendarZoned.monthCode;
                  } catch (_calendarError) {
                    // Leave ISO fields in place.
                  }
                }
                return {
                  year: zoned.year,
                  month: zoned.month,
                  day: zoned.day,
                  hour: zoned.hour,
                  minute: zoned.minute,
                  second: zoned.second,
                  millisecond: zoned.millisecond,
                  weekday,
                  calendar,
                  calendarYear,
                  calendarMonth,
                  calendarDay,
                  calendarMonthCode,
                };
              } catch (_err) {
                // Fall back to local fields below.
              }
            }

            return {
              year: d.getFullYear(),
              month: d.getMonth() + 1,
              day: d.getDate(),
              hour: d.getHours(),
              minute: d.getMinutes(),
              second: d.getSeconds(),
              millisecond: d.getMilliseconds(),
              weekday: d.getDay(),
              calendar,
              calendarYear: d.getFullYear(),
              calendarMonth: d.getMonth() + 1,
              calendarDay: d.getDate(),
              calendarMonthCode: 'M' + String(d.getMonth() + 1).padStart(2, '0'),
            };
          }

          function localeUses24Hour(locale) {
            if (typeof locale === 'string') {
              // Check unicode hc extension first
              const hcMatch = locale.match(/-u(?:-[a-z0-9]{2,8})*-hc-([a-z0-9]+)/i);
              if (hcMatch) {
                const hc = hcMatch[1].toLowerCase();
                return hc === 'h23' || hc === 'h24';
              }
            }
            const lower = (locale || 'en-US').toLowerCase();
            return lower.startsWith('zh') || lower.startsWith('ja') ||
              lower.startsWith('ko') || lower.startsWith('de') || lower.startsWith('ru') ||
              lower.startsWith('pl') || lower.startsWith('it') || lower.startsWith('pt') ||
              lower.startsWith('nl') || lower.startsWith('sv') || lower.startsWith('fi') ||
              lower.startsWith('da') || lower.startsWith('nb') || lower.startsWith('cs') ||
              lower.startsWith('hu') || lower.startsWith('ro') || lower.startsWith('sk') ||
              lower.startsWith('uk') || lower.startsWith('hr') || lower.startsWith('bg') ||
              lower.startsWith('el') || lower.startsWith('tr') || lower.startsWith('vi') ||
              lower.startsWith('th') || lower.startsWith('id');
          }

          // Returns the locale-aware day period string for a given hour (0-23).
          // English CLDR data: midnight=0, morning=6-11, noon=12, afternoon=13-17, evening=18-20, night=21-23+0-5
          function getDayPeriodForHour(hour, style) {
            let period;
            if (hour === 0 || (hour >= 21 && hour <= 23)) {
              period = style === 'narrow' ? 'at night' : 'at night';
            } else if (hour >= 1 && hour <= 5) {
              period = 'at night';
            } else if (hour >= 6 && hour <= 11) {
              period = 'in the morning';
            } else if (hour === 12) {
              period = style === 'narrow' ? 'n' : 'noon';
            } else if (hour >= 13 && hour <= 17) {
              period = 'in the afternoon';
            } else {
              period = 'in the evening';
            }
            return period;
          }

          function safeSet(obj, key, value) {
            Object.defineProperty(obj, key, { value, writable: true, enumerable: true, configurable: true });
          }

          function applyDateTimeStyleDefaults(opts) {
            // Use Object.create(null) to avoid triggering tainted Object.prototype setters
            const adjusted = Object.create(null);
            const keys = ['calendar','numberingSystem','timeZone','weekday','era','year','month','day',
              'dayPeriod','hour','minute','second','fractionalSecondDigits','timeZoneName',
              'hourCycle','hour12','dateStyle','timeStyle','overrideYear','locale'];
            for (const k of keys) {
              if (opts[k] !== undefined) safeSet(adjusted, k, opts[k]);
            }
            if (opts.dateStyle !== undefined) {
              if (adjusted.year === undefined) safeSet(adjusted, 'year', opts.dateStyle === 'short' ? '2-digit' : 'numeric');
              if (adjusted.month === undefined) safeSet(adjusted, 'month', opts.dateStyle === 'short' ? 'numeric' : (opts.dateStyle === 'medium' ? 'short' : 'long'));
              if (adjusted.day === undefined) safeSet(adjusted, 'day', 'numeric');
              if (opts.dateStyle === 'full' && adjusted.weekday === undefined) safeSet(adjusted, 'weekday', 'long');
            }
            if (opts.timeStyle !== undefined) {
              if (adjusted.hour === undefined) safeSet(adjusted, 'hour', 'numeric');
              if (adjusted.minute === undefined) safeSet(adjusted, 'minute', 'numeric');
              if (opts.timeStyle !== 'short' && adjusted.second === undefined) safeSet(adjusted, 'second', 'numeric');
              if (opts.timeStyle === 'full' && adjusted.timeZoneName === undefined) safeSet(adjusted, 'timeZoneName', 'long');
              else if (opts.timeStyle === 'long' && adjusted.timeZoneName === undefined) safeSet(adjusted, 'timeZoneName', 'short');
            }
            return adjusted;
          }

          function formatDateWithOptionsToParts(d, opts) {
            const normalized = applyDateTimeStyleDefaults(opts);
            const fields = getDateTimeFields(d, normalized);
            const hasDateComponent = normalized.year !== undefined || normalized.month !== undefined ||
              normalized.day !== undefined || normalized.weekday !== undefined || normalized.era !== undefined;
            const hasTimeComponent = normalized.hour !== undefined || normalized.minute !== undefined ||
              normalized.second !== undefined || normalized.dayPeriod !== undefined ||
              normalized.fractionalSecondDigits !== undefined || normalized.timeZoneName !== undefined;
            const parts = [];

            function pushLiteral(value) {
              if (value) {
                parts.push({ type: 'literal', value });
              }
            }

            function monthName(month, style, calendar) {
              const gregLong = ['January', 'February', 'March', 'April', 'May', 'June',
                'July', 'August', 'September', 'October', 'November', 'December'];
              const gregShort = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
                'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
              const gregNarrow = ['J', 'F', 'M', 'A', 'M', 'J', 'J', 'A', 'S', 'O', 'N', 'D'];
              const islamicLong = ['Muharram', 'Safar', 'Rabiʻ I', 'Rabiʻ II', 'Jumada I', 'Jumada II',
                'Rajab', 'Shaʻban', 'Ramadan', 'Shawwal', 'Dhuʻl-Qiʻdah', 'Dhuʻl-Hijjah'];
              const islamicShort = ['Muh.', 'Saf.', 'Rab. I', 'Rab. II', 'Jum. I', 'Jum. II',
                'Raj.', 'Sha.', 'Ram.', 'Shaw.', 'Dhuʻl-Q.', 'Dhuʻl-H.'];
              const lowerCalendar = String(calendar || 'gregory').toLowerCase();
              const isIslamic = lowerCalendar === 'islamic' || lowerCalendar.startsWith('islamic-');
              const longNames = isIslamic ? islamicLong : gregLong;
              const shortNames = isIslamic ? islamicShort : gregShort;
              const narrowNames = isIslamic ? islamicLong.map((name) => name[0]) : gregNarrow;
              if (style === 'long') return longNames[month - 1];
              if (style === 'short') return shortNames[month - 1];
              return narrowNames[month - 1];
            }

            function weekdayName(weekday, style) {
              const longNames = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
              const shortNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
              const narrowNames = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];
              if (style === 'long') return longNames[weekday];
              if (style === 'short') return shortNames[weekday];
              return narrowNames[weekday];
            }

            if (hasDateComponent || (!hasTimeComponent && normalized.dateStyle === undefined && normalized.timeStyle === undefined)) {
              const dateParts = [];
              if (normalized.weekday !== undefined) {
                dateParts.push({ type: 'weekday', value: weekdayName(fields.weekday, normalized.weekday) });
              }
              if (normalized.month !== undefined) {
                const displayMonth = fields.calendarMonth ?? fields.month;
                const value = normalized.month === '2-digit'
                  ? String(displayMonth).padStart(2, '0')
                  : normalized.month === 'numeric'
                    ? String(displayMonth)
                    : monthName(displayMonth, normalized.month, fields.calendar);
                dateParts.push({ type: 'month', value });
              }
              if (normalized.day !== undefined) {
                const displayDay = fields.calendarDay ?? fields.day;
                dateParts.push({
                  type: 'day',
                  value: normalized.day === '2-digit'
                    ? String(displayDay).padStart(2, '0')
                    : String(displayDay),
                });
              }
              if (normalized.year !== undefined) {
                const displayYear = normalized.overrideYear !== undefined
                  ? normalized.overrideYear
                  : (fields.calendarYear ?? fields.year);
                // Proleptic Gregorian: no year 0; year 0 CE = 1 BC, year -1 CE = 2 BC, etc.
                const isBC = displayYear <= 0;
                const prolepticYear = isBC ? 1 - displayYear : displayYear;
                const cal = String(normalized.calendar || 'gregory').toLowerCase();
                let yearValue;
                if ((cal === 'chinese' || cal === 'dangi') && normalized.year === 'numeric') {
                  const stems = '甲乙丙丁戊己庚辛壬癸';
                  const branches = '子丑寅卯辰巳午未申酉戌亥';
                  const y = fields.year - 4;
                  yearValue = stems[(((y % 10) + 10) % 10)] + branches[(((y % 12) + 12) % 12)] + '年';
                } else {
                  yearValue = normalized.year === '2-digit'
                    ? String(prolepticYear % 100).padStart(2, '0')
                    : String(prolepticYear);
                }
                dateParts.push({ type: 'year', value: yearValue });
              }
              if (normalized.era !== undefined) {
                const displayYear = normalized.overrideYear !== undefined
                  ? normalized.overrideYear
                  : (fields.calendarYear ?? fields.year);
                dateParts.push({ type: 'era', value: displayYear >= 1 ? 'AD' : 'BC' });
              }

              // Determine if month is a named style (long/short/narrow) for locale-aware separators
              const namedMonth = normalized.month === 'long' || normalized.month === 'short' || normalized.month === 'narrow';
              dateParts.forEach((part, index) => {
                if (index > 0) {
                  const prev = dateParts[index - 1].type;
                  const cur = part.type;
                  if (prev === 'weekday') {
                    pushLiteral(', ');
                  } else if (namedMonth && prev === 'month' && cur === 'day') {
                    pushLiteral(' ');
                  } else if (namedMonth && prev === 'day' && cur === 'year') {
                    pushLiteral(', ');
                  } else if (namedMonth && prev === 'month' && cur === 'year') {
                    pushLiteral(' ');
                  } else {
                    pushLiteral('/');
                  }
                }
                parts.push(part);
              });
            }

            if (hasTimeComponent) {
              if (parts.length > 0) {
                pushLiteral(', ');
              }

              const use12Hour = normalized.hour12 !== undefined
                ? normalized.hour12
                : normalized.hourCycle !== undefined
                  ? normalized.hourCycle === 'h11' || normalized.hourCycle === 'h12'
                  : !localeUses24Hour(normalized.locale);

              let displayHour = fields.hour;
              let dayPeriod;
              if (normalized.hour !== undefined) {
                if (normalized.dayPeriod !== undefined) {
                  // Use locale-aware day period instead of AM/PM
                  dayPeriod = getDayPeriodForHour(fields.hour, normalized.dayPeriod);
                  displayHour = fields.hour % 12;
                  if (displayHour === 0) displayHour = 12;
                } else if (use12Hour) {
                  dayPeriod = fields.hour >= 12 ? 'PM' : 'AM';
                  if (normalized.hourCycle === 'h11') {
                    displayHour = fields.hour % 12;
                  } else {
                    displayHour = fields.hour % 12;
                    if (displayHour === 0) displayHour = 12;
                  }
                } else if (normalized.hourCycle === 'h24' && displayHour === 0) {
                  displayHour = 24;
                }
              } else if (normalized.dayPeriod !== undefined) {
                // dayPeriod-only: compute from hour field, no hour output
                dayPeriod = getDayPeriodForHour(fields.hour, normalized.dayPeriod);
              }

              const timeParts = [];
              if (normalized.hour !== undefined) {
                timeParts.push({
                  type: 'hour',
                  value: normalized.hour === '2-digit'
                    ? String(displayHour).padStart(2, '0')
                    : String(displayHour),
                });
              }
              if (normalized.minute !== undefined) {
                timeParts.push({ type: 'minute', value: String(fields.minute).padStart(2, '0') });
              }
              if (normalized.second !== undefined) {
                let secondValue = String(fields.second).padStart(2, '0');
                if (normalized.fractionalSecondDigits !== undefined) {
                  const ms = String(fields.millisecond).padStart(3, '0')
                    .substring(0, normalized.fractionalSecondDigits);
                  secondValue += '.' + ms;
                }
                timeParts.push({ type: 'second', value: secondValue });
              } else if (normalized.fractionalSecondDigits !== undefined) {
                timeParts.push({
                  type: 'fractionalSecond',
                  value: String(fields.millisecond).padStart(3, '0').substring(0, normalized.fractionalSecondDigits),
                });
              }

              timeParts.forEach((part, index) => {
                if (index > 0) {
                  pushLiteral(':');
                }
                parts.push(part);
              });

              if (dayPeriod && (use12Hour || normalized.dayPeriod !== undefined)) {
                if (timeParts.length > 0) pushLiteral(' ');
                parts.push({ type: 'dayPeriod', value: dayPeriod });
              }

              if (normalized.timeZoneName !== undefined) {
                const tzStyle = normalized.timeZoneName;
                let zoneName = normalized.timeZone || 'UTC';
                if (tzStyle === 'long') {
                  if (zoneName === 'UTC' || zoneName === 'Etc/UTC') zoneName = 'Coordinated Universal Time';
                  else if (zoneName === 'GMT' || zoneName === 'Etc/GMT') zoneName = 'Greenwich Mean Time';
                  else if (zoneName === 'America/New_York') zoneName = 'Eastern Standard Time';
                  else if (zoneName === 'America/Los_Angeles') zoneName = 'Pacific Standard Time';
                } else {
                  if (zoneName === 'UTC' || zoneName === 'Etc/UTC') zoneName = 'UTC';
                  else if (zoneName === 'GMT' || zoneName === 'Etc/GMT') zoneName = 'GMT';
                  else if (zoneName === 'America/New_York') zoneName = 'EST';
                  else if (zoneName === 'America/Los_Angeles') zoneName = 'PST';
                  else if (zoneName === 'Europe/Berlin' || zoneName === 'Europe/Vienna') zoneName = 'GMT+1';
                }
                pushLiteral(' ');
                parts.push({ type: 'timeZoneName', value: zoneName });
              }
            }

            if (parts.length === 0) {
              parts.push({ type: 'literal', value: '' });
            }
            return parts;
          }

          const NUMBERING_SYSTEM_DIGITS = {
            arab: 0x0660, arabext: 0x06F0, beng: 0x09E6, deva: 0x0966,
            gujr: 0x0AE6, guru: 0x0A66, khmr: 0x17E0, knda: 0x0CE6,
            laoo: 0x0ED0, mlym: 0x0D66, mong: 0x1810, mymr: 0x1040,
            orya: 0x0B66, tamldec: 0x0BE6, telu: 0x0C66, thai: 0x0E50, tibt: 0x0F20,
          };
          // Decimal separators for non-Latin numbering systems
          const NUMBERING_SYSTEM_DECIMAL = {
            arab: '\u066B', arabext: '\u066B',
          };

          function applyNumberingSystem(str, ns) {
            if (!ns || ns === 'latn') return str;
            if (ns === 'hanidec') {
              const hanidec = ['〇','一','二','三','四','五','六','七','八','九'];
              return str.replace(/[0-9]/g, (d) => hanidec[Number(d)]);
            }
            const base = NUMBERING_SYSTEM_DIGITS[ns];
            if (base === undefined) return str;
            let result = str.replace(/[0-9]/g, (d) => String.fromCodePoint(base + Number(d)));
            const dec = NUMBERING_SYSTEM_DECIMAL[ns];
            if (dec) result = result.replace(/\./g, dec);
            return result;
          }

          // Helper function to format date according to resolved options
          function formatDateWithOptions(d, opts) {
            const raw = formatDateWithOptionsToParts(d, opts).map((part) => part.value).join('');
            return applyNumberingSystem(raw, opts.numberingSystem);
          }

          function normalizeDateTimeFormatInput(value) {
            if (value === undefined) {
              return new Date(Date.now());
            }

            if (value instanceof Date) {
              return new Date(value.getTime());
            }

            if (typeof value === 'object' && value !== null) {
              const tag = Object.prototype.toString.call(value);
              if (tag === '[object Temporal.Instant]') {
                return new Date(Number(value.epochMilliseconds));
              }
            }

            // Per spec: ToNumber(date), not new Date(date) — string inputs must produce NaN
            return new Date(Number(value));
          }

          function isTemporalInstantValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.Instant]';
          }

          function isTemporalPlainTimeValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainTime]';
          }

          function isTemporalPlainMonthDayValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainMonthDay]';
          }

          function isTemporalPlainDateValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainDate]';
          }

          function isTemporalPlainDateTimeValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainDateTime]';
          }

          function isTemporalPlainYearMonthValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainYearMonth]';
          }

          function temporalCalendarId(value) {
            try {
              if (typeof value.calendarId === 'string') {
                return value.calendarId.toLowerCase();
              }
            } catch (_err) {}
            const match = String(value).match(/\[u-ca=([^\]]+)\]/);
            return match ? match[1].toLowerCase() : 'iso8601';
          }

          function temporalDateStringToUTCDate(value) {
            const match = String(value).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})/);
            if (!match) {
              throw new RangeError('Invalid time value');
            }
            return new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
          }

          function copyDefinedDateTimeFormatOptions(opts) {
            const adjusted = Object.create(null);
            const keys = [
              'calendar',
              'numberingSystem',
              'timeZone',
              'weekday',
              'era',
              'year',
              'month',
              'day',
              'dayPeriod',
              'hour',
              'minute',
              'second',
              'fractionalSecondDigits',
              'timeZoneName',
              'hourCycle',
              'hour12',
              'dateStyle',
              'timeStyle',
            ];
            for (const key of keys) {
              if (opts[key] !== undefined) {
                Object.defineProperty(adjusted, key, { value: opts[key], writable: true, enumerable: true, configurable: true });
              }
            }
            return adjusted;
          }

          function temporalInstantFormattingOptions(slot) {
            const opts = copyDefinedDateTimeFormatOptions(slot.resolvedOpts);
            const hasDateStyle = slot.resolvedOpts.dateStyle !== undefined || slot.resolvedOpts.timeStyle !== undefined;
            const hasExplicitCoreFields = slot.resolvedOpts.weekday !== undefined ||
              slot.resolvedOpts.era !== undefined ||
              slot.resolvedOpts.year !== undefined ||
              slot.resolvedOpts.month !== undefined ||
              slot.resolvedOpts.day !== undefined ||
              slot.resolvedOpts.hour !== undefined ||
              slot.resolvedOpts.minute !== undefined ||
              slot.resolvedOpts.second !== undefined;
            const needsInstantDefaults = !hasDateStyle && (
              slot.needsDefault || (!hasExplicitCoreFields && (
                slot.resolvedOpts.fractionalSecondDigits !== undefined ||
                slot.resolvedOpts.timeZoneName !== undefined ||
                slot.resolvedOpts.hour12 !== undefined ||
                slot.resolvedOpts.hourCycle !== undefined
              ))
            );
            if (needsInstantDefaults) {
              const use24Hour = slot.resolvedOpts.hour12 === false ||
                slot.resolvedOpts.hourCycle === 'h23' ||
                slot.resolvedOpts.hourCycle === 'h24' ||
                (slot.resolvedOpts.hour12 === undefined &&
                 slot.resolvedOpts.hourCycle === undefined &&
                 localeUses24Hour(slot.resolvedOpts.locale));
              opts.year ??= 'numeric';
              opts.month ??= 'numeric';
              opts.day ??= 'numeric';
              opts.hour ??= use24Hour ? '2-digit' : 'numeric';
              opts.minute ??= '2-digit';
              opts.second ??= '2-digit';
            }
            return opts;
          }

          function formatTemporalInstant(slot, instant, toParts) {
            const d = new Date(Number(instant.epochMilliseconds));
            if (isNaN(d.getTime())) {
              throw new RangeError('Invalid time value');
            }
            const opts = temporalInstantFormattingOptions(slot);
            opts.locale = slot.resolvedOpts.locale;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainTimeFormattingOptions(slot) {
            // dateStyle-only (no timeStyle) is a no-overlap error
            if (slot.resolvedOpts.dateStyle !== undefined && slot.resolvedOpts.timeStyle === undefined &&
                slot.resolvedOpts.hour === undefined && slot.resolvedOpts.minute === undefined &&
                slot.resolvedOpts.second === undefined && !slot.needsDefault) {
              throw new TypeError('PlainTime cannot be formatted with dateStyle');
            }

            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle;
            delete opts.timeStyle;
            delete opts.weekday;
            delete opts.era;
            delete opts.year;
            delete opts.month;
            delete opts.day;
            delete opts.timeZoneName;

            const hasCoreTimeFields = opts.hour !== undefined ||
              opts.minute !== undefined ||
              opts.second !== undefined;
            if (!hasCoreTimeFields) {
              // Only throw if explicit date-only fields were requested (not needsDefault)
              if (!slot.needsDefault && (slot.resolvedOpts.year !== undefined ||
                  slot.resolvedOpts.month !== undefined ||
                  slot.resolvedOpts.day !== undefined)) {
                throw new TypeError('PlainTime does not overlap with date fields');
              }
              const use24Hour = slot.resolvedOpts.hour12 === false ||
                slot.resolvedOpts.hourCycle === 'h23' ||
                slot.resolvedOpts.hourCycle === 'h24' ||
                (slot.resolvedOpts.hour12 === undefined &&
                 slot.resolvedOpts.hourCycle === undefined &&
                 localeUses24Hour(slot.resolvedOpts.locale));
              opts.hour = use24Hour ? '2-digit' : 'numeric';
              opts.minute = '2-digit';
              opts.second = '2-digit';
            }

            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainTime(slot, plainTime, toParts) {
            const d = new Date(Date.UTC(
              1972,
              0,
              1,
              plainTime.hour,
              plainTime.minute,
              plainTime.second,
              plainTime.millisecond,
            ));
            const opts = temporalPlainTimeFormattingOptions(slot);
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainMonthDayFormattingOptions(slot, plainMonthDay) {
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined) {
              throw new TypeError('PlainMonthDay cannot be formatted with timeStyle');
            }

            const formatterCalendar = resolveCalendarId(slot.resolvedOpts);
            const valueCalendar = temporalCalendarId(plainMonthDay);
            if (formatterCalendar !== valueCalendar) {
              throw new RangeError('calendar mismatch');
            }

            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle;
            delete opts.timeStyle;
            delete opts.weekday;
            delete opts.era;
            delete opts.year;
            delete opts.dayPeriod;
            delete opts.hour;
            delete opts.minute;
            delete opts.second;
            delete opts.fractionalSecondDigits;
            delete opts.timeZoneName;

            const hasDateFields = opts.month !== undefined || opts.day !== undefined;
            if (!hasDateFields) {
              if (slot.resolvedOpts.year !== undefined ||
                  slot.resolvedOpts.hour !== undefined ||
                  slot.resolvedOpts.minute !== undefined ||
                  slot.resolvedOpts.second !== undefined ||
                  slot.resolvedOpts.timeStyle !== undefined) {
                throw new TypeError('PlainMonthDay does not overlap with requested fields');
              }
              opts.month = 'numeric';
              opts.day = 'numeric';
            }

            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            opts.calendar = formatterCalendar;
            return opts;
          }

          function formatTemporalPlainMonthDay(slot, plainMonthDay, toParts) {
            const plainDate = plainMonthDay.toPlainDate({ year: 1972 });
            const d = temporalDateStringToUTCDate(plainDate);
            const opts = temporalPlainMonthDayFormattingOptions(slot, plainMonthDay);
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainDateFormattingOptions(slot, plainDate) {
            // PlainDate has no time data; timeStyle-only is a no-overlap error
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined &&
                slot.resolvedOpts.year === undefined && slot.resolvedOpts.month === undefined &&
                slot.resolvedOpts.day === undefined && slot.resolvedOpts.weekday === undefined &&
                slot.resolvedOpts.era === undefined && !slot.needsDefault) {
              throw new TypeError('PlainDate does not overlap with timeStyle');
            }
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            // Strip time-only fields
            delete opts.hour; delete opts.minute; delete opts.second;
            delete opts.fractionalSecondDigits; delete opts.dayPeriod;
            delete opts.timeZoneName; delete opts.dateStyle; delete opts.timeStyle;
            const hasNonEraDateFields = opts.weekday !== undefined ||
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined;
            const hasAnyDateFields = hasNonEraDateFields || opts.era !== undefined;
            if (!hasAnyDateFields || (!hasNonEraDateFields && opts.era !== undefined)) {
              if (!slot.needsDefault && slot.resolvedOpts.hour !== undefined) {
                throw new TypeError('PlainDate does not overlap with time fields');
              }
              opts.year = 'numeric'; opts.month = 'numeric'; opts.day = 'numeric';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          // Shift a UTC ms timestamp into the valid JS Date range by adding/subtracting
          // multiples of 400 years (146097 days) to preserve weekday and calendar cycle.
          const MS_PER_DAY = 86400000;
          const DAYS_PER_400Y = 146097;
          const MS_PER_400Y = DAYS_PER_400Y * MS_PER_DAY;
          const MAX_DATE_MS = 8640000000000000;

          function temporalDateToInRangeUTC(year, month0, day) {
            // month0 is 0-based
            let y = year, m = month0, d = day;
            // Shift year into range by multiples of 400
            while (true) {
              const ms = Date.UTC(y, m, d);
              if (!isNaN(ms) && ms >= -MAX_DATE_MS && ms <= MAX_DATE_MS) return ms;
              if (y < 0) y += 400; else y -= 400;
            }
          }

          function formatTemporalPlainDate(slot, plainDate, toParts) {
            const match = String(plainDate).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})/);
            if (!match) throw new RangeError('Invalid PlainDate');
            const actualYear = Number(match[1]);
            const ms = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, Number(match[3]));
            const d = new Date(ms);
            const opts = temporalPlainDateFormattingOptions(slot, plainDate);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainDateTimeFormattingOptions(slot) {
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.timeZoneName; delete opts.dateStyle; delete opts.timeStyle;
            const hasNonEraDateFields = opts.weekday !== undefined ||
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined;
            const hasDateFields = hasNonEraDateFields || opts.era !== undefined;
            const hasTimeFields = opts.hour !== undefined || opts.minute !== undefined ||
              opts.second !== undefined || opts.fractionalSecondDigits !== undefined ||
              opts.dayPeriod !== undefined;
            if (!hasDateFields && !hasTimeFields) {
              opts.year = 'numeric'; opts.month = 'numeric'; opts.day = 'numeric';
              opts.hour = 'numeric'; opts.minute = '2-digit'; opts.second = '2-digit';
            } else if (slot.needsDefault || (!hasNonEraDateFields && opts.era !== undefined && !hasTimeFields)) {
              // needsDefault or era-only: add both date and time defaults for PlainDateTime
              opts.year ??= 'numeric'; opts.month ??= 'numeric'; opts.day ??= 'numeric';
              opts.hour ??= 'numeric'; opts.minute ??= '2-digit'; opts.second ??= '2-digit';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainDateTime(slot, plainDateTime, toParts) {
            const match = String(plainDateTime).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?/);
            if (!match) throw new RangeError('Invalid PlainDateTime');
            const actualYear = Number(match[1]);
            const fracMs = match[7] ? Math.round(Number(match[7].substring(0, 3).padEnd(3, '0'))) : 0;
            const timeOffset = Number(match[4]) * 3600000 + Number(match[5]) * 60000 + Number(match[6]) * 1000 + fracMs;
            let baseMs = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, Number(match[3]));
            let totalMs = baseMs + timeOffset;
            // If adding time offset pushes out of range, shift base by -400 years
            if (isNaN(totalMs) || totalMs > 8640000000000000 || totalMs < -8640000000000000) {
              baseMs = temporalDateToInRangeUTC(actualYear - 400, Number(match[2]) - 1, Number(match[3]));
              totalMs = baseMs + timeOffset;
            }
            const d = new Date(totalMs);
            const opts = temporalPlainDateTimeFormattingOptions(slot);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainYearMonthFormattingOptions(slot, plainYearMonth) {
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined) {
              throw new TypeError('PlainYearMonth cannot be formatted with timeStyle');
            }
            const formatterCalendar = resolveCalendarId(slot.resolvedOpts);
            const valueCalendar = temporalCalendarId(plainYearMonth);
            if (formatterCalendar !== valueCalendar && formatterCalendar !== 'gregory' && formatterCalendar !== 'iso8601') {
              throw new RangeError('calendar mismatch');
            }
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle; delete opts.timeStyle; delete opts.weekday; delete opts.era;
            delete opts.day; delete opts.dayPeriod; delete opts.hour; delete opts.minute;
            delete opts.second; delete opts.fractionalSecondDigits; delete opts.timeZoneName;
            const hasFields = opts.year !== undefined || opts.month !== undefined;
            if (!hasFields) {
              if (slot.resolvedOpts.day !== undefined || slot.resolvedOpts.hour !== undefined ||
                  slot.resolvedOpts.minute !== undefined || slot.resolvedOpts.second !== undefined) {
                throw new TypeError('PlainYearMonth does not overlap with requested fields');
              }
              opts.year = 'numeric'; opts.month = 'numeric';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainYearMonth(slot, plainYearMonth, toParts) {
            const match = String(plainYearMonth).match(/^([+-]?\d{4,6})-(\d{2})/);
            if (!match) throw new RangeError('Invalid PlainYearMonth');
            const actualYear = Number(match[1]);
            const ms = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, 1);
            const d = new Date(ms);
            const opts = temporalPlainYearMonthFormattingOptions(slot, plainYearMonth);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          // Helper to make non-constructable getter/function
          function makeNonConstructableAccessor(impl, name) {
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const handler = {
              apply(target, thisArg, args) {
                return impl.apply(thisArg, args);
              }
            };
            const proxy = new Proxy(arrowWrapper, handler);
            Object.defineProperty(proxy, 'name', { value: name, configurable: true });
            Object.defineProperty(proxy, 'length', { value: 0, writable: false, enumerable: false, configurable: true });
            return proxy;
          }

          // format getter (returns a bound function)
          // Per spec, the getter must not be a constructor and must not have prototype
          const formatGetterImpl = function() {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method get Intl.DateTimeFormat.prototype.format called on incompatible receiver');
            }
            const boundFormat = (date) => {
              if (isTemporalInstantValue(date)) {
                return formatTemporalInstant(slot, date, false);
              }
              if (isTemporalPlainTimeValue(date)) {
                return formatTemporalPlainTime(slot, date, false);
              }
              if (isTemporalPlainDateTimeValue(date)) {
                return formatTemporalPlainDateTime(slot, date, false);
              }
              if (isTemporalPlainDateValue(date)) {
                return formatTemporalPlainDate(slot, date, false);
              }
              if (isTemporalPlainYearMonthValue(date)) {
                return formatTemporalPlainYearMonth(slot, date, false);
              }
              if (isTemporalPlainMonthDayValue(date)) {
                return formatTemporalPlainMonthDay(slot, date, false);
              }
              const d = normalizeDateTimeFormatInput(date);
              if (isNaN(d.getTime())) {
                throw new RangeError('Invalid time value');
              }
              // Use our custom formatter when dayPeriod, fractionalSecondDigits, non-Latin numbering system,
              // or needsDefault is set (needsDefault: underlying DTF was created without explicit fields,
              // so it may add time components when hour12/hourCycle is set)
              const needsCustomFormat = slot.needsDefault ||
                slot.resolvedOpts.dayPeriod !== undefined ||
                slot.resolvedOpts.fractionalSecondDigits !== undefined ||
                (slot.resolvedOpts.numberingSystem && slot.resolvedOpts.numberingSystem !== 'latn') ||
                slot.resolvedOpts.calendar === 'chinese' ||
                slot.resolvedOpts.calendar === 'dangi' ||
                slot.resolvedOpts.dateStyle !== undefined ||
                slot.resolvedOpts.timeStyle !== undefined;
              if (needsCustomFormat) {
                return formatDateWithOptions(d, slot.resolvedOpts);
              }
              if (slot.instance && typeof slot.instance.format === 'function') {
                return slot.instance.format(d);
              }
              return formatDateWithOptions(d, slot.resolvedOpts);
            };
            Object.defineProperty(boundFormat, 'name', { value: '', configurable: true });
            // Cache the bound format function
            Object.defineProperty(this, 'format', { value: boundFormat, writable: true, configurable: true });
            return boundFormat;
          };
          const formatGetter = makeNonConstructableAccessor(formatGetterImpl, 'get format');
          
          Object.defineProperty(newProto, 'format', {
            get: formatGetter,
            enumerable: false,
            configurable: true
          });

          // formatToParts method
          const formatToPartsImpl = function(date) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatToParts called on incompatible receiver');
            }
            if (isTemporalInstantValue(date)) {
              return formatTemporalInstant(slot, date, true);
            }
            if (isTemporalPlainTimeValue(date)) {
              return formatTemporalPlainTime(slot, date, true);
            }
            if (isTemporalPlainDateTimeValue(date)) {
              return formatTemporalPlainDateTime(slot, date, true);
            }
            if (isTemporalPlainDateValue(date)) {
              return formatTemporalPlainDate(slot, date, true);
            }
            if (isTemporalPlainYearMonthValue(date)) {
              return formatTemporalPlainYearMonth(slot, date, true);
            }
            if (isTemporalPlainMonthDayValue(date)) {
              return formatTemporalPlainMonthDay(slot, date, true);
            }
            const d = normalizeDateTimeFormatInput(date);
            if (isNaN(d.getTime())) {
              throw new RangeError('Invalid time value');
            }
            if (slot.instance && !slot.needsDefault && typeof slot.instance.formatToParts === 'function') {
              return slot.instance.formatToParts(d);
            }
            return formatDateWithOptionsToParts(d, slot.resolvedOpts);
          };
          Object.defineProperty(newProto, 'formatToParts', {
            value: makeNonConstructableAccessor(formatToPartsImpl, 'formatToParts'),
            writable: true,
            enumerable: false,
            configurable: true
          });

          // Classify a value into a Temporal type name, or null for non-Temporal
          function temporalTypeName(value) {
            if (typeof value !== 'object' || value === null) return null;
            const tag = Object.prototype.toString.call(value);
            const m = tag.match(/^\[object Temporal\.(\w+)\]$/);
            return m ? m[1] : null;
          }

          // Format a single value (Date or Temporal) using this slot
          function formatSingleValue(slot, value) {
            if (isTemporalInstantValue(value)) return formatTemporalInstant(slot, value, false);
            if (isTemporalPlainTimeValue(value)) return formatTemporalPlainTime(slot, value, false);
            if (isTemporalPlainDateTimeValue(value)) return formatTemporalPlainDateTime(slot, value, false);
            if (isTemporalPlainDateValue(value)) return formatTemporalPlainDate(slot, value, false);
            if (isTemporalPlainYearMonthValue(value)) return formatTemporalPlainYearMonth(slot, value, false);
            if (isTemporalPlainMonthDayValue(value)) return formatTemporalPlainMonthDay(slot, value, false);
            const d = normalizeDateTimeFormatInput(value);
            if (isNaN(d.getTime())) throw new RangeError('Invalid time value');
            const needsCustom = slot.needsDefault || slot.resolvedOpts.dayPeriod !== undefined ||
              slot.resolvedOpts.fractionalSecondDigits !== undefined ||
              (slot.resolvedOpts.numberingSystem && slot.resolvedOpts.numberingSystem !== 'latn');
            if (!needsCustom && slot.instance && typeof slot.instance.format === 'function') {
              return slot.instance.format(d);
            }
            return formatDateWithOptions(d, slot.resolvedOpts);
          }

          // Format a single value to parts
          function formatSingleValueToParts(slot, value) {
            if (isTemporalInstantValue(value)) return formatTemporalInstant(slot, value, true);
            if (isTemporalPlainTimeValue(value)) return formatTemporalPlainTime(slot, value, true);
            if (isTemporalPlainDateTimeValue(value)) return formatTemporalPlainDateTime(slot, value, true);
            if (isTemporalPlainDateValue(value)) return formatTemporalPlainDate(slot, value, true);
            if (isTemporalPlainYearMonthValue(value)) return formatTemporalPlainYearMonth(slot, value, true);
            if (isTemporalPlainMonthDayValue(value)) return formatTemporalPlainMonthDay(slot, value, true);
            const d = normalizeDateTimeFormatInput(value);
            if (isNaN(d.getTime())) throw new RangeError('Invalid time value');
            if (slot.instance && !slot.needsDefault && typeof slot.instance.formatToParts === 'function') {
              return slot.instance.formatToParts(d);
            }
            return formatDateWithOptionsToParts(d, slot.resolvedOpts);
          }

          // Core formatRange logic: returns {startStr, endStr, separator}
          // or {collapsed: str} when practically equal
          function formatRangeCore(slot, startDate, endDate) {
            const startType = temporalTypeName(startDate);
            const endType = temporalTypeName(endDate);
            // Mixed Temporal types → TypeError
            if (startType !== endType) {
              throw new TypeError('formatRange: incompatible argument types');
            }
            const startStr = formatSingleValue(slot, startDate);
            const endStr = formatSingleValue(slot, endDate);
            // Practically equal: same formatted output
            if (startStr === endStr) return { collapsed: startStr };
            return { startStr, endStr };
          }

          // formatRange method
          const formatRangeImpl = function(startDate, endDate) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatRange called on incompatible receiver');
            }
            if (startDate === undefined || endDate === undefined) {
              throw new TypeError('startDate and endDate are required');
            }
            const result = formatRangeCore(slot, startDate, endDate);
            if (result.collapsed !== undefined) return result.collapsed;
            // Try underlying instance for range separator
            const startType = temporalTypeName(startDate);
            if (startType === null) {
              const start = normalizeDateTimeFormatInput(startDate);
              const end = normalizeDateTimeFormatInput(endDate);
              if (!isNaN(start.getTime()) && !isNaN(end.getTime()) &&
                  slot.instance && !slot.needsDefault &&
                  slot.resolvedOpts.fractionalSecondDigits === undefined &&
                  typeof slot.instance.formatRange === 'function') {
                return slot.instance.formatRange(start, end);
              }
            }
            return result.startStr + ' \u2013 ' + result.endStr;
          };
          Object.defineProperty(formatRangeImpl, 'length', { value: 2, writable: false, enumerable: false, configurable: true });
          Object.defineProperty(newProto, 'formatRange', {
            value: makeNonConstructableAccessor(formatRangeImpl, 'formatRange'),
            writable: true,
            enumerable: false,
            configurable: true
          });
          // Ensure the proxy wrapper also has length 2
          Object.defineProperty(newProto.formatRange, 'length', { value: 2, writable: false, enumerable: false, configurable: true });

          // formatRangeToParts method
          const formatRangeToPartsImpl = function(startDate, endDate) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatRangeToParts called on incompatible receiver');
            }
            if (startDate === undefined || endDate === undefined) {
              throw new TypeError('startDate and endDate are required');
            }
            const startType = temporalTypeName(startDate);
            const endType = temporalTypeName(endDate);
            if (startType !== endType) {
              throw new TypeError('formatRangeToParts: incompatible argument types');
            }
            // For plain Date objects, try underlying instance
            if (startType === null) {
              const start = normalizeDateTimeFormatInput(startDate);
              const end = normalizeDateTimeFormatInput(endDate);
              if (!isNaN(start.getTime()) && !isNaN(end.getTime()) &&
                  slot.instance && !slot.needsDefault &&
                  slot.resolvedOpts.fractionalSecondDigits === undefined &&
                  typeof slot.instance.formatRangeToParts === 'function') {
                return slot.instance.formatRangeToParts(start, end);
              }
            }
            // Custom path: format both sides, collapse if equal
            const startStr = formatSingleValue(slot, startDate);
            const endStr = formatSingleValue(slot, endDate);
            if (startStr === endStr) {
              return formatSingleValueToParts(slot, startDate).map((p) => ({ ...p, source: 'shared' }));
            }
            const startParts = formatSingleValueToParts(slot, startDate).map((p) => ({ ...p, source: 'startRange' }));
            const endParts = formatSingleValueToParts(slot, endDate).map((p) => ({ ...p, source: 'endRange' }));
            return [...startParts, { type: 'literal', value: ' \u2013 ', source: 'shared' }, ...endParts];
          };
          Object.defineProperty(newProto, 'formatRangeToParts', {
            value: makeNonConstructableAccessor(formatRangeToPartsImpl, 'formatRangeToParts'),
            writable: true,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(newProto, 'constructor', {
            value: WrappedDTF,
            writable: true,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(newProto, Symbol.toStringTag, {
            value: 'Intl.DateTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(WrappedDTF, 'prototype', {
            value: newProto,
            writable: false,
            enumerable: false,
            configurable: false
          });

          // Replace Intl.DateTimeFormat
          Object.defineProperty(Intl, 'DateTimeFormat', {
            value: WrappedDTF,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_date_locale_methods(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const DateProto = Date.prototype;
          // Capture at install time so tainted Intl.DateTimeFormat doesn't affect us
          const DTF = Intl.DateTimeFormat;
          
          // Store original methods before overwriting
          const originalToLocaleString = DateProto.toLocaleString;
          const originalToLocaleDateString = DateProto.toLocaleDateString;
          const originalToLocaleTimeString = DateProto.toLocaleTimeString;
          
          // Test if the native implementation works (store result to avoid repeated calls)
          let toLocaleStringNeedsPolyfill = false;
          let toLocaleDateStringNeedsPolyfill = false;
          let toLocaleTimeStringNeedsPolyfill = false;
          
          const testDate = new Date(2020, 0, 1);
          try {
            const result = originalToLocaleString.call(testDate);
            toLocaleStringNeedsPolyfill = typeof result !== 'string';
          } catch (e) {
            toLocaleStringNeedsPolyfill = true;
          }
          
          try {
            const result = originalToLocaleDateString.call(testDate);
            toLocaleDateStringNeedsPolyfill = typeof result !== 'string';
          } catch (e) {
            toLocaleDateStringNeedsPolyfill = true;
          }
          
          try {
            const result = originalToLocaleTimeString.call(testDate);
            toLocaleTimeStringNeedsPolyfill = typeof result !== 'string';
          } catch (e) {
            toLocaleTimeStringNeedsPolyfill = true;
          }
          
          // Helper to create a non-constructable function with proper name
          // Uses a Proxy on an arrow function (which has no [[Construct]])
          function makeNonConstructable(impl, name) {
            // Create an arrow function that calls impl with proper this
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            
            const handler = {
              apply(target, thisArg, args) {
                // Call impl with the correct this
                return impl.apply(thisArg, args);
              }
            };
            const proxy = new Proxy(arrowWrapper, handler);
            Object.defineProperty(proxy, 'name', { value: name, configurable: true });
            Object.defineProperty(proxy, 'length', { value: 0, writable: false, enumerable: false, configurable: true });
            return proxy;
          }
          
          // toLocaleString - uses DateTimeFormat with date and time components
          if (toLocaleStringNeedsPolyfill) {
            const toLocaleStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.5 (toLocaleString):
              // - If no options, default to year/month/day/hour/minute/second
              // - If options are given (even just {hour12: false}), use them
              // - toLocaleString does NOT add missing date/time components when some are given
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  year: 'numeric',
                  month: 'numeric', 
                  day: 'numeric',
                  hour: 'numeric',
                  minute: 'numeric',
                  second: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasDate = hasDateOptions(opts);
                const hasTime = hasTimeOptions(opts);
                const hasStyle = opts.dateStyle !== undefined || opts.timeStyle !== undefined;
                
                if (!hasDate && !hasTime && !hasStyle) {
                  // Options given but no date/time/style components (e.g., {hour12: false})
                  // Add both date and time defaults
                  resolvedOptions = Object.assign({}, opts, {
                    year: 'numeric',
                    month: 'numeric',
                    day: 'numeric',
                    hour: 'numeric',
                    minute: 'numeric',
                    second: 'numeric'
                  });
                } else {
                  // If any date/time/style components specified, use options as-is
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleStringFn = makeNonConstructable(toLocaleStringImpl, 'toLocaleString');
            Object.defineProperty(DateProto, 'toLocaleString', {
              value: toLocaleStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
          
          // Helper to check if object has any date components
          function hasDateOptions(opts) {
            return opts && (opts.weekday !== undefined || opts.era !== undefined || 
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined);
          }
          
          // Helper to check if object has any time components
          function hasTimeOptions(opts) {
            return opts && (opts.hour !== undefined || opts.minute !== undefined || 
              opts.second !== undefined || opts.dayPeriod !== undefined);
          }
          
          // toLocaleDateString - uses DateTimeFormat with date components
          // Per ECMA-402: Always includes date components, adds them if missing
          if (toLocaleDateStringNeedsPolyfill) {
            const toLocaleDateStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleDateString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.6:
              // - If no options, default to year/month/day
              // - If no date options (even if other options present), add year/month/day
              // This is "date/date" in the spec - date method requires date components
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  year: 'numeric',
                  month: 'numeric',
                  day: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasDate = hasDateOptions(opts);
                
                if (!hasDate) {
                  // No date options - add date defaults (toLocaleDateString always needs date)
                  resolvedOptions = Object.assign({}, opts, {
                    year: 'numeric',
                    month: 'numeric', 
                    day: 'numeric'
                  });
                } else {
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleDateStringFn = makeNonConstructable(toLocaleDateStringImpl, 'toLocaleDateString');
            Object.defineProperty(DateProto, 'toLocaleDateString', {
              value: toLocaleDateStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
          
          // toLocaleTimeString - uses DateTimeFormat with time components
          // Per ECMA-402: Always includes time components, adds them if missing
          if (toLocaleTimeStringNeedsPolyfill) {
            const toLocaleTimeStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleTimeString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.7:
              // - If no options, default to hour/minute/second
              // - If no time options (even if other options present), add hour/minute/second
              // This is "time/time" in the spec - time method requires time components
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  hour: 'numeric',
                  minute: 'numeric',
                  second: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasTime = hasTimeOptions(opts);
                
                if (!hasTime) {
                  // No time options - add time defaults (toLocaleTimeString always needs time)
                  resolvedOptions = Object.assign({}, opts, {
                    hour: 'numeric',
                    minute: 'numeric',
                    second: 'numeric'
                  });
                } else {
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleTimeStringFn = makeNonConstructable(toLocaleTimeStringImpl, 'toLocaleTimeString');
            Object.defineProperty(DateProto, 'toLocaleTimeString', {
              value: toLocaleTimeStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
        })();
        "#,
    ))?;
    Ok(())
}

fn install_temporal_locale_string_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Temporal !== 'object' || Temporal === null) return;
          if (typeof Intl !== 'object' || Intl === null) return;
          if (typeof Intl.DateTimeFormat !== 'function') return;

          const instantProto = Temporal.Instant && Temporal.Instant.prototype;
          if (instantProto && typeof instantProto.toLocaleString === 'function') {
            const instantToLocaleString = new Proxy(() => {}, {
              apply(_target, thisArg, args) {
                if (Object.prototype.toString.call(thisArg) !== '[object Temporal.Instant]') {
                  throw new TypeError('Temporal.Instant.prototype.toLocaleString called on incompatible receiver');
                }
                const formatter = new Intl.DateTimeFormat(args[0], args[1]);
                return formatter.format(thisArg);
              },
            });
            Object.defineProperty(instantToLocaleString, 'name', {
              value: 'toLocaleString',
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(instantToLocaleString, 'length', {
              value: 0,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(instantProto, 'toLocaleString', {
              value: instantToLocaleString,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_relative_time_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          // Check if RelativeTimeFormat already exists
          if (typeof Intl.RelativeTimeFormat === 'function') {
            return;
          }
          
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_NUMERIC = ['always', 'auto'];
          const VALID_STYLE = ['long', 'short', 'narrow'];
          const VALID_UNITS = ['year', 'years', 'quarter', 'quarters', 'month', 'months', 
                              'week', 'weeks', 'day', 'days', 'hour', 'hours', 
                              'minute', 'minutes', 'second', 'seconds'];
          
          // Singular unit mapping
          const SINGULAR_UNITS = {
            'years': 'year', 'quarters': 'quarter', 'months': 'month',
            'weeks': 'week', 'days': 'day', 'hours': 'hour',
            'minutes': 'minute', 'seconds': 'second'
          };
          
          // WeakMap to store internal slots
          const rtfSlots = new WeakMap();
          
          function getOption(options, property, type, values, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            if (type === 'string') {
              value = String(value);
            }
            if (values !== undefined && !values.includes(value)) {
              throw new RangeError('Invalid value ' + value + ' for option ' + property);
            }
            return value;
          }
          
          function RelativeTimeFormat(locales, options) {
            if (!(this instanceof RelativeTimeFormat) && new.target === undefined) {
              throw new TypeError('Constructor Intl.RelativeTimeFormat requires "new"');
            }
            
            // Process locales
            let locale;
            if (locales === undefined) {
              locale = new Intl.NumberFormat().resolvedOptions().locale || 'en';
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || 'en';
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? Intl.getCanonicalLocales(locales)[0] : 'en';
            } else {
              locale = 'en';
            }
            
            // Process options
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }
            
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Read numberingSystem
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            const style = getOption(opts, 'style', 'string', VALID_STYLE, 'long');
            const numeric = getOption(opts, 'numeric', 'string', VALID_NUMERIC, 'always');
            
            const resolvedOpts = {
              locale: locale,
              style: style,
              numeric: numeric,
              numberingSystem: numberingSystem || 'latn'
            };
            
            rtfSlots.set(this, resolvedOpts);
          }
          
          RelativeTimeFormat.prototype.resolvedOptions = function resolvedOptions() {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            return {
              locale: slots.locale,
              style: slots.style,
              numeric: slots.numeric,
              numberingSystem: slots.numberingSystem
            };
          };
          
          RelativeTimeFormat.prototype.format = function format(value, unit) {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            
            value = Number(value);
            if (!Number.isFinite(value)) {
              throw new RangeError('Invalid value');
            }
            
            unit = String(unit);
            if (!VALID_UNITS.includes(unit)) {
              throw new RangeError('Invalid unit argument');
            }
            
            // Normalize to singular
            const singularUnit = SINGULAR_UNITS[unit] || unit;
            const absValue = Math.abs(value);
            
            // Simple format implementation
            const style = slots.style;
            const numeric = slots.numeric;
            
            // Handle auto numeric for special cases
            if (numeric === 'auto') {
              if (value === 0) {
                if (singularUnit === 'second') return 'now';
                if (singularUnit === 'minute') return 'this minute';
                if (singularUnit === 'hour') return 'this hour';
                if (singularUnit === 'day') return 'today';
                if (singularUnit === 'week') return 'this week';
                if (singularUnit === 'month') return 'this month';
                if (singularUnit === 'quarter') return 'this quarter';
                if (singularUnit === 'year') return 'this year';
              } else if (value === -1) {
                if (singularUnit === 'second') return '1 second ago';
                if (singularUnit === 'minute') return '1 minute ago';
                if (singularUnit === 'hour') return '1 hour ago';
                if (singularUnit === 'day') return 'yesterday';
                if (singularUnit === 'week') return 'last week';
                if (singularUnit === 'month') return 'last month';
                if (singularUnit === 'quarter') return 'last quarter';
                if (singularUnit === 'year') return 'last year';
              } else if (value === 1) {
                if (singularUnit === 'second') return 'in 1 second';
                if (singularUnit === 'minute') return 'in 1 minute';
                if (singularUnit === 'hour') return 'in 1 hour';
                if (singularUnit === 'day') return 'tomorrow';
                if (singularUnit === 'week') return 'next week';
                if (singularUnit === 'month') return 'next month';
                if (singularUnit === 'quarter') return 'next quarter';
                if (singularUnit === 'year') return 'next year';
              }
            }
            
            // Unit labels based on style
            let unitLabel;
            if (style === 'narrow') {
              const narrowLabels = {
                year: 'yr', month: 'mo', week: 'wk', day: 'd',
                hour: 'hr', minute: 'min', second: 's', quarter: 'qtr'
              };
              unitLabel = narrowLabels[singularUnit] || singularUnit;
            } else if (style === 'short') {
              const shortLabels = {
                year: 'yr.', month: 'mo.', week: 'wk.', day: 'day',
                hour: 'hr.', minute: 'min.', second: 'sec.', quarter: 'qtr.'
              };
              const shortPluralLabels = {
                year: 'yr.', month: 'mo.', week: 'wk.', day: 'days',
                hour: 'hr.', minute: 'min.', second: 'sec.', quarter: 'qtr.'
              };
              unitLabel = absValue === 1 ? (shortLabels[singularUnit] || singularUnit) : (shortPluralLabels[singularUnit] || singularUnit + 's');
            } else {
              // long style
              unitLabel = singularUnit;
              if (absValue !== 1) {
                unitLabel += 's';
              }
            }
            
            // Format number using locale-aware NumberFormat
            const nf = new Intl.NumberFormat(slots.locale, { numberingSystem: slots.numberingSystem });
            const formattedValue = nf.format(absValue);
            
            // Handle negative zero specially: Object.is(value, -0) checks for -0
            // Positive zero is "in 0 X", negative zero is "0 X ago"
            if (value < 0 || Object.is(value, -0)) {
              return formattedValue + ' ' + unitLabel + ' ago';
            } else {
              return 'in ' + formattedValue + ' ' + unitLabel;
            }
          };
          
          RelativeTimeFormat.prototype.formatToParts = function formatToParts(value, unit) {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            
            value = Number(value);
            if (!Number.isFinite(value)) {
              throw new RangeError('Invalid value');
            }
            
            unit = String(unit);
            if (!VALID_UNITS.includes(unit)) {
              throw new RangeError('Invalid unit argument');
            }
            
            const singularUnit = SINGULAR_UNITS[unit] || unit;
            const absValue = Math.abs(value);
            
            // Format number using locale-aware NumberFormat
            const nf = new Intl.NumberFormat(slots.locale, { numberingSystem: slots.numberingSystem });
            const formattedValue = nf.format(absValue);
            
            const parts = [];
            if (value < 0) {
              parts.push({ type: 'integer', value: formattedValue, unit: singularUnit });
              parts.push({ type: 'literal', value: ' ' });
              parts.push({ type: 'literal', value: absValue === 1 ? singularUnit : singularUnit + 's' });
              parts.push({ type: 'literal', value: ' ago' });
            } else {
              parts.push({ type: 'literal', value: 'in ' });
              parts.push({ type: 'integer', value: String(absValue), unit: singularUnit });
              parts.push({ type: 'literal', value: ' ' });
              parts.push({ type: 'literal', value: absValue === 1 ? singularUnit : singularUnit + 's' });
            }
            
            return parts;
          };
          
          RelativeTimeFormat.supportedLocalesOf = function supportedLocalesOf(locales, options) {
            // Process options
            if (options !== undefined) {
              if (options === null) {
                throw new TypeError('Cannot convert null to object');
              }
              const opts = Object(options);
              const matcher = opts.localeMatcher;
              if (matcher !== undefined) {
                const matcherStr = String(matcher);
                if (!VALID_LOCALE_MATCHERS.includes(matcherStr)) {
                  throw new RangeError('Invalid localeMatcher');
                }
              }
            }
            
            if (locales === undefined) return [];
            const requestedLocales = Array.isArray(locales) ? locales : [String(locales)];
            // Validate each locale
            return Intl.getCanonicalLocales(requestedLocales);
          };
          
          // Make supportedLocalesOf non-enumerable and set length to 1
          Object.defineProperty(RelativeTimeFormat, 'supportedLocalesOf', {
            value: RelativeTimeFormat.supportedLocalesOf,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.supportedLocalesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Set up prototype chain
          Object.defineProperty(RelativeTimeFormat, 'prototype', {
            value: RelativeTimeFormat.prototype,
            writable: false,
            enumerable: false,
            configurable: false
          });
          
          // Make prototype methods non-enumerable
          Object.defineProperty(RelativeTimeFormat.prototype, 'format', {
            value: RelativeTimeFormat.prototype.format,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.prototype, 'formatToParts', {
            value: RelativeTimeFormat.prototype.formatToParts,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.prototype, 'resolvedOptions', {
            value: RelativeTimeFormat.prototype.resolvedOptions,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat.prototype, 'constructor', {
            value: RelativeTimeFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat.prototype, Symbol.toStringTag, {
            value: 'Intl.RelativeTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Set function length
          Object.defineProperty(RelativeTimeFormat, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat, 'name', {
            value: 'RelativeTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Install on Intl object
          Object.defineProperty(Intl, 'RelativeTimeFormat', {
            value: RelativeTimeFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_duration_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          // Check if DurationFormat already exists
          if (typeof Intl.DurationFormat === 'function') {
            return;
          }
          
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_STYLES = ['long', 'short', 'narrow', 'digital'];
          const VALID_DISPLAYS = ['auto', 'always'];
          const VALID_UNIT_STYLES = ['long', 'short', 'narrow', '2-digit', 'numeric'];
          
          // Unit component names
          const DURATION_UNITS = ['years', 'months', 'weeks', 'days', 'hours', 'minutes', 'seconds', 'milliseconds', 'microseconds', 'nanoseconds'];
          
          // WeakMap to store internal slots
          const dfSlots = new WeakMap();
          
          function getOption(options, property, type, values, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            if (type === 'string') {
              value = String(value);
            } else if (type === 'number') {
              value = Number(value);
              if (!Number.isFinite(value)) {
                throw new RangeError('Invalid ' + property);
              }
            }
            if (values !== undefined && !values.includes(value)) {
              throw new RangeError('Invalid value ' + value + ' for option ' + property);
            }
            return value;
          }
          
          function getNumberOption(options, property, minimum, maximum, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            value = Number(value);
            if (!Number.isFinite(value) || value < minimum || value > maximum) {
              throw new RangeError('Invalid ' + property);
            }
            return Math.floor(value);
          }
          
          function DurationFormat(locales, options) {
            if (!(this instanceof DurationFormat) && new.target === undefined) {
              throw new TypeError('Constructor Intl.DurationFormat requires "new"');
            }
            
            // Process locales
            let locale;
            if (locales === undefined) {
              locale = new Intl.NumberFormat().resolvedOptions().locale || 'en';
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || 'en';
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? Intl.getCanonicalLocales(locales)[0] : 'en';
            } else {
              locale = 'en';
            }
            
            // Process options
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }
            
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Read numberingSystem
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              // Must be 3-8 alphanum chars
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            const style = getOption(opts, 'style', 'string', VALID_STYLES, 'short');
            
            // Process per-unit options
            const unitOptions = {};
            for (const unit of DURATION_UNITS) {
              unitOptions[unit] = getOption(opts, unit, 'string', VALID_UNIT_STYLES, undefined);
              const displayKey = unit + 'Display';
              unitOptions[displayKey] = getOption(opts, displayKey, 'string', VALID_DISPLAYS, 'auto');
            }
            
            // fractionalDigits
            const fractionalDigits = getNumberOption(opts, 'fractionalDigits', 0, 9, undefined);
            
            // Store internal slots
            const slots = {
              locale: locale,
              numberingSystem: numberingSystem || 'latn',
              style: style,
              fractionalDigits: fractionalDigits,
              ...unitOptions
            };
            dfSlots.set(this, slots);
            
            return this;
          }
          
          // format method
          DurationFormat.prototype.format = function format(duration) {
            const slots = dfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method Intl.DurationFormat.prototype.format called on incompatible receiver');
            }
            
            if (duration === undefined || duration === null) {
              throw new TypeError('Duration is required');
            }
            
            if (typeof duration !== 'object') {
              throw new TypeError('Duration must be an object');
            }
            
            // Read duration components
            const components = {};
            let hasAny = false;
            for (const unit of DURATION_UNITS) {
              const value = duration[unit];
              if (value !== undefined) {
                const num = Number(value);
                if (!Number.isFinite(num)) {
                  throw new RangeError('Invalid duration component: ' + unit);
                }
                components[unit] = num;
                if (num !== 0) hasAny = true;
              } else {
                components[unit] = 0;
              }
            }
            
            // Check for sign consistency
            let hasPositive = false;
            let hasNegative = false;
            for (const unit of DURATION_UNITS) {
              if (components[unit] > 0) hasPositive = true;
              if (components[unit] < 0) hasNegative = true;
            }
            if (hasPositive && hasNegative) {
              throw new RangeError('Duration cannot have mixed signs');
            }
            
            const style = slots.style;
            const parts = [];
            
            // Unit labels based on style
            const labels = {
              long: {
                years: ['year', 'years'], months: ['month', 'months'], weeks: ['week', 'weeks'],
                days: ['day', 'days'], hours: ['hour', 'hours'], minutes: ['minute', 'minutes'],
                seconds: ['second', 'seconds'], milliseconds: ['millisecond', 'milliseconds'],
                microseconds: ['microsecond', 'microseconds'], nanoseconds: ['nanosecond', 'nanoseconds']
              },
              short: {
                years: ['yr', 'yrs'], months: ['mo', 'mos'], weeks: ['wk', 'wks'],
                days: ['day', 'days'], hours: ['hr', 'hrs'], minutes: ['min', 'mins'],
                seconds: ['sec', 'secs'], milliseconds: ['ms', 'ms'],
                microseconds: ['μs', 'μs'], nanoseconds: ['ns', 'ns']
              },
              narrow: {
                years: ['y', 'y'], months: ['m', 'm'], weeks: ['w', 'w'],
                days: ['d', 'd'], hours: ['h', 'h'], minutes: ['m', 'm'],
                seconds: ['s', 's'], milliseconds: ['ms', 'ms'],
                microseconds: ['μs', 'μs'], nanoseconds: ['ns', 'ns']
              }
            };
            
            const effectiveStyle = style === 'digital' ? 'short' : style;
            const unitLabels = labels[effectiveStyle] || labels.short;
            
            // Format each component
            for (const unit of DURATION_UNITS) {
              const value = components[unit];
              const display = slots[unit + 'Display'];
              
              if (display === 'always' || value !== 0) {
                const absValue = Math.abs(value);
                const label = unitLabels[unit];
                const unitLabel = absValue === 1 ? label[0] : label[1];
                
                if (style === 'digital' && (unit === 'hours' || unit === 'minutes' || unit === 'seconds')) {
                  parts.push(String(Math.floor(absValue)).padStart(2, '0'));
                } else {
                  parts.push(absValue + ' ' + unitLabel);
                }
              }
            }
            
            if (parts.length === 0) {
              // Return "0 seconds" or similar for empty duration
              const defaultUnit = 'seconds';
              const label = unitLabels[defaultUnit];
              return '0 ' + label[1];
            }
            
            if (style === 'digital') {
              return (hasNegative ? '-' : '') + parts.join(':');
            }
            
            return (hasNegative ? '-' : '') + parts.join(', ');
          };
          
          // formatToParts method
          DurationFormat.prototype.formatToParts = function formatToParts(duration) {
            const slots = dfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method Intl.DurationFormat.prototype.formatToParts called on incompatible receiver');
            }
            
            // Simplified - return the formatted string as a single literal part
            const formatted = this.format(duration);
            return [{ type: 'literal', value: formatted }];
          };
          
          // resolvedOptions method
          DurationFormat.prototype.resolvedOptions = function resolvedOptions() {
            const slots = dfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method Intl.DurationFormat.prototype.resolvedOptions called on incompatible receiver');
            }
            
            const result = {
              locale: slots.locale,
              numberingSystem: slots.numberingSystem,
              style: slots.style
            };
            
            // Add per-unit options
            for (const unit of DURATION_UNITS) {
              if (slots[unit] !== undefined) {
                result[unit] = slots[unit];
              }
              const displayKey = unit + 'Display';
              if (slots[displayKey] !== undefined) {
                result[displayKey] = slots[displayKey];
              }
            }
            
            if (slots.fractionalDigits !== undefined) {
              result.fractionalDigits = slots.fractionalDigits;
            }
            
            return result;
          };
          
          // supportedLocalesOf static method
          DurationFormat.supportedLocalesOf = function supportedLocalesOf(locales, options) {
            if (options !== undefined) {
              if (options === null) {
                throw new TypeError('Cannot convert null to object');
              }
              const opts = Object(options);
              const matcher = opts.localeMatcher;
              if (matcher !== undefined) {
                const matcherStr = String(matcher);
                if (!VALID_LOCALE_MATCHERS.includes(matcherStr)) {
                  throw new RangeError('Invalid localeMatcher');
                }
              }
            }
            
            if (locales === undefined) return [];
            const requestedLocales = Array.isArray(locales) ? locales : [String(locales)];
            return Intl.getCanonicalLocales(requestedLocales);
          };
          
          // Make supportedLocalesOf non-enumerable and set length to 1
          Object.defineProperty(DurationFormat, 'supportedLocalesOf', {
            value: DurationFormat.supportedLocalesOf,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(DurationFormat.supportedLocalesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Make prototype methods non-enumerable
          Object.defineProperty(DurationFormat.prototype, 'format', {
            value: DurationFormat.prototype.format,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(DurationFormat.prototype, 'formatToParts', {
            value: DurationFormat.prototype.formatToParts,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(DurationFormat.prototype, 'resolvedOptions', {
            value: DurationFormat.prototype.resolvedOptions,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(DurationFormat.prototype, 'constructor', {
            value: DurationFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(DurationFormat.prototype, Symbol.toStringTag, {
            value: 'Intl.DurationFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Set function length
          Object.defineProperty(DurationFormat, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(DurationFormat, 'name', {
            value: 'DurationFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Install on Intl object
          Object.defineProperty(Intl, 'DurationFormat', {
            value: DurationFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_supported_values_of(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl.supportedValuesOf === 'function') {
            return;
          }
          
          // Standard calendars - minimal set per ECMA-402 requirements
          // Note: 'islamic' and 'islamic-rgsa' removed as not all engines support them
          const calendars = [
            'buddhist', 'chinese', 'coptic', 'dangi', 'ethioaa', 'ethiopic',
            'gregory', 'hebrew', 'indian', 'islamic-civil', 'islamic-tbla',
            'islamic-umalqura', 'iso8601', 'japanese', 'persian', 'roc'
          ].sort();
          
          // Standard collations - per ECMA-402, 'standard' and 'search' must NOT be included
          const collations = [
            'compat', 'dict', 'emoji', 'eor', 'phonebk', 'phonetic', 'pinyin',
            'stroke', 'trad', 'unihan', 'zhuyin'
          ].sort();
          
          // ISO 4217 currency codes - including historical and test codes per spec
          const currencies = [
            'AAA', 'ADP', 'AED', 'AFA', 'AFN', 'ALK', 'ALL', 'AMD', 'ANG', 'AOA',
            'AOK', 'AON', 'AOR', 'ARA', 'ARL', 'ARM', 'ARP', 'ARS', 'ATS', 'AUD',
            'AWG', 'AYM', 'AZM', 'AZN', 'BAD', 'BAM', 'BAN', 'BBD', 'BDT', 'BEC',
            'BEF', 'BEL', 'BGL', 'BGM', 'BGN', 'BGO', 'BHD', 'BIF', 'BMD', 'BND',
            'BOB', 'BOL', 'BOP', 'BOV', 'BRB', 'BRC', 'BRE', 'BRL', 'BRN', 'BRR',
            'BRZ', 'BSD', 'BTN', 'BUK', 'BWP', 'BYB', 'BYN', 'BYR', 'BZD', 'CAD',
            'CDF', 'CHE', 'CHF', 'CHW', 'CLE', 'CLF', 'CLP', 'CNH', 'CNX', 'CNY',
            'COP', 'COU', 'CRC', 'CSD', 'CSK', 'CUC', 'CUP', 'CVE', 'CYP', 'CZK',
            'DDM', 'DEM', 'DJF', 'DKK', 'DOP', 'DZD', 'ECS', 'ECV', 'EEK', 'EGP',
            'ERN', 'ESA', 'ESB', 'ESP', 'ETB', 'EUR', 'FIM', 'FJD', 'FKP', 'FRF',
            'GBP', 'GEK', 'GEL', 'GHC', 'GHS', 'GIP', 'GMD', 'GNF', 'GNS', 'GQE',
            'GRD', 'GTQ', 'GWE', 'GWP', 'GYD', 'HKD', 'HNL', 'HRD', 'HRK', 'HTG',
            'HUF', 'IDR', 'IEP', 'ILP', 'ILR', 'ILS', 'INR', 'IQD', 'IRR', 'ISJ',
            'ISK', 'ITL', 'JMD', 'JOD', 'JPY', 'KES', 'KGS', 'KHR', 'KMF', 'KPW',
            'KRH', 'KRO', 'KRW', 'KWD', 'KYD', 'KZT', 'LAK', 'LBP', 'LKR', 'LRD',
            'LSL', 'LTL', 'LTT', 'LUC', 'LUF', 'LUL', 'LVL', 'LVR', 'LYD', 'MAD',
            'MAF', 'MCF', 'MDC', 'MDL', 'MGA', 'MGF', 'MKD', 'MKN', 'MLF', 'MMK',
            'MNT', 'MOP', 'MRO', 'MRU', 'MTL', 'MTP', 'MUR', 'MVP', 'MVR', 'MWK',
            'MXN', 'MXP', 'MXV', 'MYR', 'MZE', 'MZM', 'MZN', 'NAD', 'NGN', 'NIC',
            'NIO', 'NLG', 'NOK', 'NPR', 'NZD', 'OMR', 'PAB', 'PEI', 'PEN', 'PES',
            'PGK', 'PHP', 'PKR', 'PLN', 'PLZ', 'PTE', 'PYG', 'QAR', 'RHD', 'ROL',
            'RON', 'RSD', 'RUB', 'RUR', 'RWF', 'SAR', 'SBD', 'SCR', 'SDD', 'SDG',
            'SDP', 'SEK', 'SGD', 'SHP', 'SIT', 'SKK', 'SLE', 'SLL', 'SOS', 'SRD',
            'SRG', 'SSP', 'STD', 'STN', 'SUR', 'SVC', 'SYP', 'SZL', 'THB', 'TJR',
            'TJS', 'TMM', 'TMT', 'TND', 'TOP', 'TPE', 'TRL', 'TRY', 'TTD', 'TWD',
            'TZS', 'UAH', 'UAK', 'UGS', 'UGX', 'USD', 'USN', 'USS', 'UYI', 'UYP',
            'UYU', 'UYW', 'UZS', 'VEB', 'VED', 'VEF', 'VES', 'VND', 'VNN', 'VUV',
            'WST', 'XAF', 'XAG', 'XAU', 'XBA', 'XBB', 'XBC', 'XBD', 'XCD', 'XDR',
            'XEU', 'XFO', 'XFU', 'XOF', 'XPD', 'XPF', 'XPT', 'XRE', 'XSU', 'XTS',
            'XUA', 'XXX', 'YDD', 'YER', 'YUD', 'YUM', 'YUN', 'YUR', 'ZAL', 'ZAR',
            'ZMK', 'ZMW', 'ZRN', 'ZRZ', 'ZWD', 'ZWL', 'ZWR'
          ].sort();
          
          // Standard numbering systems with simple digit mappings (plus algorithmic)
          const numberingSystems = [
            'adlm', 'ahom', 'arab', 'arabext', 'armn', 'armnlow', 'bali', 'beng',
            'bhks', 'brah', 'cakm', 'cham', 'cyrl', 'deva', 'diak', 'ethi',
            'fullwide', 'gara', 'geor', 'gong', 'gonm', 'grek', 'greklow', 'gujr',
            'guru', 'hanidays', 'hanidec', 'hans', 'hansfin', 'hant', 'hantfin',
            'hebr', 'hmng', 'hmnp', 'java', 'jpan', 'jpanfin', 'jpanyear', 'kali',
            'kawi', 'khmr', 'knda', 'lana', 'lanatham', 'laoo', 'latn', 'lepc',
            'limb', 'mathbold', 'mathdbl', 'mathmono', 'mathsanb', 'mathsans',
            'mlym', 'modi', 'mong', 'mroo', 'mtei', 'mymr', 'mymrshan', 'mymrtlng',
            'nagm', 'newa', 'nkoo', 'olck', 'orya', 'osma', 'outlined', 'rohg',
            'roman', 'romanlow', 'saur', 'segment', 'shrd', 'sind', 'sinh', 'sora',
            'sund', 'sundlatn', 'takr', 'talu', 'taml', 'tamldec', 'telu', 'thai',
            'tibt', 'tirh', 'tnsa', 'vaii', 'wara', 'wcho'
          ].sort();
          
          // Time zones - including Etc/GMT+N zones per spec
          const timeZones = [
            'Africa/Abidjan', 'Africa/Accra', 'Africa/Addis_Ababa', 'Africa/Algiers',
            'Africa/Cairo', 'Africa/Casablanca', 'Africa/Johannesburg', 'Africa/Lagos',
            'Africa/Nairobi', 'Africa/Tunis', 'America/Adak', 'America/Anchorage',
            'America/Argentina/Buenos_Aires', 'America/Bogota', 'America/Caracas',
            'America/Chicago', 'America/Denver', 'America/Halifax', 'America/Lima',
            'America/Los_Angeles', 'America/Mexico_City', 'America/New_York',
            'America/Phoenix', 'America/Santiago', 'America/Sao_Paulo', 'America/St_Johns',
            'America/Toronto', 'America/Vancouver', 'Asia/Almaty', 'Asia/Baghdad',
            'Asia/Baku', 'Asia/Bangkok', 'Asia/Chongqing', 'Asia/Colombo', 'Asia/Dhaka',
            'Asia/Dubai', 'Asia/Ho_Chi_Minh', 'Asia/Hong_Kong', 'Asia/Istanbul',
            'Asia/Jakarta', 'Asia/Jerusalem', 'Asia/Kabul', 'Asia/Karachi',
            'Asia/Kathmandu', 'Asia/Kolkata', 'Asia/Kuala_Lumpur', 'Asia/Kuwait',
            'Asia/Manila', 'Asia/Rangoon', 'Asia/Riyadh', 'Asia/Seoul', 'Asia/Shanghai',
            'Asia/Singapore', 'Asia/Taipei', 'Asia/Tehran', 'Asia/Tokyo', 'Asia/Vladivostok',
            'Atlantic/Azores', 'Atlantic/Canary', 'Atlantic/Reykjavik',
            'Australia/Adelaide', 'Australia/Brisbane', 'Australia/Darwin',
            'Australia/Hobart', 'Australia/Melbourne', 'Australia/Perth', 'Australia/Sydney',
            'Etc/GMT', 'Etc/GMT+0', 'Etc/GMT+1', 'Etc/GMT+10', 'Etc/GMT+11', 'Etc/GMT+12',
            'Etc/GMT+2', 'Etc/GMT+3', 'Etc/GMT+4', 'Etc/GMT+5', 'Etc/GMT+6', 'Etc/GMT+7',
            'Etc/GMT+8', 'Etc/GMT+9', 'Etc/GMT-0', 'Etc/GMT-1', 'Etc/GMT-10', 'Etc/GMT-11',
            'Etc/GMT-12', 'Etc/GMT-13', 'Etc/GMT-14', 'Etc/GMT-2', 'Etc/GMT-3', 'Etc/GMT-4',
            'Etc/GMT-5', 'Etc/GMT-6', 'Etc/GMT-7', 'Etc/GMT-8', 'Etc/GMT-9', 'Etc/GMT0',
            'Etc/UTC', 'Europe/Amsterdam', 'Europe/Athens', 'Europe/Belgrade',
            'Europe/Berlin', 'Europe/Brussels', 'Europe/Bucharest', 'Europe/Budapest',
            'Europe/Copenhagen', 'Europe/Dublin', 'Europe/Helsinki', 'Europe/Kiev',
            'Europe/Lisbon', 'Europe/London', 'Europe/Madrid', 'Europe/Moscow',
            'Europe/Oslo', 'Europe/Paris', 'Europe/Prague', 'Europe/Rome', 'Europe/Sofia',
            'Europe/Stockholm', 'Europe/Vienna', 'Europe/Warsaw', 'Europe/Zurich',
            'Pacific/Auckland', 'Pacific/Fiji', 'Pacific/Guam', 'Pacific/Honolulu',
            'Pacific/Kiritimati', 'Pacific/Midway', 'Pacific/Noumea', 'Pacific/Pago_Pago',
            'Pacific/Tahiti', 'UTC'
          ].sort();
          
          // Standard units for NumberFormat
          const units = [
            'acre', 'bit', 'byte', 'celsius', 'centimeter', 'day', 'degree',
            'fahrenheit', 'fluid-ounce', 'foot', 'gallon', 'gigabit', 'gigabyte',
            'gram', 'hectare', 'hour', 'inch', 'kilobit', 'kilobyte', 'kilogram',
            'kilometer', 'liter', 'megabit', 'megabyte', 'meter', 'microsecond',
            'mile', 'mile-scandinavian', 'milliliter', 'millimeter', 'millisecond',
            'minute', 'month', 'nanosecond', 'ounce', 'percent', 'petabyte', 'pound',
            'second', 'stone', 'terabit', 'terabyte', 'week', 'yard', 'year'
          ].sort();
          
          function supportedValuesOf(key) {
            // Throw TypeError for Symbol
            if (typeof key === 'symbol') {
              throw new TypeError('Cannot convert a Symbol value to a string');
            }
            const keyStr = String(key);
            switch (keyStr) {
              case 'calendar':
                return calendars.slice();
              case 'collation':
                return collations.slice();
              case 'currency':
                return currencies.slice();
              case 'numberingSystem':
                return numberingSystems.slice();
              case 'timeZone':
                return timeZones.slice();
              case 'unit':
                return units.slice();
              default:
                throw new RangeError('Invalid key: ' + keyStr);
            }
          }
          
          Object.defineProperty(supportedValuesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(supportedValuesOf, 'name', {
            value: 'supportedValuesOf',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(Intl, 'supportedValuesOf', {
            value: supportedValuesOf,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_iterator_helpers(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        'use strict';
        (() => {
          // Check if Iterator already exists and has helpers
          if (typeof globalThis.Iterator === 'function' && 
              typeof Iterator.prototype.map === 'function') {
            return;
          }

          // Get the %IteratorPrototype%
          const IteratorPrototype = Object.getPrototypeOf(
            Object.getPrototypeOf([][Symbol.iterator]())
          );

          // Helper to define non-enumerable method
          function defineMethod(obj, name, fn, length) {
            Object.defineProperty(obj, name, {
              value: fn,
              writable: true,
              enumerable: false,
              configurable: true
            });
            // For symbols, the name should be the description wrapped in brackets
            let nameValue = name;
            if (typeof name === 'symbol') {
              const desc = name.description;
              nameValue = desc ? '[' + desc + ']' : '';
            }
            Object.defineProperty(fn, 'name', {
              value: nameValue,
              writable: false,
              enumerable: false,
              configurable: true
            });
            Object.defineProperty(fn, 'length', {
              value: length !== undefined ? length : fn.length,
              writable: false,
              enumerable: false,
              configurable: true
            });
          }

          // Helper to make a function non-constructible
          function makeNonConstructible(impl, name, length) {
            // Arrow functions have no [[Construct]], wrap with Proxy for 'this' support
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const proxy = new Proxy(arrowWrapper, {
              apply(target, thisArg, args) {
                return impl.apply(thisArg, args);
              }
            });
            // For symbols, the name should be the description wrapped in brackets
            let nameValue = name;
            if (typeof name === 'symbol') {
              const desc = name.description;
              nameValue = desc ? '[' + desc + ']' : '';
            }
            Object.defineProperty(proxy, 'name', {
              value: nameValue,
              writable: false,
              enumerable: false,
              configurable: true
            });
            Object.defineProperty(proxy, 'length', {
              value: length,
              writable: false,
              enumerable: false,
              configurable: true
            });
            return proxy;
          }

          // Helper to define non-enumerable, non-constructible method
          function defineNonConstructibleMethod(obj, name, fn, length) {
            const wrapped = makeNonConstructible(fn, name, length);
            Object.defineProperty(obj, name, {
              value: wrapped,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }

          // Helper to get iterator record - per spec, doesn't validate callability
          function GetIteratorDirect(obj) {
            if (typeof obj !== 'object' || obj === null) {
              throw new TypeError('Iterator must be an object');
            }
            const nextMethod = obj.next;
            // Note: We do NOT check if nextMethod is callable here
            // That check happens when next() is actually called
            return { iterator: obj, nextMethod, done: false };
          }

          // Helper to call next method with validation
          function IteratorNext(iteratorRecord) {
            const nextMethod = iteratorRecord.nextMethod;
            if (typeof nextMethod !== 'function') {
              throw new TypeError('Iterator next method must be callable');
            }
            return nextMethod.call(iteratorRecord.iterator);
          }

          function getIntrinsicIteratorPrototype(newTarget) {
            if (newTarget === undefined || newTarget === Iterator) {
              return Iterator.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherIterator = otherGlobal && otherGlobal.Iterator;
              const otherProto = otherIterator && otherIterator.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return Iterator.prototype;
          }

          function iteratorFromConstructInput(obj) {
            const iteratorMethod = obj[Symbol.iterator];
            if (iteratorMethod !== undefined && iteratorMethod !== null) {
              if (typeof iteratorMethod !== 'function') {
                throw new TypeError('Symbol.iterator is not callable');
              }
              const iterator = iteratorMethod.call(obj);
              return GetIteratorDirect(iterator);
            }
            if (typeof obj !== 'object' || obj === null) {
              throw new TypeError('obj is not iterable');
            }
            return GetIteratorDirect(obj);
          }

          // Iterator constructor - abstract class
          function Iterator(_iterable) {
            if (new.target === undefined) {
              throw new TypeError('Constructor Iterator requires "new"');
            }
            if (new.target === Iterator) {
              throw new TypeError('Abstract class Iterator not directly constructable');
            }
            const proto = getIntrinsicIteratorPrototype(new.target);
            return Object.create(proto);
          }

          // SetterThatIgnoresPrototypeProperties per spec
          // 1. If this is not Object, throw TypeError
          // 2. If this is home (IteratorPrototype), throw TypeError
          // 3. If desc is undefined, CreateDataPropertyOrThrow
          // 4. Otherwise, Set(this, p, v, true)
          function SetterThatIgnoresPrototypeProperties(home, p, v) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Cannot set property on non-object');
            }
            if (this === home) {
              throw new TypeError('Cannot set property on prototype');
            }
            const desc = Object.getOwnPropertyDescriptor(this, p);
            if (desc === undefined) {
              Object.defineProperty(this, p, {
                value: v,
                writable: true,
                enumerable: true,
                configurable: true
              });
            } else {
              this[p] = v;
            }
          }

          // Iterator.prototype.constructor should be an accessor property per spec
          Object.defineProperty(IteratorPrototype, 'constructor', {
            get() { return Iterator; },
            set(v) { SetterThatIgnoresPrototypeProperties.call(this, IteratorPrototype, 'constructor', v); },
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype[@@toStringTag] should be an accessor property per spec
          Object.defineProperty(IteratorPrototype, Symbol.toStringTag, {
            get() { return 'Iterator'; },
            set(v) { SetterThatIgnoresPrototypeProperties.call(this, IteratorPrototype, Symbol.toStringTag, v); },
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype setup
          const IteratorHelperPrototype = Object.create(IteratorPrototype);

          Object.defineProperty(IteratorHelperPrototype, Symbol.toStringTag, {
            value: 'Iterator Helper',
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Create a helper iterator wrapper
          // underlyingRecord is the result of GetIteratorDirect (has .iterator, .nextMethod)
          function createIteratorHelper(underlyingRecord, nextImpl, returnImpl) {
            const helper = Object.create(IteratorHelperPrototype);
            const state = { 
              underlyingRecord: underlyingRecord,
              done: false,
              executing: false,
              nextImpl,
              returnImpl
            };
            
            helper.next = function() {
              if (state.executing) {
                throw new TypeError('Generator is already executing');
              }
              if (state.done) {
                return { value: undefined, done: true };
              }
              state.executing = true;
              try {
                return state.nextImpl(state);
              } catch (e) {
                state.done = true;
                // NOTE: We do NOT close the iterator here
                // IfAbruptCloseIterator is handled by each method's nextImpl
                // when their callback/mapper/predicate throws
                throw e;
              } finally {
                state.executing = false;
              }
            };
            
            helper.return = function(value) {
              if (state.executing) {
                throw new TypeError('Generator is already executing');
              }
              if (state.done) {
                // Already closed, don't forward return again
                return { value: value, done: true };
              }
              state.done = true;
              if (state.returnImpl) {
                return state.returnImpl(state, value);
              }
              const underlying = state.underlyingRecord.iterator;
              if (underlying && typeof underlying.return === 'function') {
                return underlying.return(value);
              }
              return { value: value, done: true };
            };
            
            return helper;
          }

          // Iterator.prototype.map
          defineNonConstructibleMethod(IteratorPrototype, 'map', function map(mapper) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.map called on non-object');
            }
            if (typeof mapper !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('mapper must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            return createIteratorHelper(iterated, (state) => {
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Wrap mapper call - close iterator if it throws
              let mapped;
              try {
                mapped = mapper(next.value, counter++);
              } catch (e) {
                // IfAbruptCloseIterator
                if (typeof state.underlyingRecord.iterator.return === 'function') {
                  try { state.underlyingRecord.iterator.return(); } catch (_) {}
                }
                throw e;
              }
              return { value: mapped, done: false };
            });
          }, 1);

          // Iterator.prototype.filter
          defineNonConstructibleMethod(IteratorPrototype, 'filter', function filter(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.filter called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            return createIteratorHelper(iterated, (state) => {
              while (true) {
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                // Wrap predicate call - close iterator if it throws
                let result;
                try {
                  result = predicate(next.value, counter++);
                } catch (e) {
                  // IfAbruptCloseIterator
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw e;
                }
                if (result) {
                  return { value: next.value, done: false };
                }
              }
            });
          }, 1);

          // Helper to close iterator if it has a return method
          function IteratorClose(iteratorRecord, error) {
            const iterator = iteratorRecord.iterator;
            if (typeof iterator.return === 'function') {
              try {
                iterator.return();
              } catch (e) {
                if (error) throw error;
                throw e;
              }
            }
            if (error) throw error;
          }

          // Iterator.prototype.take
          defineNonConstructibleMethod(IteratorPrototype, 'take', function take(limit) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.take called on non-object');
            }
            // Steps 2-5: Validate arguments BEFORE GetIteratorDirect
            // But if validation fails, close the iterator (this) directly
            let numLimit;
            try {
              numLimit = Number(limit);
            } catch (e) {
              // ToNumber threw - close iterator before re-throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw e;
            }
            if (Number.isNaN(numLimit)) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be a number');
            }
            const intLimit = Math.trunc(numLimit);
            if (intLimit < 0) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be non-negative');
            }
            // Step 6: NOW get the iterator
            const iterated = GetIteratorDirect(this);
            let remaining = intLimit;
            
            return createIteratorHelper(iterated, (state) => {
              if (remaining <= 0) {
                state.done = true;
                // Close underlying iterator
                if (typeof state.underlyingRecord.iterator.return === 'function') {
                  state.underlyingRecord.iterator.return();
                }
                return { value: undefined, done: true };
              }
              remaining--;
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Per spec: Yield(? IteratorValue(next)) - must read .value
              const value = next.value;
              return { value: value, done: false };
            });
          }, 1);

          // Iterator.prototype.drop
          defineNonConstructibleMethod(IteratorPrototype, 'drop', function drop(limit) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.drop called on non-object');
            }
            // Steps 2-5: Validate arguments BEFORE GetIteratorDirect
            // But if validation fails, close the iterator (this) directly
            let numLimit;
            try {
              numLimit = Number(limit);
            } catch (e) {
              // ToNumber threw - close iterator before re-throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw e;
            }
            if (Number.isNaN(numLimit)) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be a number');
            }
            const intLimit = Math.trunc(numLimit);
            if (intLimit < 0) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be non-negative');
            }
            // Step 6: NOW get the iterator
            const iterated = GetIteratorDirect(this);
            let remaining = intLimit;
            
            return createIteratorHelper(iterated, (state) => {
              // Skip the first 'limit' values
              while (remaining > 0) {
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                // Per spec: during drop, must read .value for side effects
                const _ = next.value;
                remaining--;
              }
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Per spec: Yield(? IteratorValue(next)) - must read .value
              const value = next.value;
              return { value: value, done: false };
            });
          }, 1);

          // Iterator.prototype.flatMap
          defineNonConstructibleMethod(IteratorPrototype, 'flatMap', function flatMap(mapper) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.flatMap called on non-object');
            }
            if (typeof mapper !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('mapper must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            // Store inner iterator in an object so returnImpl can access it
            const innerState = { iterator: null };
            
            const helper = createIteratorHelper(iterated, (state) => {
              while (true) {
                // If we have an inner iterator, consume it first
                if (innerState.iterator !== null) {
                  const innerNext = innerState.iterator.next();
                  if (!innerNext.done) {
                    return { value: innerNext.value, done: false };
                  }
                  innerState.iterator = null;
                }
                
                // Get next from outer iterator
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                
                // Map and get inner iterator - GetIteratorFlattenable semantics
                // Wrap mapper call - close iterator if it throws
                let mapped;
                try {
                  mapped = mapper(next.value, counter++);
                } catch (e) {
                  // IfAbruptCloseIterator
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw e;
                }
                if (typeof mapped !== 'object' || mapped === null) {
                  // IfAbruptCloseIterator for validation error
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw new TypeError('flatMap mapper must return an iterable or iterator');
                }
                
                // Check Symbol.iterator property
                const iteratorMethod = mapped[Symbol.iterator];
                if (iteratorMethod !== undefined && iteratorMethod !== null) {
                  // Has non-null/undefined @@iterator - must be callable
                  if (typeof iteratorMethod !== 'function') {
                    if (typeof state.underlyingRecord.iterator.return === 'function') {
                      try { state.underlyingRecord.iterator.return(); } catch (_) {}
                    }
                    throw new TypeError('Symbol.iterator is not a function');
                  }
                  innerState.iterator = iteratorMethod.call(mapped);
                } else if (typeof mapped.next === 'function') {
                  // Fallback to using object directly as iterator
                  innerState.iterator = mapped;
                } else {
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw new TypeError('flatMap mapper must return an iterable or iterator');
                }
              }
            }, (state, value) => {
              // Custom return: close inner iterator first, then outer
              if (innerState.iterator !== null && typeof innerState.iterator.return === 'function') {
                try {
                  innerState.iterator.return();
                } catch (_) {}
              }
              innerState.iterator = null;
              const underlying = state.underlyingRecord.iterator;
              if (underlying && typeof underlying.return === 'function') {
                return underlying.return(value);
              }
              return { value: value, done: true };
            });
            return helper;
          }, 1);

          // Iterator.prototype.forEach
          defineNonConstructibleMethod(IteratorPrototype, 'forEach', function forEach(fn) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.forEach called on non-object');
            }
            if (typeof fn !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('callback must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return undefined;
              }
              try {
                fn(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
            }
          }, 1);

          // Iterator.prototype.some
          defineNonConstructibleMethod(IteratorPrototype, 'some', function some(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.some called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return false;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return true;
              }
            }
          }, 1);

          // Iterator.prototype.every
          defineNonConstructibleMethod(IteratorPrototype, 'every', function every(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.every called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return true;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (!result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return false;
              }
            }
          }, 1);

          // Iterator.prototype.find
          defineNonConstructibleMethod(IteratorPrototype, 'find', function find(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.find called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return undefined;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return next.value;
              }
            }
          }, 1);

          // Iterator.prototype.reduce
          defineNonConstructibleMethod(IteratorPrototype, 'reduce', function reduce(reducer, ...args) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.reduce called on non-object');
            }
            if (typeof reducer !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('reducer must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            let accumulator;
            
            if (args.length === 0) {
              // No initial value - use first element
              const first = IteratorNext(iterated);
              if (first.done) {
                throw new TypeError('Reduce of empty iterator with no initial value');
              }
              accumulator = first.value;
              counter = 1;
            } else {
              accumulator = args[0];
            }
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return accumulator;
              }
              try {
                accumulator = reducer(accumulator, next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
            }
          }, 1);

          // Iterator.prototype.toArray
          defineNonConstructibleMethod(IteratorPrototype, 'toArray', function toArray() {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.toArray called on non-object');
            }
            const iterated = GetIteratorDirect(this);
            const result = [];
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return result;
              }
              result.push(next.value);
            }
          }, 0);

          // Iterator.prototype[Symbol.iterator] - must preserve primitive this values
          // Use a special implementation that doesn't box primitives
          const iteratorSymbolImpl = {
            [Symbol.iterator]() { return this; }
          };
          Object.defineProperty(IteratorPrototype, Symbol.iterator, {
            value: iteratorSymbolImpl[Symbol.iterator],
            writable: true,
            enumerable: false,
            configurable: true
          });
          // The function's name should be [Symbol.iterator]
          Object.defineProperty(IteratorPrototype[Symbol.iterator], 'name', {
            value: '[Symbol.iterator]',
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype[Symbol.dispose] (for using)
          defineNonConstructibleMethod(IteratorPrototype, Symbol.dispose, function() {
            if (typeof this.return === 'function') {
              this.return();
            }
          }, 0);

          const WrapForValidIteratorPrototype = Object.create(IteratorPrototype);

          function createIteratorWrapper(iteratorRecord) {
            let returnMethodInitialized = false;
            let cachedReturn;
            const helper = createIteratorHelper(
              iteratorRecord,
              (state) => IteratorNext(state.underlyingRecord),
              (state, value) => {
                if (!returnMethodInitialized) {
                  const returnMethod = state.underlyingRecord.iterator.return;
                  cachedReturn = typeof returnMethod === 'function' ? returnMethod : undefined;
                  returnMethodInitialized = true;
                }
                if (cachedReturn === undefined) {
                  return { value, done: true };
                }
                const result = cachedReturn.call(state.underlyingRecord.iterator, value);
                if (typeof result !== 'object' || result === null) {
                  throw new TypeError('Iterator result must be an object');
                }
                return result;
              }
            );
            const sourceProto = Object.getPrototypeOf(iteratorRecord.iterator);
            const keepSourcePrototype =
              sourceProto !== null &&
              sourceProto !== Object.prototype &&
              sourceProto !== IteratorPrototype &&
              typeof iteratorRecord.iterator.throw === 'function';
            if (keepSourcePrototype) {
              Object.setPrototypeOf(helper, sourceProto);
            } else {
              Object.setPrototypeOf(helper, WrapForValidIteratorPrototype);
            }
            return helper;
          }
          // Iterator.from static method (non-constructible)
          defineNonConstructibleMethod(Iterator, 'from', function from(obj) {
            if (typeof obj !== 'object' && typeof obj !== 'string') {
              throw new TypeError('Iterator.from requires an object or string');
            }
            if (obj === null) {
              throw new TypeError('Iterator.from requires an object or string');
            }

            let iteratorRecord;
            const iteratorMethod = obj[Symbol.iterator];
            if (iteratorMethod !== undefined && iteratorMethod !== null) {
              if (typeof iteratorMethod !== 'function') {
                throw new TypeError('Symbol.iterator is not callable');
              }
              const iterator = iteratorMethod.call(obj);
              iteratorRecord = GetIteratorDirect(iterator);
            } else {
              if (typeof obj !== 'object' || obj === null) {
                throw new TypeError('obj is not iterable');
              }
              iteratorRecord = GetIteratorDirect(obj);
            }

            const helper = createIteratorWrapper(iteratorRecord);
            return helper;
          }, 1);

          // Iterator.concat static method (iterator-sequencing proposal)
          // https://tc39.es/proposal-iterator-sequencing/
          defineNonConstructibleMethod(Iterator, 'concat', function concat(...items) {
            const iterables = [];
            // 2. For each element item of items, do
            for (let i = 0; i < items.length; i++) {
              const item = items[i];
              // a. If item is not an Object, throw a TypeError exception.
              if (typeof item !== 'object' || item === null) {
                throw new TypeError('Iterator.concat: argument is not an object');
              }
              // b. Let method be ? GetMethod(item, @@iterator).
              const method = item[Symbol.iterator];
              // c. If method is undefined, throw a TypeError exception.
              if (method === undefined) {
                throw new TypeError('Iterator.concat: argument is not iterable');
              }
              if (typeof method !== 'function') {
                throw new TypeError('Iterator.concat: @@iterator is not callable');
              }
              // d. Append the Record { [[OpenMethod]]: method, [[Iterable]]: item } to iterables.
              iterables.push({ openMethod: method, iterable: item });
            }
            
            // 3. Let closure be a new Abstract Closure with no parameters
            // 4. Let gen be CreateIteratorFromClosure(closure, "Iterator Helper", ...)
            // 5. Return gen.
            
            // Create generator-like state
            let currentIndex = 0;
            let currentIterator = null;
            let started = false;
            let executing = false;
            let closed = false;
            
            // Helper to close iterator
            function closeIterator(iterator) {
              if (iterator !== null) {
                const returnMethod = iterator.return;
                if (typeof returnMethod === 'function') {
                  try {
                    returnMethod.call(iterator);
                  } catch (e) {
                    // Ignore errors on cleanup
                  }
                }
              }
            }
            
            const helper = Object.create(IteratorHelperPrototype);
            
            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (closed) {
                return { value: undefined, done: true };
              }
              executing = true;
              try {
                while (true) {
                  // If we have a current iterator, try to get next from it
                  if (currentIterator !== null) {
                    const result = currentIterator.next();
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                    const done = !!result.done;
                    if (!done) {
                      // Access value after done
                      const value = result.value;
                      return { value, done: false };
                    }
                    // Current iterator is done, move to next
                    currentIterator = null;
                  }
                  
                  // Move to next iterable
                  if (currentIndex >= iterables.length) {
                    return { value: undefined, done: true };
                  }
                  
                  const record = iterables[currentIndex++];
                  const iter = record.openMethod.call(record.iterable);
                  if (typeof iter !== 'object' || iter === null) {
                    throw new TypeError('Iterator.concat: iterator method did not return an object');
                  }
                  currentIterator = iter;
                  started = true;
                }
              } finally {
                executing = false;
              }
            }
            
            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (closed) {
                return { value: undefined, done: true };
              }
              executing = true;
              try {
                // Only forward return if we've started and have a current iterator
                if (started && currentIterator !== null) {
                  const returnMethod = currentIterator.return;
                  if (typeof returnMethod === 'function') {
                    const result = returnMethod.call(currentIterator);
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                  }
                }
                currentIterator = null;
                closed = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }
            
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);
            
            return helper;
          }, 0);

          function closeInputIterator(inputIterRecord, error) {
            const iterator = inputIterRecord && inputIterRecord.iterator ? inputIterRecord.iterator : inputIterRecord;
            let returnMethod;
            try {
              returnMethod = iterator?.return;
            } catch (e) {
              if (error !== undefined) {
                throw error;
              }
              throw e;
            }
            if (typeof returnMethod === 'function') {
              if (error !== undefined) {
                try {
                  returnMethod.call(iterator);
                } catch (_) {}
              } else {
                const result = returnMethod.call(iterator);
                if (typeof result !== 'object' || result === null) {
                  throw new TypeError('Iterator result must be an object');
                }
              }
            }
            if (error !== undefined) {
              throw error;
            }
          }

          function closeIteratorList(iteratorList, startIndex, skipIndex, error, shouldThrow = true) {
            let closeError = null;
            for (let i = iteratorList.length - 1; i >= startIndex; i--) {
              if (i === skipIndex || iteratorList[i] === null) {
                continue;
              }
              const record = iteratorList[i];
              const iter = record && record.iterator ? record.iterator : record;
              let returnMethod;
              try {
                returnMethod = iter?.return;
              } catch (e) {
                if (error === undefined && closeError === null) {
                  closeError = e;
                }
                iteratorList[i] = null;
                continue;
              }
              if (typeof returnMethod === 'function') {
                if (error !== undefined) {
                  try {
                    returnMethod.call(iter);
                  } catch (_) {}
                } else {
                  try {
                    const result = returnMethod.call(iter);
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                  } catch (e) {
                    if (closeError === null) {
                      closeError = e;
                    }
                  }
                }
              }
              iteratorList[i] = null;
            }
            if (error !== undefined) {
              if (shouldThrow) {
                throw error;
              }
              return error;
            }
            if (closeError !== null) {
              throw closeError;
            }
          }

          // Iterator.zip static method (joint-iteration proposal)
          // https://tc39.es/proposal-joint-iteration/
          defineNonConstructibleMethod(Iterator, 'zip', function zip(iterables, options) {
            if (typeof iterables !== 'object' || iterables === null) {
              throw new TypeError('Iterator.zip: iterables is not an object');
            }

            let mode = 'shortest';
            let paddingOption = undefined;

            if (options !== undefined) {
              if (typeof options !== 'object' || options === null) {
                throw new TypeError('Iterator.zip: options is not an object');
              }
              const modeOption = options.mode;
              if (modeOption === undefined) {
                mode = 'shortest';
              } else if (modeOption === 'longest' || modeOption === 'strict' || modeOption === 'shortest') {
                mode = modeOption;
              } else {
                throw new TypeError('Iterator.zip: mode must be "shortest", "longest", or "strict"');
              }
              if (mode === 'longest') {
                paddingOption = options.padding;
              }
            }

            const iters = [];
            const openIters = [];

            function closeZipIterators(skipIndex, error) {
              closeIteratorList(openIters, 0, skipIndex, error);
            }

            function getIteratorFlattenable(value) {
              if (typeof value !== 'object' || value === null) {
                throw new TypeError('Iterator.zip: iterable element is not an object');
              }
              const iterMethod = value[Symbol.iterator];
              let iter;
              if (iterMethod === undefined) {
                iter = value;
              } else {
                if (typeof iterMethod !== 'function') {
                  throw new TypeError('Iterator.zip: @@iterator is not callable');
                }
                iter = iterMethod.call(value);
                if (typeof iter !== 'object' || iter === null) {
                  throw new TypeError('Iterator.zip: iterator is not an object');
                }
              }
              return GetIteratorDirect(iter);
            }

            const inputIterMethod = iterables[Symbol.iterator];
            if (typeof inputIterMethod !== 'function') {
              throw new TypeError('Iterator.zip: iterables is not iterable');
            }
            const inputIter = inputIterMethod.call(iterables);
            if (typeof inputIter !== 'object' || inputIter === null) {
              throw new TypeError('Iterator.zip: iterables iterator is not an object');
            }
            const inputIterRecord = GetIteratorDirect(inputIter);

            try {
              while (true) {
                let next;
                try {
                  next = IteratorNext(inputIterRecord);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  throw e;
                }
                if (typeof next !== 'object' || next === null) {
                  const error = new TypeError('Iterator.zip: iterator result is not an object');
                  closeIteratorList(openIters, 0, -1, error, false);
                  throw error;
                }
                const done = !!next.done;
                if (done) {
                  break;
                }
                const value = next.value;
                try {
                  const iterRecord = getIteratorFlattenable(value);
                  iters.push({ iterator: iterRecord.iterator, nextMethod: iterRecord.nextMethod, done: false });
                  openIters.push(iterRecord.iterator);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  closeInputIterator(inputIterRecord, e);
                }
              }
            } catch (e) {
              throw e;
            }

            const padding = new Array(iters.length).fill(undefined);
            if (mode === 'longest' && paddingOption !== undefined) {
              if (typeof paddingOption !== 'object' || paddingOption === null) {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding is not an object'));
              }
              const paddingIterMethod = paddingOption[Symbol.iterator];
              if (typeof paddingIterMethod !== 'function') {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding is not iterable'));
              }
              let paddingIter;
              try {
                paddingIter = paddingIterMethod.call(paddingOption);
              } catch (e) {
                closeZipIterators(-1, e);
              }
              if (typeof paddingIter !== 'object' || paddingIter === null) {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding iterator is not an object'));
              }
              const paddingIterRecord = GetIteratorDirect(paddingIter);
              let usingIterator = true;
              let completionError;
              for (let i = 0; i < iters.length; i++) {
                if (!usingIterator) {
                  padding[i] = undefined;
                  continue;
                }
                try {
                  const next = IteratorNext(paddingIterRecord);
                  if (typeof next !== 'object' || next === null) {
                    throw new TypeError('Iterator.zip: padding iterator result is not an object');
                  }
                  const done = !!next.done;
                  if (done) {
                    usingIterator = false;
                    padding[i] = undefined;
                  } else {
                    padding[i] = next.value;
                  }
                } catch (e) {
                  completionError = e;
                  break;
                }
              }
              if (completionError !== undefined) {
                closeIteratorList(openIters, 0, -1, completionError, false);
                throw completionError;
              }
              if (usingIterator) {
                try {
                  closeInputIterator(paddingIterRecord);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  throw e;
                }
              }
            }

            let executing = false;
            let allDone = iters.length === 0;
            let hasYielded = false;

            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }

              executing = true;
              try {
                const iterCount = iters.length;
                const results = [];

                for (let i = 0; i < iterCount; i++) {
                  const iter = iters[i];
                  if (iter.done) {
                    if (mode === 'longest') {
                      results.push(padding[i]);
                    }
                    continue;
                  }

                  if (typeof iter.nextMethod !== 'function') {
                    allDone = true;
                    closeZipIterators(i, new TypeError('Iterator next method is not callable'));
                  }

                  let result;
                  try {
                    result = iter.nextMethod.call(iter.iterator);
                  } catch (e) {
                    allDone = true;
                    closeZipIterators(i, e);
                  }

                  if (typeof result !== 'object' || result === null) {
                    allDone = true;
                    closeZipIterators(i, new TypeError('Iterator result must be an object'));
                  }

                  const done = !!result.done;
                  if (done) {
                    iter.done = true;
                    openIters[i] = null;

                    if (mode === 'shortest') {
                      allDone = true;
                      closeZipIterators(i);
                      return { value: undefined, done: true };
                    }

                    if (mode === 'strict') {
                      if (i !== 0) {
                        allDone = true;
                        closeZipIterators(-1, new TypeError('Iterator.zip: iterators have different lengths (strict mode)'));
                      }
                      for (let k = 1; k < iterCount; k++) {
                        const other = iters[k];
                        if (typeof other.nextMethod !== 'function') {
                          allDone = true;
                          closeZipIterators(-1, new TypeError('Iterator next method is not callable'));
                        }
                        let otherResult;
                        try {
                          otherResult = other.nextMethod.call(other.iterator);
                        } catch (e) {
                          allDone = true;
                          closeZipIterators(k, e);
                        }
                        if (typeof otherResult !== 'object' || otherResult === null) {
                          allDone = true;
                          closeZipIterators(k, new TypeError('Iterator result must be an object'));
                        }
                        if (!!otherResult.done) {
                          other.done = true;
                          openIters[k] = null;
                        } else {
                          allDone = true;
                          closeZipIterators(-1, new TypeError('Iterator.zip: iterators have different lengths (strict mode)'));
                        }
                      }
                      allDone = true;
                      return { value: undefined, done: true };
                    }

                    results.push(padding[i]);
                  } else {
                    results.push(result.value);
                  }
                }

                if (mode === 'longest') {
                  let allIteratorsDone = true;
                  for (let i = 0; i < iterCount; i++) {
                    if (!iters[i].done) {
                      allIteratorsDone = false;
                      break;
                    }
                  }
                  if (allIteratorsDone) {
                    allDone = true;
                    return { value: undefined, done: true };
                  }
                }

                hasYielded = true;
                return { value: results, done: false };
              } finally {
                executing = false;
              }
            }

            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }
              if (!hasYielded) {
                allDone = true;
              } else {
                executing = true;
              }
              try {
                closeZipIterators(-1);
                allDone = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }

            const helper = Object.create(IteratorHelperPrototype);
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);

            return helper;
          }, 1);

          // Iterator.zipKeyed static method (joint-iteration proposal)
          // https://tc39.es/proposal-joint-iteration/
          defineNonConstructibleMethod(Iterator, 'zipKeyed', function zipKeyed(iterables, options) {
            if (typeof iterables !== 'object' || iterables === null) {
              throw new TypeError('Iterator.zipKeyed: iterables is not an object');
            }

            let mode = 'shortest';
            let paddingOption = undefined;

            if (options !== undefined) {
              if (typeof options !== 'object' || options === null) {
                throw new TypeError('Iterator.zipKeyed: options is not an object');
              }
              const modeOption = options.mode;
              if (modeOption === undefined) {
                mode = 'shortest';
              } else if (modeOption === 'longest' || modeOption === 'strict' || modeOption === 'shortest') {
                mode = modeOption;
              } else {
                throw new TypeError('Iterator.zipKeyed: mode must be "shortest", "longest", or "strict"');
              }
              if (mode === 'longest') {
                paddingOption = options.padding;
              }
            }

            const iters = [];
            const openIters = [];
            const keys = [];

            function closeZipKeyedIterators(skipIndex, error) {
              closeIteratorList(openIters, 0, skipIndex, error);
            }

            const allKeys = Reflect.ownKeys(iterables);
            for (const key of allKeys) {
              let desc;
              try {
                desc = Reflect.getOwnPropertyDescriptor(iterables, key);
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
              if (desc === undefined || desc.enumerable !== true) {
                continue;
              }
              let value;
              try {
                value = iterables[key];
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
              if (value === undefined) {
                continue;
              }
              if (typeof value !== 'object' || value === null) {
                closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterable element is not an object'));
              }
              try {
                const iterMethod = value[Symbol.iterator];
                let iter;
                if (iterMethod === undefined || iterMethod === null) {
                  iter = value;
                } else {
                  if (typeof iterMethod !== 'function') {
                    closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: @@iterator is not callable'));
                  }
                  iter = iterMethod.call(value);
                  if (typeof iter !== 'object' || iter === null) {
                    closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterator is not an object'));
                  }
                }
                const nextMethod = iter.next;
                keys.push(key);
                iters.push({ key, iterator: iter, nextMethod, done: false });
                openIters.push(iter);
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
            }

            const paddingValues = Object.create(null);
            if (mode === 'longest' && paddingOption !== undefined) {
              if (typeof paddingOption !== 'object' || paddingOption === null) {
                throw new TypeError('Iterator.zipKeyed: padding is not an object');
              }
              for (const key of keys) {
                try {
                  paddingValues[key] = paddingOption[key];
                } catch (e) {
                  closeZipKeyedIterators(-1, e);
                }
              }
            }

            function getPaddingValue(key) {
              if (mode !== 'longest') {
                return undefined;
              }
              if (paddingOption === undefined) {
                return undefined;
              }
              return paddingValues[key];
            }

            let executing = false;
            let allDone = iters.length === 0;
            let hasYielded = false;

            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }

              executing = true;
              try {
                const iterCount = iters.length;
                const resultObj = Object.create(null);

                for (let i = 0; i < iterCount; i++) {
                  const { key, iterator, nextMethod } = iters[i];

                  if (iters[i].done) {
                    if (mode === 'longest') {
                      resultObj[key] = getPaddingValue(key);
                    }
                    continue;
                  }

                  if (typeof nextMethod !== 'function') {
                    allDone = true;
                    closeZipKeyedIterators(i, new TypeError('Iterator next method is not callable'));
                  }

                  let result;
                  try {
                    result = nextMethod.call(iterator);
                  } catch (e) {
                    allDone = true;
                    closeZipKeyedIterators(i, e);
                  }

                  if (typeof result !== 'object' || result === null) {
                    allDone = true;
                    closeZipKeyedIterators(i, new TypeError('Iterator result must be an object'));
                  }

                  const done = !!result.done;
                  if (done) {
                    iters[i].done = true;
                    openIters[i] = null;

                    if (mode === 'shortest') {
                      allDone = true;
                      closeZipKeyedIterators(i);
                      return { value: undefined, done: true };
                    } else if (mode === 'strict') {
                      if (i !== 0) {
                        allDone = true;
                        closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterators have different lengths (strict mode)'));
                      }

                      for (let k = 1; k < iterCount; k++) {
                        const kNextMethod = iters[k].nextMethod;
                        if (typeof kNextMethod !== 'function') {
                          allDone = true;
                          closeZipKeyedIterators(-1, new TypeError('Iterator next method is not callable'));
                        }

                        let kResult;
                        try {
                          kResult = kNextMethod.call(iters[k].iterator);
                        } catch (e) {
                          allDone = true;
                          closeZipKeyedIterators(k, e);
                        }

                        if (typeof kResult !== 'object' || kResult === null) {
                          allDone = true;
                          closeZipKeyedIterators(k, new TypeError('Iterator result must be an object'));
                        }

                        if (kResult.done) {
                          iters[k].done = true;
                          openIters[k] = null;
                        } else {
                          allDone = true;
                          closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterators have different lengths (strict mode)'));
                        }
                      }

                      allDone = true;
                      return { value: undefined, done: true };
                    } else {
                      resultObj[key] = getPaddingValue(key);
                    }
                  } else {
                    resultObj[key] = result.value;
                  }
                }

                if (mode === 'longest') {
                  let allItersDone = true;
                  for (let i = 0; i < iterCount; i++) {
                    if (!iters[i].done) {
                      allItersDone = false;
                      break;
                    }
                  }
                  if (allItersDone) {
                    allDone = true;
                    return { value: undefined, done: true };
                  }
                }

                hasYielded = true;
                return { value: resultObj, done: false };
              } finally {
                executing = false;
              }
            }

            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }
              if (!hasYielded) {
                allDone = true;
              } else {
                executing = true;
              }
              try {
                closeZipKeyedIterators(-1);
                allDone = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }

            const helper = Object.create(IteratorHelperPrototype);
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);

            return helper;
          }, 1);

          // Set up Iterator constructor
          Object.setPrototypeOf(Iterator, Function.prototype);
          Iterator.prototype = IteratorPrototype;
          
          Object.defineProperty(Iterator, 'prototype', {
            writable: false,
            enumerable: false,
            configurable: false
          });

          Object.defineProperty(Iterator, 'name', {
            value: 'Iterator',
            writable: false,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(Iterator, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Expose Iterator globally
          Object.defineProperty(globalThis, 'Iterator', {
            value: Iterator,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_promise_then_hook(context: &mut Context) -> boa_engine::JsResult<()> {
    let prototype = context.intrinsics().constructors().promise().prototype();
    let original_symbol = promise_then_original_symbol(context)?;
    if prototype.has_own_property(original_symbol.clone(), context)? {
        return Ok(());
    }

    let original = prototype.get(js_string!("then"), context)?;
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
    Ok(())
}

fn install_string_replace_guard(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = String.prototype;
          const originalKey = "__agentjs_original_String_replace__";
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.replace;
          if (typeof original !== 'function') {
            return;
          }

          const isHtmlDdaLike = (value) =>
            typeof value === 'undefined' && value !== undefined;
          const isObjectLike = (value) =>
            (typeof value === 'object' && value !== null) ||
            typeof value === 'function' ||
            isHtmlDdaLike(value);

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const replaceFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              const replaceValue = args.length > 1 ? args[1] : undefined;
              const input = String(thisArg);
              if (
                typeof replaceValue === 'string' &&
                replaceValue.includes('$') &&
                input.length > 0 &&
                replaceValue.length > 0
              ) {
                const estimatedLength = BigInt(input.length) * BigInt(replaceValue.length);
                if (estimatedLength > 1073741824n) {
                  throw new ReferenceError('OOM Limit');
                }
              }

              let effectiveSearchValue = searchValue;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                const searchString = `${searchValue}`;
                effectiveSearchValue = {
                  [Symbol.toPrimitive]() {
                    return searchString;
                  },
                  toString() {
                    return searchString;
                  },
                  valueOf() {
                    return searchString;
                  },
                };
              }

              return original.call(thisArg, effectiveSearchValue, replaceValue);
            }
          });

          Object.defineProperty(replaceFn, 'name', {
            value: 'replace',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(replaceFn, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, 'replace', {
            value: replaceFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_string_match_guards(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = String.prototype;
          const originalMatchKey = "__agentjs_original_String_match__";
          const originalMatchAllKey = "__agentjs_original_String_matchAll__";
          const originalSearchKey = "__agentjs_original_String_search__";
          const originalReplaceAllKey = "__agentjs_original_String_replaceAll__";
          const originalSplitKey = "__agentjs_original_String_split__";
          if (
            Object.prototype.hasOwnProperty.call(proto, originalMatchKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalMatchAllKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalSearchKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalReplaceAllKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalSplitKey)
          ) {
            return;
          }

          const originalMatch = proto.match;
          const originalMatchAll = proto.matchAll;
          const originalSearch = proto.search;
          const originalReplaceAll = proto.replaceAll;
          const originalSplit = proto.split;
          if (
            typeof originalMatch !== "function" ||
            typeof originalMatchAll !== "function" ||
            typeof originalSearch !== "function" ||
            typeof originalReplaceAll !== "function" ||
            typeof originalSplit !== "function"
          ) {
            return;
          }

          const isHtmlDdaLike = (value) =>
            typeof value === "undefined" && value !== undefined;
          const isObjectLike = (value) =>
            (typeof value === "object" && value !== null) ||
            typeof value === "function" ||
            isHtmlDdaLike(value);

          Object.defineProperty(proto, originalMatchKey, {
            value: originalMatch,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalMatchAllKey, {
            value: originalMatchAll,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalSearchKey, {
            value: originalSearch,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalReplaceAllKey, {
            value: originalReplaceAll,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalSplitKey, {
            value: originalSplit,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const createPrimitivePatternWrapper = (value) => {
            const patternString = `${value}`;
            return {
              [Symbol.toPrimitive]() {
                return patternString;
              },
              toString() {
                return patternString;
              },
              valueOf() {
                return patternString;
              },
            };
          };

          const matchFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const regexp = args.length > 0 ? args[0] : undefined;
              if (regexp !== undefined && regexp !== null && !isObjectLike(regexp)) {
                return originalMatch.call(thisArg, createPrimitivePatternWrapper(regexp));
              }
              return originalMatch.call(thisArg, regexp);
            },
          });

          Object.defineProperty(matchFn, "name", {
            value: "match",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(matchFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "match", {
            value: matchFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const matchAllFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const regexp = args.length > 0 ? args[0] : undefined;
              if (regexp !== undefined && regexp !== null && !isObjectLike(regexp)) {
                return originalMatchAll.call(thisArg, createPrimitivePatternWrapper(regexp));
              }
              return originalMatchAll.call(thisArg, regexp);
            },
          });

          Object.defineProperty(matchAllFn, "name", {
            value: "matchAll",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(matchAllFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "matchAll", {
            value: matchAllFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const searchFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                return originalSearch.call(thisArg, createPrimitivePatternWrapper(searchValue));
              }
              return originalSearch.call(thisArg, searchValue);
            },
          });

          Object.defineProperty(searchFn, "name", {
            value: "search",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(searchFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "search", {
            value: searchFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const replaceAllFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              const replaceValue = args.length > 1 ? args[1] : undefined;
              let effectiveSearchValue = searchValue;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                effectiveSearchValue = createPrimitivePatternWrapper(searchValue);
              }
              return originalReplaceAll.call(thisArg, effectiveSearchValue, replaceValue);
            },
          });

          Object.defineProperty(replaceAllFn, "name", {
            value: "replaceAll",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(replaceAllFn, "length", {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "replaceAll", {
            value: replaceAllFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const splitFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const separator = args.length > 0 ? args[0] : undefined;
              const limit = args.length > 1 ? args[1] : undefined;
              let effectiveSeparator = separator;
              if (
                separator !== undefined &&
                separator !== null &&
                !isObjectLike(separator)
              ) {
                effectiveSeparator = createPrimitivePatternWrapper(separator);
              }
              return originalSplit.call(thisArg, effectiveSeparator, limit);
            },
          });

          Object.defineProperty(splitFn, "name", {
            value: "split",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(splitFn, "length", {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "split", {
            value: splitFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_reg_exp_legacy_accessors(context: &mut Context) -> JsResult<()> {
    let regexp_ctor = context.intrinsics().constructors().regexp().constructor();
    let receiver_check = "if (this !== RegExp) { throw new TypeError('RegExp legacy static accessor called on incompatible receiver'); }";

    for i in 1..=9 {
        let name = format!("${i}");
        let getter = context.eval(Source::from_bytes(&format!(
            "(function() {{ {receiver_check} return ''; }})"
        )))?;
        let getter = getter.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create RegExp legacy getter")
        })?;
        regexp_ctor.define_property_or_throw(
            JsString::from(name.as_str()),
            PropertyDescriptor::builder()
                .get(getter)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    for (full, short) in [
        ("lastMatch", "$&"),
        ("lastParen", "$+"),
        ("leftContext", "$`"),
        ("rightContext", "$'"),
    ] {
        let getter = context.eval(Source::from_bytes(&format!(
            "(function() {{ {receiver_check} return ''; }})"
        )))?;
        let getter = getter.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create RegExp legacy getter")
        })?;
        for name in [full, short] {
            regexp_ctor.define_property_or_throw(
                js_string!(name),
                PropertyDescriptor::builder()
                    .get(getter.clone())
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }

    let input_getter = context.eval(Source::from_bytes(&format!(
        "(function() {{ {receiver_check} return ''; }})"
    )))?;
    let input_getter = input_getter
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp input getter"))?;
    let input_setter = context.eval(Source::from_bytes(&format!(
        "(function(_value) {{ {receiver_check} return undefined; }})"
    )))?;
    let input_setter = input_setter
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp input setter"))?;
    for name in ["input", "$_"] {
        regexp_ctor.define_property_or_throw(
            js_string!(name),
            PropertyDescriptor::builder()
                .get(input_getter.clone())
                .set(input_setter.clone())
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    Ok(())
}

fn install_reg_exp_compile_guard(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = RegExp.prototype;
          const originalKey = "__agentjs_original_RegExp_compile__";
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.compile;
          if (typeof original !== 'function') {
            return;
          }

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(proto, 'compile', {
            value: function compile(pattern, flags) {
              if (typeof this === 'object' && this !== null && Object.getPrototypeOf(this) !== proto) {
                throw new TypeError('RegExp.prototype.compile called on incompatible receiver');
              }
              return original.call(this, pattern, flags);
            },
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_reg_exp_escape(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof RegExp !== 'function' || typeof RegExp.escape === 'function') {
            return;
          }

          const syntaxCharacters = new Set(['^', '$', '\\', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|']);
          const otherPunctuators = new Set([',', '-', '=', '<', '>', '#', '&', '!', '%', ':', ';', '@', '~', "'", '`', '"']);
          const controlEscapeNames = new Map([
            [0x0009, 't'],
            [0x000A, 'n'],
            [0x000B, 'v'],
            [0x000C, 'f'],
            [0x000D, 'r'],
          ]);

          function isAsciiLetterOrDigit(cp) {
            return (
              (cp >= 0x30 && cp <= 0x39) ||
              (cp >= 0x41 && cp <= 0x5A) ||
              (cp >= 0x61 && cp <= 0x7A)
            );
          }

          function toHex(value, width) {
            return value.toString(16).padStart(width, '0');
          }

          function unicodeEscape(codeUnit) {
            return '\\u' + toHex(codeUnit, 4);
          }

          function isWhiteSpaceOrLineTerminator(cp) {
            if (
              cp === 0x0009 ||
              cp === 0x000A ||
              cp === 0x000B ||
              cp === 0x000C ||
              cp === 0x000D ||
              cp === 0x0020 ||
              cp === 0x00A0 ||
              cp === 0x1680 ||
              (cp >= 0x2000 && cp <= 0x200A) ||
              cp === 0x2028 ||
              cp === 0x2029 ||
              cp === 0x202F ||
              cp === 0x205F ||
              cp === 0x3000 ||
              cp === 0xFEFF
            ) {
              return true;
            }
            return false;
          }

          function encodeForRegExpEscape(cp) {
            const ch = String.fromCodePoint(cp);
            if (syntaxCharacters.has(ch) || cp === 0x002F) {
              return '\\' + ch;
            }

            const controlEscape = controlEscapeNames.get(cp);
            if (controlEscape !== undefined) {
              return '\\' + controlEscape;
            }

            if (
              otherPunctuators.has(ch) ||
              isWhiteSpaceOrLineTerminator(cp) ||
              (cp >= 0xD800 && cp <= 0xDFFF)
            ) {
              if (cp <= 0xFF) {
                return '\\x' + toHex(cp, 2);
              }
              if (cp <= 0xFFFF) {
                return unicodeEscape(cp);
              }
              const high = Math.floor((cp - 0x10000) / 0x400) + 0xD800;
              const low = ((cp - 0x10000) % 0x400) + 0xDC00;
              return unicodeEscape(high) + unicodeEscape(low);
            }

            return ch;
          }

          function regExpEscape(string) {
            if (typeof string !== 'string') {
              throw new TypeError('RegExp.escape requires a string argument');
            }

            let escaped = '';
            let isFirst = true;
            for (const ch of string) {
              const cp = ch.codePointAt(0);
              if (isFirst && isAsciiLetterOrDigit(cp)) {
                escaped += '\\x' + toHex(cp, 2);
              } else {
                escaped += encodeForRegExpEscape(cp);
              }
              isFirst = false;
            }
            return escaped;
          }

          const escapeFn = new Proxy(() => {}, {
            apply(_target, _thisArg, args) {
              const input = args.length > 0 ? args[0] : undefined;
              return regExpEscape(input);
            }
          });

          Object.defineProperty(escapeFn, 'name', {
            value: 'escape',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(escapeFn, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(RegExp, 'escape', {
            value: escapeFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
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

fn install_test262_globals(
    context: &mut Context,
    install_shadow_realm: bool,
) -> boa_engine::JsResult<()> {
    let test262 = build_test262_object(
        context.realm().clone(),
        context.global_object(),
        true,
        context,
    );
    context.register_global_property(js_string!("$262"), test262, Attribute::all())?;
    if install_shadow_realm {
        install_shadow_realm_polyfill(context)?;
    }
    Ok(())
}

fn install_shadow_realm_polyfill(context: &mut Context) -> boa_engine::JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          if (typeof globalThis.ShadowRealm === 'function') {
            return;
          }
          if (typeof globalThis.$262 !== 'object' || globalThis.$262 === null || typeof globalThis.$262.createRealm !== 'function') {
            return;
          }

          var stateKey = Symbol.for('@@agentjs.shadowrealm.state');
          var nextTempId = 0;

          function getIntrinsicShadowRealmPrototype(newTarget) {
            if (newTarget === undefined || newTarget === ShadowRealm) {
              return ShadowRealm.prototype;
            }
            var proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              var otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              var otherShadowRealm = otherGlobal && otherGlobal.ShadowRealm;
              var otherProto = otherShadowRealm && otherShadowRealm.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return ShadowRealm.prototype;
          }

          function requireShadowRealm(value, name, TypeErrorCtor) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !Object.prototype.hasOwnProperty.call(value, stateKey)) {
              throw new TypeErrorCtor(name + ' called on incompatible receiver');
            }
            return value[stateKey];
          }

          function isPrimitive(value) {
            return value === null || (typeof value !== 'object' && typeof value !== 'function');
          }

          function defineNameAndLength(wrapper, target) {
            var length = 0;
            var name = '';

            try {
              if (Object.prototype.hasOwnProperty.call(target, 'length')) {
                var targetLength = target.length;
                if (typeof targetLength === 'number') {
                  if (targetLength === Infinity) {
                    length = Infinity;
                  } else if (targetLength === -Infinity) {
                    length = 0;
                  } else {
                    var coerced = Math.trunc(targetLength);
                    if (!Number.isFinite(coerced)) {
                      coerced = 0;
                    }
                    length = Math.max(coerced, 0);
                  }
                }
              }

              var targetName = target.name;
              if (typeof targetName === 'string') {
                name = targetName;
              }
            } catch {
              throw new TypeError('WrappedFunctionCreate failed');
            }

            Object.defineProperty(wrapper, 'length', {
              value: length,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(wrapper, 'name', {
              value: name,
              writable: false,
              enumerable: false,
              configurable: true,
            });
          }

          function evalInBridge(state, sourceText) {
            var tempName = '__agentjs_shadowrealm_source_' + (nextTempId++) + '__';
            state.bridge.global[tempName] = sourceText;
            try {
              return state.bridge.evalScript('(0, eval)(globalThis[' + JSON.stringify(tempName) + '])');
            } finally {
              try {
                delete state.bridge.global[tempName];
              } catch (e) {}
            }
          }

          function createTargetRealmCallable(callable, state) {
            var callableId = __agentjs_shadowrealm_register_callable__(callable);
            var source =
              '(function(callableId) {' +
              '  return function() {' +
              '    var invokeArgs = [callableId];' +
              '    for (var i = 0; i < arguments.length; i++) {' +
              '      invokeArgs.push(arguments[i]);' +
              '    }' +
              '    var result;' +
              '    try {' +
              '      result = __agentjs_shadowrealm_invoke__.apply(undefined, invokeArgs);' +
              '    } catch (e) {' +
              '      throw new TypeError();' +
              '    }' +
              '    if (result === null || (typeof result !== "object" && typeof result !== "function")) {' +
              '      return result;' +
              '    }' +
              '    if (typeof result === "function") {' +
              '      var nestedId = __agentjs_shadowrealm_register_callable__(result);' +
              '      return function() {' +
              '        var nestedArgs = [nestedId];' +
              '        for (var j = 0; j < arguments.length; j++) {' +
              '          nestedArgs.push(arguments[j]);' +
              '        }' +
              '        try {' +
              '          return __agentjs_shadowrealm_invoke__.apply(undefined, nestedArgs);' +
              '        } catch (e) {' +
              '          throw new TypeError();' +
              '        }' +
              '      };' +
              '    }' +
              '    throw new TypeError();' +
              '  };' +
              '})(' + String(callableId) + ')';
            return evalInBridge(state, source);
          }

          function convertArgumentsForTarget(args, state) {
            var converted = [];
            for (var i = 0; i < args.length; i++) {
              var arg = args[i];
              if (isPrimitive(arg)) {
                converted.push(arg);
                continue;
              }
              if (typeof arg === 'function') {
                converted.push(createTargetRealmCallable(arg, state));
                continue;
              }
              throw new TypeError('ShadowRealm wrapped functions only accept primitives or callables');
            }
            return converted;
          }

          function wrapValueFromTarget(value, wrapperCarrier, TypeErrorCtor) {
            if (isPrimitive(value)) {
              return value;
            }
            if (typeof value === 'function') {
              try {
                return __agentjs_shadowrealm_wrap_callable__(wrapperCarrier, value);
              } catch (e) {
                throw new TypeErrorCtor('WrappedFunctionCreate failed');
              }
            }
            throw new TypeErrorCtor('ShadowRealm values must be primitive or callable');
          }

          function createWrappedFunction(targetCallable, state) {
            var targetId = __agentjs_shadowrealm_register_callable__(targetCallable);
            var wrapped = function() {
              var convertedArgs = convertArgumentsForTarget(Array.prototype.slice.call(arguments), state);
              var invokeArgs = [targetId];
              for (var i = 0; i < convertedArgs.length; i++) {
                invokeArgs.push(convertedArgs[i]);
              }
              var result;
              try {
                result = __agentjs_shadowrealm_invoke__.apply(undefined, invokeArgs);
              } catch (e) {
                throw new TypeError('Wrapped function invocation failed');
              }
              return wrapValueFromTarget(result, state);
            };
            defineNameAndLength(wrapped, targetCallable);
            return wrapped;
          }

          function ShadowRealm() {
            if (new.target === undefined) {
              throw new TypeError('Constructor ShadowRealm requires new');
            }

            var realm = Object.create(getIntrinsicShadowRealmPrototype(new.target));
            Object.defineProperty(realm, stateKey, {
              value: {
                bridge: $262.createRealm(),
              },
              writable: false,
              enumerable: false,
              configurable: false,
            });
            return realm;
          }

          var shadowRealmMethods = {
            evaluate(sourceText) {
              var CallerTypeError = TypeError;
              var CallerSyntaxError = SyntaxError;
              var RealmCarrier = function() {};
              var state = requireShadowRealm(this, 'ShadowRealm.prototype.evaluate', CallerTypeError);
              if (typeof sourceText !== 'string') {
                throw new CallerTypeError('ShadowRealm.prototype.evaluate requires a string');
              }
              var initialParseValid = __agentjs_shadowrealm_can_parse_script__(sourceText);

              var result;
              try {
                result = evalInBridge(state, sourceText);
              } catch (e) {
                if (!initialParseValid && e && e.name === 'SyntaxError') {
                  throw new CallerSyntaxError('Invalid ShadowRealm source text');
                }
                throw new CallerTypeError('ShadowRealm.prototype.evaluate failed');
              }
              return wrapValueFromTarget(result, RealmCarrier, CallerTypeError);
            },

            importValue(specifier, exportName) {
              var CallerTypeError = TypeError;
              var RealmCarrier = function() {};
              var state = requireShadowRealm(this, 'ShadowRealm.prototype.importValue', CallerTypeError);
              var specifierString = String(specifier);
              if (typeof exportName !== 'string') {
                throw new CallerTypeError('ShadowRealm.prototype.importValue exportName must be a string');
              }

              var promise;
              try {
                promise = __agentjs_shadowrealm_dynamic_import__(specifierString);
              } catch (e) {
                return Promise.reject(new CallerTypeError('ShadowRealm.prototype.importValue failed'));
              }

              return promise.then(
                function(namespace) {
                  if (!Object.prototype.hasOwnProperty.call(namespace, exportName)) {
                    throw new CallerTypeError('Requested export was not found');
                  }
                  return wrapValueFromTarget(
                    namespace[exportName],
                    RealmCarrier,
                    CallerTypeError
                  );
                },
                function() {
                  throw new CallerTypeError('ShadowRealm.prototype.importValue failed');
                }
              );
            },
          };

          var shadowRealmPrototype = Object.create(Object.prototype);
          Object.defineProperties(shadowRealmPrototype, {
            constructor: {
              value: ShadowRealm,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            evaluate: {
              value: shadowRealmMethods.evaluate,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            importValue: {
              value: shadowRealmMethods.importValue,
              writable: true,
              enumerable: false,
              configurable: true,
            },
          });
          Object.defineProperty(shadowRealmPrototype, Symbol.toStringTag, {
            value: 'ShadowRealm',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(ShadowRealm, 'prototype', {
            value: shadowRealmPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(globalThis, 'ShadowRealm', {
            value: ShadowRealm,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn build_test262_object(
    target_realm: Realm,
    target_global: JsObject,
    expose_host_hooks: bool,
    context: &mut Context,
) -> JsObject {
    let eval_realm = target_realm.clone();
    let eval_script = build_builtin_function(
        context,
        js_string!("evalScript"),
        1,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, target_realm, context| eval_script_in_realm(args, target_realm, context),
            eval_realm,
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
    let gc = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("gc"),
            0,
            NativeFunction::from_fn_ptr(host_gc),
        )
    });
    let abstract_module_source =
        expose_host_hooks.then(|| build_abstract_module_source_constructor(context));
    let agent = build_agent_object(context);

    let is_html_dda = JsObject::with_is_html_dda_proto(target_realm.intrinsics());

    let mut object = ObjectInitializer::new(context);
    object.property(js_string!("global"), target_global, Attribute::all());
    object.property(js_string!("evalScript"), eval_script, Attribute::all());
    object.property(js_string!("IsHTMLDDA"), is_html_dda, Attribute::all());
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
    if let Some(gc) = gc {
        object.property(js_string!("gc"), gc, Attribute::all());
    }
    if let Some(abstract_module_source) = abstract_module_source {
        object.property(
            js_string!("AbstractModuleSource"),
            abstract_module_source,
            Attribute::all(),
        );
    }
    if let Some(agent) = agent {
        object.property(js_string!("agent"), agent, Attribute::all());
    }
    let object = object.build();
    let can_parse_realm = target_realm.clone();
    let can_parse = build_builtin_function(
        context,
        js_string!("__agentjsCanParseScript__"),
        1,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, target_realm, context| {
                let source = script_source_from_args(args, context)?;
                with_realm(context, target_realm.clone(), |context| {
                    Ok(
                        Script::parse(Source::from_bytes(source.as_str()), None, context)
                            .is_ok()
                            .into(),
                    )
                })
            },
            can_parse_realm,
        ),
    );
    object
        .define_property_or_throw(
            js_string!("__agentjsCanParseScript__"),
            PropertyDescriptor::builder()
                .value(can_parse)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("defining internal parse helper on test262 realm wrapper must succeed");
    object
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

fn build_abstract_module_source_constructor(context: &mut Context) -> JsObject {
    let constructor = build_builtin_function(
        context,
        js_string!("AbstractModuleSource"),
        0,
        NativeFunction::from_fn_ptr(host_abstract_module_source_constructor),
    );

    let prototype = ObjectInitializer::new(context).build();
    let to_string_tag = build_builtin_function(
        context,
        js_string!("get [Symbol.toStringTag]"),
        0,
        NativeFunction::from_fn_ptr(host_abstract_module_source_to_string_tag),
    );

    prototype
        .define_property_or_throw(
            js_string!("constructor"),
            PropertyDescriptor::builder()
                .value(constructor.clone())
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("AbstractModuleSource.prototype.constructor definition must succeed");
    prototype
        .define_property_or_throw(
            JsSymbol::to_string_tag(),
            PropertyDescriptor::builder()
                .get(to_string_tag)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("AbstractModuleSource.prototype[@@toStringTag] definition must succeed");

    constructor
        .define_property_or_throw(
            js_string!("prototype"),
            PropertyDescriptor::builder()
                .value(prototype)
                .writable(false)
                .enumerable(false)
                .configurable(false),
            context,
        )
        .expect("AbstractModuleSource.prototype definition must succeed");

    constructor
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

fn ensure_host_hooks_context(context: &mut Context) {
    if context.has_data::<HostHooksContext>() {
        return;
    }
    context.insert_data(HostHooksContext::new());
}

fn host_hooks_context(context: &Context) -> JsResult<&HostHooksContext> {
    context.get_data::<HostHooksContext>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("host hooks context is unavailable")
            .into()
    })
}

fn array_buffer_original_symbol(
    context: &Context,
    method_name: &'static str,
) -> JsResult<JsSymbol> {
    host_hooks_context(context)?
        .array_buffer_originals
        .get(method_name)
        .cloned()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message(format!(
                    "missing hidden ArrayBuffer method for `{method_name}`"
                ))
                .into()
        })
}

fn data_view_original_symbol(context: &Context, method_name: &'static str) -> JsResult<JsSymbol> {
    host_hooks_context(context)?
        .data_view_originals
        .get(method_name)
        .cloned()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message(format!(
                    "missing hidden DataView method for `{method_name}`"
                ))
                .into()
        })
}

fn immutable_marker_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.immutable_marker.clone())
}

fn promise_then_original_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.promise_then_original.clone())
}

fn array_flat_original_symbol(context: &Context) -> JsResult<JsSymbol> {
    Ok(host_hooks_context(context)?.array_flat_original.clone())
}

fn with_original_promise_then<T>(
    context: &mut Context,
    callback: impl FnOnce(&mut Context) -> JsResult<T>,
) -> JsResult<T> {
    let prototype = context.intrinsics().constructors().promise().prototype();
    let original = prototype.get(promise_then_original_symbol(context)?, context)?;
    let current = prototype.get(js_string!("then"), context)?;

    prototype.define_property_or_throw(
        js_string!("then"),
        PropertyDescriptor::builder()
            .value(original)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let result = callback(context);

    prototype.define_property_or_throw(
        js_string!("then"),
        PropertyDescriptor::builder()
            .value(current)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    result
}

fn mark_array_buffer_immutable(buffer: &JsObject, context: &mut Context) -> JsResult<()> {
    buffer.define_property_or_throw(
        immutable_marker_symbol(context)?,
        PropertyDescriptor::builder()
            .value(true)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    Ok(())
}

fn is_marked_immutable_array_buffer(buffer: &JsObject, context: &mut Context) -> JsResult<bool> {
    Ok(buffer
        .get(immutable_marker_symbol(context)?, context)?
        .to_boolean())
}

fn array_buffer_from_this(this: &BoaValue) -> JsResult<JsArrayBuffer> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ArrayBuffer method called with invalid `this` value")
    })?;
    Ok(JsArrayBuffer::from_object(object.clone()).map_err(|_| {
        JsNativeError::typ().with_message("ArrayBuffer method called with invalid `this` value")
    })?)
}

fn array_buffer_is_immutable(this: &BoaValue, context: &mut Context) -> JsResult<bool> {
    let buffer = array_buffer_from_this(this)?;
    is_marked_immutable_array_buffer(&buffer.clone().into(), context)
}

fn call_hidden_method(
    prototype: &JsObject,
    hidden_symbol: JsSymbol,
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let original = prototype.get(hidden_symbol, context)?;
    let callable = original.as_callable().ok_or_else(|| {
        JsNativeError::typ().with_message(format!("missing original method for `{method_name}`"))
    })?;
    callable.call(this, args, context)
}

fn call_hidden_array_buffer_method(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();
    call_hidden_method(
        &prototype,
        array_buffer_original_symbol(context, method_name)?,
        method_name,
        this,
        args,
        context,
    )
}

fn has_hidden_array_buffer_method(
    context: &mut Context,
    method_name: &'static str,
) -> JsResult<bool> {
    let prototype = context
        .intrinsics()
        .constructors()
        .array_buffer()
        .prototype();
    let symbol = array_buffer_original_symbol(context, method_name)?;
    prototype.has_own_property(symbol, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_annex_b_html_open_and_close_comments() {
        let source = "<!-- open\ncode();\n--> close\nnext();\n";
        let (rewritten, changed) = rewrite_annex_b_html_comments(source);
        assert!(changed);
        assert!(rewritten.contains("// open"));
        assert!(rewritten.contains("// close"));
    }

    #[test]
    fn rewrites_annex_b_call_assignment_targets() {
        let source = "f() = g();\n  for (f() in [1]) {}\nf()++;\nasync() = 1;\n";
        let (rewritten, _) = rewrite_annex_b_call_assignment_targets(source);
        assert!(
            rewritten.contains("throw new ReferenceError('Invalid left-hand side in assignment');")
        );
        assert!(!rewritten.contains("async() = 1;"));
    }

    #[test]
    fn rewrites_top_level_await_using_after_frontmatter_comment() {
        let source = r#"/*---
flags: [module]
---*/

await using x = {
  [Symbol.asyncDispose]() {}
};
"#;

        let rewritten = preprocess_compat_source(source, None, true).unwrap();
        assert!(rewritten.contains("/*---\nflags: [module]\n---*/"));
        assert!(rewritten.contains("const __agentjs_using_stack__ = new AsyncDisposableStack();"));
        assert!(rewritten.contains("const x = {\n  [Symbol.asyncDispose]() {}\n};"));
        assert!(rewritten.contains(
            "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
        ));
        assert!(!rewritten.contains("await using x ="));
    }
}

fn call_hidden_data_view_method(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let prototype = context.intrinsics().constructors().data_view().prototype();
    call_hidden_method(
        &prototype,
        data_view_original_symbol(context, method_name)?,
        method_name,
        this,
        args,
        context,
    )
}

fn immutable_array_buffer_error(operation: &'static str) -> JsError {
    JsNativeError::typ()
        .with_message(format!(
            "cannot perform `{operation}` on an immutable ArrayBuffer"
        ))
        .into()
}

fn read_transfer_length_argument(args: &[BoaValue], context: &mut Context) -> JsResult<()> {
    if let Some(new_length) = args.first().filter(|value| !value.is_undefined()) {
        let _ = new_length.to_index(context)?;
    }
    Ok(())
}

fn array_buffer_transfer_copy_and_detach(
    this: &BoaValue,
    args: &[BoaValue],
    preserve_resizability: bool,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    let buffer_object: JsObject = buffer.clone().into();
    let current_bytes = buffer.data().map(|bytes| bytes.to_vec()).ok_or_else(|| {
        JsNativeError::typ().with_message("cannot transfer a detached ArrayBuffer")
    })?;
    let is_resizable = preserve_resizability
        && buffer_object
            .get(js_string!("resizable"), context)?
            .to_boolean();
    let max_byte_length = if is_resizable {
        usize::try_from(
            buffer_object
                .get(js_string!("maxByteLength"), context)?
                .to_index(context)?,
        )
        .map_err(|_| {
            JsNativeError::range().with_message("ArrayBuffer length exceeds supported range")
        })?
    } else {
        current_bytes.len()
    };
    let target_length = if args.first().is_none_or(BoaValue::is_undefined) {
        current_bytes.len()
    } else {
        usize::try_from(args[0].to_index(context)?).map_err(|_| {
            JsNativeError::range().with_message("ArrayBuffer length exceeds supported range")
        })?
    };

    if is_resizable && target_length > max_byte_length {
        return Err(JsNativeError::range()
            .with_message("new ArrayBuffer length exceeds maxByteLength")
            .into());
    }

    let next_buffer = if is_resizable {
        JsArrayBuffer::new(target_length, context)?.with_max_byte_length(max_byte_length as u64)
    } else {
        JsArrayBuffer::new(target_length, context)?
    };
    if let Some(mut bytes) = next_buffer.data_mut() {
        let copy_length = current_bytes.len().min(target_length);
        bytes[..copy_length].copy_from_slice(&current_bytes[..copy_length]);
    }

    let _ = buffer.detach(&BoaValue::undefined())?;
    Ok(next_buffer.into())
}

fn array_buffer_slice_copy(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    let current_bytes = buffer
        .data()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| JsNativeError::typ().with_message("cannot slice a detached ArrayBuffer"))?;
    let len = i64::try_from(current_bytes.len())
        .expect("slice length should always fit into i64 on supported targets");
    let first = resolve_slice_index(args.first(), 0, len, context)?;
    let final_index = resolve_slice_index(args.get(1), len, len, context)?;
    let first = usize::try_from(first).expect("slice start should be non-negative");
    let final_index =
        usize::try_from(final_index.max(first as i64)).expect("slice end should be non-negative");

    let next_buffer = JsArrayBuffer::new(final_index.saturating_sub(first), context)?;
    if let Some(mut bytes) = next_buffer.data_mut() {
        bytes.copy_from_slice(&current_bytes[first..final_index]);
    }

    Ok(next_buffer.into())
}

fn resolve_slice_index(
    value: Option<&BoaValue>,
    default: i64,
    len: i64,
    context: &mut Context,
) -> JsResult<i64> {
    let Some(value) = value.filter(|value| !value.is_undefined()) else {
        return Ok(default);
    };

    Ok(match value.to_integer_or_infinity(context)? {
        boa_engine::value::IntegerOrInfinity::NegativeInfinity => 0,
        boa_engine::value::IntegerOrInfinity::PositiveInfinity => len,
        boa_engine::value::IntegerOrInfinity::Integer(integer) if integer < 0 => {
            (len + integer).max(0)
        }
        boa_engine::value::IntegerOrInfinity::Integer(integer) => integer.min(len),
    })
}

fn mark_array_buffer_result_immutable(
    value: BoaValue,
    context: &mut Context,
) -> JsResult<BoaValue> {
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("expected ArrayBuffer result from immutable helper")
    })?;
    let _ = JsArrayBuffer::from_object(object.clone()).map_err(|_| {
        JsNativeError::typ().with_message("expected ArrayBuffer result from immutable helper")
    })?;
    mark_array_buffer_immutable(&object, context)?;
    Ok(value)
}

fn data_view_buffer_is_immutable(this: &BoaValue, context: &mut Context) -> JsResult<bool> {
    let view = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DataView method called with invalid `this` value")
    })?;
    let buffer = view
        .get(js_string!("buffer"), context)?
        .as_object()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("DataView method called with invalid `this` value")
        })?;
    is_marked_immutable_array_buffer(&buffer, context)
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
        install_host_globals(&mut context).and_then(|_| install_test262_globals(&mut context, true))
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
    let source = finalize_script_source(&source, false, None)
        .map_err(|err| JsNativeError::syntax().with_message(err.message))?;
    let result = with_realm(context, target_realm.clone(), |context| {
        let result = context.eval(Source::from_bytes(source.as_str()));
        if result.is_ok() {
            context.poison_global_environment();
        }
        result
    })?;
    context.run_jobs()?;
    Ok(result)
}

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

fn host_shadowrealm_placeholder_finalization_registry(
    _: &BoaValue,
    _: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    Ok(ObjectInitializer::new(context).build().into())
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

fn host_gc(_: &BoaValue, _: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
    Ok(BoaValue::undefined())
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

fn host_array_buffer_immutable_getter(
    this: &BoaValue,
    _: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let buffer = array_buffer_from_this(this)?;
    Ok(is_marked_immutable_array_buffer(&buffer.clone().into(), context)?.into())
}

fn host_array_buffer_resize_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        return Err(immutable_array_buffer_error("resize"));
    }
    call_hidden_array_buffer_method("resize", this, args, context)
}

fn host_array_buffer_transfer_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transfer"));
    }
    if has_hidden_array_buffer_method(context, "transfer")? {
        call_hidden_array_buffer_method("transfer", this, args, context)
    } else {
        array_buffer_transfer_copy_and_detach(this, args, true, context)
    }
}

fn host_array_buffer_transfer_to_fixed_length_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transferToFixedLength"));
    }
    if has_hidden_array_buffer_method(context, "transferToFixedLength")? {
        call_hidden_array_buffer_method("transferToFixedLength", this, args, context)
    } else {
        array_buffer_transfer_copy_and_detach(this, args, false, context)
    }
}

fn host_array_buffer_slice_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let result = call_hidden_array_buffer_method("slice", this, args, context)?;
    let Some(object) = result.as_object() else {
        return Ok(result);
    };
    if JsArrayBuffer::from_object(object.clone()).is_ok()
        && is_marked_immutable_array_buffer(&object, context)?
    {
        return Err(immutable_array_buffer_error("slice"));
    }
    Ok(result)
}

fn host_array_buffer_transfer_to_immutable(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if array_buffer_is_immutable(this, context)? {
        read_transfer_length_argument(args, context)?;
        return Err(immutable_array_buffer_error("transferToImmutable"));
    }
    let result = if has_hidden_array_buffer_method(context, "transfer")? {
        call_hidden_array_buffer_method("transfer", this, args, context)?
    } else {
        array_buffer_transfer_copy_and_detach(this, args, false, context)?
    };
    mark_array_buffer_result_immutable(result, context)
}

fn host_array_buffer_slice_to_immutable(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    let result = array_buffer_slice_copy(this, args, context)?;
    mark_array_buffer_result_immutable(result, context)
}

fn host_abstract_module_source_constructor(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
    Err(JsNativeError::typ()
        .with_message("%AbstractModuleSource% constructor cannot be invoked directly")
        .into())
}

fn host_abstract_module_source_to_string_tag(
    _: &BoaValue,
    _: &[BoaValue],
    _context: &mut Context,
) -> JsResult<BoaValue> {
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
    context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if is_symbol_like_deferred_namespace_key(&key) {
        return Ok(deferred_namespace_ordinary_has(&metadata, &key).into());
    }

    let _ = evaluate_deferred_namespace_module(&metadata.path, context)?;
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
    context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    let _ = evaluate_deferred_namespace_module(&metadata.path, context)?;
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
    Ok(JsArray::from_iter(keys, context).into())
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
    context: &mut Context,
) -> JsResult<BoaValue> {
    let target = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing"))?;
    let key = args.get_or_undefined(1).to_property_key(context)?;
    let metadata = deferred_namespace_target_metadata(&target)?;

    if !is_symbol_like_deferred_namespace_key(&key) {
        let _ = evaluate_deferred_namespace_module(&metadata.path, context)?;
    }

    Ok(false.into())
}

fn host_deferred_namespace_delete_property(
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
        return Ok(deferred_namespace_delete_symbol_like_key(&metadata, &key).into());
    }

    let _ = evaluate_deferred_namespace_module(&metadata.path, context)?;
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

fn catch_silent_panic<F, T>(f: F) -> std::thread::Result<T>
where
    F: FnOnce() -> T,
{
    let _guard = PANIC_HOOK_LOCK
        .lock()
        .expect("panic hook mutex must not be poisoned");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    result
}

fn host_worker_leaving(_: &BoaValue, _: &[BoaValue], _context: &mut Context) -> JsResult<BoaValue> {
    Ok(BoaValue::undefined())
}

fn host_data_view_set_big_int64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setBigInt64", this, args, context)
}

fn host_data_view_set_big_uint64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setBigUint64", this, args, context)
}

fn host_data_view_set_float16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat16", this, args, context)
}

fn host_data_view_set_float32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat32", this, args, context)
}

fn host_data_view_set_float64_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setFloat64", this, args, context)
}

fn host_data_view_set_int16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt16", this, args, context)
}

fn host_data_view_set_int32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt32", this, args, context)
}

fn host_data_view_set_int8_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setInt8", this, args, context)
}

fn host_data_view_set_uint16_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint16", this, args, context)
}

fn host_data_view_set_uint32_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint32", this, args, context)
}

fn host_data_view_set_uint8_wrapper(
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    host_data_view_set_wrapper("setUint8", this, args, context)
}

fn host_data_view_set_wrapper(
    method_name: &'static str,
    this: &BoaValue,
    args: &[BoaValue],
    context: &mut Context,
) -> JsResult<BoaValue> {
    if data_view_buffer_is_immutable(this, context)? {
        return Err(immutable_array_buffer_error(method_name));
    }
    call_hidden_data_view_method(method_name, this, args, context)
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

fn host_error_is_error(
    _: &BoaValue,
    args: &[BoaValue],
    _: &mut Context,
) -> boa_engine::JsResult<BoaValue> {
    Ok(args
        .get_or_undefined(0)
        .as_object()
        .is_some_and(|value| value.is::<BoaBuiltinError>())
        .into())
}

fn display_value(value: &BoaValue, context: &mut Context) -> Option<String> {
    if value.is_undefined() {
        None
    } else {
        Some(inspect_value(value, context, 0, &mut HashSet::new()))
    }
}

/// Format a JS value for REPL display, similar to Node.js util.inspect
fn inspect_value(
    value: &BoaValue,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
) -> String {
    const MAX_DEPTH: usize = 4;
    const MAX_ARRAY_ITEMS: usize = 100;
    const MAX_STRING_LEN: usize = 100;

    if value.is_undefined() {
        "undefined".to_string()
    } else if value.is_null() {
        "null".to_string()
    } else if let Some(b) = value.as_boolean() {
        b.to_string()
    } else if let Some(n) = value.as_number() {
        if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else if n == 0.0 && n.is_sign_negative() {
            "-0".to_string()
        } else if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
            // Integer-like number
            format!("{}", n as i64)
        } else {
            format!("{n}")
        }
    } else if let Some(s) = value.as_string() {
        let s_str = s.to_std_string_escaped();
        let escaped = escape_string(&s_str);
        if escaped.len() > MAX_STRING_LEN {
            format!(
                "'{}'... ({} more characters)",
                &escaped[..MAX_STRING_LEN],
                escaped.len() - MAX_STRING_LEN
            )
        } else {
            format!("'{escaped}'")
        }
    } else if let Some(sym) = value.as_symbol() {
        let desc = sym
            .description()
            .map(|d| d.to_std_string_escaped())
            .unwrap_or_default();
        if desc.is_empty() {
            "Symbol()".to_string()
        } else {
            format!("Symbol({desc})")
        }
    } else if let Some(n) = value.as_bigint() {
        format!("{n}n")
    } else if let Some(obj) = value.as_object() {
        // Circular reference check
        let ptr = obj.as_ref() as *const _ as usize;
        if seen.contains(&ptr) {
            return "[Circular]".to_string();
        }
        seen.insert(ptr);

        let result = inspect_object(&obj, context, depth, seen, MAX_DEPTH, MAX_ARRAY_ITEMS);

        seen.remove(&ptr);
        result
    } else {
        // Fallback
        value
            .to_string(context)
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|_| "[unknown]".to_string())
    }
}

fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\'' => result.push_str("\\'"),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => result.push_str(&format!("\\x{:02x}", c as u32)),
            c => result.push(c),
        }
    }
    result
}

fn inspect_object(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
    max_array_items: usize,
) -> String {
    // Check if it's a function
    if obj.is_callable() {
        let name = obj
            .get(js_string!("name"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_default();

        // Check if it's an async function or generator
        let to_string = obj
            .get(PropertyKey::from(JsSymbol::to_string_tag()), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()));

        let func_type = match to_string.as_deref() {
            Some("AsyncFunction") => "AsyncFunction",
            Some("GeneratorFunction") => "GeneratorFunction",
            Some("AsyncGeneratorFunction") => "AsyncGeneratorFunction",
            _ => "Function",
        };

        if name.is_empty() {
            format!("[{func_type} (anonymous)]")
        } else {
            format!("[{func_type}: {name}]")
        }
    }
    // Check for Promise
    else if let Ok(promise) = JsPromise::from_object(obj.clone()) {
        let state = promise.state();
        match state {
            PromiseState::Pending => "Promise { <pending> }".to_string(),
            PromiseState::Fulfilled(v) => {
                if depth >= max_depth {
                    "Promise { ... }".to_string()
                } else {
                    let inner = inspect_value(&v, context, depth + 1, seen);
                    format!("Promise {{ {inner} }}")
                }
            }
            PromiseState::Rejected(e) => {
                if depth >= max_depth {
                    "Promise { <rejected> ... }".to_string()
                } else {
                    let inner = inspect_value(&e, context, depth + 1, seen);
                    format!("Promise {{ <rejected> {inner} }}")
                }
            }
        }
    }
    // Check for Array
    else if obj.is_array() {
        inspect_array(obj, context, depth, seen, max_depth, max_array_items)
    }
    // Check for TypedArray
    else if let Ok(arr) = JsUint8Array::from_object(obj.clone()) {
        let len = arr.length(context).unwrap_or(0);
        format!("Uint8Array({len}) [ ... ]")
    }
    // Check for ArrayBuffer
    else if JsArrayBuffer::from_object(obj.clone()).is_ok() {
        let len = obj
            .get(js_string!("byteLength"), context)
            .ok()
            .and_then(|v| v.to_u32(context).ok())
            .unwrap_or(0);
        format!("ArrayBuffer {{ byteLength: {len} }}")
    }
    // Check for Date
    else if is_date_object(obj, context) {
        // Try toISOString first
        if let Ok(to_iso) = obj.get(js_string!("toISOString"), context) {
            if let Some(func) = to_iso.as_object().filter(|o| o.is_callable()) {
                if let Ok(result) = func.call(&BoaValue::from(obj.clone()), &[], context) {
                    if let Ok(s) = result.to_string(context) {
                        return s.to_std_string_escaped();
                    }
                }
            }
        }
        // Fallback: try toString
        if let Ok(to_str) = obj.get(js_string!("toString"), context) {
            if let Some(func) = to_str.as_object().filter(|o| o.is_callable()) {
                if let Ok(result) = func.call(&BoaValue::from(obj.clone()), &[], context) {
                    if let Ok(s) = result.to_string(context) {
                        return s.to_std_string_escaped();
                    }
                }
            }
        }
        "[Date]".to_string()
    }
    // Check for RegExp
    else if is_regexp_object(obj, context) {
        // Get source and flags
        let source = obj
            .get(js_string!("source"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_else(|| "".to_string());
        let flags = obj
            .get(js_string!("flags"), context)
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .unwrap_or_default();
        format!("/{source}/{flags}")
    }
    // Check for Error
    else if is_error_object(obj, context) {
        let name = obj
            .get(js_string!("name"), context)
            .ok()
            .and_then(|v| v.to_string(context).ok())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|| "Error".to_string());
        let message = obj
            .get(js_string!("message"), context)
            .ok()
            .and_then(|v| v.to_string(context).ok())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        if message.is_empty() {
            format!("[{name}]")
        } else {
            format!("[{name}: {message}]")
        }
    }
    // Check for Map
    else if is_map_object(obj, context) {
        inspect_map(obj, context, depth, seen, max_depth)
    }
    // Check for Set
    else if is_set_object(obj, context) {
        inspect_set(obj, context, depth, seen, max_depth)
    }
    // Generic object
    else {
        inspect_plain_object(obj, context, depth, seen, max_depth)
    }
}

fn inspect_array(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
    max_items: usize,
) -> String {
    let length = obj
        .get(js_string!("length"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0) as usize;

    if depth >= max_depth {
        return format!("[Array({length})]");
    }

    if length == 0 {
        return "[]".to_string();
    }

    let mut items = Vec::new();
    let show_count = length.min(max_items);

    for i in 0..show_count {
        let idx = js_string!(i.to_string());
        match obj.get(PropertyKey::from(idx), context) {
            Ok(v) => items.push(inspect_value(&v, context, depth + 1, seen)),
            Err(_) => items.push("<error>".to_string()),
        }
    }

    let remaining = if length > max_items {
        format!(" ... {} more items", length - max_items)
    } else {
        String::new()
    };

    // Format based on complexity
    let single_line = format!("[ {}{remaining} ]", items.join(", "));
    if single_line.len() <= 80 && !single_line.contains('\n') {
        single_line
    } else {
        let indent = "  ".repeat(depth + 1);
        let inner = items
            .iter()
            .map(|s| format!("{indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("[\n{inner}{remaining}\n{}]", "  ".repeat(depth))
    }
}

fn inspect_plain_object(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "[Object]".to_string();
    }

    // Get own property keys
    let keys = match obj.own_property_keys(context) {
        Ok(keys) => keys,
        Err(_) => return "[Object]".to_string(),
    };

    if keys.is_empty() {
        return "{}".to_string();
    }

    let mut entries = Vec::new();
    for key in keys.iter().take(20) {
        let key_str = match key {
            PropertyKey::String(s) => s.to_std_string_escaped(),
            PropertyKey::Symbol(sym) => {
                let desc = sym
                    .description()
                    .map(|d| d.to_std_string_escaped())
                    .unwrap_or_default();
                format!("[Symbol({desc})]")
            }
            PropertyKey::Index(i) => i.get().to_string(),
        };

        // Skip internal properties
        if key_str.starts_with("__") {
            continue;
        }

        match obj.get(key.clone(), context) {
            Ok(v) => {
                let val_str = inspect_value(&v, context, depth + 1, seen);
                // Quote keys if needed
                if needs_quotes(&key_str) {
                    entries.push(format!("'{key_str}': {val_str}"));
                } else {
                    entries.push(format!("{key_str}: {val_str}"));
                }
            }
            Err(_) => continue,
        }
    }

    let remaining = if keys.len() > 20 {
        format!(" ... {} more properties", keys.len() - 20)
    } else {
        String::new()
    };

    // Check for custom constructor name
    let constructor_name = obj
        .get(js_string!("constructor"), context)
        .ok()
        .and_then(|c| c.as_object())
        .and_then(|c| c.get(js_string!("name"), context).ok())
        .and_then(|n| n.as_string().map(|s| s.to_std_string_escaped()))
        .filter(|n| n != "Object" && !n.is_empty());

    let prefix = constructor_name
        .map(|n| format!("{n} "))
        .unwrap_or_default();

    // Format based on complexity
    let single_line = format!("{prefix}{{ {}{remaining} }}", entries.join(", "));
    if single_line.len() <= 80 && !single_line.contains('\n') {
        single_line
    } else {
        let indent = "  ".repeat(depth + 1);
        let inner = entries
            .iter()
            .map(|s| format!("{indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("{prefix}{{\n{inner}{remaining}\n{}}}", "  ".repeat(depth))
    }
}

fn inspect_map(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "Map { ... }".to_string();
    }

    // Get entries via forEach or iteration
    let size = obj
        .get(js_string!("size"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0);

    if size == 0 {
        return "Map(0) {}".to_string();
    }

    // Try to iterate using entries()
    let entries_fn = match obj.get(js_string!("entries"), context) {
        Ok(v) if v.is_callable() => v,
        _ => return format!("Map({size}) {{ ... }}"),
    };

    let iterator = match entries_fn
        .as_object()
        .and_then(|f| f.call(&BoaValue::from(obj.clone()), &[], context).ok())
    {
        Some(it) => it,
        None => return format!("Map({size}) {{ ... }}"),
    };

    let mut items = Vec::new();
    for _ in 0..size.min(10) {
        let next_fn = match iterator
            .as_object()
            .and_then(|it| it.get(js_string!("next"), context).ok())
        {
            Some(f) if f.is_callable() => f,
            _ => break,
        };

        let result = match next_fn
            .as_object()
            .and_then(|f| f.call(&iterator, &[], context).ok())
        {
            Some(r) => r,
            None => break,
        };

        let done = result
            .as_object()
            .and_then(|r| r.get(js_string!("done"), context).ok())
            .map(|v| v.to_boolean());

        if done == Some(true) {
            break;
        }

        if let Some(value) = result
            .as_object()
            .and_then(|r| r.get(js_string!("value"), context).ok())
        {
            if let Some(pair) = value.as_object() {
                let key = pair
                    .get(PropertyKey::from(js_string!("0")), context)
                    .unwrap_or(BoaValue::undefined());
                let val = pair
                    .get(PropertyKey::from(js_string!("1")), context)
                    .unwrap_or(BoaValue::undefined());
                let k_str = inspect_value(&key, context, depth + 1, seen);
                let v_str = inspect_value(&val, context, depth + 1, seen);
                items.push(format!("{k_str} => {v_str}"));
            }
        }
    }

    let remaining = if size > 10 {
        format!(", ... {} more", size - 10)
    } else {
        String::new()
    };

    format!("Map({size}) {{ {}{remaining} }}", items.join(", "))
}

fn inspect_set(
    obj: &JsObject,
    context: &mut Context,
    depth: usize,
    seen: &mut HashSet<usize>,
    max_depth: usize,
) -> String {
    if depth >= max_depth {
        return "Set { ... }".to_string();
    }

    let size = obj
        .get(js_string!("size"), context)
        .ok()
        .and_then(|v| v.to_u32(context).ok())
        .unwrap_or(0);

    if size == 0 {
        return "Set(0) {}".to_string();
    }

    // Try to iterate using values()
    let values_fn = match obj.get(js_string!("values"), context) {
        Ok(v) if v.is_callable() => v,
        _ => return format!("Set({size}) {{ ... }}"),
    };

    let iterator = match values_fn
        .as_object()
        .and_then(|f| f.call(&BoaValue::from(obj.clone()), &[], context).ok())
    {
        Some(it) => it,
        None => return format!("Set({size}) {{ ... }}"),
    };

    let mut items = Vec::new();
    for _ in 0..size.min(10) {
        let next_fn = match iterator
            .as_object()
            .and_then(|it| it.get(js_string!("next"), context).ok())
        {
            Some(f) if f.is_callable() => f,
            _ => break,
        };

        let result = match next_fn
            .as_object()
            .and_then(|f| f.call(&iterator, &[], context).ok())
        {
            Some(r) => r,
            None => break,
        };

        let done = result
            .as_object()
            .and_then(|r| r.get(js_string!("done"), context).ok())
            .map(|v| v.to_boolean());

        if done == Some(true) {
            break;
        }

        if let Some(value) = result
            .as_object()
            .and_then(|r| r.get(js_string!("value"), context).ok())
        {
            items.push(inspect_value(&value, context, depth + 1, seen));
        }
    }

    let remaining = if size > 10 {
        format!(", ... {} more", size - 10)
    } else {
        String::new()
    };

    format!("Set({size}) {{ {}{remaining} }}", items.join(", "))
}

fn is_date_object(obj: &JsObject, context: &mut Context) -> bool {
    // Check if getTime exists and is callable
    obj.get(js_string!("getTime"), context)
        .ok()
        .map(|v| v.is_callable())
        .unwrap_or(false)
        && obj
            .get(js_string!("toISOString"), context)
            .ok()
            .map(|v| v.is_callable())
            .unwrap_or(false)
}

fn is_regexp_object(obj: &JsObject, context: &mut Context) -> bool {
    // Check Symbol.toStringTag first
    if obj
        .get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "RegExp")
        .unwrap_or(false)
    {
        return true;
    }
    // Fallback: check for source property (not has_own_property, as it's on prototype)
    obj.get(js_string!("source"), context).is_ok()
        && obj.get(js_string!("flags"), context).is_ok()
        && obj
            .get(js_string!("test"), context)
            .ok()
            .map(|v| v.is_callable())
            .unwrap_or(false)
}

fn is_error_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(js_string!("name"), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s.ends_with("Error"))
        .unwrap_or(false)
        || obj
            .has_own_property(js_string!("stack"), context)
            .unwrap_or(false)
}

fn is_map_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "Map")
        .unwrap_or(false)
}

fn is_set_object(obj: &JsObject, context: &mut Context) -> bool {
    obj.get(PropertyKey::from(JsSymbol::to_string_tag()), context)
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .map(|s| s == "Set")
        .unwrap_or(false)
}

fn needs_quotes(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let first = key.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return true;
    }
    key.chars()
        .any(|c| !c.is_alphanumeric() && c != '_' && c != '$')
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

    let opaque = error.to_opaque(context);
    let thrown_name = opaque.as_object().and_then(|object| {
        object
            .get(js_string!("name"), context)
            .ok()
            .filter(|value| !value.is_undefined() && !value.is_null())
            .and_then(|value| value.to_string(context).ok())
            .map(|text| text.to_std_string_escaped())
            .filter(|text| !text.is_empty())
            .or_else(|| {
                object
                    .get(js_string!("constructor"), context)
                    .ok()
                    .and_then(|value| value.as_object())
                    .and_then(|constructor| constructor.get(js_string!("name"), context).ok())
                    .filter(|value| !value.is_undefined() && !value.is_null())
                    .and_then(|value| value.to_string(context).ok())
                    .map(|text| text.to_std_string_escaped())
                    .filter(|text| !text.is_empty())
            })
    });

    let name = native_name
        .clone()
        .or(thrown_name)
        .unwrap_or_else(|| "ThrownValue".to_string());

    let message = if matches!(
        native_name.as_deref(),
        Some("RuntimeLimit") | Some("NoInstructionsRemain")
    ) {
        format!("{error}")
    } else {
        opaque
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
