use crate::engine::env::Environment;
use crate::engine::interpreter::{GeneratorState, Interpreter, RuntimeError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct FunctionValue {
    pub id: usize,
    pub env: Rc<RefCell<Environment>>,
    pub prototype: JsValue,
    pub properties: JsObjectMap,
    pub super_binding: Option<JsValue>,
    pub super_property_base: Option<JsValue>,
    pub home_object: Option<JsValue>,
    pub private_brand: Option<usize>,
    pub uses_lexical_this: bool,
    pub can_construct: bool,
    pub is_class_constructor: bool,
    pub is_derived_constructor: bool,
}

#[derive(Debug, Clone)]
pub enum PromiseState {
    Pending,
    Fulfilled(JsValue),
    Rejected(JsValue),
}

#[derive(Debug, Clone)]
pub enum PromiseReactionKind {
    Then {
        on_fulfilled: Option<JsValue>,
        on_rejected: Option<JsValue>,
    },
    Finally {
        on_finally: Option<JsValue>,
    },
    FinallyPassThrough {
        original_state: PromiseState,
    },
}

#[derive(Debug, Clone)]
pub struct PromiseReaction {
    pub kind: PromiseReactionKind,
    pub result_promise: Rc<RefCell<PromiseValue>>,
}

#[derive(Debug, Clone)]
pub struct PromiseValue {
    pub state: PromiseState,
    pub reactions: Vec<PromiseReaction>,
}

#[derive(Debug, Clone)]
pub enum BuiltinFunction {
    PromiseConstructor,
    PromiseResolve,
    PromiseReject,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    ModuleBindingGetter {
        env: Rc<RefCell<Environment>>,
        binding: String,
    },
    NamespaceBindingGetter {
        namespace: JsValue,
        export_name: String,
    },
    AsyncGeneratorResultMapper {
        done: bool,
    },
    PromiseResolver {
        promise: Rc<RefCell<PromiseValue>>,
        is_resolve: bool,
    },
}

/// A native (Rust) built-in function.
pub type NativeFn = fn(&mut Interpreter, &JsValue, &[JsValue]) -> Result<JsValue, RuntimeError>;

#[derive(Clone)]
pub struct NativeFunction {
    pub name: &'static str,
    pub func: NativeFn,
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[native function {}]", self.name)
    }
}

#[derive(Debug, Clone)]
pub enum PropertyValue {
    Data(JsValue),
    Accessor {
        getter: Option<JsValue>,
        setter: Option<JsValue>,
    },
}

pub type JsObjectMap = Rc<RefCell<HashMap<String, PropertyValue>>>;

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(i64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    Object(JsObjectMap),
    EnvironmentObject(Rc<RefCell<Environment>>),
    Promise(Rc<RefCell<PromiseValue>>),
    GeneratorState(Rc<RefCell<GeneratorState>>),
    ImportBinding {
        namespace: JsObjectMap,
        export_name: String,
    },
    Function(Rc<FunctionValue>),
    NativeFunction(Rc<NativeFunction>),
    BuiltinFunction(Rc<BuiltinFunction>),
}

include!("equality.rs");
include!("object.rs");
include!("js_value_impl.rs");
include!("namespace.rs");
