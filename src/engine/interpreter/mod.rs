use crate::engine::env::{Environment, ResourceRecord};
use crate::engine::value::{
    BuiltinFunction, FunctionValue, JsValue, PromiseReaction, PromiseReactionKind, PromiseState,
    PromiseValue, PropertyValue, get_object_property, get_property_value, has_object_property,
    native_fn, new_object_map, object_with_proto, resolve_indirect_value,
};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    #[error("Reference Error: {0} is not defined")]
    ReferenceError(String),
    #[error("Type Error: {0}")]
    TypeError(String),
    #[error("Range Error: {0}")]
    RangeError(String),
    #[error("Syntax Error: {0}")]
    SyntaxError(String),
    #[error("Return: {0:?}")]
    Return(JsValue),
    #[error("Throw: {0:?}")]
    Throw(JsValue),
    #[error("Break: {0:?}")]
    Break(Option<String>),
    #[error("Continue: {0:?}")]
    Continue(Option<String>),
    #[error("Timeout: Infinite loop or too many instructions")]
    Timeout,
}

type GeneratorValueContinuation =
    Rc<dyn Fn(&mut Interpreter, JsValue) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorErrorContinuation =
    Rc<dyn Fn(&mut Interpreter, RuntimeError) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorContinuation =
    Rc<dyn Fn(&mut Interpreter, ResumeAction) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorCompletionContinuation =
    Rc<dyn Fn(&mut Interpreter) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorStringContinuation =
    Rc<dyn Fn(&mut Interpreter, String) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorArgsContinuation =
    Rc<dyn Fn(&mut Interpreter, Vec<JsValue>) -> Result<GeneratorExecution, RuntimeError>>;
type GeneratorTaggedTemplateContinuation = Rc<
    dyn Fn(
        &mut Interpreter,
        Vec<JsValue>,
        Vec<JsValue>,
    ) -> Result<GeneratorExecution, RuntimeError>,
>;
type GeneratorCallTargetContinuation =
    Rc<dyn Fn(&mut Interpreter, JsValue, JsValue) -> Result<GeneratorExecution, RuntimeError>>;

#[derive(Clone)]
enum ResumeAction {
    Next(JsValue),
    Return(JsValue),
    Throw(JsValue),
}

enum GeneratorExecution {
    Complete(JsValue),
    Yielded {
        value: JsValue,
        continuation: GeneratorContinuation,
    },
}

fn loop_control_matches(label: &Option<String>, current_label: Option<&str>) -> bool {
    match label {
        None => true,
        Some(label) => current_label.is_some_and(|current| current == label),
    }
}

fn value_is_object_like(value: &JsValue) -> bool {
    matches!(
        value,
        JsValue::Array(_)
            | JsValue::Object(_)
            | JsValue::EnvironmentObject(_)
            | JsValue::Promise(_)
            | JsValue::GeneratorState(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_)
            | JsValue::BuiltinFunction(_)
    )
}

enum GeneratorStatus {
    SuspendedStart,
    SuspendedYield(GeneratorContinuation),
    Executing,
    Completed,
}

pub struct GeneratorState {
    declaration_id: usize,
    env: Rc<RefCell<Environment>>,
    status: GeneratorStatus,
    is_async: bool,
}

impl std::fmt::Debug for GeneratorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = match &self.status {
            GeneratorStatus::SuspendedStart => "SuspendedStart",
            GeneratorStatus::SuspendedYield(_) => "SuspendedYield",
            GeneratorStatus::Executing => "Executing",
            GeneratorStatus::Completed => "Completed",
        };
        f.debug_struct("GeneratorState")
            .field("declaration_id", &self.declaration_id)
            .field("status", &status)
            .field("is_async", &self.is_async)
            .finish()
    }
}

pub struct Interpreter {
    pub global_env: Rc<RefCell<Environment>>,
    pub instruction_count: usize,
    pub functions: Vec<FunctionDeclaration<'static>>,
    class_instance_fields: HashMap<usize, Vec<InstanceFieldDefinition>>,
    class_private_elements: HashMap<usize, ClassPrivateElements>,
    object_private_slots: HashMap<usize, HashMap<(usize, String), PrivateSlot>>,
    object_private_brands: HashMap<usize, HashSet<usize>>,
    next_private_brand: usize,
    module_cache: HashMap<PathBuf, JsValue>,
    module_exports_stack: Vec<crate::engine::value::JsObjectMap>,
    module_base_dirs: Vec<PathBuf>,
    microtask_queue: VecDeque<Microtask>,
    call_stack: Vec<ActiveCallFrame>,
}

#[derive(Debug, Clone)]
enum Microtask {
    PromiseReaction {
        state: PromiseState,
        reaction: PromiseReaction,
    },
}

#[derive(Clone)]
struct InstanceFieldDefinition {
    key: String,
    initializer: Option<Expression<'static>>,
}

#[derive(Clone)]
struct PrivateFieldDefinition {
    name: String,
    initializer: Option<Expression<'static>>,
}

#[derive(Clone)]
enum PrivateElementKind {
    Field,
    Method(JsValue),
    Accessor {
        getter: Option<JsValue>,
        setter: Option<JsValue>,
    },
}

#[derive(Clone)]
struct PrivateElementDefinition {
    kind: PrivateElementKind,
}

#[derive(Clone, Default)]
struct ClassPrivateElements {
    instance: HashMap<String, PrivateElementDefinition>,
    static_members: HashMap<String, PrivateElementDefinition>,
    instance_fields: Vec<PrivateFieldDefinition>,
    static_fields: Vec<PrivateFieldDefinition>,
}

#[derive(Clone)]
enum PrivateSlot {
    Data(JsValue),
}

#[derive(Default)]
struct PrivateDeclarationRecord {
    is_static: bool,
    has_field: bool,
    has_method: bool,
    has_getter: bool,
    has_setter: bool,
}

#[derive(Clone, Copy)]
enum PrivateDeclarationKind {
    Field,
    Method,
    Getter,
    Setter,
}

struct ActiveCallFrame {
    function: Rc<FunctionValue>,
    instance_fields_initialized: bool,
}

enum IteratorCursor {
    Array {
        values: Rc<RefCell<Vec<JsValue>>>,
        index: usize,
    },
    String {
        chars: Vec<JsValue>,
        index: usize,
    },
    Object {
        map: crate::engine::value::JsObjectMap,
        done: bool,
    },
}

enum IteratorStep {
    Yield(JsValue),
    Complete(JsValue),
}

mod async_runtime;
mod callables;
mod expression_ops;
mod functions_classes;
mod generator_eval;
mod generator_loops;
mod helpers;
mod members_private;
mod modules_ops;
mod statements;

#[allow(unused_imports)]
use self::{
    async_runtime::*, callables::*, expression_ops::*, functions_classes::*, generator_eval::*,
    generator_loops::*, helpers::*, members_private::*, modules_ops::*, statements::*,
};
