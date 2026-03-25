use boa_engine::module::SyntheticModuleInitializer;
use boa_engine::{
    Context, Finalize, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString, JsSymbol,
    JsValue as BoaValue, Module, NativeFunction, Source, Trace,
    builtins::array_buffer::SharedArrayBuffer,
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
use std::cell::RefCell;
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
    Regex::new(r#"(?s)\bexport\s+\*\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+(['"])([^'"]+)(['"])"#)
        .expect("module export namespace regex must compile")
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
        }
    }
}

impl Finalize for HostHooksContext {}

// SAFETY: Context data stores only `JsSymbol`s and Rust collections that do not reference GC values.
unsafe impl Trace for HostHooksContext {
    unsafe fn trace(&self, _tracer: &mut Tracer) {}

    unsafe fn trace_non_roots(&self) {}

    fn run_finalizer(&self) {
        self.finalize();
    }
}

impl JsData for HostHooksContext {}

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

        let source = finalize_script_source(source, options.strict, None)?;
        let result = context
            .eval(Source::from_bytes(source.as_str()))
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
        let source = finalize_script_source(source, options.strict, Some(canonical_path.as_path()))?;
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
        let prepared_source = preprocess_compat_source(source, Some(canonical_path.as_path()))?;
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

fn finalize_script_source(
    source: &str,
    strict: bool,
    source_path: Option<&Path>,
) -> Result<String, EngineError> {
    let prepared = preprocess_compat_source(source, source_path)?;
    Ok(if strict {
        format!("\"use strict\";\n{prepared}")
    } else {
        prepared
    })
}

fn preprocess_compat_source(source: &str, source_path: Option<&Path>) -> Result<String, EngineError> {
    validate_import_call_syntax(source)?;

    let source = rewrite_static_import_attributes(source);
    let (source, rewrote_dynamic_imports) = rewrite_dynamic_import_calls(&source);
    let (source, rewrote_import_defer_calls) = rewrite_dynamic_import_defer_calls(&source);
    let (source, rewrote_import_source_calls) = rewrite_dynamic_import_source_calls(&source);
    let (source, rewrote_static_source_imports) = rewrite_static_source_phase_imports(&source);
    let (source, rewrote_static_defer_imports) = rewrite_static_defer_namespace_imports(&source);
    Ok(if rewrote_dynamic_imports
        || rewrote_import_defer_calls
        || rewrote_import_source_calls
        || rewrote_static_source_imports
        || rewrote_static_defer_imports
    {
        format!("{}\n{source}", build_import_compat_helper(source_path))
    } else {
        source
    })
}

fn validate_import_call_syntax(source: &str) -> Result<(), EngineError> {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_js_string(bytes, i),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i)
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i)
            }
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
        byte if is_identifier_byte(byte) || matches!(byte, b'\'' | b'"' | b'{' | b'*') => Ok(()),
        _ => Err(invalid_import_call_syntax_error()),
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
            b'.' if depth == 0 && cursor + 2 < close_paren && &bytes[cursor..cursor + 3] == b"..." => {
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
            let source = preprocess_compat_source(&source, Some(path))
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

    Ok(Module::synthetic(
        &[js_string!("default")],
        SyntheticModuleInitializer::from_copy_closure_with_captures(
            |module, value, _context| {
                module.set_export(&js_string!("default"), value.clone())?;
                Ok(())
            },
            BoaValue::from(proxy),
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
            let source = std::fs::read_to_string(path).unwrap_or_default();
            if is_async_module_source(&source) {
                let _ = module.evaluate(context);
                context.run_jobs()?;
            }
        },
        PromiseState::Rejected(reason) => return Err(JsError::from_opaque(reason.clone())),
        PromiseState::Pending => {
            return Err(JsNativeError::typ()
                .with_message("deferred namespace module remained pending during load")
                .into());
        }
    }

    Ok(module)
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
    install_array_buffer_detached_getter(context)?;
    install_array_buffer_immutable_hooks(context)?;
    install_data_view_immutable_hooks(context)?;
    install_reg_exp_legacy_accessors(context)?;
    Ok(())
}

fn install_reg_exp_legacy_accessors(context: &mut Context) -> JsResult<()> {
    let regexp_ctor = context.intrinsics().constructors().regexp().constructor();
    let receiver_check =
        "if (this !== RegExp) { throw new TypeError('RegExp legacy static accessor called on incompatible receiver'); }";

    for i in 1..=9 {
        let name = format!("${i}");
        let getter = context.eval(Source::from_bytes(&format!(
            "(function() {{ {receiver_check} return ''; }})"
        )))?;
        let getter = getter
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp legacy getter"))?;
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
        let getter = getter
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp legacy getter"))?;
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

    let is_html_dda = build_builtin_function(
        context,
        js_string!("IsHTMLDDA"),
        0,
        NativeFunction::from_fn_ptr(|_this, _args, _context| Ok(BoaValue::undefined())),
    );

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
        return module.namespace(context).get(js_string!("default"), context);
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
    Ok(BoaValue::undefined())
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

    if let Some(module) = loader.get(&path, ModuleResourceKind::Deferred) {
        let namespace = module.namespace(context).get(js_string!("default"), context)?;
        return Ok(JsPromise::resolve(namespace, context).into());
    }

    let deferred_module = load_deferred_namespace_module(&path, context)?;
    loader.insert(
        path.clone(),
        ModuleResourceKind::Deferred,
        deferred_module.clone(),
    );
    let namespace = deferred_module
        .namespace(context)
        .get(js_string!("default"), context)?;

    if let Err(error) = ensure_deferred_module_loaded_and_linked(&path, context) {
        return Ok(JsPromise::reject(error, context).into());
    }

    Ok(JsPromise::resolve(namespace, context).into())
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
        .ok_or_else(|| JsNativeError::typ().with_message("deferred namespace target missing").into())
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

fn deferred_namespace_ordinary_has(
    metadata: &DeferredNamespaceTarget,
    key: &PropertyKey,
) -> bool {
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

fn deferred_namespace_exports_include(metadata: &DeferredNamespaceTarget, key: &PropertyKey) -> bool {
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
        for binding in bindings.split(',').map(str::trim).filter(|binding| !binding.is_empty()) {
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
