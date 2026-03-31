use crate::engine::env::Environment;
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

impl Interpreter {
    pub fn new() -> Self {
        let global_env = Rc::new(RefCell::new(Environment::new(None)));
        global_env.borrow_mut().define(
            "globalThis".to_string(),
            JsValue::EnvironmentObject(Rc::clone(&global_env)),
        );
        global_env.borrow_mut().define(
            "this".to_string(),
            JsValue::EnvironmentObject(Rc::clone(&global_env)),
        );
        global_env.borrow_mut().define(
            "Promise".to_string(),
            JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseConstructor)),
        );
        Self {
            global_env,
            instruction_count: 0,
            functions: Vec::new(),
            class_instance_fields: HashMap::new(),
            class_private_elements: HashMap::new(),
            object_private_slots: HashMap::new(),
            object_private_brands: HashMap::new(),
            next_private_brand: 0,
            module_cache: HashMap::new(),
            module_exports_stack: Vec::new(),
            module_base_dirs: Vec::new(),
            microtask_queue: VecDeque::new(),
            call_stack: Vec::new(),
        }
    }

    fn generator_result_object(value: JsValue, done: bool) -> JsValue {
        let mut result = HashMap::new();
        result.insert("value".to_string(), PropertyValue::Data(value));
        result.insert(
            "done".to_string(),
            PropertyValue::Data(JsValue::Boolean(done)),
        );
        JsValue::Object(Rc::new(RefCell::new(result)))
    }

    fn promise_with_state(state: PromiseState) -> JsValue {
        JsValue::Promise(Rc::new(RefCell::new(PromiseValue {
            state,
            reactions: Vec::new(),
        })))
    }

    fn pending_promise() -> Rc<RefCell<PromiseValue>> {
        Rc::new(RefCell::new(PromiseValue {
            state: PromiseState::Pending,
            reactions: Vec::new(),
        }))
    }

    fn resolved_promise(value: JsValue) -> JsValue {
        Self::promise_with_state(PromiseState::Fulfilled(value))
    }

    fn rejected_promise(reason: JsValue) -> JsValue {
        Self::promise_with_state(PromiseState::Rejected(reason))
    }

    fn async_generator_result_value(
        &mut self,
        value: JsValue,
        done: bool,
    ) -> Result<JsValue, RuntimeError> {
        match value {
            JsValue::Promise(promise) => {
                let result_promise = Self::pending_promise();
                let mapper = JsValue::BuiltinFunction(Rc::new(
                    BuiltinFunction::AsyncGeneratorResultMapper { done },
                ));
                self.attach_promise_reaction(
                    promise,
                    PromiseReaction {
                        kind: PromiseReactionKind::Then {
                            on_fulfilled: Some(mapper),
                            on_rejected: None,
                        },
                        result_promise: Rc::clone(&result_promise),
                    },
                );
                Ok(JsValue::Promise(result_promise))
            }
            other => Ok(Self::resolved_promise(Self::generator_result_object(
                other, done,
            ))),
        }
    }

    fn to_rejection_value(&self, error: RuntimeError) -> JsValue {
        match error {
            RuntimeError::Throw(value) => value,
            other => JsValue::String(other.to_string()),
        }
    }

    fn await_value(&mut self, value: JsValue) -> Result<JsValue, RuntimeError> {
        match value {
            JsValue::Promise(promise) => {
                self.drain_microtasks()?;
                self.drain_microtasks_until_promise_settled(&promise)?;
                match &promise.borrow().state {
                    PromiseState::Pending => Err(RuntimeError::TypeError(
                        "await on pending promise could not be completed".into(),
                    )),
                    PromiseState::Fulfilled(value) => Ok(value.clone()),
                    PromiseState::Rejected(reason) => Err(RuntimeError::Throw(reason.clone())),
                }
            }
            other => Ok(other),
        }
    }

    fn attach_promise_reaction(
        &mut self,
        promise: Rc<RefCell<PromiseValue>>,
        reaction: PromiseReaction,
    ) {
        let settled_state = {
            let mut borrowed = promise.borrow_mut();
            match &borrowed.state {
                PromiseState::Pending => {
                    borrowed.reactions.push(reaction);
                    return;
                }
                PromiseState::Fulfilled(value) => PromiseState::Fulfilled(value.clone()),
                PromiseState::Rejected(reason) => PromiseState::Rejected(reason.clone()),
            }
        };

        self.microtask_queue.push_back(Microtask::PromiseReaction {
            state: settled_state,
            reaction,
        });
    }

    fn settle_promise(&mut self, promise: Rc<RefCell<PromiseValue>>, state: PromiseState) {
        let reactions = {
            let mut borrowed = promise.borrow_mut();
            if !matches!(borrowed.state, PromiseState::Pending) {
                return;
            }
            borrowed.state = state.clone();
            std::mem::take(&mut borrowed.reactions)
        };

        for reaction in reactions {
            self.microtask_queue.push_back(Microtask::PromiseReaction {
                state: state.clone(),
                reaction,
            });
        }
    }

    fn resolve_promise_value(&mut self, promise: Rc<RefCell<PromiseValue>>, value: JsValue) {
        match value {
            JsValue::Promise(inner) => {
                if Rc::ptr_eq(&promise, &inner) {
                    self.settle_promise(
                        promise,
                        PromiseState::Rejected(JsValue::String(
                            "Chaining cycle detected for promise".to_string(),
                        )),
                    );
                    return;
                }

                self.attach_promise_reaction(
                    inner,
                    PromiseReaction {
                        kind: PromiseReactionKind::Then {
                            on_fulfilled: None,
                            on_rejected: None,
                        },
                        result_promise: promise,
                    },
                );
            }
            other => self.settle_promise(promise, PromiseState::Fulfilled(other)),
        }
    }

    fn process_then_reaction(
        &mut self,
        result_promise: Rc<RefCell<PromiseValue>>,
        state: PromiseState,
        on_fulfilled: Option<JsValue>,
        on_rejected: Option<JsValue>,
    ) {
        match state {
            PromiseState::Fulfilled(value) => {
                if let Some(handler) = on_fulfilled
                    && !matches!(handler, JsValue::Undefined)
                {
                    match self.invoke_callable(handler, JsValue::Undefined, vec![value]) {
                        Ok(result) => self.resolve_promise_value(result_promise, result),
                        Err(error) => self.settle_promise(
                            result_promise,
                            PromiseState::Rejected(self.to_rejection_value(error)),
                        ),
                    }
                } else {
                    self.settle_promise(result_promise, PromiseState::Fulfilled(value));
                }
            }
            PromiseState::Rejected(reason) => {
                if let Some(handler) = on_rejected
                    && !matches!(handler, JsValue::Undefined)
                {
                    match self.invoke_callable(handler, JsValue::Undefined, vec![reason]) {
                        Ok(result) => self.resolve_promise_value(result_promise, result),
                        Err(error) => self.settle_promise(
                            result_promise,
                            PromiseState::Rejected(self.to_rejection_value(error)),
                        ),
                    }
                } else {
                    self.settle_promise(result_promise, PromiseState::Rejected(reason));
                }
            }
            PromiseState::Pending => {}
        }
    }

    fn process_finally_reaction(
        &mut self,
        result_promise: Rc<RefCell<PromiseValue>>,
        original_state: PromiseState,
        on_finally: Option<JsValue>,
    ) {
        let handler_result = if let Some(handler) = on_finally {
            if matches!(handler, JsValue::Undefined) {
                None
            } else {
                match self.invoke_callable(handler, JsValue::Undefined, vec![]) {
                    Ok(value) => Some(value),
                    Err(error) => {
                        self.settle_promise(
                            result_promise,
                            PromiseState::Rejected(self.to_rejection_value(error)),
                        );
                        return;
                    }
                }
            }
        } else {
            None
        };

        match handler_result {
            Some(JsValue::Promise(promise)) => {
                self.attach_promise_reaction(
                    promise,
                    PromiseReaction {
                        kind: PromiseReactionKind::FinallyPassThrough { original_state },
                        result_promise,
                    },
                );
            }
            Some(_) | None => self.settle_promise(result_promise, original_state),
        }
    }

    fn run_microtask(&mut self, task: Microtask) {
        match task {
            Microtask::PromiseReaction { state, reaction } => match reaction.kind {
                PromiseReactionKind::Then {
                    on_fulfilled,
                    on_rejected,
                } => self.process_then_reaction(
                    reaction.result_promise,
                    state,
                    on_fulfilled,
                    on_rejected,
                ),
                PromiseReactionKind::Finally { on_finally } => {
                    self.process_finally_reaction(reaction.result_promise, state, on_finally)
                }
                PromiseReactionKind::FinallyPassThrough { original_state } => match state {
                    PromiseState::Fulfilled(_) => {
                        self.settle_promise(reaction.result_promise, original_state)
                    }
                    PromiseState::Rejected(reason) => {
                        self.settle_promise(reaction.result_promise, PromiseState::Rejected(reason))
                    }
                    PromiseState::Pending => {}
                },
            },
        }
    }

    fn drain_microtasks(&mut self) -> Result<(), RuntimeError> {
        while let Some(task) = self.microtask_queue.pop_front() {
            self.run_microtask(task);
        }
        Ok(())
    }

    fn drain_microtasks_until_promise_settled(
        &mut self,
        promise: &Rc<RefCell<PromiseValue>>,
    ) -> Result<(), RuntimeError> {
        while matches!(promise.borrow().state, PromiseState::Pending)
            && !self.microtask_queue.is_empty()
        {
            if let Some(task) = self.microtask_queue.pop_front() {
                self.run_microtask(task);
            }
        }
        Ok(())
    }

    fn invoke_getter(
        &mut self,
        getter: JsValue,
        this_value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        self.invoke_callable(getter, this_value, vec![])
    }

    fn create_generator_iterator(&self, state: Rc<RefCell<GeneratorState>>) -> JsValue {
        let mut iterator = HashMap::new();
        iterator.insert(
            "__generator_state__".to_string(),
            PropertyValue::Data(JsValue::GeneratorState(Rc::clone(&state))),
        );
        iterator.insert(
            "next".to_string(),
            PropertyValue::Data(native_fn("next", generator_next_native)),
        );
        iterator.insert(
            "return".to_string(),
            PropertyValue::Data(native_fn("return", generator_return_native)),
        );
        iterator.insert(
            "throw".to_string(),
            PropertyValue::Data(native_fn("throw", generator_throw_native)),
        );
        JsValue::Object(Rc::new(RefCell::new(iterator)))
    }

    fn begin_iteration(&mut self, value: JsValue) -> Result<IteratorCursor, RuntimeError> {
        match value {
            JsValue::Array(values) => Ok(IteratorCursor::Array { values, index: 0 }),
            JsValue::String(value) => Ok(IteratorCursor::String {
                chars: value
                    .chars()
                    .map(|ch| JsValue::String(ch.to_string()))
                    .collect(),
                index: 0,
            }),
            JsValue::Object(map) => {
                let next = get_object_property(&map, "next");
                if matches!(next, JsValue::Undefined) {
                    return Err(RuntimeError::TypeError("value is not iterable".into()));
                }
                Ok(IteratorCursor::Object { map, done: false })
            }
            _ => Err(RuntimeError::TypeError("value is not iterable".into())),
        }
    }

    fn iterator_step(
        &mut self,
        cursor: &mut IteratorCursor,
        await_result: bool,
    ) -> Result<IteratorStep, RuntimeError> {
        self.iterator_resume(cursor, ResumeAction::Next(JsValue::Undefined), await_result)
    }

    fn iterator_resume(
        &mut self,
        cursor: &mut IteratorCursor,
        action: ResumeAction,
        await_result: bool,
    ) -> Result<IteratorStep, RuntimeError> {
        match cursor {
            IteratorCursor::Array { values, index } => {
                if let ResumeAction::Return(value) = action {
                    *index = values.borrow().len();
                    return Ok(IteratorStep::Complete(value));
                }
                if let ResumeAction::Throw(value) = action {
                    return Err(RuntimeError::Throw(value));
                }
                let next = values.borrow().get(*index).cloned();
                if let Some(value) = next {
                    *index += 1;
                    Ok(IteratorStep::Yield(value))
                } else {
                    Ok(IteratorStep::Complete(JsValue::Undefined))
                }
            }
            IteratorCursor::String { chars, index } => {
                if let ResumeAction::Return(value) = action {
                    *index = chars.len();
                    return Ok(IteratorStep::Complete(value));
                }
                if let ResumeAction::Throw(value) = action {
                    return Err(RuntimeError::Throw(value));
                }
                if let Some(value) = chars.get(*index).cloned() {
                    *index += 1;
                    Ok(IteratorStep::Yield(value))
                } else {
                    Ok(IteratorStep::Complete(JsValue::Undefined))
                }
            }
            IteratorCursor::Object { map, done } => {
                if *done {
                    return Ok(IteratorStep::Complete(JsValue::Undefined));
                }

                let (method_name, arg) = match action {
                    ResumeAction::Next(value) => ("next", value),
                    ResumeAction::Return(value) => ("return", value),
                    ResumeAction::Throw(value) => ("throw", value),
                };
                let method = get_object_property(map, method_name);
                if matches!(method, JsValue::Undefined) {
                    *done = true;
                    return match method_name {
                        "return" => Ok(IteratorStep::Complete(arg)),
                        "throw" => Err(RuntimeError::TypeError(
                            "iterator does not provide a throw method".into(),
                        )),
                        _ => Err(RuntimeError::TypeError("value is not iterable".into())),
                    };
                }

                let mut step =
                    self.invoke_callable(method, JsValue::Object(Rc::clone(map)), vec![arg])?;
                if await_result {
                    step = self.await_value(step)?;
                }
                match step {
                    JsValue::Object(step_map) => {
                        let step_done = get_object_property(&step_map, "done").is_truthy();
                        let value = get_object_property(&step_map, "value");
                        if step_done {
                            *done = true;
                            Ok(IteratorStep::Complete(value))
                        } else {
                            Ok(IteratorStep::Yield(value))
                        }
                    }
                    _ => Err(RuntimeError::TypeError(
                        "iterator result is not an object".into(),
                    )),
                }
            }
        }
    }

    fn close_iterator(
        &mut self,
        cursor: &mut IteratorCursor,
        await_result: bool,
    ) -> Result<(), RuntimeError> {
        let _ = self.iterator_resume(
            cursor,
            ResumeAction::Return(JsValue::Undefined),
            await_result,
        );
        Ok(())
    }

    fn map_generator_execution(
        &mut self,
        exec: GeneratorExecution,
        on_complete: GeneratorValueContinuation,
        on_error: GeneratorErrorContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        match exec {
            GeneratorExecution::Complete(value) => on_complete(self, value),
            GeneratorExecution::Yielded {
                value,
                continuation,
            } => Ok(GeneratorExecution::Yielded {
                value,
                continuation: Rc::new(move |interp, action| match continuation(interp, action) {
                    Ok(exec) => interp.map_generator_execution(
                        exec,
                        Rc::clone(&on_complete),
                        Rc::clone(&on_error),
                    ),
                    Err(error) => on_error(interp, error),
                }),
            }),
        }
    }

    fn continue_generator_execution(
        &mut self,
        exec: GeneratorExecution,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        self.map_generator_execution(exec, on_complete, Rc::new(|_, error| Err(error)))
    }

    fn yield_generator_value(
        &mut self,
        value: JsValue,
        on_resume: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        Ok(GeneratorExecution::Yielded {
            value,
            continuation: Rc::new(move |interp, action| match action {
                ResumeAction::Next(value) => on_resume(interp, value),
                ResumeAction::Return(value) => Err(RuntimeError::Return(value)),
                ResumeAction::Throw(value) => Err(RuntimeError::Throw(value)),
            }),
        })
    }

    fn expression_contains_yield(&self, expr: &Expression) -> bool {
        match expr {
            Expression::YieldExpression { .. } => true,
            Expression::Literal(_)
            | Expression::Identifier(_)
            | Expression::ThisExpression
            | Expression::SuperExpression
            | Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)
            | Expression::PrivateIdentifier(_) => false,
            Expression::BinaryExpression(bin) => {
                self.expression_contains_yield(&bin.left)
                    || self.expression_contains_yield(&bin.right)
            }
            Expression::UnaryExpression(unary) => self.expression_contains_yield(&unary.argument),
            Expression::AssignmentExpression(assign) => {
                self.expression_contains_yield(&assign.left)
                    || self.expression_contains_yield(&assign.right)
            }
            Expression::ArrayExpression(elements) => elements
                .iter()
                .flatten()
                .any(|expr| self.expression_contains_yield(expr)),
            Expression::ObjectExpression(properties) => properties.iter().any(|property| {
                let key_has_yield = matches!(
                    &property.key,
                    ObjectKey::Computed(expr) if self.expression_contains_yield(expr)
                );
                key_has_yield || self.expression_contains_yield(&property.value)
            }),
            Expression::MemberExpression(mem) => {
                self.expression_contains_yield(&mem.object)
                    || (mem.computed && self.expression_contains_yield(&mem.property))
            }
            Expression::CallExpression(call) | Expression::NewExpression(call) => {
                self.expression_contains_yield(&call.callee)
                    || call
                        .arguments
                        .iter()
                        .any(|arg| self.expression_contains_yield(arg))
            }
            Expression::UpdateExpression(update) => {
                self.expression_contains_yield(&update.argument)
            }
            Expression::SequenceExpression(seq) => {
                seq.iter().any(|expr| self.expression_contains_yield(expr))
            }
            Expression::ConditionalExpression {
                test,
                consequent,
                alternate,
            } => {
                self.expression_contains_yield(test)
                    || self.expression_contains_yield(consequent)
                    || self.expression_contains_yield(alternate)
            }
            Expression::SpreadElement(expr) | Expression::AwaitExpression(expr) => {
                self.expression_contains_yield(expr)
            }
            Expression::TemplateLiteral(parts) | Expression::TaggedTemplateExpression(_, parts) => {
                parts.iter().any(|part| match part {
                    TemplatePart::String(_) => false,
                    TemplatePart::Expr(expr) => self.expression_contains_yield(expr),
                })
            }
        }
    }

    fn statement_contains_yield(&self, stmt: &Statement) -> bool {
        match stmt {
            Statement::ExpressionStatement(expr) => self.expression_contains_yield(expr),
            Statement::BlockStatement(block) => block
                .body
                .iter()
                .any(|stmt| self.statement_contains_yield(stmt)),
            Statement::IfStatement(if_stmt) => {
                self.expression_contains_yield(&if_stmt.test)
                    || self.statement_contains_yield(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|stmt| self.statement_contains_yield(stmt))
            }
            Statement::TryStatement(try_stmt) => {
                try_stmt
                    .block
                    .body
                    .iter()
                    .any(|stmt| self.statement_contains_yield(stmt))
                    || try_stmt.handler.as_ref().is_some_and(|handler| {
                        handler
                            .param
                            .as_ref()
                            .is_some_and(|param| self.expression_contains_yield(param))
                            || handler
                                .body
                                .body
                                .iter()
                                .any(|stmt| self.statement_contains_yield(stmt))
                    })
                    || try_stmt.finalizer.as_ref().is_some_and(|finalizer| {
                        finalizer
                            .body
                            .iter()
                            .any(|stmt| self.statement_contains_yield(stmt))
                    })
            }
            Statement::VariableDeclaration(decl) => decl.declarations.iter().any(|declarator| {
                self.expression_contains_yield(&declarator.id)
                    || declarator
                        .init
                        .as_ref()
                        .is_some_and(|expr| self.expression_contains_yield(expr))
            }),
            Statement::ReturnStatement(expr) => expr
                .as_ref()
                .is_some_and(|expr| self.expression_contains_yield(expr)),
            Statement::ThrowStatement(expr) => self.expression_contains_yield(expr),
            Statement::ForStatement(for_stmt) => {
                for_stmt
                    .init
                    .as_ref()
                    .is_some_and(|stmt| self.statement_contains_yield(stmt))
                    || for_stmt
                        .test
                        .as_ref()
                        .is_some_and(|expr| self.expression_contains_yield(expr))
                    || for_stmt
                        .update
                        .as_ref()
                        .is_some_and(|expr| self.expression_contains_yield(expr))
                    || self.statement_contains_yield(&for_stmt.body)
            }
            Statement::WhileStatement(while_stmt) | Statement::DoWhileStatement(while_stmt) => {
                self.expression_contains_yield(&while_stmt.test)
                    || self.statement_contains_yield(&while_stmt.body)
            }
            Statement::ForInStatement(for_in) => {
                extract_for_binding(&for_in.left)
                    .is_some_and(|(pattern, _)| self.expression_contains_yield(pattern))
                    || self.expression_contains_yield(&for_in.right)
                    || self.statement_contains_yield(&for_in.body)
            }
            Statement::ForOfStatement(for_of) => {
                extract_for_binding(&for_of.left)
                    .is_some_and(|(pattern, _)| self.expression_contains_yield(pattern))
                    || self.expression_contains_yield(&for_of.right)
                    || self.statement_contains_yield(&for_of.body)
            }
            Statement::WithStatement(with_stmt) => {
                self.expression_contains_yield(&with_stmt.object)
                    || self.statement_contains_yield(&with_stmt.body)
            }
            Statement::SwitchStatement(switch) => {
                self.expression_contains_yield(&switch.discriminant)
                    || switch.cases.iter().any(|case| {
                        case.test
                            .as_ref()
                            .is_some_and(|expr| self.expression_contains_yield(expr))
                            || case
                                .consequent
                                .iter()
                                .any(|stmt| self.statement_contains_yield(stmt))
                    })
            }
            Statement::LabeledStatement(labeled) => self.statement_contains_yield(&labeled.body),
            Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::ImportDeclaration(_)
            | Statement::ExportNamedDeclaration(_)
            | Statement::ExportDefaultDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::EmptyStatement => false,
        }
    }

    fn eval_generator_block(
        &mut self,
        body: Vec<Statement<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        last_value: JsValue,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= body.len() {
            return Ok(GeneratorExecution::Complete(last_value));
        }

        let next_stmt = body[index].clone();
        let body_clone = body.clone();
        let env_clone = Rc::clone(&env);
        let exec = self.eval_generator_statement(next_stmt, env)?;
        self.continue_generator_execution(
            exec,
            Rc::new(move |interp, value| {
                interp.eval_generator_block(
                    body_clone.clone(),
                    Rc::clone(&env_clone),
                    index + 1,
                    value,
                )
            }),
        )
    }

    fn eval_generator_variable_declaration(
        &mut self,
        decl: VariableDeclaration<'static>,
        env: Rc<RefCell<Environment>>,
        index: usize,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= decl.declarations.len() {
            return Ok(GeneratorExecution::Complete(JsValue::Undefined));
        }

        let declarator = decl.declarations[index].clone();
        let decl_clone = decl.clone();
        let env_clone = Rc::clone(&env);
        let id = declarator.id.clone();

        match declarator.init {
            Some(init) => self.eval_generator_expression(
                init,
                env,
                Rc::new(move |interp, value| {
                    interp.eval_generator_assign_pattern(
                        id.clone(),
                        value,
                        Rc::clone(&env_clone),
                        true,
                        Rc::new({
                            let decl_clone = decl_clone.clone();
                            let env_clone = Rc::clone(&env_clone);
                            move |interp| {
                                interp.eval_generator_variable_declaration(
                                    decl_clone.clone(),
                                    Rc::clone(&env_clone),
                                    index + 1,
                                )
                            }
                        }),
                    )
                }),
            ),
            None => self.eval_generator_assign_pattern(
                id,
                JsValue::Undefined,
                Rc::clone(&env_clone),
                true,
                Rc::new(move |interp| {
                    interp.eval_generator_variable_declaration(
                        decl_clone.clone(),
                        Rc::clone(&env_clone),
                        index + 1,
                    )
                }),
            ),
        }
    }

    fn eval_generator_assign_array_pattern(
        &mut self,
        elements: Vec<Option<Expression<'static>>>,
        items: Rc<Vec<JsValue>>,
        env: Rc<RefCell<Environment>>,
        declare: bool,
        index: usize,
        on_complete: GeneratorCompletionContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= elements.len() {
            return on_complete(self);
        }

        match elements[index].clone() {
            None => self.eval_generator_assign_array_pattern(
                elements,
                items,
                env,
                declare,
                index + 1,
                on_complete,
            ),
            Some(Expression::SpreadElement(rest_pattern)) => {
                let rest_items = items.iter().skip(index).cloned().collect::<Vec<_>>();
                self.eval_generator_assign_pattern(
                    *rest_pattern,
                    JsValue::Array(Rc::new(RefCell::new(rest_items))),
                    env,
                    declare,
                    on_complete,
                )
            }
            Some(pattern) => {
                let item = items.get(index).cloned().unwrap_or(JsValue::Undefined);
                let elements_clone = elements.clone();
                let items_clone = Rc::clone(&items);
                let env_clone = Rc::clone(&env);
                self.eval_generator_assign_pattern(
                    pattern,
                    item,
                    env,
                    declare,
                    Rc::new(move |interp| {
                        interp.eval_generator_assign_array_pattern(
                            elements_clone.clone(),
                            Rc::clone(&items_clone),
                            Rc::clone(&env_clone),
                            declare,
                            index + 1,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
        }
    }

    fn eval_generator_assign_object_property(
        &mut self,
        properties: Vec<ObjectProperty<'static>>,
        source_value: JsValue,
        env: Rc<RefCell<Environment>>,
        declare: bool,
        index: usize,
        excluded: HashSet<String>,
        property_key: String,
        on_complete: GeneratorCompletionContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let mut next_excluded = excluded;
        next_excluded.insert(property_key.clone());
        let property_value = self.read_property_for_pattern(&source_value, &property_key)?;
        let property = properties[index].clone();
        let properties_clone = properties.clone();
        let source_clone = source_value.clone();
        let env_clone = Rc::clone(&env);
        self.eval_generator_assign_pattern(
            property.value,
            property_value,
            env,
            declare,
            Rc::new(move |interp| {
                interp.eval_generator_assign_object_pattern(
                    properties_clone.clone(),
                    source_clone.clone(),
                    Rc::clone(&env_clone),
                    declare,
                    index + 1,
                    next_excluded.clone(),
                    Rc::clone(&on_complete),
                )
            }),
        )
    }

    fn eval_generator_assign_object_pattern(
        &mut self,
        properties: Vec<ObjectProperty<'static>>,
        source_value: JsValue,
        env: Rc<RefCell<Environment>>,
        declare: bool,
        index: usize,
        excluded: HashSet<String>,
        on_complete: GeneratorCompletionContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= properties.len() {
            return on_complete(self);
        }

        let property = properties[index].clone();
        if let Expression::SpreadElement(rest_pattern) = property.value {
            let rest = self.object_rest_for_pattern(&source_value, &excluded)?;
            return self.eval_generator_assign_pattern(
                *rest_pattern,
                rest,
                env,
                declare,
                on_complete,
            );
        }

        match property.key {
            ObjectKey::Identifier(name) | ObjectKey::String(name) => self
                .eval_generator_assign_object_property(
                    properties,
                    source_value,
                    env,
                    declare,
                    index,
                    excluded,
                    name.to_string(),
                    on_complete,
                ),
            ObjectKey::Number(number) => self.eval_generator_assign_object_property(
                properties,
                source_value,
                env,
                declare,
                index,
                excluded,
                number.to_string(),
                on_complete,
            ),
            ObjectKey::Computed(expr) => {
                let properties_clone = properties.clone();
                let source_clone = source_value.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    *expr,
                    env,
                    Rc::new(move |interp, value| {
                        interp.eval_generator_assign_object_property(
                            properties_clone.clone(),
                            source_clone.clone(),
                            Rc::clone(&env_clone),
                            declare,
                            index,
                            excluded.clone(),
                            interp.property_key_from_value(value),
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
            ObjectKey::PrivateIdentifier(_) => Err(RuntimeError::SyntaxError(
                "private identifier cannot appear in object patterns".into(),
            )),
        }
    }

    fn eval_generator_assign_pattern(
        &mut self,
        pattern: Expression<'static>,
        value: JsValue,
        env: Rc<RefCell<Environment>>,
        declare: bool,
        on_complete: GeneratorCompletionContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if !self.expression_contains_yield(&pattern) {
            self.assign_pattern(&pattern, value, env, declare)?;
            return on_complete(self);
        }

        match pattern {
            Expression::Identifier(name) => {
                self.assign_identifier(name, value, env, declare)?;
                on_complete(self)
            }
            Expression::AssignmentExpression(assign)
                if matches!(assign.operator, AssignmentOperator::Assign) =>
            {
                if matches!(value, JsValue::Undefined) {
                    let left = assign.left.clone();
                    let env_clone = Rc::clone(&env);
                    self.eval_generator_expression(
                        assign.right,
                        env,
                        Rc::new(move |interp, next_value| {
                            interp.eval_generator_assign_pattern(
                                left.clone(),
                                next_value,
                                Rc::clone(&env_clone),
                                declare,
                                Rc::clone(&on_complete),
                            )
                        }),
                    )
                } else {
                    self.eval_generator_assign_pattern(
                        assign.left,
                        value,
                        env,
                        declare,
                        on_complete,
                    )
                }
            }
            Expression::ArrayExpression(elements) => {
                if matches!(value, JsValue::Null | JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "Cannot destructure null or undefined".into(),
                    ));
                }
                let items = Rc::new(self.collect_iterable_items(value)?);
                self.eval_generator_assign_array_pattern(
                    elements,
                    items,
                    env,
                    declare,
                    0,
                    on_complete,
                )
            }
            Expression::ObjectExpression(properties) => {
                if matches!(value, JsValue::Null | JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "Cannot destructure null or undefined".into(),
                    ));
                }
                self.eval_generator_assign_object_pattern(
                    properties,
                    value,
                    env,
                    declare,
                    0,
                    HashSet::new(),
                    on_complete,
                )
            }
            Expression::MemberExpression(member) if !declare => {
                if let Some(name) = self.member_private_name(&member) {
                    let value_clone = value.clone();
                    let env_clone = Rc::clone(&env);
                    let name = name.to_string();
                    let complete_write =
                        Rc::new(move |interp: &mut Interpreter, object: JsValue| {
                            interp.write_private_member_value(
                                object,
                                &name,
                                value_clone.clone(),
                                Rc::clone(&env_clone),
                            )?;
                            on_complete(interp)
                        });

                    if self.expression_contains_yield(&member.object) {
                        return self.eval_generator_expression(
                            member.object,
                            Rc::clone(&env),
                            Rc::new(move |interp, object| complete_write(interp, object)),
                        );
                    }

                    let object = self.eval_expression(&member.object, Rc::clone(&env))?;
                    return complete_write(self, object);
                }

                if matches!(member.object, Expression::SuperExpression) {
                    let value_clone = value.clone();
                    let env_clone = Rc::clone(&env);
                    let complete_write =
                        Rc::new(move |interp: &mut Interpreter, property_key: String| {
                            interp.write_super_member_value(
                                Rc::clone(&env_clone),
                                &property_key,
                                value_clone.clone(),
                            )?;
                            on_complete(interp)
                        });

                    if member.computed && self.expression_contains_yield(&member.property) {
                        return self.eval_generator_property_key(
                            true,
                            member.property,
                            env,
                            Rc::new(move |interp, property_key| {
                                complete_write(interp, property_key)
                            }),
                        );
                    }

                    let property_key = self.member_property_key(&member, env)?;
                    return complete_write(self, property_key);
                }

                let value_clone = value.clone();
                let complete_write = Rc::new(
                    move |interp: &mut Interpreter, object: JsValue, property_key: String| {
                        interp.write_member_value(object, &property_key, value_clone.clone())?;
                        on_complete(interp)
                    },
                );

                if self.expression_contains_yield(&member.object) {
                    let member_clone = member.clone();
                    let env_clone = Rc::clone(&env);
                    return self.eval_generator_expression(
                        member.object,
                        Rc::clone(&env),
                        Rc::new(move |interp, object| {
                            interp.eval_generator_property_key(
                                member_clone.computed,
                                member_clone.property.clone(),
                                Rc::clone(&env_clone),
                                Rc::new({
                                    let complete_write = Rc::clone(&complete_write);
                                    move |interp, property_key| {
                                        complete_write(interp, object.clone(), property_key)
                                    }
                                }),
                            )
                        }),
                    );
                }

                if member.computed && self.expression_contains_yield(&member.property) {
                    let object = self.eval_expression(&member.object, Rc::clone(&env))?;
                    return self.eval_generator_property_key(
                        true,
                        member.property,
                        env,
                        Rc::new(move |interp, property_key| {
                            complete_write(interp, object.clone(), property_key)
                        }),
                    );
                }

                let object = self.eval_expression(&member.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(&member, env)?;
                complete_write(self, object, property_key)
            }
            Expression::SpreadElement(inner) => {
                self.eval_generator_assign_pattern(*inner, value, env, declare, on_complete)
            }
            _ => Err(RuntimeError::SyntaxError(
                "invalid destructuring pattern".into(),
            )),
        }
    }

    fn eval_generator_sequence(
        &mut self,
        expressions: Vec<Expression<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        last_value: JsValue,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= expressions.len() {
            return on_complete(self, last_value);
        }

        let expressions_clone = expressions.clone();
        let env_clone = Rc::clone(&env);
        self.eval_generator_expression(
            expressions[index].clone(),
            env,
            Rc::new(move |interp, value| {
                interp.eval_generator_sequence(
                    expressions_clone.clone(),
                    Rc::clone(&env_clone),
                    index + 1,
                    value,
                    Rc::clone(&on_complete),
                )
            }),
        )
    }

    fn run_generator_finalizer(
        &mut self,
        finalizer: Option<BlockStatement<'static>>,
        env: Rc<RefCell<Environment>>,
        prior: Result<JsValue, RuntimeError>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        match finalizer {
            Some(finalizer) => {
                let finalizer_stmt = Statement::BlockStatement(finalizer);
                let prior_complete = prior.clone();
                let exec = self.eval_generator_statement(finalizer_stmt, env)?;
                self.map_generator_execution(
                    exec,
                    Rc::new(move |_, _| match &prior_complete {
                        Ok(value) => Ok(GeneratorExecution::Complete(value.clone())),
                        Err(error) => Err(error.clone()),
                    }),
                    Rc::new(|_, error| Err(error)),
                )
            }
            None => match prior {
                Ok(value) => Ok(GeneratorExecution::Complete(value)),
                Err(error) => Err(error),
            },
        }
    }

    fn eval_generator_catch_body(
        &mut self,
        body: BlockStatement<'static>,
        catch_env: Rc<RefCell<Environment>>,
        outer_env: Rc<RefCell<Environment>>,
        finalizer: Option<BlockStatement<'static>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let catch_exec =
            self.eval_generator_statement(Statement::BlockStatement(body), catch_env)?;
        let catch_complete_finalizer = finalizer.clone();
        let catch_complete_env = Rc::clone(&outer_env);
        let catch_error_finalizer = finalizer.clone();
        let catch_error_env = Rc::clone(&outer_env);
        self.map_generator_execution(
            catch_exec,
            Rc::new(move |interp, value| {
                interp.run_generator_finalizer(
                    catch_complete_finalizer.clone(),
                    Rc::clone(&catch_complete_env),
                    Ok(value),
                )
            }),
            Rc::new(move |interp, error| {
                interp.run_generator_finalizer(
                    catch_error_finalizer.clone(),
                    Rc::clone(&catch_error_env),
                    Err(error),
                )
            }),
        )
    }

    fn eval_generator_catch_clause(
        &mut self,
        handler: CatchClause<'static>,
        error: RuntimeError,
        outer_env: Rc<RefCell<Environment>>,
        finalizer: Option<BlockStatement<'static>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let catch_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&outer_env)))));
        let error_value = match &error {
            RuntimeError::Throw(value) => value.clone(),
            other => JsValue::String(other.to_string()),
        };

        if let Some(param) = handler.param.clone() {
            if self.expression_contains_yield(&param) {
                let body = handler.body.clone();
                let catch_env_clone = Rc::clone(&catch_env);
                let outer_env_clone = Rc::clone(&outer_env);
                return self.eval_generator_assign_pattern(
                    param,
                    error_value,
                    catch_env,
                    true,
                    Rc::new(move |interp| {
                        interp.eval_generator_catch_body(
                            body.clone(),
                            Rc::clone(&catch_env_clone),
                            Rc::clone(&outer_env_clone),
                            finalizer.clone(),
                        )
                    }),
                );
            }

            self.assign_pattern(&param, error_value, Rc::clone(&catch_env), true)?;
        }

        self.eval_generator_catch_body(handler.body, catch_env, outer_env, finalizer)
    }

    fn eval_generator_for_in_iteration_body(
        &mut self,
        binding: Option<(Expression<'static>, bool)>,
        body: Statement<'static>,
        env: Rc<RefCell<Environment>>,
        keys: Rc<Vec<String>>,
        index: usize,
        last_value: JsValue,
        label: Option<String>,
        iter_env: Rc<RefCell<Environment>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let binding_clone = binding.clone();
        let body_clone = body.clone();
        let env_clone = Rc::clone(&env);
        let keys_clone = Rc::clone(&keys);
        let continue_binding = binding.clone();
        let continue_body = body.clone();
        let continue_env = Rc::clone(&env);
        let continue_keys = Rc::clone(&keys);
        let last_for_error = last_value.clone();
        let label_for_complete = label.clone();
        let label_for_continue = label.clone();
        let label_for_error = label.clone();
        match self.eval_generator_statement(body.clone(), iter_env) {
            Ok(exec) => self.map_generator_execution(
                exec,
                Rc::new(move |interp, value| {
                    interp.eval_generator_for_in_loop(
                        binding_clone.clone(),
                        body_clone.clone(),
                        Rc::clone(&env_clone),
                        Rc::clone(&keys_clone),
                        index + 1,
                        value,
                        label_for_complete.clone(),
                    )
                }),
                Rc::new(move |interp, error| match error {
                    RuntimeError::Break(control_label)
                        if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                    {
                        Ok(GeneratorExecution::Complete(last_for_error.clone()))
                    }
                    RuntimeError::Continue(control_label)
                        if loop_control_matches(&control_label, label_for_continue.as_deref()) =>
                    {
                        interp.eval_generator_for_in_loop(
                            continue_binding.clone(),
                            continue_body.clone(),
                            Rc::clone(&continue_env),
                            Rc::clone(&continue_keys),
                            index + 1,
                            last_for_error.clone(),
                            label_for_continue.clone(),
                        )
                    }
                    other => Err(other),
                }),
            ),
            Err(RuntimeError::Break(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                Ok(GeneratorExecution::Complete(last_for_error))
            }
            Err(RuntimeError::Continue(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                self.eval_generator_for_in_loop(
                    continue_binding,
                    continue_body,
                    continue_env,
                    continue_keys,
                    index + 1,
                    last_value,
                    label,
                )
            }
            Err(other) => Err(other),
        }
    }

    fn eval_generator_for_of_iteration_body(
        &mut self,
        binding: Option<(Expression<'static>, bool)>,
        body: Statement<'static>,
        env: Rc<RefCell<Environment>>,
        cursor: Rc<RefCell<IteratorCursor>>,
        is_await: bool,
        last_value: JsValue,
        label: Option<String>,
        iter_env: Rc<RefCell<Environment>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let binding_clone = binding.clone();
        let body_clone = body.clone();
        let env_clone = Rc::clone(&env);
        let cursor_clone = Rc::clone(&cursor);
        let continue_binding = binding.clone();
        let continue_body = body.clone();
        let continue_env = Rc::clone(&env);
        let continue_cursor = Rc::clone(&cursor);
        let error_cursor = Rc::clone(&cursor);
        let last_for_error = last_value.clone();
        let label_for_complete = label.clone();
        let label_for_continue = label.clone();
        let label_for_error = label.clone();
        match self.eval_generator_statement(body.clone(), iter_env) {
            Ok(exec) => self.map_generator_execution(
                exec,
                Rc::new(move |interp, value| {
                    interp.eval_generator_for_of_loop(
                        binding_clone.clone(),
                        body_clone.clone(),
                        Rc::clone(&env_clone),
                        Rc::clone(&cursor_clone),
                        is_await,
                        value,
                        label_for_complete.clone(),
                    )
                }),
                Rc::new(move |interp, error| match error {
                    RuntimeError::Break(control_label)
                        if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                    {
                        let _ = interp.close_iterator(&mut cursor.borrow_mut(), is_await);
                        Ok(GeneratorExecution::Complete(last_for_error.clone()))
                    }
                    RuntimeError::Continue(control_label)
                        if loop_control_matches(&control_label, label_for_continue.as_deref()) =>
                    {
                        interp.eval_generator_for_of_loop(
                            continue_binding.clone(),
                            continue_body.clone(),
                            Rc::clone(&continue_env),
                            Rc::clone(&continue_cursor),
                            is_await,
                            last_for_error.clone(),
                            label_for_continue.clone(),
                        )
                    }
                    other => {
                        let _ = interp.close_iterator(&mut error_cursor.borrow_mut(), is_await);
                        Err(other)
                    }
                }),
            ),
            Err(RuntimeError::Break(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                let _ = self.close_iterator(&mut cursor.borrow_mut(), is_await);
                Ok(GeneratorExecution::Complete(last_for_error))
            }
            Err(RuntimeError::Continue(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                self.eval_generator_for_of_loop(
                    continue_binding,
                    continue_body,
                    continue_env,
                    continue_cursor,
                    is_await,
                    last_value,
                    label,
                )
            }
            Err(other) => {
                let _ = self.close_iterator(&mut cursor.borrow_mut(), is_await);
                Err(other)
            }
        }
    }

    fn eval_generator_while_loop(
        &mut self,
        while_stmt: WhileStatement<'static>,
        env: Rc<RefCell<Environment>>,
        last_value: JsValue,
        run_body_first: bool,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if run_body_first {
            self.check_timeout()?;
            let body = (*while_stmt.body).clone();
            let stmt_clone = while_stmt.clone();
            let env_for_complete = Rc::clone(&env);
            let env_for_error = Rc::clone(&env);
            let last_for_error = last_value.clone();
            let label_for_complete = label.clone();
            let label_for_error = label.clone();
            let exec = self.eval_generator_statement(body, env);
            return match exec {
                Ok(exec) => self.map_generator_execution(
                    exec,
                    Rc::new(move |interp, value| {
                        interp.eval_generator_while_loop(
                            stmt_clone.clone(),
                            Rc::clone(&env_for_complete),
                            value,
                            false,
                            label_for_complete.clone(),
                        )
                    }),
                    Rc::new(move |interp, error| match error {
                        RuntimeError::Break(control_label)
                            if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                        {
                            Ok(GeneratorExecution::Complete(last_for_error.clone()))
                        }
                        RuntimeError::Continue(control_label)
                            if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                        {
                            interp.eval_generator_while_loop(
                                while_stmt.clone(),
                                Rc::clone(&env_for_error),
                                last_for_error.clone(),
                                false,
                                label_for_error.clone(),
                            )
                        }
                        other => Err(other),
                    }),
                ),
                Err(RuntimeError::Break(control_label))
                    if loop_control_matches(&control_label, label.as_deref()) =>
                {
                    Ok(GeneratorExecution::Complete(last_for_error))
                }
                Err(RuntimeError::Continue(control_label))
                    if loop_control_matches(&control_label, label.as_deref()) =>
                {
                    self.eval_generator_while_loop(
                        while_stmt,
                        Rc::clone(&env_for_error),
                        last_value,
                        false,
                        label,
                    )
                }
                Err(other) => Err(other),
            };
        }

        let stmt_clone = while_stmt.clone();
        let env_clone = Rc::clone(&env);
        let label_clone = label.clone();
        self.eval_generator_expression(
            while_stmt.test.clone(),
            env,
            Rc::new(move |interp, test_value| {
                if !test_value.is_truthy() {
                    Ok(GeneratorExecution::Complete(last_value.clone()))
                } else {
                    interp.eval_generator_while_loop(
                        stmt_clone.clone(),
                        Rc::clone(&env_clone),
                        last_value.clone(),
                        true,
                        label_clone.clone(),
                    )
                }
            }),
        )
    }

    fn eval_generator_for_loop(
        &mut self,
        for_stmt: ForStatement<'static>,
        env: Rc<RefCell<Environment>>,
        last_value: JsValue,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        self.check_timeout()?;
        if let Some(test) = &for_stmt.test {
            let stmt_clone = for_stmt.clone();
            let env_clone = Rc::clone(&env);
            let last_clone = last_value.clone();
            let label_clone = label.clone();
            self.eval_generator_expression(
                test.clone(),
                env,
                Rc::new(move |interp, test_value| {
                    if !test_value.is_truthy() {
                        Ok(GeneratorExecution::Complete(last_clone.clone()))
                    } else {
                        interp.eval_generator_for_body(
                            stmt_clone.clone(),
                            Rc::clone(&env_clone),
                            last_clone.clone(),
                            label_clone.clone(),
                        )
                    }
                }),
            )
        } else {
            self.eval_generator_for_body(for_stmt, env, last_value, label)
        }
    }

    fn eval_generator_for_body(
        &mut self,
        for_stmt: ForStatement<'static>,
        env: Rc<RefCell<Environment>>,
        last_value: JsValue,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let body = (*for_stmt.body).clone();
        let stmt_clone = for_stmt.clone();
        let env_for_complete = Rc::clone(&env);
        let env_for_error = Rc::clone(&env);
        let last_for_error = last_value.clone();
        let label_for_complete = label.clone();
        let label_for_error = label.clone();
        match self.eval_generator_statement(body, env) {
            Ok(exec) => self.map_generator_execution(
                exec,
                Rc::new(move |interp, value| {
                    interp.eval_generator_for_update(
                        stmt_clone.clone(),
                        Rc::clone(&env_for_complete),
                        value,
                        label_for_complete.clone(),
                    )
                }),
                Rc::new(move |interp, error| match error {
                    RuntimeError::Break(control_label)
                        if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                    {
                        Ok(GeneratorExecution::Complete(last_for_error.clone()))
                    }
                    RuntimeError::Continue(control_label)
                        if loop_control_matches(&control_label, label_for_error.as_deref()) =>
                    {
                        interp.eval_generator_for_update(
                            for_stmt.clone(),
                            Rc::clone(&env_for_error),
                            last_for_error.clone(),
                            label_for_error.clone(),
                        )
                    }
                    other => Err(other),
                }),
            ),
            Err(RuntimeError::Break(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                Ok(GeneratorExecution::Complete(last_for_error))
            }
            Err(RuntimeError::Continue(control_label))
                if loop_control_matches(&control_label, label.as_deref()) =>
            {
                self.eval_generator_for_update(for_stmt, env_for_error, last_value, label)
            }
            Err(other) => Err(other),
        }
    }

    fn eval_generator_for_update(
        &mut self,
        for_stmt: ForStatement<'static>,
        env: Rc<RefCell<Environment>>,
        last_value: JsValue,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if let Some(update) = &for_stmt.update {
            let stmt_clone = for_stmt.clone();
            let env_clone = Rc::clone(&env);
            let last_clone = last_value.clone();
            let label_clone = label.clone();
            self.eval_generator_expression(
                update.clone(),
                env,
                Rc::new(move |interp, _| {
                    interp.eval_generator_for_loop(
                        stmt_clone.clone(),
                        Rc::clone(&env_clone),
                        last_clone.clone(),
                        label_clone.clone(),
                    )
                }),
            )
        } else {
            self.eval_generator_for_loop(for_stmt, env, last_value, label)
        }
    }

    fn eval_generator_for_in_loop(
        &mut self,
        binding: Option<(Expression<'static>, bool)>,
        body: Statement<'static>,
        env: Rc<RefCell<Environment>>,
        keys: Rc<Vec<String>>,
        index: usize,
        last_value: JsValue,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= keys.len() {
            return Ok(GeneratorExecution::Complete(last_value));
        }

        self.check_timeout()?;
        let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
        if let Some((pattern, declare)) = &binding {
            let binding_value = JsValue::String(keys[index].clone());
            if self.expression_contains_yield(pattern) {
                let binding_clone = binding.clone();
                let body_clone = body.clone();
                let env_clone = Rc::clone(&env);
                let keys_clone = Rc::clone(&keys);
                let label_clone = label.clone();
                let iter_env_clone = Rc::clone(&iter_env);
                return self.eval_generator_assign_pattern(
                    pattern.clone(),
                    binding_value,
                    Rc::clone(&iter_env),
                    *declare,
                    Rc::new(move |interp| {
                        interp.eval_generator_for_in_iteration_body(
                            binding_clone.clone(),
                            body_clone.clone(),
                            Rc::clone(&env_clone),
                            Rc::clone(&keys_clone),
                            index,
                            last_value.clone(),
                            label_clone.clone(),
                            Rc::clone(&iter_env_clone),
                        )
                    }),
                );
            }
            self.assign_pattern(pattern, binding_value, Rc::clone(&iter_env), *declare)?;
        }
        self.eval_generator_for_in_iteration_body(
            binding, body, env, keys, index, last_value, label, iter_env,
        )
    }

    fn eval_generator_for_of_loop(
        &mut self,
        binding: Option<(Expression<'static>, bool)>,
        body: Statement<'static>,
        env: Rc<RefCell<Environment>>,
        cursor: Rc<RefCell<IteratorCursor>>,
        is_await: bool,
        last_value: JsValue,
        label: Option<String>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        self.check_timeout()?;
        let step = {
            let mut cursor_borrow = cursor.borrow_mut();
            self.iterator_step(&mut cursor_borrow, is_await)?
        };
        let item = match step {
            IteratorStep::Yield(item) => item,
            IteratorStep::Complete(_) => return Ok(GeneratorExecution::Complete(last_value)),
        };

        let item = if is_await {
            self.await_value(item)?
        } else {
            item
        };
        let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
        if let Some((pattern, declare)) = &binding {
            if self.expression_contains_yield(pattern) {
                let binding_clone = binding.clone();
                let body_clone = body.clone();
                let env_clone = Rc::clone(&env);
                let cursor_clone = Rc::clone(&cursor);
                let label_clone = label.clone();
                let iter_env_clone = Rc::clone(&iter_env);
                return self.eval_generator_assign_pattern(
                    pattern.clone(),
                    item,
                    Rc::clone(&iter_env),
                    *declare,
                    Rc::new(move |interp| {
                        interp.eval_generator_for_of_iteration_body(
                            binding_clone.clone(),
                            body_clone.clone(),
                            Rc::clone(&env_clone),
                            Rc::clone(&cursor_clone),
                            is_await,
                            last_value.clone(),
                            label_clone.clone(),
                            Rc::clone(&iter_env_clone),
                        )
                    }),
                );
            }
            self.assign_pattern(pattern, item, Rc::clone(&iter_env), *declare)?;
        }
        self.eval_generator_for_of_iteration_body(
            binding, body, env, cursor, is_await, last_value, label, iter_env,
        )
    }

    fn eval_generator_property_key(
        &mut self,
        computed: bool,
        property: Expression<'static>,
        env: Rc<RefCell<Environment>>,
        on_complete: GeneratorStringContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if !computed {
            return match property {
                Expression::Identifier(name) => on_complete(self, name.to_string()),
                other => Err(RuntimeError::TypeError(format!(
                    "invalid member property: {other:?}"
                ))),
            };
        }

        self.eval_generator_expression(
            property,
            env,
            Rc::new(move |interp, value| {
                on_complete(interp, interp.property_key_from_value(value))
            }),
        )
    }

    fn eval_generator_array_expression(
        &mut self,
        elements: Vec<Option<Expression<'static>>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        values: Vec<JsValue>,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= elements.len() {
            return on_complete(self, JsValue::Array(Rc::new(RefCell::new(values))));
        }

        match elements[index].clone() {
            None => {
                let mut next_values = values;
                next_values.push(JsValue::Undefined);
                self.eval_generator_array_expression(
                    elements,
                    env,
                    index + 1,
                    next_values,
                    on_complete,
                )
            }
            Some(Expression::SpreadElement(expr)) => {
                let elements_clone = elements.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    *expr,
                    env,
                    Rc::new(move |interp, value| {
                        let mut next_values = values.clone();
                        next_values.extend(interp.collect_iterable_items(value)?);
                        interp.eval_generator_array_expression(
                            elements_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            next_values,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
            Some(expr) => {
                let elements_clone = elements.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    expr,
                    env,
                    Rc::new(move |interp, value| {
                        let mut next_values = values.clone();
                        next_values.push(value);
                        interp.eval_generator_array_expression(
                            elements_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            next_values,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
        }
    }

    fn eval_generator_call_arguments(
        &mut self,
        arguments: Vec<Expression<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        values: Vec<JsValue>,
        on_complete: GeneratorArgsContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= arguments.len() {
            return on_complete(self, values);
        }

        match arguments[index].clone() {
            Expression::SpreadElement(expr) => {
                let args_clone = arguments.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    *expr,
                    env,
                    Rc::new(move |interp, value| {
                        let mut next_values = values.clone();
                        next_values.extend(interp.collect_iterable_items(value)?);
                        interp.eval_generator_call_arguments(
                            args_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            next_values,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
            expr => {
                let args_clone = arguments.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    expr,
                    env,
                    Rc::new(move |interp, value| {
                        let mut next_values = values.clone();
                        next_values.push(value);
                        interp.eval_generator_call_arguments(
                            args_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            next_values,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
        }
    }

    fn eval_generator_object_expression(
        &mut self,
        properties: Vec<ObjectProperty<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        values: crate::engine::value::JsObjectMap,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= properties.len() {
            return on_complete(self, JsValue::Object(values));
        }

        let property = properties[index].clone();
        let properties_clone = properties.clone();
        let env_for_key = Rc::clone(&env);
        let object_value = JsValue::Object(Rc::clone(&values));
        let apply_property: GeneratorStringContinuation = Rc::new(
            move |interp: &mut Interpreter, key: String| match &property.kind {
                ObjectPropertyKind::Getter(func) => {
                    let getter = interp.create_accessor_function_value(
                        func,
                        Rc::clone(&env_for_key),
                        None,
                        None,
                        Some(object_value.clone()),
                        None,
                    );
                    let setter = match values.borrow().get(&key).cloned() {
                        Some(PropertyValue::Accessor { setter, .. }) => setter,
                        _ => None,
                    };
                    values.borrow_mut().insert(
                        key,
                        PropertyValue::Accessor {
                            getter: Some(getter),
                            setter,
                        },
                    );
                    interp.eval_generator_object_expression(
                        properties_clone.clone(),
                        Rc::clone(&env_for_key),
                        index + 1,
                        Rc::clone(&values),
                        Rc::clone(&on_complete),
                    )
                }
                ObjectPropertyKind::Setter(func) => {
                    let setter_fn = interp.create_accessor_function_value(
                        func,
                        Rc::clone(&env_for_key),
                        None,
                        None,
                        Some(object_value.clone()),
                        None,
                    );
                    let getter = match values.borrow().get(&key).cloned() {
                        Some(PropertyValue::Accessor { getter, .. }) => getter,
                        _ => None,
                    };
                    values.borrow_mut().insert(
                        key,
                        PropertyValue::Accessor {
                            getter,
                            setter: Some(setter_fn),
                        },
                    );
                    interp.eval_generator_object_expression(
                        properties_clone.clone(),
                        Rc::clone(&env_for_key),
                        index + 1,
                        Rc::clone(&values),
                        Rc::clone(&on_complete),
                    )
                }
                ObjectPropertyKind::Value(_) => {
                    if let Expression::SpreadElement(expr) = property.value.clone() {
                        let values_for_spread = Rc::clone(&values);
                        let properties_for_spread = properties_clone.clone();
                        let env_for_spread = Rc::clone(&env_for_key);
                        let on_complete_for_spread = Rc::clone(&on_complete);
                        interp.eval_generator_expression(
                            *expr,
                            Rc::clone(&env_for_key),
                            Rc::new(move |interp, spread_value| {
                                if let JsValue::Object(map) = spread_value {
                                    let entries = map
                                        .borrow()
                                        .iter()
                                        .map(|(k, v)| (k.clone(), v.clone()))
                                        .collect::<Vec<_>>();
                                    for (k, v) in entries {
                                        values_for_spread.borrow_mut().insert(k, v);
                                    }
                                }
                                interp.eval_generator_object_expression(
                                    properties_for_spread.clone(),
                                    Rc::clone(&env_for_spread),
                                    index + 1,
                                    Rc::clone(&values_for_spread),
                                    Rc::clone(&on_complete_for_spread),
                                )
                            }),
                        )
                    } else {
                        let values_for_value = Rc::clone(&values);
                        let properties_for_value = properties_clone.clone();
                        let env_for_value = Rc::clone(&env_for_key);
                        let on_complete_for_value = Rc::clone(&on_complete);
                        let key_for_value = key.clone();
                        if property.method {
                            if let Expression::FunctionExpression(func) = property.value.clone() {
                                let value = interp.create_method_function_value(
                                    &func,
                                    Rc::clone(&env_for_key),
                                    None,
                                    None,
                                    Some(object_value.clone()),
                                    None,
                                );
                                values_for_value
                                    .borrow_mut()
                                    .insert(key_for_value.clone(), PropertyValue::Data(value));
                                interp.eval_generator_object_expression(
                                    properties_for_value.clone(),
                                    Rc::clone(&env_for_value),
                                    index + 1,
                                    Rc::clone(&values_for_value),
                                    Rc::clone(&on_complete_for_value),
                                )
                            } else {
                                interp.eval_generator_expression(
                                    property.value.clone(),
                                    Rc::clone(&env_for_key),
                                    Rc::new(move |interp, value| {
                                        values_for_value.borrow_mut().insert(
                                            key_for_value.clone(),
                                            PropertyValue::Data(value),
                                        );
                                        interp.eval_generator_object_expression(
                                            properties_for_value.clone(),
                                            Rc::clone(&env_for_value),
                                            index + 1,
                                            Rc::clone(&values_for_value),
                                            Rc::clone(&on_complete_for_value),
                                        )
                                    }),
                                )
                            }
                        } else {
                            interp.eval_generator_expression(
                                property.value.clone(),
                                Rc::clone(&env_for_key),
                                Rc::new(move |interp, value| {
                                    values_for_value
                                        .borrow_mut()
                                        .insert(key_for_value.clone(), PropertyValue::Data(value));
                                    interp.eval_generator_object_expression(
                                        properties_for_value.clone(),
                                        Rc::clone(&env_for_value),
                                        index + 1,
                                        Rc::clone(&values_for_value),
                                        Rc::clone(&on_complete_for_value),
                                    )
                                }),
                            )
                        }
                    }
                }
            },
        );

        match property.key.clone() {
            ObjectKey::Identifier(name) | ObjectKey::String(name) => {
                apply_property.clone()(self, name.to_string())
            }
            ObjectKey::Number(number) => apply_property.clone()(self, number.to_string()),
            ObjectKey::Computed(expr) => self.eval_generator_expression(
                *expr,
                env,
                Rc::new(move |interp, value| {
                    apply_property.clone()(interp, interp.property_key_from_value(value))
                }),
            ),
            ObjectKey::PrivateIdentifier(_) => Err(RuntimeError::SyntaxError(
                "private identifier cannot appear in object literals".into(),
            )),
        }
    }

    fn eval_generator_template_literal(
        &mut self,
        parts: Vec<TemplatePart<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        current: String,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= parts.len() {
            return on_complete(self, JsValue::String(current));
        }

        match parts[index].clone() {
            TemplatePart::String(value) => self.eval_generator_template_literal(
                parts,
                env,
                index + 1,
                format!("{current}{value}"),
                on_complete,
            ),
            TemplatePart::Expr(expr) => {
                let parts_clone = parts.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    expr,
                    env,
                    Rc::new(move |interp, value| {
                        interp.eval_generator_template_literal(
                            parts_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            format!("{current}{}", value.as_string()),
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
        }
    }

    fn eval_generator_tagged_template_arguments(
        &mut self,
        parts: Vec<TemplatePart<'static>>,
        env: Rc<RefCell<Environment>>,
        index: usize,
        strings: Vec<JsValue>,
        values: Vec<JsValue>,
        on_complete: GeneratorTaggedTemplateContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if index >= parts.len() {
            return on_complete(self, strings, values);
        }

        match parts[index].clone() {
            TemplatePart::String(value) => {
                let mut next_strings = strings;
                next_strings.push(JsValue::String(value.to_string()));
                self.eval_generator_tagged_template_arguments(
                    parts,
                    env,
                    index + 1,
                    next_strings,
                    values,
                    on_complete,
                )
            }
            TemplatePart::Expr(expr) => {
                let parts_clone = parts.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    expr,
                    env,
                    Rc::new(move |interp, value| {
                        let mut next_values = values.clone();
                        next_values.push(value);
                        interp.eval_generator_tagged_template_arguments(
                            parts_clone.clone(),
                            Rc::clone(&env_clone),
                            index + 1,
                            strings.clone(),
                            next_values,
                            Rc::clone(&on_complete),
                        )
                    }),
                )
            }
        }
    }

    fn eval_generator_member_expression_value(
        &mut self,
        mem: MemberExpression<'static>,
        env: Rc<RefCell<Environment>>,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if let Some(name) = self.member_private_name(&mem) {
            let name = name.to_string();
            return self.eval_generator_expression(
                mem.object.clone(),
                env.clone(),
                Rc::new(move |interp, object| {
                    let value = interp.read_private_member_value(object, &name, Rc::clone(&env))?;
                    on_complete(interp, value)
                }),
            );
        }
        if matches!(mem.object, Expression::SuperExpression) {
            let accessor_this = env.borrow().get("this").unwrap_or(JsValue::Undefined);
            let super_binding = env
                .borrow()
                .get("__super_property_base__")
                .or_else(|| env.borrow().get("super"))
                .unwrap_or(JsValue::Undefined);
            return self.eval_generator_property_key(
                mem.computed,
                mem.property,
                Rc::clone(&env),
                Rc::new(move |interp, property_key| {
                    let value = interp.read_member_value(
                        super_binding.clone(),
                        &property_key,
                        Some(accessor_this.clone()),
                    )?;
                    on_complete(interp, value)
                }),
            );
        }

        let mem_clone = mem.clone();
        let env_clone = Rc::clone(&env);
        self.eval_generator_expression(
            mem.object.clone(),
            env,
            Rc::new(move |interp, object| {
                if mem_clone.optional && matches!(object, JsValue::Undefined | JsValue::Null) {
                    return on_complete(interp, JsValue::Undefined);
                }
                interp.eval_generator_property_key(
                    mem_clone.computed,
                    mem_clone.property.clone(),
                    Rc::clone(&env_clone),
                    Rc::new({
                        let on_complete = Rc::clone(&on_complete);
                        move |interp, property_key| {
                            let value =
                                interp.read_member_value(object.clone(), &property_key, None)?;
                            on_complete(interp, value)
                        }
                    }),
                )
            }),
        )
    }

    fn eval_generator_call_target(
        &mut self,
        callee: Expression<'static>,
        env: Rc<RefCell<Environment>>,
        on_complete: GeneratorCallTargetContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        match callee {
            Expression::MemberExpression(mem)
                if matches!(mem.object, Expression::SuperExpression) =>
            {
                let this_value = env.borrow().get("this").unwrap_or(JsValue::Undefined);
                let super_binding = env
                    .borrow()
                    .get("__super_property_base__")
                    .or_else(|| env.borrow().get("super"))
                    .unwrap_or(JsValue::Undefined);
                self.eval_generator_property_key(
                    mem.computed,
                    mem.property,
                    Rc::clone(&env),
                    Rc::new(move |interp, property_key| {
                        let callee = interp.read_member_value(
                            super_binding.clone(),
                            &property_key,
                            Some(this_value.clone()),
                        )?;
                        on_complete(interp, callee, this_value.clone())
                    }),
                )
            }
            Expression::MemberExpression(mem) if self.member_private_name(&mem).is_some() => {
                let name = self.member_private_name(&mem).unwrap().to_string();
                self.eval_generator_expression(
                    mem.object.clone(),
                    env.clone(),
                    Rc::new(move |interp, object| {
                        if mem.optional && matches!(object, JsValue::Undefined | JsValue::Null) {
                            return on_complete(interp, JsValue::Undefined, JsValue::Undefined);
                        }
                        let callee = interp.read_private_member_value(
                            object.clone(),
                            &name,
                            Rc::clone(&env),
                        )?;
                        on_complete(interp, callee, object)
                    }),
                )
            }
            Expression::MemberExpression(mem) => {
                let mem_clone = mem.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    mem.object.clone(),
                    env,
                    Rc::new(move |interp, object| {
                        if mem_clone.optional
                            && matches!(object, JsValue::Undefined | JsValue::Null)
                        {
                            return on_complete(interp, JsValue::Undefined, JsValue::Undefined);
                        }
                        interp.eval_generator_property_key(
                            mem_clone.computed,
                            mem_clone.property.clone(),
                            Rc::clone(&env_clone),
                            Rc::new({
                                let on_complete = Rc::clone(&on_complete);
                                move |interp, property_key| {
                                    let callee = interp.read_member_value(
                                        object.clone(),
                                        &property_key,
                                        None,
                                    )?;
                                    on_complete(interp, callee, object.clone())
                                }
                            }),
                        )
                    }),
                )
            }
            Expression::SuperExpression => {
                let super_binding = env.borrow().get("super").unwrap_or(JsValue::Undefined);
                if matches!(super_binding, JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "super is not available in this context".into(),
                    ));
                }
                let this_value = env
                    .borrow()
                    .get("__constructor_this__")
                    .or_else(|| env.borrow().get("this"))
                    .unwrap_or(JsValue::Undefined);
                on_complete(self, super_binding, this_value)
            }
            other => self.eval_generator_expression(
                other,
                env,
                Rc::new(move |interp, callee| on_complete(interp, callee, JsValue::Undefined)),
            ),
        }
    }

    fn eval_tagged_template_target<'a>(
        &mut self,
        tag: &Expression<'a>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<(JsValue, JsValue), RuntimeError> {
        match tag {
            Expression::MemberExpression(mem)
                if matches!(mem.object, Expression::SuperExpression) =>
            {
                let this_value = env.borrow().get("this").unwrap_or(JsValue::Undefined);
                let super_binding = env
                    .borrow()
                    .get("__super_property_base__")
                    .or_else(|| env.borrow().get("super"))
                    .unwrap_or(JsValue::Undefined);
                let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                let callee =
                    self.read_member_value(super_binding, &property_key, Some(this_value.clone()))?;
                Ok((callee, this_value))
            }
            Expression::MemberExpression(mem) if self.member_private_name(mem).is_some() => {
                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let name = self.member_private_name(mem).unwrap();
                let callee =
                    self.read_private_member_value(object.clone(), name, Rc::clone(&env))?;
                Ok((callee, object))
            }
            Expression::MemberExpression(mem) => {
                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                let callee = self.read_member_value(object.clone(), &property_key, None)?;
                Ok((callee, object))
            }
            Expression::SuperExpression => {
                let super_binding = env.borrow().get("super").unwrap_or(JsValue::Undefined);
                if matches!(super_binding, JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "super is not available in this context".into(),
                    ));
                }
                let this_value = env
                    .borrow()
                    .get("__constructor_this__")
                    .or_else(|| env.borrow().get("this"))
                    .unwrap_or(JsValue::Undefined);
                Ok((super_binding, this_value))
            }
            other => Ok((self.eval_expression(other, env)?, JsValue::Undefined)),
        }
    }

    fn eval_generator_statement(
        &mut self,
        stmt: Statement<'static>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if !self.statement_contains_yield(&stmt) {
            return Ok(GeneratorExecution::Complete(
                self.eval_statement(&stmt, env)?,
            ));
        }

        match stmt {
            Statement::ExpressionStatement(expr) => self.eval_generator_expression(
                expr,
                env,
                Rc::new(|_, value| Ok(GeneratorExecution::Complete(value))),
            ),
            Statement::VariableDeclaration(decl) => {
                self.eval_generator_variable_declaration(decl, env, 0)
            }
            Statement::BlockStatement(block) => {
                self.eval_generator_block(block.body, env, 0, JsValue::Undefined)
            }
            Statement::IfStatement(if_stmt) => self.eval_generator_expression(
                if_stmt.test,
                Rc::clone(&env),
                Rc::new(move |interp, test_value| {
                    if test_value.is_truthy() {
                        interp
                            .eval_generator_statement(*if_stmt.consequent.clone(), Rc::clone(&env))
                    } else if let Some(alternate) = &if_stmt.alternate {
                        interp.eval_generator_statement(*alternate.clone(), Rc::clone(&env))
                    } else {
                        Ok(GeneratorExecution::Complete(JsValue::Undefined))
                    }
                }),
            ),
            Statement::WithStatement(with_stmt) => self.eval_generator_expression(
                with_stmt.object,
                Rc::clone(&env),
                Rc::new(move |interp, object| {
                    let with_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                    let binding_keys = interp
                        .collect_with_scope_bindings(&object)
                        .into_iter()
                        .map(|(key, value)| {
                            with_env.borrow_mut().define(key.clone(), value);
                            key
                        })
                        .collect::<HashSet<_>>();
                    match interp
                        .eval_generator_statement(*with_stmt.body.clone(), Rc::clone(&with_env))
                    {
                        Ok(exec) => interp.map_generator_execution(
                            exec,
                            Rc::new({
                                let binding_keys = binding_keys.clone();
                                let object = object.clone();
                                let with_env = Rc::clone(&with_env);
                                move |interp, value| {
                                    interp.sync_with_scope_bindings(
                                        &object,
                                        Rc::clone(&with_env),
                                        &binding_keys,
                                    )?;
                                    Ok(GeneratorExecution::Complete(value))
                                }
                            }),
                            Rc::new({
                                let binding_keys = binding_keys.clone();
                                let object = object.clone();
                                let with_env = Rc::clone(&with_env);
                                move |interp, error| {
                                    interp.sync_with_scope_bindings(
                                        &object,
                                        Rc::clone(&with_env),
                                        &binding_keys,
                                    )?;
                                    Err(error)
                                }
                            }),
                        ),
                        Err(error) => {
                            interp.sync_with_scope_bindings(
                                &object,
                                Rc::clone(&with_env),
                                &binding_keys,
                            )?;
                            Err(error)
                        }
                    }
                }),
            ),
            Statement::WhileStatement(while_stmt) => {
                self.eval_generator_while_loop(while_stmt, env, JsValue::Undefined, false, None)
            }
            Statement::DoWhileStatement(while_stmt) => {
                self.eval_generator_while_loop(while_stmt, env, JsValue::Undefined, true, None)
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    let stmt_clone = for_stmt.clone();
                    let env_clone = Rc::clone(&env);
                    let exec = self.eval_generator_statement(*init.clone(), env)?;
                    self.map_generator_execution(
                        exec,
                        Rc::new(move |interp, _| {
                            interp.eval_generator_for_loop(
                                stmt_clone.clone(),
                                Rc::clone(&env_clone),
                                JsValue::Undefined,
                                None,
                            )
                        }),
                        Rc::new(|_, error| Err(error)),
                    )
                } else {
                    self.eval_generator_for_loop(for_stmt, env, JsValue::Undefined, None)
                }
            }
            Statement::ForInStatement(for_in) => {
                let binding = extract_for_binding(&for_in.left)
                    .map(|(pattern, declare)| (clone_expression(pattern), declare));
                let body = clone_statement(&for_in.body);
                self.eval_generator_expression(
                    for_in.right,
                    Rc::clone(&env),
                    Rc::new(move |interp, right| {
                        let keys = Rc::new(interp.collect_for_in_keys(right)?);
                        interp.eval_generator_for_in_loop(
                            binding.clone(),
                            body.clone(),
                            Rc::clone(&env),
                            keys,
                            0,
                            JsValue::Undefined,
                            None,
                        )
                    }),
                )
            }
            Statement::ForOfStatement(for_of) => {
                let binding = extract_for_binding(&for_of.left)
                    .map(|(pattern, declare)| (clone_expression(pattern), declare));
                let body = clone_statement(&for_of.body);
                let is_await = for_of.is_await;
                self.eval_generator_expression(
                    for_of.right,
                    Rc::clone(&env),
                    Rc::new(move |interp, right| {
                        let cursor = Rc::new(RefCell::new(interp.begin_iteration(right)?));
                        interp.eval_generator_for_of_loop(
                            binding.clone(),
                            body.clone(),
                            Rc::clone(&env),
                            cursor,
                            is_await,
                            JsValue::Undefined,
                            None,
                        )
                    }),
                )
            }
            Statement::SwitchStatement(switch) => self.eval_generator_switch_statement(switch, env),
            Statement::TryStatement(try_stmt) => {
                let finalizer = try_stmt.finalizer.clone();
                let handler = try_stmt.handler.clone();
                let complete_finalizer = finalizer.clone();
                let complete_env = Rc::clone(&env);
                let error_finalizer = finalizer.clone();
                let error_env = Rc::clone(&env);
                let handle_try_error: GeneratorErrorContinuation = Rc::new(move |interp, error| {
                    if let Some(handler) = &handler {
                        interp.eval_generator_catch_clause(
                            handler.clone(),
                            error,
                            Rc::clone(&error_env),
                            error_finalizer.clone(),
                        )
                    } else {
                        interp.run_generator_finalizer(
                            error_finalizer.clone(),
                            Rc::clone(&error_env),
                            Err(error),
                        )
                    }
                });

                match self.eval_generator_statement(
                    Statement::BlockStatement(try_stmt.block.clone()),
                    Rc::clone(&env),
                ) {
                    Ok(try_exec) => self.map_generator_execution(
                        try_exec,
                        Rc::new(move |interp, value| {
                            interp.run_generator_finalizer(
                                complete_finalizer.clone(),
                                Rc::clone(&complete_env),
                                Ok(value),
                            )
                        }),
                        handle_try_error,
                    ),
                    Err(error) => handle_try_error(self, error),
                }
            }
            Statement::ReturnStatement(expr) => match expr {
                Some(expr) => self.eval_generator_expression(
                    expr,
                    env,
                    Rc::new(|_, value| Err(RuntimeError::Return(value))),
                ),
                None => Err(RuntimeError::Return(JsValue::Undefined)),
            },
            Statement::ThrowStatement(expr) => self.eval_generator_expression(
                expr,
                env,
                Rc::new(|_, value| Err(RuntimeError::Throw(value))),
            ),
            Statement::LabeledStatement(labeled) => {
                let label = labeled.label.to_string();
                match *labeled.body.clone() {
                    Statement::WhileStatement(while_stmt) => self.eval_generator_while_loop(
                        while_stmt,
                        env,
                        JsValue::Undefined,
                        false,
                        Some(label),
                    ),
                    Statement::DoWhileStatement(while_stmt) => self.eval_generator_while_loop(
                        while_stmt,
                        env,
                        JsValue::Undefined,
                        true,
                        Some(label),
                    ),
                    Statement::ForStatement(for_stmt) => {
                        if let Some(init) = &for_stmt.init {
                            let stmt_clone = for_stmt.clone();
                            let env_clone = Rc::clone(&env);
                            let label_clone = label.clone();
                            let exec = self.eval_generator_statement(*init.clone(), env)?;
                            self.map_generator_execution(
                                exec,
                                Rc::new(move |interp, _| {
                                    interp.eval_generator_for_loop(
                                        stmt_clone.clone(),
                                        Rc::clone(&env_clone),
                                        JsValue::Undefined,
                                        Some(label_clone.clone()),
                                    )
                                }),
                                Rc::new(|_, error| Err(error)),
                            )
                        } else {
                            self.eval_generator_for_loop(
                                for_stmt,
                                env,
                                JsValue::Undefined,
                                Some(label),
                            )
                        }
                    }
                    Statement::ForInStatement(for_in) => {
                        let binding = extract_for_binding(&for_in.left)
                            .map(|(pattern, declare)| (clone_expression(pattern), declare));
                        let body = clone_statement(&for_in.body);
                        let label_clone = label.clone();
                        self.eval_generator_expression(
                            for_in.right,
                            Rc::clone(&env),
                            Rc::new(move |interp, right| {
                                let keys = Rc::new(interp.collect_for_in_keys(right)?);
                                interp.eval_generator_for_in_loop(
                                    binding.clone(),
                                    body.clone(),
                                    Rc::clone(&env),
                                    keys,
                                    0,
                                    JsValue::Undefined,
                                    Some(label_clone.clone()),
                                )
                            }),
                        )
                    }
                    Statement::ForOfStatement(for_of) => {
                        let binding = extract_for_binding(&for_of.left)
                            .map(|(pattern, declare)| (clone_expression(pattern), declare));
                        let body = clone_statement(&for_of.body);
                        let is_await = for_of.is_await;
                        let label_clone = label.clone();
                        self.eval_generator_expression(
                            for_of.right,
                            Rc::clone(&env),
                            Rc::new(move |interp, right| {
                                let cursor = Rc::new(RefCell::new(interp.begin_iteration(right)?));
                                interp.eval_generator_for_of_loop(
                                    binding.clone(),
                                    body.clone(),
                                    Rc::clone(&env),
                                    cursor,
                                    is_await,
                                    JsValue::Undefined,
                                    Some(label_clone.clone()),
                                )
                            }),
                        )
                    }
                    other => match self.eval_generator_statement(other, env) {
                        Ok(exec) => self.map_generator_execution(
                            exec,
                            Rc::new(|_, value| Ok(GeneratorExecution::Complete(value))),
                            Rc::new(move |_, error| match error {
                                RuntimeError::Break(Some(label_name)) if label_name == label => {
                                    Ok(GeneratorExecution::Complete(JsValue::Undefined))
                                }
                                other => Err(other),
                            }),
                        ),
                        Err(RuntimeError::Break(Some(label_name))) if label_name == label => {
                            Ok(GeneratorExecution::Complete(JsValue::Undefined))
                        }
                        Err(other) => Err(other),
                    },
                }
            }
            other => Ok(GeneratorExecution::Complete(
                self.eval_statement(&other, env)?,
            )),
        }
    }

    fn eval_generator_switch_statement(
        &mut self,
        switch: SwitchStatement<'static>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let default_index = switch.cases.iter().position(|case| case.test.is_none());
        self.eval_generator_expression(
            switch.discriminant.clone(),
            Rc::clone(&env),
            Rc::new(move |interp, discriminant| {
                interp.eval_generator_switch_match(
                    switch.clone(),
                    Rc::clone(&env),
                    discriminant,
                    0,
                    default_index,
                )
            }),
        )
    }

    fn eval_generator_switch_match(
        &mut self,
        switch: SwitchStatement<'static>,
        env: Rc<RefCell<Environment>>,
        discriminant: JsValue,
        case_index: usize,
        default_index: Option<usize>,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if case_index >= switch.cases.len() {
            return if let Some(default_index) = default_index {
                self.eval_generator_switch_consequents(
                    switch,
                    env,
                    default_index,
                    0,
                    JsValue::Undefined,
                )
            } else {
                Ok(GeneratorExecution::Complete(JsValue::Undefined))
            };
        }

        let case = switch.cases[case_index].clone();
        let Some(test) = case.test else {
            return self.eval_generator_switch_match(
                switch,
                env,
                discriminant,
                case_index + 1,
                default_index,
            );
        };

        let switch_clone = switch.clone();
        let env_clone = Rc::clone(&env);
        self.eval_generator_expression(
            test,
            env,
            Rc::new(move |interp, test_value| {
                if js_strict_eq(&discriminant, &test_value) {
                    interp.eval_generator_switch_consequents(
                        switch_clone.clone(),
                        Rc::clone(&env_clone),
                        case_index,
                        0,
                        JsValue::Undefined,
                    )
                } else {
                    interp.eval_generator_switch_match(
                        switch_clone.clone(),
                        Rc::clone(&env_clone),
                        discriminant.clone(),
                        case_index + 1,
                        default_index,
                    )
                }
            }),
        )
    }

    fn eval_generator_switch_consequents(
        &mut self,
        switch: SwitchStatement<'static>,
        env: Rc<RefCell<Environment>>,
        case_index: usize,
        statement_index: usize,
        last_value: JsValue,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if case_index >= switch.cases.len() {
            return Ok(GeneratorExecution::Complete(last_value));
        }

        let case = switch.cases[case_index].clone();
        if statement_index >= case.consequent.len() {
            return self.eval_generator_switch_consequents(
                switch,
                env,
                case_index + 1,
                0,
                last_value,
            );
        }

        let statement = case.consequent[statement_index].clone();
        let switch_clone = switch.clone();
        let env_clone = Rc::clone(&env);
        let last_for_error = last_value.clone();
        match self.eval_generator_statement(statement, env) {
            Ok(exec) => self.map_generator_execution(
                exec,
                Rc::new(move |interp, value| {
                    interp.eval_generator_switch_consequents(
                        switch_clone.clone(),
                        Rc::clone(&env_clone),
                        case_index,
                        statement_index + 1,
                        value,
                    )
                }),
                Rc::new(move |_, error| match error {
                    RuntimeError::Break(None) => {
                        Ok(GeneratorExecution::Complete(last_for_error.clone()))
                    }
                    other => Err(other),
                }),
            ),
            Err(RuntimeError::Break(None)) => Ok(GeneratorExecution::Complete(last_value)),
            Err(other) => Err(other),
        }
    }

    fn eval_generator_update_expression(
        &mut self,
        update: UpdateExpression<'static>,
        env: Rc<RefCell<Environment>>,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let operator = update.operator.clone();
        let prefix = update.prefix;
        match update.argument.clone() {
            Expression::Identifier(name) => {
                let current_value = env
                    .borrow()
                    .get(name)
                    .unwrap_or(JsValue::Undefined)
                    .as_number();
                let new_value = if operator == UpdateOperator::PlusPlus {
                    current_value + 1.0
                } else {
                    current_value - 1.0
                };
                if env.borrow().has_binding(name) {
                    env.borrow_mut()
                        .set(name, JsValue::Number(new_value))
                        .map_err(RuntimeError::TypeError)?;
                } else {
                    env.borrow_mut()
                        .define(name.to_string(), JsValue::Number(new_value));
                }
                on_complete(
                    self,
                    if prefix {
                        JsValue::Number(new_value)
                    } else {
                        JsValue::Number(current_value)
                    },
                )
            }
            Expression::MemberExpression(mem)
                if matches!(mem.object, Expression::SuperExpression) =>
            {
                let apply_update = Rc::new({
                    let on_complete = Rc::clone(&on_complete);
                    let operator = operator.clone();
                    let env_clone = Rc::clone(&env);
                    move |interp: &mut Interpreter, property_key: String| {
                        let current_value =
                            interp.read_super_member_value(Rc::clone(&env_clone), &property_key)?;
                        let current_number = current_value.as_number();
                        let new_number = if operator == UpdateOperator::PlusPlus {
                            current_number + 1.0
                        } else {
                            current_number - 1.0
                        };
                        interp.write_super_member_value(
                            Rc::clone(&env_clone),
                            &property_key,
                            JsValue::Number(new_number),
                        )?;
                        on_complete(
                            interp,
                            if prefix {
                                JsValue::Number(new_number)
                            } else {
                                JsValue::Number(current_number)
                            },
                        )
                    }
                });

                if mem.computed && self.expression_contains_yield(&mem.property) {
                    return self.eval_generator_property_key(
                        true,
                        mem.property.clone(),
                        Rc::clone(&env),
                        Rc::new(move |interp, property_key| apply_update(interp, property_key)),
                    );
                }

                let property_key = self.member_property_key(&mem, Rc::clone(&env))?;
                apply_update(self, property_key)
            }
            Expression::MemberExpression(mem) if self.member_private_name(&mem).is_some() => {
                let name = self.member_private_name(&mem).unwrap().to_string();
                let apply_update = Rc::new({
                    let on_complete = Rc::clone(&on_complete);
                    let operator = operator.clone();
                    let env_clone = Rc::clone(&env);
                    move |interp: &mut Interpreter, object: JsValue| {
                        let current_value = interp.read_private_member_value(
                            object.clone(),
                            &name,
                            Rc::clone(&env_clone),
                        )?;
                        let current_number = current_value.as_number();
                        let new_number = if operator == UpdateOperator::PlusPlus {
                            current_number + 1.0
                        } else {
                            current_number - 1.0
                        };
                        interp.write_private_member_value(
                            object,
                            &name,
                            JsValue::Number(new_number),
                            Rc::clone(&env_clone),
                        )?;
                        on_complete(
                            interp,
                            if prefix {
                                JsValue::Number(new_number)
                            } else {
                                JsValue::Number(current_number)
                            },
                        )
                    }
                });

                if self.expression_contains_yield(&mem.object) {
                    return self.eval_generator_expression(
                        mem.object.clone(),
                        Rc::clone(&env),
                        Rc::new(move |interp, object| apply_update(interp, object)),
                    );
                }

                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                apply_update(self, object)
            }
            Expression::MemberExpression(mem) => {
                let apply_update = Rc::new({
                    let on_complete = Rc::clone(&on_complete);
                    let operator = operator.clone();
                    move |interp: &mut Interpreter, object: JsValue, property_key: String| {
                        let current_value =
                            interp.read_member_value(object.clone(), &property_key, None)?;
                        let current_number = current_value.as_number();
                        let new_number = if operator == UpdateOperator::PlusPlus {
                            current_number + 1.0
                        } else {
                            current_number - 1.0
                        };
                        interp.write_member_value(
                            object,
                            &property_key,
                            JsValue::Number(new_number),
                        )?;
                        on_complete(
                            interp,
                            if prefix {
                                JsValue::Number(new_number)
                            } else {
                                JsValue::Number(current_number)
                            },
                        )
                    }
                });

                if self.expression_contains_yield(&mem.object) {
                    return self.eval_generator_expression(
                        mem.object.clone(),
                        Rc::clone(&env),
                        Rc::new({
                            let mem = mem.clone();
                            let env_clone = Rc::clone(&env);
                            let apply_update = Rc::clone(&apply_update);
                            move |interp, object| {
                                interp.eval_generator_property_key(
                                    mem.computed,
                                    mem.property.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let apply_update = Rc::clone(&apply_update);
                                        move |interp, property_key| {
                                            apply_update(interp, object.clone(), property_key)
                                        }
                                    }),
                                )
                            }
                        }),
                    );
                }

                if mem.computed && self.expression_contains_yield(&mem.property) {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    return self.eval_generator_property_key(
                        true,
                        mem.property.clone(),
                        Rc::clone(&env),
                        Rc::new(move |interp, property_key| {
                            apply_update(interp, object.clone(), property_key)
                        }),
                    );
                }

                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(&mem, Rc::clone(&env))?;
                apply_update(self, object, property_key)
            }
            _ => Err(RuntimeError::SyntaxError("invalid update target".into())),
        }
    }

    fn yield_from_iterator(
        &mut self,
        cursor: Rc<RefCell<IteratorCursor>>,
        action: ResumeAction,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        let step = {
            let mut cursor_borrow = cursor.borrow_mut();
            self.iterator_resume(&mut cursor_borrow, action, false)?
        };
        match step {
            IteratorStep::Yield(value) => Ok(GeneratorExecution::Yielded {
                value,
                continuation: Rc::new(move |interp, action| {
                    interp.yield_from_iterator(cursor.clone(), action, Rc::clone(&on_complete))
                }),
            }),
            IteratorStep::Complete(value) => on_complete(self, value),
        }
    }

    fn eval_generator_expression(
        &mut self,
        expr: Expression<'static>,
        env: Rc<RefCell<Environment>>,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        if !self.expression_contains_yield(&expr) {
            let value = self.eval_expression(&expr, env)?;
            return on_complete(self, value);
        }

        match expr {
            Expression::YieldExpression { argument, delegate } => {
                let complete = Rc::clone(&on_complete);
                let env_clone = Rc::clone(&env);
                let emit = Rc::new(move |interp: &mut Interpreter, value: JsValue| {
                    if delegate {
                        let cursor = Rc::new(RefCell::new(interp.begin_iteration(value).map_err(
                            |_| RuntimeError::TypeError("yield* requires an iterable value".into()),
                        )?));
                        interp.yield_from_iterator(
                            cursor,
                            ResumeAction::Next(JsValue::Undefined),
                            Rc::clone(&complete),
                        )
                    } else {
                        interp.yield_generator_value(value, Rc::clone(&complete))
                    }
                });

                match argument {
                    Some(argument) => self.eval_generator_expression(*argument, env_clone, emit),
                    None => emit(self, JsValue::Undefined),
                }
            }
            Expression::UnaryExpression(unary) => {
                if matches!(&unary.operator, UnaryOperator::Delete)
                    && let Expression::MemberExpression(mem) = &unary.argument
                    && self.expression_contains_yield(&unary.argument)
                {
                    let delete_member = Rc::new({
                        let on_complete = Rc::clone(&on_complete);
                        move |interp: &mut Interpreter, object: JsValue, property_key: String| {
                            let deleted = match object {
                                JsValue::Object(map) => {
                                    map.borrow_mut().remove(&property_key);
                                    true
                                }
                                JsValue::Function(function) => {
                                    function.properties.borrow_mut().remove(&property_key);
                                    true
                                }
                                JsValue::Array(arr) => {
                                    if property_key == "length" {
                                        false
                                    } else if let Ok(index) = property_key.parse::<usize>() {
                                        let mut arr = arr.borrow_mut();
                                        if index < arr.len() {
                                            arr[index] = JsValue::Undefined;
                                        }
                                        true
                                    } else {
                                        true
                                    }
                                }
                                JsValue::EnvironmentObject(env) => {
                                    env.borrow_mut().variables.remove(&property_key);
                                    true
                                }
                                JsValue::Null | JsValue::Undefined => {
                                    return Err(RuntimeError::TypeError(
                                        "value is not an object".into(),
                                    ));
                                }
                                _ => true,
                            };
                            on_complete(interp, JsValue::Boolean(deleted))
                        }
                    });

                    if self.expression_contains_yield(&mem.object) {
                        return self.eval_generator_expression(
                            mem.object.clone(),
                            Rc::clone(&env),
                            Rc::new({
                                let mem = mem.clone();
                                let env_clone = Rc::clone(&env);
                                let delete_member = Rc::clone(&delete_member);
                                move |interp, object| {
                                    interp.eval_generator_property_key(
                                        mem.computed,
                                        mem.property.clone(),
                                        Rc::clone(&env_clone),
                                        Rc::new({
                                            let delete_member = Rc::clone(&delete_member);
                                            move |interp, property_key| {
                                                delete_member(interp, object.clone(), property_key)
                                            }
                                        }),
                                    )
                                }
                            }),
                        );
                    }

                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    return self.eval_generator_property_key(
                        mem.computed,
                        mem.property.clone(),
                        Rc::clone(&env),
                        Rc::new(move |interp, property_key| {
                            delete_member(interp, object.clone(), property_key)
                        }),
                    );
                }

                let operator = unary.operator.clone();
                let argument = unary.argument.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    argument,
                    env,
                    Rc::new(move |interp, value| {
                        let result = match operator {
                            UnaryOperator::Minus => JsValue::Number(-value.as_number()),
                            UnaryOperator::Plus => JsValue::Number(value.as_number()),
                            UnaryOperator::LogicNot => JsValue::Boolean(!value.is_truthy()),
                            UnaryOperator::BitNot => {
                                JsValue::Number((!interp.to_int32(&value)) as f64)
                            }
                            UnaryOperator::Typeof => JsValue::String(value.type_of()),
                            UnaryOperator::Void => JsValue::Undefined,
                            UnaryOperator::Delete => match &unary.argument {
                                Expression::MemberExpression(mem)
                                    if !interp.expression_contains_yield(&unary.argument) =>
                                {
                                    if interp.member_private_name(mem).is_some() {
                                        return Err(RuntimeError::SyntaxError(
                                            "private fields cannot be deleted".into(),
                                        ));
                                    }
                                    let object = interp
                                        .eval_expression(&mem.object, Rc::clone(&env_clone))?;
                                    let property_key =
                                        interp.member_property_key(mem, Rc::clone(&env_clone))?;
                                    match object {
                                        JsValue::Object(map) => {
                                            map.borrow_mut().remove(&property_key);
                                        }
                                        JsValue::Function(function) => {
                                            function.properties.borrow_mut().remove(&property_key);
                                        }
                                        JsValue::Array(arr) => {
                                            if property_key != "length" {
                                                if let Ok(index) = property_key.parse::<usize>() {
                                                    let mut arr = arr.borrow_mut();
                                                    if index < arr.len() {
                                                        arr[index] = JsValue::Undefined;
                                                    }
                                                }
                                            }
                                        }
                                        JsValue::EnvironmentObject(env) => {
                                            env.borrow_mut().variables.remove(&property_key);
                                        }
                                        JsValue::Null | JsValue::Undefined => {
                                            return Err(RuntimeError::TypeError(
                                                "value is not an object".into(),
                                            ));
                                        }
                                        _ => {}
                                    }
                                    JsValue::Boolean(true)
                                }
                                _ => JsValue::Boolean(true),
                            },
                        };
                        on_complete(interp, result)
                    }),
                )
            }
            Expression::BinaryExpression(bin) => {
                if bin.operator == BinaryOperator::In
                    && let Expression::PrivateIdentifier(name) = &bin.left
                {
                    let name = (*name).to_string();
                    let right = bin.right.clone();
                    let env_clone = Rc::clone(&env);
                    return self.eval_generator_expression(
                        right,
                        env,
                        Rc::new(move |interp, right_value| {
                            let value = JsValue::Boolean(interp.has_private_member_brand(
                                &right_value,
                                &name,
                                &env_clone,
                            )?);
                            on_complete(interp, value)
                        }),
                    );
                }
                let op = bin.operator.clone();
                let left = bin.left.clone();
                let right = bin.right.clone();
                let env_clone = Rc::clone(&env);
                self.eval_generator_expression(
                    left,
                    env,
                    Rc::new(move |interp, left_value| match op {
                        BinaryOperator::LogicAnd if !left_value.is_truthy() => {
                            on_complete(interp, left_value)
                        }
                        BinaryOperator::LogicOr if left_value.is_truthy() => {
                            on_complete(interp, left_value)
                        }
                        BinaryOperator::NullishCoalescing
                            if !matches!(left_value, JsValue::Undefined | JsValue::Null) =>
                        {
                            on_complete(interp, left_value)
                        }
                        _ => interp.eval_generator_expression(
                            right.clone(),
                            Rc::clone(&env_clone),
                            Rc::new({
                                let op = op.clone();
                                let on_complete = Rc::clone(&on_complete);
                                move |interp, right_value| {
                                    let value = interp.eval_binary_operation(
                                        &op,
                                        left_value.clone(),
                                        right_value,
                                    )?;
                                    on_complete(interp, value)
                                }
                            }),
                        ),
                    }),
                )
            }
            Expression::ArrayExpression(elements) => {
                self.eval_generator_array_expression(elements, env, 0, Vec::new(), on_complete)
            }
            Expression::ObjectExpression(properties) => self.eval_generator_object_expression(
                properties,
                env,
                0,
                new_object_map(),
                on_complete,
            ),
            Expression::MemberExpression(mem) => {
                self.eval_generator_member_expression_value(*mem, env, on_complete)
            }
            Expression::AssignmentExpression(assign) => {
                let left = assign.left.clone();
                let right = assign.right.clone();
                let operator = assign.operator.clone();
                let env_clone = Rc::clone(&env);
                match &left {
                    Expression::Identifier(name) => {
                        let current = env_clone.borrow().get(name).unwrap_or(JsValue::Undefined);
                        if !self.should_apply_assignment(&operator, &current) {
                            return on_complete(self, current);
                        }
                        let name = (*name).to_string();
                        self.eval_generator_expression(
                            right,
                            env,
                            Rc::new(move |interp, right_value| {
                                let value =
                                    interp.assignment_result(&operator, &current, &right_value)?;
                                if env_clone.borrow().has_binding(&name) {
                                    env_clone
                                        .borrow_mut()
                                        .set(&name, value.clone())
                                        .map_err(RuntimeError::TypeError)?;
                                } else {
                                    env_clone.borrow_mut().define(name.clone(), value.clone());
                                }
                                on_complete(interp, value)
                            }),
                        )
                    }
                    Expression::ArrayExpression(_) | Expression::ObjectExpression(_)
                        if matches!(operator, AssignmentOperator::Assign) =>
                    {
                        self.eval_generator_expression(
                            right,
                            env,
                            Rc::new(move |interp, right_value| {
                                interp.eval_generator_assign_pattern(
                                    left.clone(),
                                    right_value.clone(),
                                    Rc::clone(&env_clone),
                                    false,
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        move |interp| on_complete(interp, right_value.clone())
                                    }),
                                )
                            }),
                        )
                    }
                    Expression::MemberExpression(mem) if self.member_private_name(mem).is_some() => {
                        let name = self.member_private_name(mem).unwrap().to_string();
                        let complete_with_object = Rc::new({
                            let on_complete = Rc::clone(&on_complete);
                            let operator = operator.clone();
                            let right = right.clone();
                            let env_clone = Rc::clone(&env_clone);
                            let name_for_current = name.clone();
                            move |interp: &mut Interpreter, object: JsValue| {
                                let current = interp.read_private_member_value(
                                    object.clone(),
                                    &name_for_current,
                                    Rc::clone(&env_clone),
                                )?;
                                if !interp.should_apply_assignment(&operator, &current) {
                                    return on_complete(interp, current);
                                }
                                interp.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        let env_clone = Rc::clone(&env_clone);
                                        let current = current.clone();
                                        let object = object.clone();
                                        let name = name_for_current.clone();
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_private_member_value(
                                                object.clone(),
                                                &name,
                                                value.clone(),
                                                Rc::clone(&env_clone),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        });

                        if self.expression_contains_yield(&mem.object) {
                            let complete_with_object_for_yield = Rc::clone(&complete_with_object);
                            self.eval_generator_expression(
                                mem.object.clone(),
                                env,
                                Rc::new(move |interp, object| {
                                    complete_with_object_for_yield(interp, object)
                                }),
                            )
                        } else {
                            let object = self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
                            let current = self.read_private_member_value(
                                object.clone(),
                                &name,
                                Rc::clone(&env_clone),
                            )?;
                            if !self.should_apply_assignment(&operator, &current) {
                                on_complete(self, current)
                            } else {
                                self.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        let env_clone = Rc::clone(&env_clone);
                                        let name = name.clone();
                                        let current = current.clone();
                                        let object = object.clone();
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_private_member_value(
                                                object.clone(),
                                                &name,
                                                value.clone(),
                                                Rc::clone(&env_clone),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        }
                    }
                    Expression::MemberExpression(mem)
                        if matches!(mem.object, Expression::SuperExpression) =>
                    {
                        let complete_with_key = Rc::new({
                            let on_complete = Rc::clone(&on_complete);
                            let operator = operator.clone();
                            let right = right.clone();
                            let env_clone = Rc::clone(&env_clone);
                            move |interp: &mut Interpreter, property_key: String| {
                                let current =
                                    interp.read_super_member_value(Rc::clone(&env_clone), &property_key)?;
                                if !interp.should_apply_assignment(&operator, &current) {
                                    return on_complete(interp, current);
                                }
                                interp.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        let env_clone = Rc::clone(&env_clone);
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_super_member_value(
                                                Rc::clone(&env_clone),
                                                &property_key,
                                                value.clone(),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        });

                        if mem.computed && self.expression_contains_yield(&mem.property) {
                            let complete_with_key_for_yield = Rc::clone(&complete_with_key);
                            self.eval_generator_property_key(
                                true,
                                mem.property.clone(),
                                env,
                                Rc::new(move |interp, property_key| {
                                    complete_with_key_for_yield(interp, property_key)
                                }),
                            )
                        } else {
                            let property_key = self.member_property_key(mem, Rc::clone(&env_clone))?;
                            let current = self.read_super_member_value(
                                Rc::clone(&env_clone),
                                &property_key,
                            )?;
                            if !self.should_apply_assignment(&operator, &current) {
                                on_complete(self, current)
                            } else {
                                self.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        let env_clone = Rc::clone(&env_clone);
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_super_member_value(
                                                Rc::clone(&env_clone),
                                                &property_key,
                                                value.clone(),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        }
                    }
                    Expression::MemberExpression(mem) => {
                        let complete_with_target = Rc::new({
                            let on_complete = Rc::clone(&on_complete);
                            let operator = operator.clone();
                            let right = right.clone();
                            let env_clone = Rc::clone(&env_clone);
                            move |interp: &mut Interpreter, object: JsValue, property_key: String| {
                                let current =
                                    interp.read_member_value(object.clone(), &property_key, None)?;
                                if !interp.should_apply_assignment(&operator, &current) {
                                    return on_complete(interp, current);
                                }
                                interp.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_member_value(
                                                object.clone(),
                                                &property_key,
                                                value.clone(),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        });

                        if self.expression_contains_yield(&mem.object) {
                            self.eval_generator_expression(
                                mem.object.clone(),
                                Rc::clone(&env),
                                Rc::new({
                                    let mem = mem.clone();
                                    let env_clone = Rc::clone(&env_clone);
                                    let complete_with_target = Rc::clone(&complete_with_target);
                                    move |interp, object| {
                                        interp.eval_generator_property_key(
                                            mem.computed,
                                            mem.property.clone(),
                                            Rc::clone(&env_clone),
                                            Rc::new({
                                                let complete_with_target =
                                                    Rc::clone(&complete_with_target);
                                                move |interp, property_key| {
                                                    complete_with_target(
                                                        interp,
                                                        object.clone(),
                                                        property_key,
                                                    )
                                                }
                                            }),
                                        )
                                    }
                                }),
                            )
                        } else if mem.computed && self.expression_contains_yield(&mem.property) {
                            let object = self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
                            let complete_with_target_for_yield = Rc::clone(&complete_with_target);
                            self.eval_generator_property_key(
                                true,
                                mem.property.clone(),
                                env,
                                Rc::new(move |interp, property_key| {
                                    complete_with_target_for_yield(
                                        interp,
                                        object.clone(),
                                        property_key,
                                    )
                                }),
                            )
                        } else {
                            let object = self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
                            let property_key = self.member_property_key(mem, Rc::clone(&env_clone))?;
                            let current = self.read_member_value(object.clone(), &property_key, None)?;
                            if !self.should_apply_assignment(&operator, &current) {
                                on_complete(self, current)
                            } else {
                                self.eval_generator_expression(
                                    right.clone(),
                                    Rc::clone(&env_clone),
                                    Rc::new({
                                        let on_complete = Rc::clone(&on_complete);
                                        let operator = operator.clone();
                                        move |interp, right_value| {
                                            let value = interp.assignment_result(
                                                &operator,
                                                &current,
                                                &right_value,
                                            )?;
                                            interp.write_member_value(
                                                object.clone(),
                                                &property_key,
                                                value.clone(),
                                            )?;
                                            on_complete(interp, value)
                                        }
                                    }),
                                )
                            }
                        }
                    }
                    _ => Err(RuntimeError::SyntaxError("invalid assignment target".into())),
                }
            }
            Expression::AwaitExpression(argument) => self.eval_generator_expression(
                *argument,
                env,
                Rc::new(move |interp, value| {
                    let value = interp.await_value(value)?;
                    on_complete(interp, value)
                }),
            ),
            Expression::CallExpression(call) => {
                let call_clone = (*call).clone();
                let is_super_call = matches!(call_clone.callee, Expression::SuperExpression);
                let short_circuits_on_undefined = call.optional
                    || matches!(&call.callee, Expression::MemberExpression(mem) if mem.optional);
                self.eval_generator_call_target(
                    call.callee,
                    Rc::clone(&env),
                    Rc::new(move |interp, callee, this_value| {
                        if short_circuits_on_undefined
                            && matches!(callee, JsValue::Undefined | JsValue::Null)
                        {
                            return on_complete(interp, JsValue::Undefined);
                        }
                        let env_for_this = Rc::clone(&env);
                        interp.eval_generator_call_arguments(
                            call_clone.arguments.clone(),
                            Rc::clone(&env),
                            0,
                            Vec::new(),
                            Rc::new({
                                let on_complete = Rc::clone(&on_complete);
                                move |interp, args| match callee.clone() {
                                    JsValue::Function(function) => {
                                        let result = interp.call_function_value(
                                            function,
                                            this_value.clone(),
                                            args,
                                            is_super_call,
                                        )?;
                                        if is_super_call {
                                            let initialized_this = if value_is_object_like(&result)
                                            {
                                                result.clone()
                                            } else {
                                                this_value.clone()
                                            };
                                            let _ = env_for_this
                                                .borrow_mut()
                                                .set("this", initialized_this);
                                        }
                                        on_complete(interp, result)
                                    }
                                    other => {
                                        let result = interp.invoke_callable(
                                            other,
                                            this_value.clone(),
                                            args,
                                        )?;
                                        on_complete(interp, result)
                                    }
                                }
                            }),
                        )
                    }),
                )
            }
            Expression::UpdateExpression(update) => {
                self.eval_generator_update_expression(*update, env, on_complete)
            }
            Expression::NewExpression(call) => {
                let call_clone = (*call).clone();
                self.eval_generator_call_target(
                    call.callee,
                    Rc::clone(&env),
                    Rc::new(move |interp, callee, _| {
                        interp.eval_generator_call_arguments(
                            call_clone.arguments.clone(),
                            Rc::clone(&env),
                            0,
                            Vec::new(),
                            Rc::new({
                                let on_complete = Rc::clone(&on_complete);
                                move |interp, args| {
                                    let result = match callee.clone() {
                                        JsValue::Function(function) => {
                                            let instance =
                                                object_with_proto(function.prototype.clone());
                                            let result = interp.call_function_value(
                                                Rc::clone(&function),
                                                instance.clone(),
                                                args,
                                                true,
                                            )?;
                                            if value_is_object_like(&result) {
                                                result
                                            } else {
                                                instance
                                            }
                                        }
                                        JsValue::BuiltinFunction(function) => interp
                                            .invoke_builtin_function(
                                                function.as_ref(),
                                                JsValue::Undefined,
                                                args,
                                            )?,
                                        _ => {
                                            return Err(RuntimeError::TypeError(
                                                "value is not a constructor".into(),
                                            ));
                                        }
                                    };
                                    on_complete(interp, result)
                                }
                            }),
                        )
                    }),
                )
            }
            Expression::SequenceExpression(seq) => {
                self.eval_generator_sequence(seq, env, 0, JsValue::Undefined, on_complete)
            }
            Expression::ConditionalExpression {
                test,
                consequent,
                alternate,
            } => self.eval_generator_expression(
                *test,
                Rc::clone(&env),
                Rc::new(move |interp, value| {
                    if value.is_truthy() {
                        interp.eval_generator_expression(
                            *consequent.clone(),
                            Rc::clone(&env),
                            Rc::clone(&on_complete),
                        )
                    } else {
                        interp.eval_generator_expression(
                            *alternate.clone(),
                            Rc::clone(&env),
                            Rc::clone(&on_complete),
                        )
                    }
                }),
            ),
            Expression::TemplateLiteral(parts) => {
                self.eval_generator_template_literal(parts, env, 0, String::new(), on_complete)
            }
            Expression::TaggedTemplateExpression(tag, parts) => {
                let env_clone = Rc::clone(&env);
                self.eval_generator_call_target(
                    *tag,
                    Rc::clone(&env),
                    Rc::new(move |interp, callee, this_value| {
                        interp.eval_generator_tagged_template_arguments(
                            parts.clone(),
                            Rc::clone(&env_clone),
                            0,
                            Vec::new(),
                            Vec::new(),
                            Rc::new({
                                let callee = callee.clone();
                                let this_value = this_value.clone();
                                let on_complete = Rc::clone(&on_complete);
                                move |interp, strings, values| {
                                    let mut call_args =
                                        vec![JsValue::Array(Rc::new(RefCell::new(strings)))];
                                    call_args.extend(values);
                                    let result = interp.invoke_callable(
                                        callee.clone(),
                                        this_value.clone(),
                                        call_args,
                                    )?;
                                    on_complete(interp, result)
                                }
                            }),
                        )
                    }),
                )
            }
            Expression::SpreadElement(expr) => {
                self.eval_generator_expression(*expr, env, on_complete)
            }
            other @ (Expression::Literal(_)
            | Expression::Identifier(_)
            | Expression::PrivateIdentifier(_)
            | Expression::ThisExpression
            | Expression::SuperExpression
            | Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)) => {
                let value = self.eval_expression(&other, env)?;
                on_complete(self, value)
            }
        }
    }

    fn invoke_callable(
        &mut self,
        callee: JsValue,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, RuntimeError> {
        match callee {
            JsValue::Function(function) => {
                self.call_function_value(function, this_value, args, false)
            }
            JsValue::NativeFunction(function) => (function.func)(self, &this_value, &args),
            JsValue::BuiltinFunction(function) => {
                self.invoke_builtin_function(function.as_ref(), this_value, args)
            }
            _ => Err(RuntimeError::TypeError("value is not callable".into())),
        }
    }

    fn invoke_builtin_function(
        &mut self,
        builtin: &BuiltinFunction,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, RuntimeError> {
        match builtin {
            BuiltinFunction::PromiseResolve => {
                let value = args.first().cloned().unwrap_or(JsValue::Undefined);
                if let JsValue::Promise(promise) = &value {
                    return Ok(JsValue::Promise(Rc::clone(promise)));
                }

                Ok(Self::resolved_promise(value))
            }
            BuiltinFunction::PromiseReject => Ok(Self::rejected_promise(
                args.first().cloned().unwrap_or(JsValue::Undefined),
            )),
            BuiltinFunction::PromiseThen => {
                let promise = match this_value {
                    JsValue::Promise(promise) => promise,
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Promise.prototype.then called on non-promise".into(),
                        ));
                    }
                };
                let on_fulfilled = args.first().cloned();
                let on_rejected = args.get(1).cloned();
                let result_promise = Self::pending_promise();
                self.attach_promise_reaction(
                    promise,
                    PromiseReaction {
                        kind: PromiseReactionKind::Then {
                            on_fulfilled,
                            on_rejected,
                        },
                        result_promise: Rc::clone(&result_promise),
                    },
                );
                Ok(JsValue::Promise(result_promise))
            }
            BuiltinFunction::PromiseCatch => self.invoke_builtin_function(
                &BuiltinFunction::PromiseThen,
                this_value,
                vec![
                    JsValue::Undefined,
                    args.first().cloned().unwrap_or(JsValue::Undefined),
                ],
            ),
            BuiltinFunction::PromiseFinally => {
                let promise = match &this_value {
                    JsValue::Promise(promise) => Rc::clone(promise),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Promise.prototype.finally called on non-promise".into(),
                        ));
                    }
                };
                let on_finally = args.first().cloned();
                let result_promise = Self::pending_promise();
                self.attach_promise_reaction(
                    promise,
                    PromiseReaction {
                        kind: PromiseReactionKind::Finally { on_finally },
                        result_promise: Rc::clone(&result_promise),
                    },
                );
                Ok(JsValue::Promise(result_promise))
            }
            BuiltinFunction::ModuleBindingGetter { env, binding } => {
                Ok(env.borrow().get(binding).unwrap_or(JsValue::Undefined))
            }
            BuiltinFunction::NamespaceBindingGetter {
                namespace,
                export_name,
            } => self.read_namespace_export(namespace, export_name),
            BuiltinFunction::AsyncGeneratorResultMapper { done } => {
                Ok(Self::generator_result_object(
                    args.first().cloned().unwrap_or(JsValue::Undefined),
                    *done,
                ))
            }
            BuiltinFunction::PromiseResolver {
                promise,
                is_resolve,
            } => {
                let value = args.first().cloned().unwrap_or(JsValue::Undefined);
                if *is_resolve {
                    self.resolve_promise_value(Rc::clone(promise), value);
                } else {
                    self.settle_promise(Rc::clone(promise), PromiseState::Rejected(value));
                }
                Ok(JsValue::Undefined)
            }
            BuiltinFunction::PromiseConstructor => {
                let promise = Self::pending_promise();
                let promise_value = JsValue::Promise(Rc::clone(&promise));
                if let Some(executor) = args.first() {
                    let resolve =
                        JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseResolver {
                            promise: Rc::clone(&promise),
                            is_resolve: true,
                        }));
                    let reject =
                        JsValue::BuiltinFunction(Rc::new(BuiltinFunction::PromiseResolver {
                            promise: Rc::clone(&promise),
                            is_resolve: false,
                        }));
                    let result = self.invoke_callable(
                        executor.clone(),
                        JsValue::Undefined,
                        vec![resolve, reject],
                    );
                    if let Err(error) = result {
                        self.settle_promise(
                            promise,
                            PromiseState::Rejected(self.to_rejection_value(error)),
                        );
                    }
                } else {
                    self.resolve_promise_value(promise, JsValue::Undefined);
                }
                Ok(promise_value)
            }
        }
    }

    fn collect_delegate_yields(
        &mut self,
        value: JsValue,
    ) -> Result<(Vec<JsValue>, JsValue), RuntimeError> {
        let mut cursor = self
            .begin_iteration(value)
            .map_err(|_| RuntimeError::TypeError("yield* requires an iterable value".into()))?;
        let mut yielded = Vec::new();
        loop {
            match self.iterator_step(&mut cursor, false)? {
                IteratorStep::Yield(value) => yielded.push(value),
                IteratorStep::Complete(value) => return Ok((yielded, value)),
            }
        }
    }

    fn collect_iterable_items(&mut self, value: JsValue) -> Result<Vec<JsValue>, RuntimeError> {
        let mut cursor = self.begin_iteration(value)?;
        let mut items = Vec::new();
        loop {
            match self.iterator_step(&mut cursor, false)? {
                IteratorStep::Yield(value) => items.push(value),
                IteratorStep::Complete(_) => return Ok(items),
            }
        }
    }

    fn collect_for_in_keys_from_object(
        &self,
        map: &crate::engine::value::JsObjectMap,
        keys: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        let borrowed = map.borrow();
        for key in borrowed.keys() {
            if key != "__proto__" && seen.insert(key.clone()) {
                keys.push(key.clone());
            }
        }
        let proto = borrowed.get("__proto__").cloned();
        drop(borrowed);
        match proto {
            Some(PropertyValue::Data(JsValue::Object(proto_map))) => {
                self.collect_for_in_keys_from_object(&proto_map, keys, seen);
            }
            Some(PropertyValue::Data(JsValue::Function(function))) => {
                self.collect_for_in_keys_from_object(&function.properties, keys, seen);
            }
            _ => {}
        }
    }

    fn collect_for_in_keys(&self, value: JsValue) -> Result<Vec<String>, RuntimeError> {
        match value {
            JsValue::Object(map) => {
                let mut keys = Vec::new();
                let mut seen = HashSet::new();
                self.collect_for_in_keys_from_object(&map, &mut keys, &mut seen);
                Ok(keys)
            }
            JsValue::EnvironmentObject(env) => Ok(env.borrow().variables.keys().cloned().collect()),
            JsValue::Function(function) => {
                let mut keys = Vec::new();
                let mut seen = HashSet::new();
                self.collect_for_in_keys_from_object(&function.properties, &mut keys, &mut seen);
                Ok(keys)
            }
            JsValue::Array(values) => Ok((0..values.borrow().len())
                .map(|index| index.to_string())
                .collect()),
            JsValue::String(value) => Ok((0..value.chars().count())
                .map(|index| index.to_string())
                .collect()),
            JsValue::Null | JsValue::Undefined => Err(RuntimeError::TypeError(
                "Cannot convert undefined or null to object".into(),
            )),
            _ => Ok(Vec::new()),
        }
    }

    fn current_module_base_dir(&self) -> Result<PathBuf, RuntimeError> {
        if let Some(dir) = self.module_base_dirs.last() {
            return Ok(dir.clone());
        }
        std::env::current_dir()
            .map_err(|err| RuntimeError::ReferenceError(format!("failed to read cwd: {err}")))
    }

    fn resolve_module_path(&self, source: &str) -> Result<PathBuf, RuntimeError> {
        let source_path = Path::new(source);
        let resolved = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            self.current_module_base_dir()?.join(source_path)
        };

        match fs::canonicalize(&resolved) {
            Ok(path) => Ok(path),
            Err(_) => Ok(resolved),
        }
    }

    fn export_identifiers_from_pattern(&self, pattern: &Expression, names: &mut Vec<String>) {
        match pattern {
            Expression::Identifier(name) => names.push((*name).to_string()),
            Expression::AssignmentExpression(assign)
                if matches!(assign.operator, AssignmentOperator::Assign) =>
            {
                self.export_identifiers_from_pattern(&assign.left, names);
            }
            Expression::ArrayExpression(elements) => {
                for element in elements.iter().flatten() {
                    self.export_identifiers_from_pattern(element, names);
                }
            }
            Expression::ObjectExpression(properties) => {
                for property in properties {
                    self.export_identifiers_from_pattern(&property.value, names);
                }
            }
            Expression::SpreadElement(inner) => self.export_identifiers_from_pattern(inner, names),
            _ => {}
        }
    }

    fn write_module_export_value(&mut self, exported_name: &str, value: JsValue) {
        if let Some(exports) = self.module_exports_stack.last() {
            exports
                .borrow_mut()
                .insert(exported_name.to_string(), PropertyValue::Data(value));
        }
    }

    fn write_module_export_binding(
        &mut self,
        exported_name: &str,
        env: Rc<RefCell<Environment>>,
        binding: &str,
    ) {
        if let Some(exports) = self.module_exports_stack.last() {
            exports.borrow_mut().insert(
                exported_name.to_string(),
                PropertyValue::Accessor {
                    getter: Some(JsValue::BuiltinFunction(Rc::new(
                        BuiltinFunction::ModuleBindingGetter {
                            env,
                            binding: binding.to_string(),
                        },
                    ))),
                    setter: None,
                },
            );
        }
    }

    fn write_module_export_namespace_binding(
        &mut self,
        exported_name: &str,
        namespace: JsValue,
        source_name: &str,
    ) {
        if let Some(exports) = self.module_exports_stack.last() {
            exports.borrow_mut().insert(
                exported_name.to_string(),
                PropertyValue::Accessor {
                    getter: Some(JsValue::BuiltinFunction(Rc::new(
                        BuiltinFunction::NamespaceBindingGetter {
                            namespace,
                            export_name: source_name.to_string(),
                        },
                    ))),
                    setter: None,
                },
            );
        }
    }

    fn read_namespace_export(
        &mut self,
        namespace: &JsValue,
        export_name: &str,
    ) -> Result<JsValue, RuntimeError> {
        match namespace {
            JsValue::Object(map) => match get_property_value(map, export_name) {
                Some(PropertyValue::Accessor {
                    getter: Some(getter),
                    ..
                }) => self.invoke_getter(getter, namespace.clone()),
                Some(PropertyValue::Data(value)) => Ok(value),
                _ => Ok(JsValue::Undefined),
            },
            _ => Err(RuntimeError::TypeError(
                "module namespace is not an object".into(),
            )),
        }
    }

    fn module_namespace_property_values(
        &self,
        namespace: &JsValue,
    ) -> Result<Vec<(String, PropertyValue)>, RuntimeError> {
        match namespace {
            JsValue::Object(map) => Ok(map
                .borrow()
                .iter()
                .filter(|(key, _)| key.as_str() != "__proto__")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()),
            _ => Err(RuntimeError::TypeError(
                "module namespace is not an object".into(),
            )),
        }
    }

    fn eval_program_in_env(
        &mut self,
        program: &Program,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        let mut last_val = JsValue::Undefined;
        for stmt in &program.body {
            match self.eval_statement(stmt, Rc::clone(&env)) {
                Ok(val) => last_val = val,
                Err(RuntimeError::Return(val)) => return Ok(val),
                Err(e) => return Err(e),
            }
        }
        self.drain_microtasks()?;
        Ok(last_val)
    }

    fn load_module_namespace(&mut self, source: &str) -> Result<JsValue, RuntimeError> {
        let path = self.resolve_module_path(source)?;
        if let Some(namespace) = self.module_cache.get(&path) {
            return Ok(namespace.clone());
        }

        let namespace = JsValue::Object(Rc::new(RefCell::new(HashMap::new())));
        self.module_cache.insert(path.clone(), namespace.clone());

        let module_source = fs::read_to_string(&path).map_err(|err| {
            RuntimeError::ReferenceError(format!("failed to load module {}: {err}", path.display()))
        })?;
        let lexer = Lexer::new(&module_source);
        let mut parser = Parser::new(lexer)
            .map_err(|err| RuntimeError::SyntaxError(format!("module parse init error: {err}")))?;
        let program = parser
            .parse_program()
            .map_err(|err| RuntimeError::SyntaxError(format!("module parse error: {err}")))?;

        let module_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(
            &self.global_env,
        )))));
        let namespace_map = match &namespace {
            JsValue::Object(map) => Rc::clone(map),
            _ => unreachable!(),
        };
        let base_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.module_exports_stack.push(Rc::clone(&namespace_map));
        self.module_base_dirs.push(base_dir);
        let eval_result = self.eval_program_in_env(&program, module_env);
        self.module_base_dirs.pop();
        self.module_exports_stack.pop();
        eval_result?;

        Ok(namespace)
    }

    fn member_property_key(
        &mut self,
        mem: &MemberExpression,
        env: Rc<RefCell<Environment>>,
    ) -> Result<String, RuntimeError> {
        if mem.computed {
            let property = self.eval_expression(&mem.property, env)?;
            Ok(match property {
                JsValue::String(value) => value,
                JsValue::Number(value) => {
                    if value.fract() == 0.0 {
                        format!("{value:.0}")
                    } else {
                        value.to_string()
                    }
                }
                _ => property.as_string(),
            })
        } else {
            match &mem.property {
                Expression::Identifier(name) => Ok((*name).to_string()),
                Expression::PrivateIdentifier(name) => Ok((*name).to_string()),
                other => Err(RuntimeError::TypeError(format!(
                    "invalid member property: {other:?}"
                ))),
            }
        }
    }

    fn property_key_from_value(&self, property: JsValue) -> String {
        match property {
            JsValue::String(value) => value,
            JsValue::Number(value) => {
                if value.fract() == 0.0 {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                }
            }
            other => other.as_string(),
        }
    }

    fn member_private_name<'a>(&self, mem: &'a MemberExpression<'a>) -> Option<&'a str> {
        if mem.computed {
            None
        } else if let Expression::PrivateIdentifier(name) = &mem.property {
            Some(*name)
        } else {
            None
        }
    }

    fn object_identity(value: &JsValue) -> Option<usize> {
        match value {
            JsValue::Object(map) => Some(Rc::as_ptr(map) as usize),
            JsValue::Function(function) => Some(Rc::as_ptr(function) as usize),
            JsValue::Array(values) => Some(Rc::as_ptr(values) as usize),
            JsValue::EnvironmentObject(env) => Some(Rc::as_ptr(env) as usize),
            _ => None,
        }
    }

    fn current_private_brand(&self, env: &Rc<RefCell<Environment>>) -> Option<usize> {
        env.borrow()
            .get("__private_brand__")
            .and_then(|value| match value {
                JsValue::Number(n) if n >= 0.0 => Some(n as usize),
                _ => None,
            })
    }

    fn brand_object(&mut self, object: &JsValue, brand: usize) {
        if let Some(id) = Self::object_identity(object) {
            self.object_private_brands
                .entry(id)
                .or_default()
                .insert(brand);
        }
    }

    fn object_has_brand(&self, object: &JsValue, brand: usize) -> bool {
        Self::object_identity(object)
            .and_then(|id| self.object_private_brands.get(&id))
            .is_some_and(|brands| brands.contains(&brand))
    }

    fn get_private_slot(&self, object: &JsValue, brand: usize, name: &str) -> Option<PrivateSlot> {
        Self::object_identity(object)
            .and_then(|id| self.object_private_slots.get(&id))
            .and_then(|slots| slots.get(&(brand, name.to_string())).cloned())
    }

    fn set_private_slot(&mut self, object: &JsValue, brand: usize, name: &str, value: JsValue) {
        if let Some(id) = Self::object_identity(object) {
            self.object_private_slots
                .entry(id)
                .or_default()
                .insert((brand, name.to_string()), PrivateSlot::Data(value));
        }
    }

    fn private_definition<'a>(
        &'a self,
        brand: usize,
        name: &str,
        is_static: bool,
    ) -> Option<&'a PrivateElementDefinition> {
        let elements = self.class_private_elements.get(&brand)?;
        if is_static {
            elements.static_members.get(name)
        } else {
            elements.instance.get(name)
        }
    }

    fn private_member_kind<'a>(
        &'a self,
        brand: usize,
        name: &str,
        object: &JsValue,
    ) -> Result<&'a PrivateElementDefinition, RuntimeError> {
        let is_static = matches!(object, JsValue::Function(_));
        self.private_definition(brand, name, is_static)
            .ok_or_else(|| {
                RuntimeError::TypeError(format!("private field '#{name}' is not defined"))
            })
    }

    fn read_private_member_value(
        &mut self,
        object: JsValue,
        name: &str,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        let brand = self.current_private_brand(&env).ok_or_else(|| {
            RuntimeError::SyntaxError("private identifier is not available in this context".into())
        })?;
        if !self.object_has_brand(&object, brand) {
            return Err(RuntimeError::TypeError(format!(
                "Cannot read private member '#{name}' from an object whose class did not declare it"
            )));
        }
        let definition = self.private_member_kind(brand, name, &object)?.clone();
        match definition.kind {
            PrivateElementKind::Field => match self.get_private_slot(&object, brand, name) {
                Some(PrivateSlot::Data(value)) => Ok(value),
                None => Ok(JsValue::Undefined),
            },
            PrivateElementKind::Method(value) => Ok(value),
            PrivateElementKind::Accessor { getter, .. } => match getter {
                Some(getter) => self.invoke_callable(getter, object, vec![]),
                None => Ok(JsValue::Undefined),
            },
        }
    }

    fn write_private_member_value(
        &mut self,
        object: JsValue,
        name: &str,
        value: JsValue,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        let brand = self.current_private_brand(&env).ok_or_else(|| {
            RuntimeError::SyntaxError("private identifier is not available in this context".into())
        })?;
        if !self.object_has_brand(&object, brand) {
            return Err(RuntimeError::TypeError(format!(
                "Cannot write private member '#{name}' to an object whose class did not declare it"
            )));
        }
        let definition = self.private_member_kind(brand, name, &object)?.clone();
        match definition.kind {
            PrivateElementKind::Field => {
                self.set_private_slot(&object, brand, name, value.clone());
                Ok(value)
            }
            PrivateElementKind::Accessor { setter, .. } => match setter {
                Some(setter) => {
                    self.invoke_callable(setter, object, vec![value.clone()])?;
                    Ok(value)
                }
                None => Err(RuntimeError::TypeError(format!(
                    "private member '#{name}' was defined without a setter"
                ))),
            },
            PrivateElementKind::Method(_) => Err(RuntimeError::TypeError(format!(
                "private member '#{name}' is not writable"
            ))),
        }
    }

    fn has_private_member_brand(
        &self,
        object: &JsValue,
        name: &str,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<bool, RuntimeError> {
        let brand = self.current_private_brand(env).ok_or_else(|| {
            RuntimeError::SyntaxError("private identifier is not available in this context".into())
        })?;
        let is_static = matches!(object, JsValue::Function(_));
        if self.private_definition(brand, name, is_static).is_none() {
            return Err(RuntimeError::TypeError(format!(
                "private field '#{name}' is not defined"
            )));
        }
        Ok(self.object_has_brand(object, brand))
    }

    fn declare_private_name(
        &self,
        declarations: &mut HashMap<String, PrivateDeclarationRecord>,
        name: &str,
        kind: PrivateDeclarationKind,
        is_static: bool,
    ) -> Result<(), RuntimeError> {
        let entry =
            declarations
                .entry(name.to_string())
                .or_insert_with(|| PrivateDeclarationRecord {
                    is_static,
                    ..PrivateDeclarationRecord::default()
                });

        if entry.is_static != is_static {
            return Err(RuntimeError::SyntaxError(format!(
                "duplicate private declaration '#{name}'"
            )));
        }

        let duplicate = match kind {
            PrivateDeclarationKind::Field => {
                entry.has_field || entry.has_method || entry.has_getter || entry.has_setter
            }
            PrivateDeclarationKind::Method => {
                entry.has_field || entry.has_method || entry.has_getter || entry.has_setter
            }
            PrivateDeclarationKind::Getter => {
                entry.has_field || entry.has_method || entry.has_getter
            }
            PrivateDeclarationKind::Setter => {
                entry.has_field || entry.has_method || entry.has_setter
            }
        };

        if duplicate {
            return Err(RuntimeError::SyntaxError(format!(
                "duplicate private declaration '#{name}'"
            )));
        }

        match kind {
            PrivateDeclarationKind::Field => entry.has_field = true,
            PrivateDeclarationKind::Method => entry.has_method = true,
            PrivateDeclarationKind::Getter => entry.has_getter = true,
            PrivateDeclarationKind::Setter => entry.has_setter = true,
        }

        Ok(())
    }

    fn ensure_private_name_declared(
        &self,
        declared_names: &HashSet<String>,
        name: &str,
    ) -> Result<(), RuntimeError> {
        if declared_names.contains(name) {
            Ok(())
        } else {
            Err(RuntimeError::SyntaxError(format!(
                "private name '#{name}' is not declared in the enclosing class"
            )))
        }
    }

    fn validate_private_names_in_function(
        &self,
        function: &FunctionDeclaration,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        for param in &function.params {
            self.validate_private_names_in_expression(&param.pattern, declared_names)?;
        }
        self.validate_private_names_in_block(&function.body, declared_names)
    }

    fn validate_private_names_in_block(
        &self,
        block: &BlockStatement,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        for statement in &block.body {
            self.validate_private_names_in_statement(statement, declared_names)?;
        }
        Ok(())
    }

    fn validate_private_names_in_statement(
        &self,
        statement: &Statement,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        match statement {
            Statement::ExpressionStatement(expr) => {
                self.validate_private_names_in_expression(expr, declared_names)
            }
            Statement::BlockStatement(block) => {
                self.validate_private_names_in_block(block, declared_names)
            }
            Statement::IfStatement(stmt) => {
                self.validate_private_names_in_expression(&stmt.test, declared_names)?;
                self.validate_private_names_in_statement(&stmt.consequent, declared_names)?;
                if let Some(alternate) = &stmt.alternate {
                    self.validate_private_names_in_statement(alternate, declared_names)?;
                }
                Ok(())
            }
            Statement::ReturnStatement(expr) => {
                if let Some(expr) = expr {
                    self.validate_private_names_in_expression(expr, declared_names)?;
                }
                Ok(())
            }
            Statement::ThrowStatement(expr) => {
                self.validate_private_names_in_expression(expr, declared_names)
            }
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    self.validate_private_names_in_expression(&declarator.id, declared_names)?;
                    if let Some(init) = &declarator.init {
                        self.validate_private_names_in_expression(init, declared_names)?;
                    }
                }
                Ok(())
            }
            Statement::FunctionDeclaration(function) => {
                self.validate_private_names_in_function(function, declared_names)
            }
            Statement::ClassDeclaration(_) => Ok(()),
            Statement::ForStatement(stmt) => {
                if let Some(init) = &stmt.init {
                    self.validate_private_names_in_statement(init, declared_names)?;
                }
                if let Some(test) = &stmt.test {
                    self.validate_private_names_in_expression(test, declared_names)?;
                }
                if let Some(update) = &stmt.update {
                    self.validate_private_names_in_expression(update, declared_names)?;
                }
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::ForInStatement(stmt) => {
                self.validate_private_names_in_statement(&stmt.left, declared_names)?;
                self.validate_private_names_in_expression(&stmt.right, declared_names)?;
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::ForOfStatement(stmt) => {
                self.validate_private_names_in_statement(&stmt.left, declared_names)?;
                self.validate_private_names_in_expression(&stmt.right, declared_names)?;
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::WhileStatement(stmt) | Statement::DoWhileStatement(stmt) => {
                self.validate_private_names_in_expression(&stmt.test, declared_names)?;
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::TryStatement(stmt) => {
                self.validate_private_names_in_block(&stmt.block, declared_names)?;
                if let Some(handler) = &stmt.handler {
                    if let Some(param) = &handler.param {
                        self.validate_private_names_in_expression(param, declared_names)?;
                    }
                    self.validate_private_names_in_block(&handler.body, declared_names)?;
                }
                if let Some(finalizer) = &stmt.finalizer {
                    self.validate_private_names_in_block(finalizer, declared_names)?;
                }
                Ok(())
            }
            Statement::SwitchStatement(stmt) => {
                self.validate_private_names_in_expression(&stmt.discriminant, declared_names)?;
                for case in &stmt.cases {
                    if let Some(test) = &case.test {
                        self.validate_private_names_in_expression(test, declared_names)?;
                    }
                    for consequent in &case.consequent {
                        self.validate_private_names_in_statement(consequent, declared_names)?;
                    }
                }
                Ok(())
            }
            Statement::LabeledStatement(stmt) => {
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::WithStatement(stmt) => {
                self.validate_private_names_in_expression(&stmt.object, declared_names)?;
                self.validate_private_names_in_statement(&stmt.body, declared_names)
            }
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(statement) = &decl.declaration {
                    self.validate_private_names_in_statement(statement, declared_names)?;
                }
                Ok(())
            }
            Statement::ExportDefaultDeclaration(decl) => match &decl.declaration {
                ExportDefaultKind::Expression(expr) => {
                    self.validate_private_names_in_expression(expr, declared_names)
                }
                ExportDefaultKind::FunctionDeclaration(function) => {
                    self.validate_private_names_in_function(function, declared_names)
                }
                ExportDefaultKind::ClassDeclaration(_) => Ok(()),
            },
            Statement::ImportDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::EmptyStatement => Ok(()),
        }
    }

    fn validate_private_names_in_expression(
        &self,
        expr: &Expression,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        match expr {
            Expression::PrivateIdentifier(name) => {
                self.ensure_private_name_declared(declared_names, name)
            }
            Expression::MemberExpression(member) => {
                self.validate_private_names_in_expression(&member.object, declared_names)?;
                if member.computed {
                    self.validate_private_names_in_expression(&member.property, declared_names)
                } else if let Expression::PrivateIdentifier(name) = &member.property {
                    self.ensure_private_name_declared(declared_names, name)
                } else {
                    Ok(())
                }
            }
            Expression::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::In
                    && let Expression::PrivateIdentifier(name) = &binary.left
                {
                    self.ensure_private_name_declared(declared_names, name)?;
                    self.validate_private_names_in_expression(&binary.right, declared_names)
                } else {
                    self.validate_private_names_in_expression(&binary.left, declared_names)?;
                    self.validate_private_names_in_expression(&binary.right, declared_names)
                }
            }
            Expression::UnaryExpression(unary) => {
                self.validate_private_names_in_expression(&unary.argument, declared_names)
            }
            Expression::AssignmentExpression(assign) => {
                self.validate_private_names_in_expression(&assign.left, declared_names)?;
                self.validate_private_names_in_expression(&assign.right, declared_names)
            }
            Expression::ArrayExpression(elements) => {
                for element in elements.iter().flatten() {
                    self.validate_private_names_in_expression(element, declared_names)?;
                }
                Ok(())
            }
            Expression::ObjectExpression(properties) => {
                for property in properties {
                    if let ObjectKey::Computed(expr) = &property.key {
                        self.validate_private_names_in_expression(expr, declared_names)?;
                    }
                    self.validate_private_names_in_expression(&property.value, declared_names)?;
                    match &property.kind {
                        ObjectPropertyKind::Value(value) => {
                            self.validate_private_names_in_expression(value, declared_names)?;
                        }
                        ObjectPropertyKind::Getter(function)
                        | ObjectPropertyKind::Setter(function) => {
                            self.validate_private_names_in_function(function, declared_names)?;
                        }
                    }
                }
                Ok(())
            }
            Expression::CallExpression(call) | Expression::NewExpression(call) => {
                self.validate_private_names_in_expression(&call.callee, declared_names)?;
                for argument in &call.arguments {
                    self.validate_private_names_in_expression(argument, declared_names)?;
                }
                Ok(())
            }
            Expression::FunctionExpression(function)
            | Expression::ArrowFunctionExpression(function) => {
                self.validate_private_names_in_function(function, declared_names)
            }
            Expression::ClassExpression(_) => Ok(()),
            Expression::UpdateExpression(update) => {
                self.validate_private_names_in_expression(&update.argument, declared_names)
            }
            Expression::SequenceExpression(seq) => {
                for expr in seq {
                    self.validate_private_names_in_expression(expr, declared_names)?;
                }
                Ok(())
            }
            Expression::ConditionalExpression {
                test,
                consequent,
                alternate,
            } => {
                self.validate_private_names_in_expression(test, declared_names)?;
                self.validate_private_names_in_expression(consequent, declared_names)?;
                self.validate_private_names_in_expression(alternate, declared_names)
            }
            Expression::SpreadElement(expr) | Expression::AwaitExpression(expr) => {
                self.validate_private_names_in_expression(expr, declared_names)
            }
            Expression::TemplateLiteral(parts) | Expression::TaggedTemplateExpression(_, parts) => {
                if let Expression::TaggedTemplateExpression(tag, _) = expr {
                    self.validate_private_names_in_expression(tag, declared_names)?;
                }
                for part in parts {
                    if let TemplatePart::Expr(expr) = part {
                        self.validate_private_names_in_expression(expr, declared_names)?;
                    }
                }
                Ok(())
            }
            Expression::YieldExpression { argument, .. } => {
                if let Some(argument) = argument {
                    self.validate_private_names_in_expression(argument, declared_names)?;
                }
                Ok(())
            }
            Expression::Literal(_)
            | Expression::Identifier(_)
            | Expression::ThisExpression
            | Expression::SuperExpression => Ok(()),
        }
    }

    fn validate_private_class_elements(
        &self,
        class_decl: &ClassDeclaration,
    ) -> Result<(), RuntimeError> {
        let mut declarations = HashMap::new();
        for element in &class_decl.body {
            let (key, kind, is_static) = match element {
                ClassElement::Method { key, is_static, .. } => {
                    (key, Some(PrivateDeclarationKind::Method), *is_static)
                }
                ClassElement::Getter { key, is_static, .. } => {
                    (key, Some(PrivateDeclarationKind::Getter), *is_static)
                }
                ClassElement::Setter { key, is_static, .. } => {
                    (key, Some(PrivateDeclarationKind::Setter), *is_static)
                }
                ClassElement::Field { key, is_static, .. } => {
                    (key, Some(PrivateDeclarationKind::Field), *is_static)
                }
                ClassElement::Constructor { .. } => continue,
            };

            if let (ObjectKey::PrivateIdentifier(name), Some(kind)) = (key, kind) {
                self.declare_private_name(&mut declarations, name, kind, is_static)?;
            }
        }

        let declared_names = declarations.into_keys().collect::<HashSet<_>>();
        for element in &class_decl.body {
            match element {
                ClassElement::Constructor { function, .. } => {
                    self.validate_private_names_in_function(function, &declared_names)?;
                }
                ClassElement::Method { key, value, .. } => {
                    if let ObjectKey::Computed(expr) = key {
                        self.validate_private_names_in_expression(expr, &declared_names)?;
                    }
                    self.validate_private_names_in_function(value, &declared_names)?;
                }
                ClassElement::Getter { key, body, .. } | ClassElement::Setter { key, body, .. } => {
                    if let ObjectKey::Computed(expr) = key {
                        self.validate_private_names_in_expression(expr, &declared_names)?;
                    }
                    self.validate_private_names_in_function(body, &declared_names)?;
                }
                ClassElement::Field {
                    key, initializer, ..
                } => {
                    if let ObjectKey::Computed(expr) = key {
                        self.validate_private_names_in_expression(expr, &declared_names)?;
                    }
                    if let Some(initializer) = initializer {
                        self.validate_private_names_in_expression(initializer, &declared_names)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn read_member_value(
        &mut self,
        object: JsValue,
        property_key: &str,
        accessor_this: Option<JsValue>,
    ) -> Result<JsValue, RuntimeError> {
        match object {
            JsValue::Object(values) => match get_property_value(&values, property_key) {
                Some(PropertyValue::Accessor {
                    getter: Some(getter),
                    ..
                }) => self.invoke_getter(
                    getter,
                    accessor_this.unwrap_or(JsValue::Object(Rc::clone(&values))),
                ),
                Some(PropertyValue::Data(value)) => Ok(value),
                _ => Ok(JsValue::Undefined),
            },
            JsValue::Array(values) => {
                let values = values.borrow();
                if property_key == "length" {
                    Ok(JsValue::Number(values.len() as f64))
                } else {
                    match property_key.parse::<usize>() {
                        Ok(index) => Ok(values.get(index).cloned().unwrap_or(JsValue::Undefined)),
                        Err(_) => Ok(JsValue::Undefined),
                    }
                }
            }
            JsValue::String(value) => match property_key {
                "length" => Ok(JsValue::Number(value.chars().count() as f64)),
                _ => {
                    if let Ok(index) = property_key.parse::<usize>() {
                        Ok(value
                            .chars()
                            .nth(index)
                            .map(|ch| JsValue::String(ch.to_string()))
                            .unwrap_or(JsValue::Undefined))
                    } else {
                        Ok(JsValue::Undefined)
                    }
                }
            },
            JsValue::EnvironmentObject(env) => {
                Ok(env.borrow().get(property_key).unwrap_or(JsValue::Undefined))
            }
            JsValue::Function(function) => {
                match get_property_value(&function.properties, property_key) {
                    Some(PropertyValue::Accessor {
                        getter: Some(getter),
                        ..
                    }) => self.invoke_getter(
                        getter,
                        accessor_this.unwrap_or(JsValue::Function(Rc::clone(&function))),
                    ),
                    Some(PropertyValue::Data(value)) => Ok(value),
                    _ => Ok(JsValue::Undefined),
                }
            }
            JsValue::Promise(_) | JsValue::BuiltinFunction(_) | JsValue::NativeFunction(_) => {
                Ok(object.get_property(property_key))
            }
            _ => Err(RuntimeError::TypeError("value is not an object".into())),
        }
    }

    fn to_int32(&self, value: &JsValue) -> i32 {
        self.to_uint32(value) as i32
    }

    fn to_uint32(&self, value: &JsValue) -> u32 {
        let number = value.as_number();
        if !number.is_finite() || number == 0.0 {
            return 0;
        }

        number.trunc().rem_euclid(4294967296.0) as u32
    }

    fn assignment_result(
        &self,
        operator: &AssignmentOperator,
        left: &JsValue,
        right: &JsValue,
    ) -> Result<JsValue, RuntimeError> {
        match operator {
            AssignmentOperator::Assign => Ok(right.clone()),
            AssignmentOperator::PlusAssign => left.add(right),
            AssignmentOperator::MinusAssign => left.sub(right),
            AssignmentOperator::MultiplyAssign => left.mul(right),
            AssignmentOperator::DivideAssign => left.div(right),
            AssignmentOperator::PercentAssign => {
                Ok(JsValue::Number(left.as_number() % right.as_number()))
            }
            AssignmentOperator::PowerAssign => {
                Ok(JsValue::Number(left.as_number().powf(right.as_number())))
            }
            AssignmentOperator::LogicAndAssign
            | AssignmentOperator::LogicOrAssign
            | AssignmentOperator::NullishAssign => Ok(right.clone()),
            AssignmentOperator::BitAndAssign => Ok(JsValue::Number(
                (self.to_int32(left) & self.to_int32(right)) as f64,
            )),
            AssignmentOperator::BitOrAssign => Ok(JsValue::Number(
                (self.to_int32(left) | self.to_int32(right)) as f64,
            )),
            AssignmentOperator::BitXorAssign => Ok(JsValue::Number(
                (self.to_int32(left) ^ self.to_int32(right)) as f64,
            )),
            AssignmentOperator::ShiftLeftAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) << shift) as f64))
            }
            AssignmentOperator::ShiftRightAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) >> shift) as f64))
            }
            AssignmentOperator::UnsignedShiftRightAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_uint32(left) >> shift) as f64))
            }
        }
    }

    fn eval_binary_operation(
        &mut self,
        operator: &BinaryOperator,
        left: JsValue,
        right: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        match operator {
            BinaryOperator::Plus => left.add(&right),
            BinaryOperator::Minus => left.sub(&right),
            BinaryOperator::Multiply => left.mul(&right),
            BinaryOperator::Divide => left.div(&right),
            BinaryOperator::Percent => Ok(JsValue::Number(left.as_number() % right.as_number())),
            BinaryOperator::BitAnd => Ok(JsValue::Number(
                (self.to_int32(&left) & self.to_int32(&right)) as f64,
            )),
            BinaryOperator::BitOr => Ok(JsValue::Number(
                (self.to_int32(&left) | self.to_int32(&right)) as f64,
            )),
            BinaryOperator::BitXor => Ok(JsValue::Number(
                (self.to_int32(&left) ^ self.to_int32(&right)) as f64,
            )),
            BinaryOperator::ShiftLeft => {
                let shift = self.to_uint32(&right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(&left) << shift) as f64))
            }
            BinaryOperator::ShiftRight => {
                let shift = self.to_uint32(&right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(&left) >> shift) as f64))
            }
            BinaryOperator::LogicalShiftRight => {
                let shift = self.to_uint32(&right) & 0x1f;
                Ok(JsValue::Number((self.to_uint32(&left) >> shift) as f64))
            }
            BinaryOperator::EqEq => Ok(JsValue::Boolean(js_abstract_eq(&left, &right))),
            BinaryOperator::EqEqEq => Ok(JsValue::Boolean(js_strict_eq(&left, &right))),
            BinaryOperator::NotEq => Ok(JsValue::Boolean(!js_abstract_eq(&left, &right))),
            BinaryOperator::NotEqEq => Ok(JsValue::Boolean(!js_strict_eq(&left, &right))),
            BinaryOperator::Less => left.lt(&right),
            BinaryOperator::LessEq => left.le(&right),
            BinaryOperator::Greater => left.gt(&right),
            BinaryOperator::GreaterEq => left.ge(&right),
            BinaryOperator::Power => Ok(JsValue::Number(left.as_number().powf(right.as_number()))),
            BinaryOperator::Instanceof => match (&left, &right) {
                (JsValue::Object(object), JsValue::Function(function)) => {
                    let prototype = function.prototype.clone();
                    let mut current = match object.borrow().get("__proto__").cloned() {
                        Some(PropertyValue::Data(value)) => Some(value),
                        _ => None,
                    };
                    while let Some(value) = current {
                        if js_strict_eq(&value, &prototype) {
                            return Ok(JsValue::Boolean(true));
                        }
                        current = match value {
                            JsValue::Object(proto) => {
                                match proto.borrow().get("__proto__").cloned() {
                                    Some(PropertyValue::Data(value)) => Some(value),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                    }
                    Ok(JsValue::Boolean(false))
                }
                _ => Ok(JsValue::Boolean(false)),
            },
            BinaryOperator::In => {
                let key = left.as_string();
                match &right {
                    JsValue::Object(map) => Ok(JsValue::Boolean(has_object_property(map, &key))),
                    JsValue::Array(arr) => {
                        if let Ok(idx) = key.parse::<usize>() {
                            Ok(JsValue::Boolean(idx < arr.borrow().len()))
                        } else {
                            Ok(JsValue::Boolean(false))
                        }
                    }
                    _ => Err(RuntimeError::TypeError(
                        "right-hand side of 'in' is not an object".into(),
                    )),
                }
            }
            BinaryOperator::LogicAnd
            | BinaryOperator::LogicOr
            | BinaryOperator::NullishCoalescing => Ok(right),
        }
    }

    fn write_member_value(
        &mut self,
        object: JsValue,
        property_key: &str,
        value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        match object {
            JsValue::Object(values) => {
                if let Some(PropertyValue::Accessor {
                    setter: Some(setter),
                    ..
                }) = get_property_value(&values, property_key)
                {
                    self.invoke_callable(
                        setter,
                        JsValue::Object(Rc::clone(&values)),
                        vec![value.clone()],
                    )?;
                    return Ok(value);
                }
                values.borrow_mut().insert(
                    property_key.to_string(),
                    crate::engine::value::PropertyValue::Data(value.clone()),
                );
                Ok(value)
            }
            JsValue::EnvironmentObject(env) => {
                if env.borrow().has_binding(property_key) {
                    env.borrow_mut()
                        .set(property_key, value.clone())
                        .map_err(RuntimeError::TypeError)?;
                } else {
                    env.borrow_mut()
                        .define(property_key.to_string(), value.clone());
                }
                Ok(value)
            }
            JsValue::Array(values) => {
                if property_key == "length" {
                    return Err(RuntimeError::TypeError(
                        "array length assignment is not supported".into(),
                    ));
                }

                let index = property_key.parse::<usize>().map_err(|_| {
                    RuntimeError::TypeError(
                        "array assignment requires a non-negative integer index".into(),
                    )
                })?;

                let mut values = values.borrow_mut();
                while values.len() < index {
                    values.push(JsValue::Undefined);
                }
                if values.len() == index {
                    values.push(value.clone());
                } else {
                    values[index] = value.clone();
                }
                Ok(value)
            }
            JsValue::Function(function) => {
                if let Some(PropertyValue::Accessor {
                    setter: Some(setter),
                    ..
                }) = get_property_value(&function.properties, property_key)
                {
                    self.invoke_callable(
                        setter,
                        JsValue::Function(Rc::clone(&function)),
                        vec![value.clone()],
                    )?;
                    return Ok(value);
                }
                function
                    .properties
                    .borrow_mut()
                    .insert(property_key.to_string(), PropertyValue::Data(value.clone()));
                Ok(value)
            }
            _ => Err(RuntimeError::TypeError("value is not an object".into())),
        }
    }

    fn write_own_member_value(
        &mut self,
        object: JsValue,
        property_key: &str,
        value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        match object {
            JsValue::Object(values) => {
                if let Some(PropertyValue::Accessor {
                    setter: Some(setter),
                    ..
                }) = values.borrow().get(property_key).cloned()
                {
                    self.invoke_callable(
                        setter,
                        JsValue::Object(Rc::clone(&values)),
                        vec![value.clone()],
                    )?;
                    return Ok(value);
                }
                values.borrow_mut().insert(
                    property_key.to_string(),
                    crate::engine::value::PropertyValue::Data(value.clone()),
                );
                Ok(value)
            }
            JsValue::EnvironmentObject(env) => {
                if env.borrow().has_binding(property_key) {
                    env.borrow_mut()
                        .set(property_key, value.clone())
                        .map_err(RuntimeError::TypeError)?;
                } else {
                    env.borrow_mut()
                        .define(property_key.to_string(), value.clone());
                }
                Ok(value)
            }
            JsValue::Array(values) => {
                if property_key == "length" {
                    return Err(RuntimeError::TypeError(
                        "array length assignment is not supported".into(),
                    ));
                }

                let index = property_key.parse::<usize>().map_err(|_| {
                    RuntimeError::TypeError(
                        "array assignment requires a non-negative integer index".into(),
                    )
                })?;

                let mut values = values.borrow_mut();
                while values.len() < index {
                    values.push(JsValue::Undefined);
                }
                if values.len() == index {
                    values.push(value.clone());
                } else {
                    values[index] = value.clone();
                }
                Ok(value)
            }
            JsValue::Function(function) => {
                if let Some(PropertyValue::Accessor {
                    setter: Some(setter),
                    ..
                }) = function.properties.borrow().get(property_key).cloned()
                {
                    self.invoke_callable(
                        setter,
                        JsValue::Function(Rc::clone(&function)),
                        vec![value.clone()],
                    )?;
                    return Ok(value);
                }
                function
                    .properties
                    .borrow_mut()
                    .insert(property_key.to_string(), PropertyValue::Data(value.clone()));
                Ok(value)
            }
            _ => Err(RuntimeError::TypeError("value is not an object".into())),
        }
    }

    fn get_property_value_from_base(
        &self,
        object: &JsValue,
        property_key: &str,
    ) -> Option<PropertyValue> {
        match object {
            JsValue::Object(values) => get_property_value(values, property_key),
            JsValue::Function(function) => get_property_value(&function.properties, property_key),
            JsValue::Array(values) => {
                let values = values.borrow();
                if property_key == "length" {
                    Some(PropertyValue::Data(JsValue::Number(values.len() as f64)))
                } else {
                    property_key
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| values.get(index).cloned())
                        .map(PropertyValue::Data)
                }
            }
            JsValue::EnvironmentObject(env) => {
                env.borrow().get(property_key).map(PropertyValue::Data)
            }
            _ => None,
        }
    }

    fn current_super_property_base(
        &self,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        let base = env
            .borrow()
            .get("__super_property_base__")
            .or_else(|| env.borrow().get("super"))
            .unwrap_or(JsValue::Undefined);
        if matches!(base, JsValue::Undefined) {
            Err(RuntimeError::TypeError(
                "super is not available in this context".into(),
            ))
        } else {
            Ok(base)
        }
    }

    fn current_super_receiver(&self, env: &Rc<RefCell<Environment>>) -> JsValue {
        env.borrow().get("this").unwrap_or(JsValue::Undefined)
    }

    fn read_super_member_value(
        &mut self,
        env: Rc<RefCell<Environment>>,
        property_key: &str,
    ) -> Result<JsValue, RuntimeError> {
        let receiver = self.current_super_receiver(&env);
        let base = self.current_super_property_base(&env)?;
        self.read_member_value(base, property_key, Some(receiver))
    }

    fn write_super_member_value(
        &mut self,
        env: Rc<RefCell<Environment>>,
        property_key: &str,
        value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        let receiver = self.current_super_receiver(&env);
        let base = self.current_super_property_base(&env)?;
        if let Some(PropertyValue::Accessor {
            setter: Some(setter),
            ..
        }) = self.get_property_value_from_base(&base, property_key)
        {
            self.invoke_callable(setter, receiver.clone(), vec![value.clone()])?;
            return Ok(value);
        }
        self.write_own_member_value(receiver, property_key, value)
    }

    fn should_apply_assignment(&self, operator: &AssignmentOperator, current: &JsValue) -> bool {
        match operator {
            AssignmentOperator::LogicAndAssign => current.is_truthy(),
            AssignmentOperator::LogicOrAssign => !current.is_truthy(),
            AssignmentOperator::NullishAssign => {
                matches!(current, JsValue::Undefined | JsValue::Null)
            }
            _ => true,
        }
    }

    fn apply_member_assignment(
        &mut self,
        object: JsValue,
        property_key: &str,
        operator: &AssignmentOperator,
        right_value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        let current = self.read_member_value(object.clone(), property_key, None)?;
        if !self.should_apply_assignment(operator, &current) {
            return Ok(current);
        }
        let value = self.assignment_result(operator, &current, &right_value)?;
        self.write_member_value(object, property_key, value.clone())?;
        Ok(value)
    }

    fn apply_super_member_assignment(
        &mut self,
        env: Rc<RefCell<Environment>>,
        property_key: &str,
        operator: &AssignmentOperator,
        right_value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        let current = self.read_super_member_value(Rc::clone(&env), property_key)?;
        if !self.should_apply_assignment(operator, &current) {
            return Ok(current);
        }
        let value = self.assignment_result(operator, &current, &right_value)?;
        self.write_super_member_value(env, property_key, value.clone())?;
        Ok(value)
    }

    fn collect_with_scope_bindings(&self, object: &JsValue) -> Vec<(String, JsValue)> {
        match object {
            JsValue::Object(map) => map
                .borrow()
                .keys()
                .cloned()
                .map(|key| {
                    let value = get_object_property(map, &key);
                    (key, value)
                })
                .collect(),
            JsValue::Function(function) => function
                .properties
                .borrow()
                .keys()
                .cloned()
                .map(|key| {
                    let value = get_object_property(&function.properties, &key);
                    (key, value)
                })
                .collect(),
            JsValue::Array(arr) => {
                let arr = arr.borrow();
                let mut bindings = arr
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), value.clone()))
                    .collect::<Vec<_>>();
                bindings.push(("length".to_string(), JsValue::Number(arr.len() as f64)));
                bindings
            }
            JsValue::String(s) => {
                let mut bindings = s
                    .chars()
                    .enumerate()
                    .map(|(index, ch)| (index.to_string(), JsValue::String(ch.to_string())))
                    .collect::<Vec<_>>();
                bindings.push((
                    "length".to_string(),
                    JsValue::Number(s.chars().count() as f64),
                ));
                bindings
            }
            JsValue::EnvironmentObject(env) => env
                .borrow()
                .variables
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn read_property_for_pattern(
        &mut self,
        source: &JsValue,
        key: &str,
    ) -> Result<JsValue, RuntimeError> {
        match source {
            JsValue::Object(values) => match get_property_value(values, key) {
                Some(PropertyValue::Accessor {
                    getter: Some(getter),
                    ..
                }) => self.invoke_getter(getter, JsValue::Object(Rc::clone(values))),
                Some(PropertyValue::Data(value)) => Ok(value),
                _ => Ok(JsValue::Undefined),
            },
            JsValue::Array(values) => {
                let values = values.borrow();
                if key == "length" {
                    Ok(JsValue::Number(values.len() as f64))
                } else {
                    Ok(key
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| values.get(index).cloned())
                        .unwrap_or(JsValue::Undefined))
                }
            }
            JsValue::String(s) => {
                if key == "length" {
                    Ok(JsValue::Number(s.chars().count() as f64))
                } else {
                    Ok(key
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| s.chars().nth(index))
                        .map(|ch| JsValue::String(ch.to_string()))
                        .unwrap_or(JsValue::Undefined))
                }
            }
            JsValue::Function(function) => match get_property_value(&function.properties, key) {
                Some(PropertyValue::Accessor {
                    getter: Some(getter),
                    ..
                }) => self.invoke_getter(getter, JsValue::Function(Rc::clone(function))),
                Some(PropertyValue::Data(value)) => Ok(value),
                _ => Ok(JsValue::Undefined),
            },
            JsValue::EnvironmentObject(env) => {
                Ok(env.borrow().get(key).unwrap_or(JsValue::Undefined))
            }
            JsValue::Null | JsValue::Undefined => Err(RuntimeError::TypeError(
                "Cannot destructure null or undefined".into(),
            )),
            _ => Ok(JsValue::Undefined),
        }
    }

    fn enumerable_keys_for_pattern(&self, source: &JsValue) -> Result<Vec<String>, RuntimeError> {
        match source {
            JsValue::Object(map) => Ok(map
                .borrow()
                .keys()
                .filter(|key| key.as_str() != "__proto__")
                .cloned()
                .collect()),
            JsValue::Function(function) => Ok(function
                .properties
                .borrow()
                .keys()
                .filter(|key| key.as_str() != "__proto__" && key.as_str() != "prototype")
                .cloned()
                .collect()),
            JsValue::Array(values) => Ok((0..values.borrow().len())
                .map(|index| index.to_string())
                .collect()),
            JsValue::String(s) => Ok((0..s.chars().count())
                .map(|index| index.to_string())
                .collect()),
            JsValue::EnvironmentObject(env) => Ok(env.borrow().variables.keys().cloned().collect()),
            JsValue::Null | JsValue::Undefined => Err(RuntimeError::TypeError(
                "Cannot destructure null or undefined".into(),
            )),
            _ => Ok(vec![]),
        }
    }

    fn object_rest_for_pattern(
        &mut self,
        source: &JsValue,
        excluded: &HashSet<String>,
    ) -> Result<JsValue, RuntimeError> {
        let mut rest = std::collections::HashMap::new();
        for key in self.enumerable_keys_for_pattern(source)? {
            if excluded.contains(&key) {
                continue;
            }
            let value = self.read_property_for_pattern(source, &key)?;
            rest.insert(key, PropertyValue::Data(value));
        }
        Ok(JsValue::Object(Rc::new(RefCell::new(rest))))
    }

    fn assign_identifier(
        &mut self,
        name: &str,
        value: JsValue,
        env: Rc<RefCell<Environment>>,
        declare: bool,
    ) -> Result<(), RuntimeError> {
        if declare {
            env.borrow_mut().define(name.to_string(), value);
        } else if env.borrow().has_binding(name) {
            env.borrow_mut()
                .set(name, value)
                .map_err(RuntimeError::TypeError)?;
        } else {
            env.borrow_mut().define(name.to_string(), value);
        }
        Ok(())
    }

    fn assign_pattern(
        &mut self,
        pattern: &Expression,
        value: JsValue,
        env: Rc<RefCell<Environment>>,
        declare: bool,
    ) -> Result<(), RuntimeError> {
        match pattern {
            Expression::Identifier(name) => self.assign_identifier(name, value, env, declare),
            Expression::AssignmentExpression(assign)
                if matches!(assign.operator, AssignmentOperator::Assign) =>
            {
                let next_value = if matches!(value, JsValue::Undefined) {
                    self.eval_expression(&assign.right, Rc::clone(&env))?
                } else {
                    value
                };
                self.assign_pattern(&assign.left, next_value, env, declare)
            }
            Expression::ArrayExpression(elements) => {
                if matches!(value, JsValue::Null | JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "Cannot destructure null or undefined".into(),
                    ));
                }
                let items = self.collect_iterable_items(value)?;

                let mut index = 0usize;
                for element in elements {
                    match element {
                        None => index += 1,
                        Some(Expression::SpreadElement(rest_pattern)) => {
                            let rest_items = items.iter().skip(index).cloned().collect::<Vec<_>>();
                            self.assign_pattern(
                                rest_pattern,
                                JsValue::Array(Rc::new(RefCell::new(rest_items))),
                                Rc::clone(&env),
                                declare,
                            )?;
                            break;
                        }
                        Some(element_pattern) => {
                            let item = items.get(index).cloned().unwrap_or(JsValue::Undefined);
                            self.assign_pattern(element_pattern, item, Rc::clone(&env), declare)?;
                            index += 1;
                        }
                    }
                }
                Ok(())
            }
            Expression::ObjectExpression(properties) => {
                if matches!(value, JsValue::Null | JsValue::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "Cannot destructure null or undefined".into(),
                    ));
                }

                let mut excluded = HashSet::new();
                for prop in properties {
                    if let Expression::SpreadElement(rest_pattern) = &prop.value {
                        let rest = self.object_rest_for_pattern(&value, &excluded)?;
                        self.assign_pattern(rest_pattern, rest, Rc::clone(&env), declare)?;
                        continue;
                    }

                    let key = match &prop.key {
                        ObjectKey::Identifier(name) | ObjectKey::String(name) => {
                            (*name).to_string()
                        }
                        ObjectKey::Number(n) => n.to_string(),
                        ObjectKey::Computed(expr) => {
                            self.eval_expression(expr, Rc::clone(&env))?.as_string()
                        }
                        ObjectKey::PrivateIdentifier(_) => {
                            return Err(RuntimeError::SyntaxError(
                                "private identifier cannot appear in object patterns".into(),
                            ));
                        }
                    };
                    excluded.insert(key.clone());
                    let prop_value = self.read_property_for_pattern(&value, &key)?;
                    self.assign_pattern(&prop.value, prop_value, Rc::clone(&env), declare)?;
                }
                Ok(())
            }
            Expression::MemberExpression(member) if !declare => {
                if let Some(name) = self.member_private_name(member) {
                    let object = self.eval_expression(&member.object, Rc::clone(&env))?;
                    self.write_private_member_value(object, name, value, env)?;
                } else if matches!(member.object, Expression::SuperExpression) {
                    let property_key = self.member_property_key(member, Rc::clone(&env))?;
                    self.write_super_member_value(env, &property_key, value)?;
                } else {
                    let object = self.eval_expression(&member.object, Rc::clone(&env))?;
                    let property_key = self.member_property_key(member, Rc::clone(&env))?;
                    self.write_member_value(object, &property_key, value)?;
                }
                Ok(())
            }
            Expression::SpreadElement(inner) => self.assign_pattern(inner, value, env, declare),
            _ => Err(RuntimeError::SyntaxError(
                "invalid destructuring pattern".into(),
            )),
        }
    }

    fn bind_parameters(
        &mut self,
        params: &[Param],
        args: &[JsValue],
        env: Rc<RefCell<Environment>>,
    ) -> Result<(), RuntimeError> {
        let mut arg_index = 0usize;
        for param in params {
            if param.is_rest {
                let rest = args.get(arg_index..).unwrap_or(&[]).to_vec();
                self.assign_pattern(
                    &param.pattern,
                    JsValue::Array(Rc::new(RefCell::new(rest))),
                    Rc::clone(&env),
                    true,
                )?;
                break;
            }

            let value = args.get(arg_index).cloned().unwrap_or(JsValue::Undefined);
            self.assign_pattern(&param.pattern, value, Rc::clone(&env), true)?;
            arg_index += 1;
        }
        Ok(())
    }

    fn sync_with_scope_bindings(
        &mut self,
        object: &JsValue,
        with_env: Rc<RefCell<Environment>>,
        binding_keys: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        if !matches!(
            object,
            JsValue::Object(_)
                | JsValue::Function(_)
                | JsValue::Array(_)
                | JsValue::EnvironmentObject(_)
        ) {
            return Ok(());
        }
        for key in binding_keys {
            if key == "length" {
                continue;
            }
            let value = with_env.borrow().variables.get(key).cloned();
            if let Some(value) = value {
                self.write_member_value(object.clone(), key, value)?;
            }
        }
        Ok(())
    }

    fn check_timeout(&mut self) -> Result<(), RuntimeError> {
        self.instruction_count += 1;
        if self.instruction_count > 2_000 {
            return Err(RuntimeError::Timeout);
        }
        Ok(())
    }

    fn create_function_value(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
    ) -> JsValue {
        let private_brand = env
            .borrow()
            .get("__private_brand__")
            .and_then(|value| match value {
                JsValue::Number(n) if n >= 0.0 => Some(n as usize),
                _ => None,
            });
        self.create_function_value_with_meta(
            func,
            env,
            None,
            None,
            None,
            private_brand,
            false,
            false,
            false,
            true,
        )
    }

    fn create_arrow_function_value(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
    ) -> JsValue {
        let super_binding = env.borrow().get("super");
        let super_property_base = env.borrow().get("__super_property_base__");
        let home_object = env.borrow().get("__home_object__");
        let private_brand = env
            .borrow()
            .get("__private_brand__")
            .and_then(|value| match value {
                JsValue::Number(n) if n >= 0.0 => Some(n as usize),
                _ => None,
            });
        self.create_function_value_with_meta(
            func,
            env,
            super_binding,
            super_property_base,
            home_object,
            private_brand,
            false,
            false,
            true,
            false,
        )
    }

    fn create_method_function_value(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
        super_binding: Option<JsValue>,
        super_property_base: Option<JsValue>,
        home_object: Option<JsValue>,
        private_brand: Option<usize>,
    ) -> JsValue {
        self.create_function_value_with_meta(
            func,
            env,
            super_binding,
            super_property_base,
            home_object,
            private_brand,
            false,
            false,
            false,
            false,
        )
    }

    fn create_accessor_function_value(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
        super_binding: Option<JsValue>,
        super_property_base: Option<JsValue>,
        home_object: Option<JsValue>,
        private_brand: Option<usize>,
    ) -> JsValue {
        self.create_function_value_with_meta(
            func,
            env,
            super_binding,
            super_property_base,
            home_object,
            private_brand,
            false,
            false,
            false,
            false,
        )
    }

    fn create_function_value_with_meta(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
        super_binding: Option<JsValue>,
        super_property_base: Option<JsValue>,
        home_object: Option<JsValue>,
        private_brand: Option<usize>,
        is_class_constructor: bool,
        is_derived_constructor: bool,
        uses_lexical_this: bool,
        can_construct: bool,
    ) -> JsValue {
        let id = self.functions.len();
        self.functions.push(clone_function_declaration(func));
        let prototype = object_with_proto(JsValue::Null);
        let properties = crate::engine::value::new_object_map();
        if can_construct {
            properties.borrow_mut().insert(
                "prototype".to_string(),
                crate::engine::value::PropertyValue::Data(prototype.clone()),
            );
        }
        JsValue::Function(Rc::new(FunctionValue {
            id,
            env,
            prototype,
            properties,
            super_binding,
            super_property_base,
            home_object,
            private_brand,
            uses_lexical_this,
            can_construct,
            is_class_constructor,
            is_derived_constructor,
        }))
    }

    fn current_home_object_super_property_base(&self, home_object: &JsValue) -> Option<JsValue> {
        match home_object {
            JsValue::Object(map) => match map.borrow().get("__proto__").cloned() {
                Some(PropertyValue::Data(value)) => Some(value),
                None => Some(JsValue::Null),
                _ => None,
            },
            JsValue::Function(function) => {
                match function.properties.borrow().get("__proto__").cloned() {
                    Some(PropertyValue::Data(value)) => Some(value),
                    None => Some(JsValue::Null),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn bind_function_super_context(
        &self,
        target_env: &Rc<RefCell<Environment>>,
        function: &FunctionValue,
    ) {
        if let Some(home_object) = &function.home_object {
            target_env
                .borrow_mut()
                .define("__home_object__".to_string(), home_object.clone());
        }
        if let Some(private_brand) = function.private_brand {
            target_env.borrow_mut().define(
                "__private_brand__".to_string(),
                JsValue::Number(private_brand as f64),
            );
        }

        let super_property_base = function
            .home_object
            .as_ref()
            .and_then(|home_object| self.current_home_object_super_property_base(home_object))
            .or_else(|| function.super_property_base.clone());

        if let Some(super_property_base) = super_property_base {
            target_env
                .borrow_mut()
                .define("__super_property_base__".to_string(), super_property_base);
        }
    }

    fn initialize_instance_fields_for_function(
        &mut self,
        function: Rc<FunctionValue>,
        this_value: JsValue,
    ) -> Result<(), RuntimeError> {
        if let Some(private_brand) = function.private_brand {
            self.brand_object(&this_value, private_brand);
            if let Some(private_elements) = self.class_private_elements.get(&private_brand).cloned()
            {
                let field_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(
                    &function.env,
                )))));
                field_env
                    .borrow_mut()
                    .define("this".to_string(), this_value.clone());
                field_env
                    .borrow_mut()
                    .define("__constructor_this__".to_string(), this_value.clone());
                field_env.borrow_mut().define(
                    "__private_brand__".to_string(),
                    JsValue::Number(private_brand as f64),
                );
                if let Some(super_binding) = &function.super_binding {
                    field_env
                        .borrow_mut()
                        .define("super".to_string(), super_binding.clone());
                }
                self.bind_function_super_context(&field_env, function.as_ref());
                for field in private_elements.instance_fields {
                    let value = match &field.initializer {
                        Some(expr) => self.eval_expression(expr, Rc::clone(&field_env))?,
                        None => JsValue::Undefined,
                    };
                    self.set_private_slot(&this_value, private_brand, &field.name, value);
                }
            }
        }

        let Some(fields) = self.class_instance_fields.get(&function.id).cloned() else {
            return Ok(());
        };

        if fields.is_empty() {
            return Ok(());
        }

        let field_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(
            &function.env,
        )))));
        field_env
            .borrow_mut()
            .define("this".to_string(), this_value.clone());
        field_env
            .borrow_mut()
            .define("__constructor_this__".to_string(), this_value.clone());
        if let Some(super_binding) = &function.super_binding {
            field_env
                .borrow_mut()
                .define("super".to_string(), super_binding.clone());
        }
        self.bind_function_super_context(&field_env, function.as_ref());

        for field in fields {
            let value = match &field.initializer {
                Some(expr) => self.eval_expression(expr, Rc::clone(&field_env))?,
                None => JsValue::Undefined,
            };
            self.write_member_value(this_value.clone(), &field.key, value)?;
        }

        Ok(())
    }

    fn maybe_initialize_current_instance_fields(
        &mut self,
        this_value: JsValue,
    ) -> Result<(), RuntimeError> {
        let Some(frame_index) = self.call_stack.len().checked_sub(1) else {
            return Ok(());
        };

        if self.call_stack[frame_index].instance_fields_initialized {
            return Ok(());
        }

        let function = Rc::clone(&self.call_stack[frame_index].function);
        self.initialize_instance_fields_for_function(function, this_value)?;
        self.call_stack[frame_index].instance_fields_initialized = true;
        Ok(())
    }

    fn class_public_key_to_string(
        &mut self,
        key: &ObjectKey<'_>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<String, RuntimeError> {
        match key {
            ObjectKey::Identifier(name) | ObjectKey::String(name) => Ok((*name).to_string()),
            ObjectKey::Number(n) => Ok(n.to_string()),
            ObjectKey::Computed(expr) => Ok(self.eval_expression(expr, env)?.as_string()),
            ObjectKey::PrivateIdentifier(_) => Err(RuntimeError::SyntaxError(
                "private key is not a public property name".into(),
            )),
        }
    }

    fn build_class_value(
        &mut self,
        class_decl: &ClassDeclaration,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        self.validate_private_class_elements(class_decl)?;
        let super_value = match &class_decl.super_class {
            Some(expr) => Some(self.eval_expression(expr, Rc::clone(&env))?),
            None => None,
        };
        let super_prototype = match &super_value {
            Some(JsValue::Function(function)) => function.prototype.clone(),
            Some(_) => {
                return Err(RuntimeError::TypeError(
                    "class extends value is not a constructor".into(),
                ));
            }
            None => JsValue::Null,
        };
        let private_brand = self.next_private_brand;
        self.next_private_brand += 1;

        let constructor_decl = class_decl
            .body
            .iter()
            .find_map(|element| match element {
                ClassElement::Constructor {
                    function: func,
                    is_default: _,
                } => Some(func.clone()),
                _ => None,
            })
            .unwrap_or(FunctionDeclaration {
                id: class_decl.id,
                params: vec![],
                body: BlockStatement { body: vec![] },
                is_generator: false,
                is_async: false,
            });

        let class_value = self.create_function_value_with_meta(
            &constructor_decl,
            Rc::clone(&env),
            super_value.clone(),
            if class_decl.super_class.is_some() {
                Some(super_prototype.clone())
            } else {
                None
            },
            None,
            Some(private_brand),
            true,
            class_decl.super_class.is_some(),
            false,
            true,
        );
        let JsValue::Function(function) = &class_value else {
            unreachable!();
        };

        if let JsValue::Object(proto_map) = &function.prototype {
            proto_map.borrow_mut().insert(
                "__proto__".to_string(),
                crate::engine::value::PropertyValue::Data(super_prototype.clone()),
            );
            proto_map.borrow_mut().insert(
                "constructor".to_string(),
                crate::engine::value::PropertyValue::Data(class_value.clone()),
            );
        }
        function.properties.borrow_mut().insert(
            "__proto__".to_string(),
            crate::engine::value::PropertyValue::Data(super_value.clone().unwrap_or(JsValue::Null)),
        );
        self.brand_object(&class_value, private_brand);

        let mut instance_fields = Vec::new();
        let mut private_elements = ClassPrivateElements::default();

        for element in &class_decl.body {
            match element {
                ClassElement::Method {
                    key,
                    value,
                    is_static,
                } => {
                    if let ObjectKey::PrivateIdentifier(name) = key {
                        let method_value = self.create_method_function_value(
                            value,
                            Rc::clone(&env),
                            None,
                            None,
                            None,
                            Some(private_brand),
                        );
                        let target = if *is_static {
                            &mut private_elements.static_members
                        } else {
                            &mut private_elements.instance
                        };
                        if target.contains_key(*name) {
                            return Err(RuntimeError::TypeError(format!(
                                "duplicate private member '#{name}'"
                            )));
                        }
                        target.insert(
                            (*name).to_string(),
                            PrivateElementDefinition {
                                kind: PrivateElementKind::Method(method_value),
                            },
                        );
                        continue;
                    }
                    let method_key = self.class_public_key_to_string(key, Rc::clone(&env))?;
                    let method_super_binding = if *is_static {
                        super_value.clone()
                    } else {
                        Some(super_prototype.clone())
                    };
                    let method_super_property_base = method_super_binding.clone();
                    let method_home_object = if *is_static {
                        Some(class_value.clone())
                    } else {
                        Some(function.prototype.clone())
                    };
                    let method_value = self.create_method_function_value(
                        value,
                        Rc::clone(&env),
                        method_super_binding,
                        method_super_property_base,
                        method_home_object,
                        Some(private_brand),
                    );
                    if *is_static {
                        function
                            .properties
                            .borrow_mut()
                            .insert(method_key, PropertyValue::Data(method_value));
                    } else if let JsValue::Object(proto_map) = &function.prototype {
                        proto_map
                            .borrow_mut()
                            .insert(method_key, PropertyValue::Data(method_value));
                    }
                }
                ClassElement::Getter {
                    key,
                    body,
                    is_static,
                } => {
                    if let ObjectKey::PrivateIdentifier(name) = key {
                        let getter = self.create_accessor_function_value(
                            body,
                            Rc::clone(&env),
                            None,
                            None,
                            None,
                            Some(private_brand),
                        );
                        let target = if *is_static {
                            &mut private_elements.static_members
                        } else {
                            &mut private_elements.instance
                        };
                        let entry = target.entry((*name).to_string()).or_insert_with(|| {
                            PrivateElementDefinition {
                                kind: PrivateElementKind::Accessor {
                                    getter: None,
                                    setter: None,
                                },
                            }
                        });
                        match &mut entry.kind {
                            PrivateElementKind::Accessor { getter: slot, .. } => {
                                if slot.is_some() {
                                    return Err(RuntimeError::TypeError(format!(
                                        "duplicate private getter '#{name}'"
                                    )));
                                }
                                *slot = Some(getter);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError(format!(
                                    "duplicate private member '#{name}'"
                                )));
                            }
                        }
                        continue;
                    }
                    let property_key = self.class_public_key_to_string(key, Rc::clone(&env))?;
                    let getter_super_binding = if *is_static {
                        super_value.clone()
                    } else {
                        Some(super_prototype.clone())
                    };
                    let getter_super_property_base = getter_super_binding.clone();
                    let getter_home_object = if *is_static {
                        Some(class_value.clone())
                    } else {
                        Some(function.prototype.clone())
                    };
                    let getter = self.create_accessor_function_value(
                        body,
                        Rc::clone(&env),
                        getter_super_binding,
                        getter_super_property_base,
                        getter_home_object,
                        Some(private_brand),
                    );
                    let target = if *is_static {
                        &function.properties
                    } else if let JsValue::Object(proto_map) = &function.prototype {
                        proto_map
                    } else {
                        unreachable!()
                    };
                    let setter = match get_property_value(target, &property_key) {
                        Some(PropertyValue::Accessor { setter, .. }) => setter,
                        _ => None,
                    };
                    target.borrow_mut().insert(
                        property_key,
                        PropertyValue::Accessor {
                            getter: Some(getter),
                            setter,
                        },
                    );
                }
                ClassElement::Setter {
                    key,
                    body,
                    is_static,
                } => {
                    if let ObjectKey::PrivateIdentifier(name) = key {
                        let setter_fn = self.create_accessor_function_value(
                            body,
                            Rc::clone(&env),
                            None,
                            None,
                            None,
                            Some(private_brand),
                        );
                        let target = if *is_static {
                            &mut private_elements.static_members
                        } else {
                            &mut private_elements.instance
                        };
                        let entry = target.entry((*name).to_string()).or_insert_with(|| {
                            PrivateElementDefinition {
                                kind: PrivateElementKind::Accessor {
                                    getter: None,
                                    setter: None,
                                },
                            }
                        });
                        match &mut entry.kind {
                            PrivateElementKind::Accessor { setter, .. } => {
                                if setter.is_some() {
                                    return Err(RuntimeError::TypeError(format!(
                                        "duplicate private setter '#{name}'"
                                    )));
                                }
                                *setter = Some(setter_fn);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError(format!(
                                    "duplicate private member '#{name}'"
                                )));
                            }
                        }
                        continue;
                    }
                    let property_key = self.class_public_key_to_string(key, Rc::clone(&env))?;
                    let setter_super_binding = if *is_static {
                        super_value.clone()
                    } else {
                        Some(super_prototype.clone())
                    };
                    let setter_super_property_base = setter_super_binding.clone();
                    let setter_home_object = if *is_static {
                        Some(class_value.clone())
                    } else {
                        Some(function.prototype.clone())
                    };
                    let setter_fn = self.create_accessor_function_value(
                        body,
                        Rc::clone(&env),
                        setter_super_binding,
                        setter_super_property_base,
                        setter_home_object,
                        Some(private_brand),
                    );
                    let target = if *is_static {
                        &function.properties
                    } else if let JsValue::Object(proto_map) = &function.prototype {
                        proto_map
                    } else {
                        unreachable!()
                    };
                    let getter = match get_property_value(target, &property_key) {
                        Some(PropertyValue::Accessor { getter, .. }) => getter,
                        _ => None,
                    };
                    target.borrow_mut().insert(
                        property_key,
                        PropertyValue::Accessor {
                            getter,
                            setter: Some(setter_fn),
                        },
                    );
                }
                ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                } => {
                    if let ObjectKey::PrivateIdentifier(name) = key {
                        let target = if *is_static {
                            &mut private_elements.static_fields
                        } else {
                            &mut private_elements.instance_fields
                        };
                        if target.iter().any(|field| field.name == *name)
                            || if *is_static {
                                private_elements.static_members.contains_key(*name)
                            } else {
                                private_elements.instance.contains_key(*name)
                            }
                        {
                            return Err(RuntimeError::TypeError(format!(
                                "duplicate private member '#{name}'"
                            )));
                        }
                        let members = if *is_static {
                            &mut private_elements.static_members
                        } else {
                            &mut private_elements.instance
                        };
                        members.insert(
                            (*name).to_string(),
                            PrivateElementDefinition {
                                kind: PrivateElementKind::Field,
                            },
                        );
                        target.push(PrivateFieldDefinition {
                            name: (*name).to_string(),
                            initializer: initializer.as_ref().map(clone_expression),
                        });
                        continue;
                    }
                    let field_key = self.class_public_key_to_string(key, Rc::clone(&env))?;
                    if *is_static {
                        let field_value = match initializer {
                            Some(expr) => self.eval_expression(expr, Rc::clone(&env))?,
                            None => JsValue::Undefined,
                        };
                        function.properties.borrow_mut().insert(
                            field_key,
                            crate::engine::value::PropertyValue::Data(field_value),
                        );
                    } else {
                        instance_fields.push(InstanceFieldDefinition {
                            key: field_key,
                            initializer: initializer.as_ref().map(clone_expression),
                        });
                    }
                }
                _ => {}
            }
        }

        if instance_fields.is_empty() {
            self.class_instance_fields.remove(&function.id);
        } else {
            self.class_instance_fields
                .insert(function.id, instance_fields);
        }
        self.class_private_elements
            .insert(private_brand, private_elements.clone());

        if !private_elements.static_fields.is_empty() {
            let static_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
            static_env
                .borrow_mut()
                .define("this".to_string(), class_value.clone());
            static_env
                .borrow_mut()
                .define("__constructor_this__".to_string(), class_value.clone());
            static_env.borrow_mut().define(
                "__private_brand__".to_string(),
                JsValue::Number(private_brand as f64),
            );
            for field in private_elements.static_fields {
                let value = match &field.initializer {
                    Some(expr) => self.eval_expression(expr, Rc::clone(&static_env))?,
                    None => JsValue::Undefined,
                };
                self.set_private_slot(&class_value, private_brand, &field.name, value);
            }
        }

        Ok(class_value)
    }

    fn call_function_value(
        &mut self,
        function: Rc<FunctionValue>,
        this_value: JsValue,
        args: Vec<JsValue>,
        is_construct_call: bool,
    ) -> Result<JsValue, RuntimeError> {
        if is_construct_call && !function.can_construct {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        if function.is_class_constructor && !is_construct_call {
            return Err(RuntimeError::TypeError(
                "class constructor cannot be invoked without 'new'".into(),
            ));
        }
        let declaration = self
            .functions
            .get(function.id)
            .cloned()
            .ok_or_else(|| RuntimeError::TypeError("function body missing".into()))?;
        let call_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(
            &function.env,
        )))));
        let initial_this = if function.is_derived_constructor {
            JsValue::Undefined
        } else if function.uses_lexical_this {
            function
                .env
                .borrow()
                .get("this")
                .unwrap_or(JsValue::Undefined)
        } else {
            this_value.clone()
        };
        call_env
            .borrow_mut()
            .define("this".to_string(), initial_this);
        call_env
            .borrow_mut()
            .define("__constructor_this__".to_string(), this_value.clone());
        if let Some(super_binding) = &function.super_binding {
            call_env
                .borrow_mut()
                .define("super".to_string(), super_binding.clone());
        }
        self.bind_function_super_context(&call_env, function.as_ref());

        self.call_stack.push(ActiveCallFrame {
            function: Rc::clone(&function),
            instance_fields_initialized: false,
        });

        let call_result = (|| -> Result<JsValue, RuntimeError> {
            self.bind_parameters(&declaration.params, &args, Rc::clone(&call_env))?;
            if !function.is_derived_constructor {
                self.maybe_initialize_current_instance_fields(this_value.clone())?;
            }

            if declaration.is_generator {
                let state = Rc::new(RefCell::new(GeneratorState {
                    declaration_id: function.id,
                    env: Rc::clone(&call_env),
                    status: GeneratorStatus::SuspendedStart,
                    is_async: declaration.is_async,
                }));
                return Ok(self.create_generator_iterator(state));
            }

            let result = match self.eval_statement(
                &Statement::BlockStatement(declaration.body.clone()),
                Rc::clone(&call_env),
            ) {
                Ok(value) => Ok(value),
                Err(RuntimeError::Return(value)) => Ok(value),
                Err(error) => Err(error),
            };
            if declaration.is_async && !declaration.is_generator {
                return match result {
                    Ok(value) => Ok(Self::resolved_promise(value)),
                    Err(error) => Ok(Self::rejected_promise(self.to_rejection_value(error))),
                };
            }
            let result = result?;

            if function.is_derived_constructor
                && matches!(
                    call_env.borrow().get("this"),
                    Some(JsValue::Undefined) | None
                )
                && !value_is_object_like(&result)
            {
                return Err(RuntimeError::TypeError(
                    "derived constructor must call super() before accessing this".into(),
                ));
            }

            Ok(result)
        })();

        self.call_stack.pop();
        call_result
    }

    fn eval_while_statement(
        &mut self,
        while_stmt: &WhileStatement,
        env: Rc<RefCell<Environment>>,
        label: Option<&str>,
        run_body_first: bool,
    ) -> Result<JsValue, RuntimeError> {
        let mut last_val = JsValue::Undefined;
        loop {
            self.check_timeout()?;
            if !run_body_first {
                let test_val = self.eval_expression(&while_stmt.test, Rc::clone(&env))?;
                if !test_val.is_truthy() {
                    break;
                }
            }
            match self.eval_statement(&while_stmt.body, Rc::clone(&env)) {
                Ok(val) => last_val = val,
                Err(RuntimeError::Break(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    break;
                }
                Err(RuntimeError::Continue(control_label))
                    if loop_control_matches(&control_label, label) => {}
                Err(error) => return Err(error),
            }
            if run_body_first {
                let test_val = self.eval_expression(&while_stmt.test, Rc::clone(&env))?;
                if !test_val.is_truthy() {
                    break;
                }
            }
        }
        Ok(last_val)
    }

    fn eval_for_statement(
        &mut self,
        for_stmt: &ForStatement,
        env: Rc<RefCell<Environment>>,
        label: Option<&str>,
    ) -> Result<JsValue, RuntimeError> {
        let for_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
        if let Some(init) = &for_stmt.init {
            self.eval_statement(init, Rc::clone(&for_env))?;
        }
        let mut last_val = JsValue::Undefined;
        loop {
            self.check_timeout()?;
            if let Some(test) = &for_stmt.test {
                let test_val = self.eval_expression(test, Rc::clone(&for_env))?;
                if !test_val.is_truthy() {
                    break;
                }
            }
            match self.eval_statement(&for_stmt.body, Rc::clone(&for_env)) {
                Ok(val) => last_val = val,
                Err(RuntimeError::Break(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    break;
                }
                Err(RuntimeError::Continue(control_label))
                    if loop_control_matches(&control_label, label) => {}
                Err(error) => return Err(error),
            }
            if let Some(update) = &for_stmt.update {
                self.eval_expression(update, Rc::clone(&for_env))?;
            }
        }
        Ok(last_val)
    }

    fn eval_for_in_statement(
        &mut self,
        for_in: &ForInStatement,
        env: Rc<RefCell<Environment>>,
        label: Option<&str>,
    ) -> Result<JsValue, RuntimeError> {
        let right = self.eval_expression(&for_in.right, Rc::clone(&env))?;
        let keys = self.collect_for_in_keys(right)?;
        let binding = extract_for_binding(&for_in.left);
        let mut last_val = JsValue::Undefined;
        for key in keys {
            self.check_timeout()?;
            let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
            if let Some((pattern, declare)) = binding {
                self.assign_pattern(pattern, JsValue::String(key), Rc::clone(&iter_env), declare)?;
            }
            match self.eval_statement(&for_in.body, Rc::clone(&iter_env)) {
                Ok(val) => last_val = val,
                Err(RuntimeError::Break(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    break;
                }
                Err(RuntimeError::Continue(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(last_val)
    }

    fn eval_for_of_statement(
        &mut self,
        for_of: &ForOfStatement,
        env: Rc<RefCell<Environment>>,
        label: Option<&str>,
    ) -> Result<JsValue, RuntimeError> {
        let right = self.eval_expression(&for_of.right, Rc::clone(&env))?;
        let mut iterator = self.begin_iteration(right)?;
        let binding = extract_for_binding(&for_of.left);
        let mut last_val = JsValue::Undefined;
        loop {
            let item = match self.iterator_step(&mut iterator, for_of.is_await)? {
                IteratorStep::Yield(item) => item,
                IteratorStep::Complete(_) => break,
            };
            self.check_timeout()?;
            let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
            let item = if for_of.is_await {
                self.await_value(item)?
            } else {
                item
            };
            if let Some((pattern, declare)) = binding {
                self.assign_pattern(pattern, item, Rc::clone(&iter_env), declare)?;
            }
            match self.eval_statement(&for_of.body, Rc::clone(&iter_env)) {
                Ok(val) => last_val = val,
                Err(RuntimeError::Break(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    self.close_iterator(&mut iterator, for_of.is_await)?;
                    break;
                }
                Err(RuntimeError::Continue(control_label))
                    if loop_control_matches(&control_label, label) =>
                {
                    continue;
                }
                Err(error) => {
                    self.close_iterator(&mut iterator, for_of.is_await)?;
                    return Err(error);
                }
            }
        }
        Ok(last_val)
    }

    pub fn eval_program(&mut self, program: &Program) -> Result<JsValue, RuntimeError> {
        self.eval_program_in_env(program, Rc::clone(&self.global_env))
    }

    pub fn eval_statement(
        &mut self,
        stmt: &Statement,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let val = match &d.init {
                        Some(expr) => self.eval_expression(expr, Rc::clone(&env))?,
                        None => JsValue::Undefined,
                    };
                    self.assign_pattern(&d.id, val, Rc::clone(&env), true)?;
                }
                Ok(JsValue::Undefined)
            }
            Statement::ExpressionStatement(expr) => self.eval_expression(expr, env),
            Statement::BlockStatement(block) => {
                let block_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                let mut last_val = JsValue::Undefined;
                for s in &block.body {
                    self.check_timeout()?;
                    last_val = self.eval_statement(s, Rc::clone(&block_env))?;
                }
                Ok(last_val)
            }
            Statement::IfStatement(if_stmt) => {
                let test_val = self.eval_expression(&if_stmt.test, Rc::clone(&env))?;
                if test_val.is_truthy() {
                    self.eval_statement(&if_stmt.consequent, Rc::clone(&env))
                } else if let Some(alt) = &if_stmt.alternate {
                    self.eval_statement(alt, Rc::clone(&env))
                } else {
                    Ok(JsValue::Undefined)
                }
            }
            Statement::WithStatement(with_stmt) => {
                let object = self.eval_expression(&with_stmt.object, Rc::clone(&env))?;
                let with_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                let binding_keys = self
                    .collect_with_scope_bindings(&object)
                    .into_iter()
                    .map(|(key, value)| {
                        with_env.borrow_mut().define(key.clone(), value);
                        key
                    })
                    .collect::<HashSet<_>>();

                let result = self.eval_statement(&with_stmt.body, Rc::clone(&with_env));
                self.sync_with_scope_bindings(&object, Rc::clone(&with_env), &binding_keys)?;
                result
            }
            Statement::WhileStatement(while_stmt) => {
                self.eval_while_statement(while_stmt, env, None, false)
            }
            Statement::DoWhileStatement(while_stmt) => {
                self.eval_while_statement(while_stmt, env, None, true)
            }
            Statement::ForStatement(for_stmt) => self.eval_for_statement(for_stmt, env, None),
            Statement::ForInStatement(for_in) => self.eval_for_in_statement(for_in, env, None),
            Statement::ForOfStatement(for_of) => self.eval_for_of_statement(for_of, env, None),
            Statement::SwitchStatement(switch) => {
                let discriminant = self.eval_expression(&switch.discriminant, Rc::clone(&env))?;
                let mut matched = false;
                let mut default_index: Option<usize> = None;
                let mut last_val = JsValue::Undefined;

                // find default case index
                for (i, case) in switch.cases.iter().enumerate() {
                    if case.test.is_none() {
                        default_index = Some(i);
                    }
                }

                'outer: for (i, case) in switch.cases.iter().enumerate() {
                    if !matched {
                        match &case.test {
                            None => continue, // skip default on first pass
                            Some(test) => {
                                let test_val = self.eval_expression(test, Rc::clone(&env))?;
                                if !js_strict_eq(&discriminant, &test_val) {
                                    continue;
                                }
                                matched = true;
                            }
                        }
                    }
                    for stmt in &case.consequent {
                        match self.eval_statement(stmt, Rc::clone(&env)) {
                            Ok(val) => last_val = val,
                            Err(RuntimeError::Break(None)) => break 'outer,
                            Err(e) => return Err(e),
                        }
                    }
                    let _ = i;
                }

                // if nothing matched, run default
                if !matched {
                    if let Some(di) = default_index {
                        let mut in_default = true;
                        'default: for case in switch.cases.iter().skip(di) {
                            if in_default || case.test.is_none() {
                                in_default = false;
                            }
                            for stmt in &case.consequent {
                                match self.eval_statement(stmt, Rc::clone(&env)) {
                                    Ok(val) => last_val = val,
                                    Err(RuntimeError::Break(None)) => break 'default,
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    }
                }
                Ok(last_val)
            }
            Statement::BreakStatement(label) => {
                Err(RuntimeError::Break(label.map(|s| s.to_string())))
            }
            Statement::ContinueStatement(label) => {
                Err(RuntimeError::Continue(label.map(|s| s.to_string())))
            }
            Statement::LabeledStatement(labeled) => match &*labeled.body {
                Statement::WhileStatement(while_stmt) => {
                    self.eval_while_statement(while_stmt, env, Some(labeled.label), false)
                }
                Statement::DoWhileStatement(while_stmt) => {
                    self.eval_while_statement(while_stmt, env, Some(labeled.label), true)
                }
                Statement::ForStatement(for_stmt) => {
                    self.eval_for_statement(for_stmt, env, Some(labeled.label))
                }
                Statement::ForInStatement(for_in) => {
                    self.eval_for_in_statement(for_in, env, Some(labeled.label))
                }
                Statement::ForOfStatement(for_of) => {
                    self.eval_for_of_statement(for_of, env, Some(labeled.label))
                }
                _ => match self.eval_statement(&labeled.body, Rc::clone(&env)) {
                    Err(RuntimeError::Break(Some(ref l))) if l == labeled.label => {
                        Ok(JsValue::Undefined)
                    }
                    other => other,
                },
            },
            Statement::TryStatement(try_stmt) => {
                let res = self.eval_statement(
                    &Statement::BlockStatement(try_stmt.block.clone()),
                    Rc::clone(&env),
                );
                let mut final_val = match res {
                    Ok(val) => Ok(val),
                    Err(RuntimeError::Return(v)) => Err(RuntimeError::Return(v)),
                    Err(RuntimeError::Timeout) => Err(RuntimeError::Timeout),
                    Err(RuntimeError::Break(label)) => Err(RuntimeError::Break(label)),
                    Err(RuntimeError::Continue(label)) => Err(RuntimeError::Continue(label)),
                    Err(e) => {
                        if let Some(handler) = &try_stmt.handler {
                            let catch_env =
                                Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                            if let Some(param) = &handler.param {
                                let err_val = match e {
                                    RuntimeError::Throw(v) => v,
                                    err => JsValue::String(err.to_string()),
                                };
                                self.assign_pattern(param, err_val, Rc::clone(&catch_env), true)?;
                            }
                            self.eval_statement(
                                &Statement::BlockStatement(handler.body.clone()),
                                catch_env,
                            )
                        } else {
                            Err(e)
                        }
                    }
                };

                if let Some(finalizer) = &try_stmt.finalizer {
                    // Execute finally block
                    let finally_res = self.eval_statement(
                        &Statement::BlockStatement(finalizer.clone()),
                        Rc::clone(&env),
                    );
                    if finally_res.is_err() {
                        final_val = finally_res; // Finally error overwrites try/catch error
                    }
                }

                final_val
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(name) = func.id {
                    let function = self.create_function_value(func, Rc::clone(&env));
                    env.borrow_mut().define(name.to_string(), function);
                }
                Ok(JsValue::Undefined)
            }
            Statement::ClassDeclaration(class_decl) => {
                let class_value = self.build_class_value(class_decl, Rc::clone(&env))?;
                if let Some(name) = class_decl.id {
                    env.borrow_mut().define(name.to_string(), class_value);
                }
                Ok(JsValue::Undefined)
            }
            Statement::ImportDeclaration(import_decl) => {
                let namespace = self.load_module_namespace(import_decl.source)?;
                let namespace_map = match &namespace {
                    JsValue::Object(map) => Rc::clone(map),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "module namespace is not an object".into(),
                        ));
                    }
                };
                for specifier in &import_decl.specifiers {
                    match specifier {
                        ImportSpecifier::Default(local) => {
                            env.borrow_mut().define_import(
                                (*local).to_string(),
                                Rc::clone(&namespace_map),
                                "default".to_string(),
                            );
                        }
                        ImportSpecifier::Namespace(local) => {
                            env.borrow_mut()
                                .define((*local).to_string(), namespace.clone());
                        }
                        ImportSpecifier::Named { imported, local } => {
                            env.borrow_mut().define_import(
                                (*local).to_string(),
                                Rc::clone(&namespace_map),
                                (*imported).to_string(),
                            );
                        }
                    }
                }
                Ok(JsValue::Undefined)
            }
            Statement::ExportNamedDeclaration(export_decl) => {
                if let Some(declaration) = &export_decl.declaration {
                    let result = self.eval_statement(declaration, Rc::clone(&env))?;
                    match declaration.as_ref() {
                        Statement::VariableDeclaration(decl) => {
                            let mut names = Vec::new();
                            for declarator in &decl.declarations {
                                self.export_identifiers_from_pattern(&declarator.id, &mut names);
                            }
                            for name in names {
                                self.write_module_export_binding(&name, Rc::clone(&env), &name);
                            }
                        }
                        Statement::FunctionDeclaration(func) => {
                            if let Some(name) = func.id {
                                self.write_module_export_binding(name, Rc::clone(&env), name);
                            }
                        }
                        Statement::ClassDeclaration(class_decl) => {
                            if let Some(name) = class_decl.id {
                                self.write_module_export_binding(name, Rc::clone(&env), name);
                            }
                        }
                        _ => {}
                    }
                    Ok(result)
                } else if let Some(source) = export_decl.source {
                    let namespace = self.load_module_namespace(source)?;
                    for specifier in &export_decl.specifiers {
                        self.write_module_export_namespace_binding(
                            specifier.exported,
                            namespace.clone(),
                            specifier.local,
                        );
                    }
                    Ok(JsValue::Undefined)
                } else {
                    for specifier in &export_decl.specifiers {
                        self.write_module_export_binding(
                            specifier.exported,
                            Rc::clone(&env),
                            specifier.local,
                        );
                    }
                    Ok(JsValue::Undefined)
                }
            }
            Statement::ExportDefaultDeclaration(export_decl) => match &export_decl.declaration {
                ExportDefaultKind::Expression(expr) => {
                    let value = self.eval_expression(expr, env)?;
                    self.write_module_export_value("default", value.clone());
                    Ok(value)
                }
                ExportDefaultKind::FunctionDeclaration(func) => {
                    let function = self.create_function_value(func, Rc::clone(&env));
                    if let Some(name) = func.id {
                        env.borrow_mut().define(name.to_string(), function.clone());
                    }
                    self.write_module_export_value("default", function);
                    Ok(JsValue::Undefined)
                }
                ExportDefaultKind::ClassDeclaration(class_decl) => {
                    let class_value = self.build_class_value(class_decl, Rc::clone(&env))?;
                    if let Some(name) = class_decl.id {
                        env.borrow_mut()
                            .define(name.to_string(), class_value.clone());
                    }
                    self.write_module_export_value("default", class_value);
                    Ok(JsValue::Undefined)
                }
            },
            Statement::ExportAllDeclaration(export_decl) => {
                let namespace = self.load_module_namespace(export_decl.source)?;
                if let Some(exported) = export_decl.exported {
                    self.write_module_export_value(exported, namespace);
                } else {
                    for (name, value) in self.module_namespace_property_values(&namespace)? {
                        if name != "default" {
                            if let Some(exports) = self.module_exports_stack.last() {
                                exports.borrow_mut().insert(name, value);
                            }
                        }
                    }
                }
                Ok(JsValue::Undefined)
            }
            Statement::ReturnStatement(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expression(e, env)?
                } else {
                    JsValue::Undefined
                };
                Err(RuntimeError::Return(val))
            }
            Statement::ThrowStatement(expr) => {
                let val = self.eval_expression(expr, env)?;
                Err(RuntimeError::Throw(val))
            }
            Statement::EmptyStatement => Ok(JsValue::Undefined),
        }
    }

    pub fn eval_expression(
        &mut self,
        expr: &Expression,
        env: Rc<RefCell<Environment>>,
    ) -> Result<JsValue, RuntimeError> {
        match expr {
            Expression::Literal(lit) => match lit {
                Literal::Number(n) => Ok(JsValue::Number(*n)),
                Literal::String(s) => Ok(JsValue::String(s.to_string())),
                Literal::Boolean(b) => Ok(JsValue::Boolean(*b)),
                Literal::Null => Ok(JsValue::Null),
                Literal::Undefined => Ok(JsValue::Undefined),
                Literal::BigInt(n) => Ok(JsValue::Number(*n as f64)),
                Literal::RegExp(pattern, flags) => Ok(crate::engine::value::make_object([
                    ("source", JsValue::String(pattern.to_string())),
                    ("flags", JsValue::String(flags.to_string())),
                ])),
            },
            Expression::Identifier(name) => {
                match *name {
                    "undefined" => return Ok(JsValue::Undefined),
                    "NaN" => return Ok(JsValue::Number(f64::NAN)),
                    "Infinity" => return Ok(JsValue::Number(f64::INFINITY)),
                    "null" => return Ok(JsValue::Null),
                    _ => {}
                }
                Ok(env.borrow().get(name).unwrap_or(JsValue::Undefined))
            }
            Expression::PrivateIdentifier(_) => Err(RuntimeError::SyntaxError(
                "private identifier is not available in this context".into(),
            )),
            Expression::AssignmentExpression(assign) => match &assign.left {
                Expression::Identifier(name) => {
                    let current = env.borrow().get(name).unwrap_or(JsValue::Undefined);
                    if !self.should_apply_assignment(&assign.operator, &current) {
                        return Ok(current);
                    }

                    let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                    let value = self.assignment_result(&assign.operator, &current, &right)?;
                    if env.borrow().has_binding(name) {
                        env.borrow_mut()
                            .set(name, value.clone())
                            .map_err(RuntimeError::TypeError)?;
                    } else {
                        env.borrow_mut().define(name.to_string(), value.clone());
                    }
                    Ok(value)
                }
                Expression::ArrayExpression(_) | Expression::ObjectExpression(_)
                    if matches!(assign.operator, AssignmentOperator::Assign) =>
                {
                    let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                    self.assign_pattern(&assign.left, right.clone(), env, false)?;
                    Ok(right)
                }
                Expression::MemberExpression(mem) if self.member_private_name(mem).is_some() => {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    let name = self.member_private_name(mem).unwrap();
                    let current =
                        self.read_private_member_value(object.clone(), name, Rc::clone(&env))?;
                    if !self.should_apply_assignment(&assign.operator, &current) {
                        Ok(current)
                    } else {
                        let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        self.write_private_member_value(object, name, value.clone(), env)?;
                        Ok(value)
                    }
                }
                Expression::MemberExpression(mem)
                    if matches!(mem.object, Expression::SuperExpression) =>
                {
                    let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                    let current = self.read_super_member_value(Rc::clone(&env), &property_key)?;
                    if !self.should_apply_assignment(&assign.operator, &current) {
                        Ok(current)
                    } else {
                        let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        self.write_super_member_value(env, &property_key, value.clone())?;
                        Ok(value)
                    }
                }
                Expression::MemberExpression(mem) => {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                    let current = self.read_member_value(object.clone(), &property_key, None)?;
                    if !self.should_apply_assignment(&assign.operator, &current) {
                        Ok(current)
                    } else {
                        let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        self.write_member_value(object, &property_key, value)
                    }
                }
                _ => Err(RuntimeError::SyntaxError("invalid assignment target".into())),
            },
            Expression::BinaryExpression(bin) => {
                if bin.operator == BinaryOperator::In
                    && let Expression::PrivateIdentifier(name) = &bin.left
                {
                    let right = self.eval_expression(&bin.right, Rc::clone(&env))?;
                    return Ok(JsValue::Boolean(
                        self.has_private_member_brand(&right, name, &env)?,
                    ));
                }
                let left = self.eval_expression(&bin.left, Rc::clone(&env))?;
                // Short-circuiting Logic Operators
                match bin.operator {
                    BinaryOperator::LogicAnd => {
                        if !left.is_truthy() {
                            return Ok(left);
                        }
                        return self.eval_expression(&bin.right, env);
                    }
                    BinaryOperator::LogicOr => {
                        if left.is_truthy() {
                            return Ok(left);
                        }
                        return self.eval_expression(&bin.right, env);
                    }
                    BinaryOperator::NullishCoalescing => {
                        if !matches!(left, JsValue::Undefined | JsValue::Null) {
                            return Ok(left);
                        }
                        return self.eval_expression(&bin.right, env);
                    }
                    _ => {}
                }

                let right = self.eval_expression(&bin.right, env)?;
                match bin.operator {
                    BinaryOperator::Plus => left.add(&right),
                    BinaryOperator::Minus => left.sub(&right),
                    BinaryOperator::Multiply => left.mul(&right),
                    BinaryOperator::Divide => left.div(&right),
                    BinaryOperator::Percent => {
                        Ok(JsValue::Number(left.as_number() % right.as_number()))
                    }
                    BinaryOperator::BitAnd => Ok(JsValue::Number(
                        (self.to_int32(&left) & self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::BitOr => Ok(JsValue::Number(
                        (self.to_int32(&left) | self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::BitXor => Ok(JsValue::Number(
                        (self.to_int32(&left) ^ self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::ShiftLeft => {
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) << shift) as f64))
                    }
                    BinaryOperator::ShiftRight => {
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) >> shift) as f64))
                    }
                    BinaryOperator::LogicalShiftRight => {
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_uint32(&left) >> shift) as f64))
                    }
                    BinaryOperator::EqEq => Ok(JsValue::Boolean(js_abstract_eq(&left, &right))),
                    BinaryOperator::EqEqEq => Ok(JsValue::Boolean(js_strict_eq(&left, &right))),
                    BinaryOperator::NotEq => Ok(JsValue::Boolean(!js_abstract_eq(&left, &right))),
                    BinaryOperator::NotEqEq => Ok(JsValue::Boolean(!js_strict_eq(&left, &right))),
                    BinaryOperator::Less => left.lt(&right),
                    BinaryOperator::LessEq => left.le(&right),
                    BinaryOperator::Greater => left.gt(&right),
                    BinaryOperator::GreaterEq => left.ge(&right),
                    BinaryOperator::Power => {
                        Ok(JsValue::Number(left.as_number().powf(right.as_number())))
                    }
                    BinaryOperator::Instanceof => match (&left, &right) {
                        (JsValue::Object(object), JsValue::Function(function)) => {
                            let prototype = function.prototype.clone();
                            let mut current = match object.borrow().get("__proto__").cloned() {
                                Some(PropertyValue::Data(value)) => Some(value),
                                _ => None,
                            };
                            while let Some(value) = current {
                                if js_strict_eq(&value, &prototype) {
                                    return Ok(JsValue::Boolean(true));
                                }
                                current = match value {
                                    JsValue::Object(proto) => {
                                        match proto.borrow().get("__proto__").cloned() {
                                            Some(PropertyValue::Data(value)) => Some(value),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                };
                            }
                            Ok(JsValue::Boolean(false))
                        }
                        _ => Ok(JsValue::Boolean(false)),
                    },
                    BinaryOperator::In => {
                        let key = left.as_string();
                        match &right {
                            JsValue::Object(map) => {
                                Ok(JsValue::Boolean(has_object_property(map, &key)))
                            }
                            JsValue::Array(arr) => {
                                if let Ok(idx) = key.parse::<usize>() {
                                    Ok(JsValue::Boolean(idx < arr.borrow().len()))
                                } else {
                                    Ok(JsValue::Boolean(false))
                                }
                            }
                            _ => Err(RuntimeError::TypeError(
                                "right-hand side of 'in' is not an object".into(),
                            )),
                        }
                    }
                    _ => Ok(JsValue::Undefined),
                }
            }
            Expression::UnaryExpression(unary) => match unary.operator {
                UnaryOperator::Delete => match &unary.argument {
                    Expression::MemberExpression(mem) => {
                        if self.member_private_name(mem).is_some() {
                            return Err(RuntimeError::SyntaxError(
                                "private fields cannot be deleted".into(),
                            ));
                        }
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                        let deleted = match object {
                            JsValue::Object(map) => {
                                map.borrow_mut().remove(&property_key);
                                true
                            }
                            JsValue::Function(function) => {
                                function.properties.borrow_mut().remove(&property_key);
                                true
                            }
                            JsValue::Array(arr) => {
                                if property_key == "length" {
                                    false
                                } else if let Ok(index) = property_key.parse::<usize>() {
                                    let mut arr = arr.borrow_mut();
                                    if index < arr.len() {
                                        arr[index] = JsValue::Undefined;
                                    }
                                    true
                                } else {
                                    true
                                }
                            }
                            JsValue::EnvironmentObject(env) => {
                                env.borrow_mut().variables.remove(&property_key);
                                true
                            }
                            JsValue::Null | JsValue::Undefined => {
                                return Err(RuntimeError::TypeError(
                                    "value is not an object".into(),
                                ));
                            }
                            _ => true,
                        };
                        Ok(JsValue::Boolean(deleted))
                    }
                    Expression::Identifier(_) => Ok(JsValue::Boolean(false)),
                    expr => {
                        let _ = self.eval_expression(expr, env)?;
                        Ok(JsValue::Boolean(true))
                    }
                },
                _ => {
                    let arg = self.eval_expression(&unary.argument, env)?;
                    match unary.operator {
                        UnaryOperator::Minus => Ok(JsValue::Number(-arg.as_number())),
                        UnaryOperator::Plus => Ok(JsValue::Number(arg.as_number())),
                        UnaryOperator::LogicNot => Ok(JsValue::Boolean(!arg.is_truthy())),
                        UnaryOperator::BitNot => Ok(JsValue::Number((!self.to_int32(&arg)) as f64)),
                        UnaryOperator::Typeof => Ok(JsValue::String(arg.type_of())),
                        UnaryOperator::Void => Ok(JsValue::Undefined),
                        UnaryOperator::Delete => unreachable!(),
                    }
                }
            },
            Expression::ArrayExpression(elements) => {
                let mut values = Vec::new();
                for element in elements {
                    match element {
                        Some(Expression::SpreadElement(spread_expr)) => {
                            let spread_val = self.eval_expression(spread_expr, Rc::clone(&env))?;
                            values.extend(self.collect_iterable_items(spread_val)?);
                        }
                        Some(expr) => values.push(self.eval_expression(expr, Rc::clone(&env))?),
                        None => values.push(JsValue::Undefined),
                    }
                }
                Ok(JsValue::Array(Rc::new(RefCell::new(values))))
            }
            Expression::ObjectExpression(properties) => {
                let values = new_object_map();
                let object_value = JsValue::Object(Rc::clone(&values));
                for prop in properties {
                    let key = match &prop.key {
                        ObjectKey::Identifier(name) | ObjectKey::String(name) => {
                            (*name).to_string()
                        }
                        ObjectKey::Number(n) => n.to_string(),
                        ObjectKey::Computed(expr) => {
                            self.eval_expression(expr, Rc::clone(&env))?.as_string()
                        }
                        ObjectKey::PrivateIdentifier(_) => {
                            return Err(RuntimeError::SyntaxError(
                                "private identifier cannot appear in object patterns".into(),
                            ));
                        }
                    };
                    match &prop.kind {
                        ObjectPropertyKind::Getter(func) => {
                            let getter = self.create_accessor_function_value(
                                func,
                                Rc::clone(&env),
                                None,
                                None,
                                Some(object_value.clone()),
                                None,
                            );
                            let setter = match values.borrow().get(&key).cloned() {
                                Some(PropertyValue::Accessor { setter, .. }) => setter,
                                _ => None,
                            };
                            values.borrow_mut().insert(
                                key,
                                PropertyValue::Accessor {
                                    getter: Some(getter),
                                    setter,
                                },
                            );
                        }
                        ObjectPropertyKind::Setter(func) => {
                            let setter_fn = self.create_accessor_function_value(
                                func,
                                Rc::clone(&env),
                                None,
                                None,
                                Some(object_value.clone()),
                                None,
                            );
                            let getter = match values.borrow().get(&key).cloned() {
                                Some(PropertyValue::Accessor { getter, .. }) => getter,
                                _ => None,
                            };
                            values.borrow_mut().insert(
                                key,
                                PropertyValue::Accessor {
                                    getter,
                                    setter: Some(setter_fn),
                                },
                            );
                        }
                        ObjectPropertyKind::Value(_) => {
                            if let Expression::SpreadElement(spread_expr) = &prop.value {
                                let spread_val =
                                    self.eval_expression(spread_expr, Rc::clone(&env))?;
                                if let JsValue::Object(map) = spread_val {
                                    let entries = map
                                        .borrow()
                                        .iter()
                                        .map(|(k, v)| (k.clone(), v.clone()))
                                        .collect::<Vec<_>>();
                                    for (k, v) in entries {
                                        values.borrow_mut().insert(k, v);
                                    }
                                }
                            } else {
                                let val = if prop.method {
                                    if let Expression::FunctionExpression(func) = &prop.value {
                                        self.create_method_function_value(
                                            func,
                                            Rc::clone(&env),
                                            None,
                                            None,
                                            Some(object_value.clone()),
                                            None,
                                        )
                                    } else {
                                        self.eval_expression(&prop.value, Rc::clone(&env))?
                                    }
                                } else {
                                    self.eval_expression(&prop.value, Rc::clone(&env))?
                                };
                                values.borrow_mut().insert(key, PropertyValue::Data(val));
                            }
                        }
                    }
                }
                Ok(object_value)
            }
            Expression::MemberExpression(mem) => {
                if mem.optional {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    if matches!(object, JsValue::Undefined | JsValue::Null) {
                        return Ok(JsValue::Undefined);
                    }
                    let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                    return match object {
                        JsValue::Object(values) => match get_property_value(&values, &property_key)
                        {
                            Some(PropertyValue::Accessor {
                                getter: Some(getter),
                                ..
                            }) => self.invoke_getter(getter, JsValue::Object(Rc::clone(&values))),
                            Some(PropertyValue::Data(value)) => Ok(value),
                            _ => Ok(JsValue::Undefined),
                        },
                        JsValue::Array(values) => {
                            let values = values.borrow();
                            if property_key == "length" {
                                Ok(JsValue::Number(values.len() as f64))
                            } else {
                                match property_key.parse::<usize>() {
                                    Ok(index) => {
                                        Ok(values.get(index).cloned().unwrap_or(JsValue::Undefined))
                                    }
                                    Err(_) => Ok(JsValue::Undefined),
                                }
                            }
                        }
                        JsValue::String(s) => match property_key.as_str() {
                            "length" => Ok(JsValue::Number(s.chars().count() as f64)),
                            _ => {
                                if let Ok(index) = property_key.parse::<usize>() {
                                    Ok(s.chars()
                                        .nth(index)
                                        .map(|c| JsValue::String(c.to_string()))
                                        .unwrap_or(JsValue::Undefined))
                                } else {
                                    Ok(JsValue::Undefined)
                                }
                            }
                        },
                        JsValue::EnvironmentObject(env) => Ok(env
                            .borrow()
                            .get(&property_key)
                            .unwrap_or(JsValue::Undefined)),
                        JsValue::Function(function) => {
                            match get_property_value(&function.properties, &property_key) {
                                Some(PropertyValue::Accessor {
                                    getter: Some(getter),
                                    ..
                                }) => self
                                    .invoke_getter(getter, JsValue::Function(Rc::clone(&function))),
                                Some(PropertyValue::Data(value)) => Ok(value),
                                _ => Ok(JsValue::Undefined),
                            }
                        }
                        JsValue::Promise(_)
                        | JsValue::BuiltinFunction(_)
                        | JsValue::NativeFunction(_) => Ok(object.get_property(&property_key)),
                        _ => Err(RuntimeError::TypeError("value is not an object".into())),
                    };
                }

                if matches!(mem.object, Expression::SuperExpression) {
                    let super_binding = env
                        .borrow()
                        .get("__super_property_base__")
                        .or_else(|| env.borrow().get("super"))
                        .unwrap_or(JsValue::Undefined);
                    let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                    let this_value = env.borrow().get("this").unwrap_or(JsValue::Undefined);
                    if matches!(super_binding, JsValue::Undefined) {
                        return Err(RuntimeError::TypeError(
                            "super is not available in this context".into(),
                        ));
                    }
                    return self.read_member_value(super_binding, &property_key, Some(this_value));
                }

                if let Some(name) = self.member_private_name(mem) {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    return self.read_private_member_value(object, name, env);
                }

                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(mem, Rc::clone(&env))?;

                match object {
                    JsValue::Object(values) => match get_property_value(&values, &property_key) {
                        Some(PropertyValue::Accessor {
                            getter: Some(getter),
                            ..
                        }) => self.invoke_getter(getter, JsValue::Object(Rc::clone(&values))),
                        Some(PropertyValue::Data(value)) => Ok(value),
                        _ => Ok(JsValue::Undefined),
                    },
                    JsValue::Array(values) => {
                        let values = values.borrow();
                        if property_key == "length" {
                            return Ok(JsValue::Number(values.len() as f64));
                        }
                        match property_key.parse::<usize>() {
                            Ok(index) => {
                                Ok(values.get(index).cloned().unwrap_or(JsValue::Undefined))
                            }
                            Err(_) => Ok(JsValue::Undefined),
                        }
                    }
                    JsValue::String(s) => match property_key.as_str() {
                        "length" => Ok(JsValue::Number(s.chars().count() as f64)),
                        _ => {
                            if let Ok(index) = property_key.parse::<usize>() {
                                Ok(s.chars()
                                    .nth(index)
                                    .map(|c| JsValue::String(c.to_string()))
                                    .unwrap_or(JsValue::Undefined))
                            } else {
                                Ok(JsValue::Undefined)
                            }
                        }
                    },
                    JsValue::EnvironmentObject(env) => Ok(env
                        .borrow()
                        .get(&property_key)
                        .unwrap_or(JsValue::Undefined)),
                    JsValue::Function(function) => {
                        match get_property_value(&function.properties, &property_key) {
                            Some(PropertyValue::Accessor {
                                getter: Some(getter),
                                ..
                            }) => {
                                self.invoke_getter(getter, JsValue::Function(Rc::clone(&function)))
                            }
                            Some(PropertyValue::Data(value)) => Ok(value),
                            _ => Ok(JsValue::Undefined),
                        }
                    }
                    JsValue::Promise(_)
                    | JsValue::BuiltinFunction(_)
                    | JsValue::NativeFunction(_) => Ok(object.get_property(&property_key)),
                    _ => Err(RuntimeError::TypeError("value is not an object".into())),
                }
            }
            Expression::CallExpression(call) => {
                if call.optional {
                    let callee = self.eval_expression(&call.callee, Rc::clone(&env))?;
                    if matches!(callee, JsValue::Undefined | JsValue::Null) {
                        return Ok(JsValue::Undefined);
                    }
                }

                let (callee, this_value) = match &call.callee {
                    Expression::MemberExpression(mem)
                        if matches!(mem.object, Expression::SuperExpression) =>
                    {
                        let this_value = env.borrow().get("this").unwrap_or(JsValue::Undefined);
                        let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                        let super_binding = env
                            .borrow()
                            .get("__super_property_base__")
                            .or_else(|| env.borrow().get("super"))
                            .unwrap_or(JsValue::Undefined);
                        let callee = self.read_member_value(
                            super_binding,
                            &property_key,
                            Some(this_value.clone()),
                        )?;
                        (callee, this_value)
                    }
                    Expression::MemberExpression(mem)
                        if self.member_private_name(mem).is_some() =>
                    {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let name = self.member_private_name(mem).unwrap();
                        let callee =
                            self.read_private_member_value(object.clone(), name, Rc::clone(&env))?;
                        (callee, object)
                    }
                    Expression::MemberExpression(mem) => {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                        let callee = match &object {
                            JsValue::Object(values) => get_object_property(values, &property_key),
                            _ => object.get_property(&property_key),
                        };
                        (callee, object)
                    }
                    Expression::SuperExpression => {
                        let super_binding = env.borrow().get("super").unwrap_or(JsValue::Undefined);
                        if matches!(super_binding, JsValue::Undefined) {
                            return Err(RuntimeError::TypeError(
                                "super is not available in this context".into(),
                            ));
                        }
                        let this_value = env
                            .borrow()
                            .get("__constructor_this__")
                            .or_else(|| env.borrow().get("this"))
                            .unwrap_or(JsValue::Undefined);
                        (super_binding, this_value)
                    }
                    _ => (
                        self.eval_expression(&call.callee, Rc::clone(&env))?,
                        JsValue::Undefined,
                    ),
                };
                let mut args = Vec::new();
                for arg in &call.arguments {
                    match arg {
                        Expression::SpreadElement(spread_expr) => {
                            let spread_val = self.eval_expression(spread_expr, Rc::clone(&env))?;
                            args.extend(self.collect_iterable_items(spread_val)?);
                        }
                        expr => args.push(self.eval_expression(expr, Rc::clone(&env))?),
                    }
                }

                match callee {
                    JsValue::Function(function) => {
                        let result = self.call_function_value(
                            function,
                            this_value.clone(),
                            args,
                            matches!(call.callee, Expression::SuperExpression),
                        )?;
                        if matches!(call.callee, Expression::SuperExpression) {
                            let initialized_this = if value_is_object_like(&result) {
                                result.clone()
                            } else {
                                this_value
                            };
                            let _ = env.borrow_mut().set("this", initialized_this.clone());
                            self.maybe_initialize_current_instance_fields(initialized_this)?;
                        }
                        Ok(result)
                    }
                    other => self.invoke_callable(other, this_value, args),
                }
            }
            Expression::UpdateExpression(update) => match &update.argument {
                Expression::Identifier(name) => {
                    let current_val = env
                        .borrow()
                        .get(name)
                        .unwrap_or(JsValue::Undefined)
                        .as_number();
                    let new_val = if update.operator == UpdateOperator::PlusPlus {
                        current_val + 1.0
                    } else {
                        current_val - 1.0
                    };
                    if env.borrow().has_binding(name) {
                        env.borrow_mut()
                            .set(name, JsValue::Number(new_val))
                            .map_err(RuntimeError::TypeError)?;
                    } else {
                        env.borrow_mut()
                            .define(name.to_string(), JsValue::Number(new_val));
                    }
                    if update.prefix {
                        Ok(JsValue::Number(new_val))
                    } else {
                        Ok(JsValue::Number(current_val))
                    }
                }
                Expression::MemberExpression(mem) if self.member_private_name(mem).is_some() => {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    let name = self.member_private_name(mem).unwrap();
                    let current_val = self
                        .read_private_member_value(object.clone(), name, Rc::clone(&env))?
                        .as_number();
                    let new_val = if update.operator == UpdateOperator::PlusPlus {
                        current_val + 1.0
                    } else {
                        current_val - 1.0
                    };
                    self.write_private_member_value(object, name, JsValue::Number(new_val), env)?;
                    if update.prefix {
                        Ok(JsValue::Number(new_val))
                    } else {
                        Ok(JsValue::Number(current_val))
                    }
                }
                Expression::MemberExpression(mem)
                    if matches!(mem.object, Expression::SuperExpression) =>
                {
                    let property_key = if mem.computed {
                        self.eval_expression(&mem.property, Rc::clone(&env))?
                            .as_string()
                    } else if let Expression::Identifier(name) = &mem.property {
                        name.to_string()
                    } else {
                        return Ok(JsValue::Undefined);
                    };
                    let current_val = self
                        .read_super_member_value(Rc::clone(&env), &property_key)?
                        .as_number();
                    let new_val = if update.operator == UpdateOperator::PlusPlus {
                        current_val + 1.0
                    } else {
                        current_val - 1.0
                    };
                    self.write_super_member_value(env, &property_key, JsValue::Number(new_val))?;
                    if update.prefix {
                        Ok(JsValue::Number(new_val))
                    } else {
                        Ok(JsValue::Number(current_val))
                    }
                }
                Expression::MemberExpression(mem) => {
                    let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                    let property_key = if mem.computed {
                        self.eval_expression(&mem.property, Rc::clone(&env))?
                            .as_string()
                    } else if let Expression::Identifier(name) = &mem.property {
                        name.to_string()
                    } else {
                        return Ok(JsValue::Undefined);
                    };
                    let current_val = match &object {
                        JsValue::Object(map) => get_object_property(map, &property_key).as_number(),
                        JsValue::Array(arr) => arr
                            .borrow()
                            .get(property_key.parse::<usize>().unwrap_or(usize::MAX))
                            .cloned()
                            .unwrap_or(JsValue::Undefined)
                            .as_number(),
                        _ => f64::NAN,
                    };
                    let new_val = if update.operator == UpdateOperator::PlusPlus {
                        current_val + 1.0
                    } else {
                        current_val - 1.0
                    };
                    self.write_member_value(object, &property_key, JsValue::Number(new_val))?;
                    if update.prefix {
                        Ok(JsValue::Number(new_val))
                    } else {
                        Ok(JsValue::Number(current_val))
                    }
                }
                _ => Ok(JsValue::Undefined),
            },
            Expression::ArrowFunctionExpression(func) => {
                Ok(self.create_arrow_function_value(func, Rc::clone(&env)))
            }
            Expression::ClassExpression(class_decl) => {
                self.build_class_value(class_decl, Rc::clone(&env))
            }
            Expression::SuperExpression => {
                Ok(env.borrow().get("super").unwrap_or(JsValue::Undefined))
            }
            Expression::FunctionExpression(func) => {
                Ok(self.create_function_value(func, Rc::clone(&env)))
            }
            Expression::ThisExpression => {
                let this_value = env.borrow().get("this").unwrap_or(JsValue::Undefined);
                if matches!(this_value, JsValue::Undefined) && env.borrow().get("super").is_some() {
                    return Err(RuntimeError::TypeError(
                        "derived constructor must call super() before accessing this".into(),
                    ));
                }
                Ok(this_value)
            }
            Expression::SequenceExpression(seq) => {
                let mut res = JsValue::Undefined;
                for expr in seq {
                    res = self.eval_expression(expr, env.clone())?;
                }
                Ok(res)
            }
            Expression::ConditionalExpression {
                test,
                consequent,
                alternate,
            } => {
                let cond = self.eval_expression(test, env.clone())?;
                if cond.is_truthy() {
                    self.eval_expression(consequent, env.clone())
                } else {
                    self.eval_expression(alternate, env.clone())
                }
            }
            Expression::NewExpression(new_exp) => {
                let callee = self.eval_expression(&new_exp.callee, Rc::clone(&env))?;
                let mut args = Vec::new();
                for arg in &new_exp.arguments {
                    match arg {
                        Expression::SpreadElement(spread_expr) => {
                            let spread_val = self.eval_expression(spread_expr, Rc::clone(&env))?;
                            args.extend(self.collect_iterable_items(spread_val)?);
                        }
                        expr => args.push(self.eval_expression(expr, Rc::clone(&env))?),
                    }
                }

                match callee {
                    JsValue::Function(function) => {
                        let instance = object_with_proto(function.prototype.clone());
                        let result = self.call_function_value(
                            Rc::clone(&function),
                            instance.clone(),
                            args,
                            true,
                        )?;
                        if value_is_object_like(&result) {
                            Ok(result)
                        } else {
                            Ok(instance)
                        }
                    }
                    JsValue::BuiltinFunction(function) => {
                        self.invoke_builtin_function(function.as_ref(), JsValue::Undefined, args)
                    }
                    _ => Err(RuntimeError::TypeError("value is not a constructor".into())),
                }
            }
            Expression::SpreadElement(expr) => self.eval_expression(expr, env),
            Expression::TemplateLiteral(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        TemplatePart::String(s) => result.push_str(s),
                        TemplatePart::Expr(expr) => {
                            let val = self.eval_expression(expr, Rc::clone(&env))?;
                            result.push_str(&val.as_string());
                        }
                    }
                }
                Ok(JsValue::String(result))
            }
            Expression::YieldExpression { argument, delegate } => {
                let value = match argument {
                    Some(expr) => self.eval_expression(expr, env)?,
                    None => JsValue::Undefined,
                };
                if *delegate {
                    let (_, return_value) = self.collect_delegate_yields(value)?;
                    Ok(return_value)
                } else {
                    Ok(value)
                }
            }
            Expression::AwaitExpression(expr) => {
                let value = self.eval_expression(expr, env)?;
                self.await_value(value)
            }
            Expression::TaggedTemplateExpression(tag, parts) => {
                let (tag_val, this_value) =
                    self.eval_tagged_template_target(tag, Rc::clone(&env))?;
                let mut strings = Vec::new();
                let mut values = Vec::new();
                for part in parts {
                    match part {
                        TemplatePart::String(s) => strings.push(JsValue::String(s.to_string())),
                        TemplatePart::Expr(expr) => {
                            values.push(self.eval_expression(expr, Rc::clone(&env))?);
                        }
                    }
                }
                let strings_arr = JsValue::Array(Rc::new(RefCell::new(strings)));
                let mut call_args = vec![strings_arr];
                call_args.extend(values);
                self.invoke_callable(tag_val, this_value, call_args)
            }
        }
    }

    fn resume_generator(
        &mut self,
        state: Rc<RefCell<GeneratorState>>,
        action: ResumeAction,
    ) -> Result<JsValue, RuntimeError> {
        let is_async = state.borrow().is_async;
        let status = {
            let mut borrowed = state.borrow_mut();
            std::mem::replace(&mut borrowed.status, GeneratorStatus::Executing)
        };

        match status {
            GeneratorStatus::SuspendedStart => match action {
                ResumeAction::Next(_) => {
                    let (declaration_id, env) = {
                        let borrowed = state.borrow();
                        (borrowed.declaration_id, Rc::clone(&borrowed.env))
                    };
                    let declaration =
                        self.functions.get(declaration_id).cloned().ok_or_else(|| {
                            RuntimeError::TypeError("generator body missing".into())
                        })?;
                    let result = self.eval_generator_statement(
                        Statement::BlockStatement(declaration.body.clone()),
                        env,
                    );
                    self.finish_generator_resume(state, result, is_async)
                }
                ResumeAction::Return(value) => self.finish_generator_resume(
                    state,
                    Ok(GeneratorExecution::Complete(value)),
                    is_async,
                ),
                ResumeAction::Throw(value) => {
                    self.finish_generator_resume(state, Err(RuntimeError::Throw(value)), is_async)
                }
            },
            GeneratorStatus::SuspendedYield(continuation) => {
                let result = continuation(self, action);
                self.finish_generator_resume(state, result, is_async)
            }
            GeneratorStatus::Executing => {
                state.borrow_mut().status = GeneratorStatus::Executing;
                let error = RuntimeError::TypeError("generator is already executing".into());
                if is_async {
                    Ok(Self::rejected_promise(self.to_rejection_value(error)))
                } else {
                    Err(error)
                }
            }
            GeneratorStatus::Completed => match action {
                ResumeAction::Next(_) => {
                    if is_async {
                        self.async_generator_result_value(JsValue::Undefined, true)
                    } else {
                        Ok(Self::generator_result_object(JsValue::Undefined, true))
                    }
                }
                ResumeAction::Return(value) => {
                    if is_async {
                        self.async_generator_result_value(value, true)
                    } else {
                        Ok(Self::generator_result_object(value, true))
                    }
                }
                ResumeAction::Throw(value) => {
                    let error = RuntimeError::Throw(value);
                    if is_async {
                        Ok(Self::rejected_promise(self.to_rejection_value(error)))
                    } else {
                        Err(error)
                    }
                }
            },
        }
    }

    fn finish_generator_resume(
        &mut self,
        state: Rc<RefCell<GeneratorState>>,
        result: Result<GeneratorExecution, RuntimeError>,
        is_async: bool,
    ) -> Result<JsValue, RuntimeError> {
        match result {
            Ok(GeneratorExecution::Complete(value)) => {
                state.borrow_mut().status = GeneratorStatus::Completed;
                if is_async {
                    self.async_generator_result_value(value, true)
                } else {
                    Ok(Self::generator_result_object(value, true))
                }
            }
            Ok(GeneratorExecution::Yielded {
                value,
                continuation,
            }) => {
                state.borrow_mut().status = GeneratorStatus::SuspendedYield(continuation);
                if is_async {
                    self.async_generator_result_value(value, false)
                } else {
                    Ok(Self::generator_result_object(value, false))
                }
            }
            Err(RuntimeError::Return(value)) => {
                state.borrow_mut().status = GeneratorStatus::Completed;
                if is_async {
                    self.async_generator_result_value(value, true)
                } else {
                    Ok(Self::generator_result_object(value, true))
                }
            }
            Err(error) => {
                state.borrow_mut().status = GeneratorStatus::Completed;
                if is_async {
                    Ok(Self::rejected_promise(self.to_rejection_value(error)))
                } else {
                    Err(error)
                }
            }
        }
    }
}

fn generator_state_from_receiver(
    this: &JsValue,
    method_name: &str,
) -> Result<Rc<RefCell<GeneratorState>>, RuntimeError> {
    let JsValue::Object(map) = this else {
        return Err(RuntimeError::TypeError(format!(
            "generator.{method_name} called with non-object receiver"
        )));
    };
    match get_property_value(map, "__generator_state__") {
        Some(PropertyValue::Data(JsValue::GeneratorState(state))) => Ok(state),
        _ => Err(RuntimeError::TypeError("generator state is missing".into())),
    }
}

fn generator_next_native(
    interpreter: &mut Interpreter,
    this: &JsValue,
    args: &[JsValue],
) -> Result<JsValue, RuntimeError> {
    let state = generator_state_from_receiver(this, "next")?;
    interpreter.resume_generator(
        state,
        ResumeAction::Next(args.first().cloned().unwrap_or(JsValue::Undefined)),
    )
}

fn generator_return_native(
    interpreter: &mut Interpreter,
    this: &JsValue,
    args: &[JsValue],
) -> Result<JsValue, RuntimeError> {
    let state = generator_state_from_receiver(this, "return")?;
    interpreter.resume_generator(
        state,
        ResumeAction::Return(args.first().cloned().unwrap_or(JsValue::Undefined)),
    )
}

fn generator_throw_native(
    interpreter: &mut Interpreter,
    this: &JsValue,
    args: &[JsValue],
) -> Result<JsValue, RuntimeError> {
    let state = generator_state_from_receiver(this, "throw")?;
    interpreter.resume_generator(
        state,
        ResumeAction::Throw(args.first().cloned().unwrap_or(JsValue::Undefined)),
    )
}

fn clone_function_declaration(func: &FunctionDeclaration<'_>) -> FunctionDeclaration<'static> {
    let id = func.id.map(|value| {
        let leaked: &'static mut str = String::leak(value.to_string());
        leaked as &'static str
    });
    let params = func.params.iter().map(clone_param).collect();
    FunctionDeclaration {
        id,
        params,
        body: clone_block_statement(&func.body),
        is_generator: func.is_generator,
        is_async: func.is_async,
    }
}

fn clone_param(param: &Param<'_>) -> Param<'static> {
    Param {
        pattern: clone_expression(&param.pattern),
        is_rest: param.is_rest,
    }
}

fn clone_object_key(key: &ObjectKey<'_>) -> ObjectKey<'static> {
    match key {
        ObjectKey::Identifier(name) => ObjectKey::Identifier(String::leak((*name).to_string())),
        ObjectKey::PrivateIdentifier(name) => {
            ObjectKey::PrivateIdentifier(String::leak((*name).to_string()))
        }
        ObjectKey::String(name) => ObjectKey::String(String::leak((*name).to_string())),
        ObjectKey::Number(n) => ObjectKey::Number(*n),
        ObjectKey::Computed(expr) => ObjectKey::Computed(Box::new(clone_expression(expr))),
    }
}

fn clone_block_statement(block: &BlockStatement<'_>) -> BlockStatement<'static> {
    BlockStatement {
        body: block.body.iter().map(clone_statement).collect(),
    }
}

fn clone_statement(stmt: &Statement<'_>) -> Statement<'static> {
    match stmt {
        Statement::ExpressionStatement(expr) => {
            Statement::ExpressionStatement(clone_expression(expr))
        }
        Statement::BlockStatement(block) => Statement::BlockStatement(clone_block_statement(block)),
        Statement::IfStatement(stmt) => Statement::IfStatement(IfStatement {
            test: clone_expression(&stmt.test),
            consequent: Box::new(clone_statement(&stmt.consequent)),
            alternate: stmt
                .alternate
                .as_ref()
                .map(|alt| Box::new(clone_statement(alt))),
        }),
        Statement::WithStatement(stmt) => Statement::WithStatement(WithStatement {
            object: clone_expression(&stmt.object),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::WhileStatement(stmt) => Statement::WhileStatement(WhileStatement {
            test: clone_expression(&stmt.test),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::ForStatement(stmt) => Statement::ForStatement(ForStatement {
            init: stmt
                .init
                .as_ref()
                .map(|init| Box::new(clone_statement(init))),
            test: stmt.test.as_ref().map(clone_expression),
            update: stmt.update.as_ref().map(clone_expression),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::TryStatement(stmt) => Statement::TryStatement(TryStatement {
            block: clone_block_statement(&stmt.block),
            handler: stmt.handler.as_ref().map(|handler| CatchClause {
                param: handler.param.as_ref().map(clone_expression),
                body: clone_block_statement(&handler.body),
            }),
            finalizer: stmt.finalizer.as_ref().map(clone_block_statement),
        }),
        Statement::ThrowStatement(expr) => Statement::ThrowStatement(clone_expression(expr)),
        Statement::VariableDeclaration(decl) => {
            Statement::VariableDeclaration(VariableDeclaration {
                kind: decl.kind.clone(),
                declarations: decl
                    .declarations
                    .iter()
                    .map(|decl| VariableDeclarator {
                        id: clone_expression(&decl.id),
                        init: decl.init.as_ref().map(clone_expression),
                    })
                    .collect(),
            })
        }
        Statement::FunctionDeclaration(func) => {
            Statement::FunctionDeclaration(clone_function_declaration(func))
        }
        Statement::ClassDeclaration(class_decl) => {
            let id = class_decl
                .id
                .map(|value| String::leak(value.to_string()) as &'static str);
            let super_class = class_decl.super_class.as_ref().map(clone_expression);
            let body = class_decl
                .body
                .iter()
                .map(|element| match element {
                    ClassElement::Constructor {
                        function: func,
                        is_default,
                    } => ClassElement::Constructor {
                        function: clone_function_declaration(func),
                        is_default: *is_default,
                    },
                    ClassElement::Method {
                        key,
                        value,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Method {
                            key,
                            value: clone_function_declaration(value),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Getter {
                        key,
                        body,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Getter {
                            key,
                            body: clone_function_declaration(body),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Setter {
                        key,
                        body,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Setter {
                            key,
                            body: clone_function_declaration(body),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Field {
                        key,
                        initializer,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Field {
                            key,
                            initializer: initializer.as_ref().map(clone_expression),
                            is_static: *is_static,
                        }
                    }
                })
                .collect();
            Statement::ClassDeclaration(ClassDeclaration {
                id,
                super_class,
                body,
            })
        }
        Statement::ImportDeclaration(import_decl) => {
            Statement::ImportDeclaration(ImportDeclaration {
                specifiers: import_decl
                    .specifiers
                    .iter()
                    .map(|specifier| match specifier {
                        ImportSpecifier::Default(local) => {
                            ImportSpecifier::Default(String::leak(local.to_string()))
                        }
                        ImportSpecifier::Namespace(local) => {
                            ImportSpecifier::Namespace(String::leak(local.to_string()))
                        }
                        ImportSpecifier::Named { imported, local } => ImportSpecifier::Named {
                            imported: String::leak(imported.to_string()),
                            local: String::leak(local.to_string()),
                        },
                    })
                    .collect(),
                source: String::leak(import_decl.source.to_string()),
            })
        }
        Statement::ExportNamedDeclaration(export_decl) => {
            Statement::ExportNamedDeclaration(ExportNamedDeclaration {
                declaration: export_decl
                    .declaration
                    .as_ref()
                    .map(|decl| Box::new(clone_statement(decl))),
                specifiers: export_decl
                    .specifiers
                    .iter()
                    .map(|specifier| ExportSpecifier {
                        local: String::leak(specifier.local.to_string()),
                        exported: String::leak(specifier.exported.to_string()),
                    })
                    .collect(),
                source: export_decl
                    .source
                    .map(|source| String::leak(source.to_string()) as &'static str),
            })
        }
        Statement::ExportDefaultDeclaration(export_decl) => {
            Statement::ExportDefaultDeclaration(ExportDefaultDeclaration {
                declaration: match &export_decl.declaration {
                    ExportDefaultKind::Expression(expr) => {
                        ExportDefaultKind::Expression(clone_expression(expr))
                    }
                    ExportDefaultKind::FunctionDeclaration(func) => {
                        ExportDefaultKind::FunctionDeclaration(clone_function_declaration(func))
                    }
                    ExportDefaultKind::ClassDeclaration(class_decl) => {
                        let id = class_decl
                            .id
                            .map(|value| String::leak(value.to_string()) as &'static str);
                        let super_class = class_decl.super_class.as_ref().map(clone_expression);
                        let body = class_decl
                            .body
                            .iter()
                            .map(|element| match element {
                                ClassElement::Constructor {
                                    function: func,
                                    is_default,
                                } => ClassElement::Constructor {
                                    function: clone_function_declaration(func),
                                    is_default: *is_default,
                                },
                                ClassElement::Method {
                                    key,
                                    value,
                                    is_static,
                                } => {
                                    let key = clone_object_key(key);
                                    ClassElement::Method {
                                        key,
                                        value: clone_function_declaration(value),
                                        is_static: *is_static,
                                    }
                                }
                                ClassElement::Getter {
                                    key,
                                    body,
                                    is_static,
                                } => {
                                    let key = clone_object_key(key);
                                    ClassElement::Getter {
                                        key,
                                        body: clone_function_declaration(body),
                                        is_static: *is_static,
                                    }
                                }
                                ClassElement::Setter {
                                    key,
                                    body,
                                    is_static,
                                } => {
                                    let key = clone_object_key(key);
                                    ClassElement::Setter {
                                        key,
                                        body: clone_function_declaration(body),
                                        is_static: *is_static,
                                    }
                                }
                                ClassElement::Field {
                                    key,
                                    initializer,
                                    is_static,
                                } => {
                                    let key = clone_object_key(key);
                                    ClassElement::Field {
                                        key,
                                        initializer: initializer.as_ref().map(clone_expression),
                                        is_static: *is_static,
                                    }
                                }
                            })
                            .collect();
                        ExportDefaultKind::ClassDeclaration(ClassDeclaration {
                            id,
                            super_class,
                            body,
                        })
                    }
                },
            })
        }
        Statement::ExportAllDeclaration(export_decl) => {
            Statement::ExportAllDeclaration(ExportAllDeclaration {
                exported: export_decl
                    .exported
                    .map(|name| String::leak(name.to_string()) as &'static str),
                source: String::leak(export_decl.source.to_string()),
            })
        }
        Statement::ReturnStatement(expr) => {
            Statement::ReturnStatement(expr.as_ref().map(clone_expression))
        }
        Statement::EmptyStatement => Statement::EmptyStatement,
        Statement::DoWhileStatement(stmt) => Statement::DoWhileStatement(WhileStatement {
            test: clone_expression(&stmt.test),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::ForInStatement(stmt) => Statement::ForInStatement(ForInStatement {
            left: Box::new(clone_statement(&stmt.left)),
            right: clone_expression(&stmt.right),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::ForOfStatement(stmt) => Statement::ForOfStatement(ForOfStatement {
            left: Box::new(clone_statement(&stmt.left)),
            right: clone_expression(&stmt.right),
            body: Box::new(clone_statement(&stmt.body)),
            is_await: stmt.is_await,
        }),
        Statement::SwitchStatement(stmt) => Statement::SwitchStatement(SwitchStatement {
            discriminant: clone_expression(&stmt.discriminant),
            cases: stmt
                .cases
                .iter()
                .map(|case| SwitchCase {
                    test: case.test.as_ref().map(clone_expression),
                    consequent: case.consequent.iter().map(clone_statement).collect(),
                })
                .collect(),
        }),
        Statement::BreakStatement(label) => Statement::BreakStatement(label.map(|s| {
            let l: &'static mut str = String::leak(s.to_string());
            l as &'static str
        })),
        Statement::ContinueStatement(label) => Statement::ContinueStatement(label.map(|s| {
            let l: &'static mut str = String::leak(s.to_string());
            l as &'static str
        })),
        Statement::LabeledStatement(stmt) => Statement::LabeledStatement(LabeledStatement {
            label: {
                let l: &'static mut str = String::leak(stmt.label.to_string());
                l as &'static str
            },
            body: Box::new(clone_statement(&stmt.body)),
        }),
    }
}

fn clone_expression(expr: &Expression<'_>) -> Expression<'static> {
    match expr {
        Expression::Literal(Literal::Number(n)) => Expression::Literal(Literal::Number(*n)),
        Expression::Literal(Literal::String(s)) => {
            Expression::Literal(Literal::String(String::leak((*s).to_string())))
        }
        Expression::Literal(Literal::Boolean(b)) => Expression::Literal(Literal::Boolean(*b)),
        Expression::Literal(Literal::Null) => Expression::Literal(Literal::Null),
        Expression::Literal(Literal::Undefined) => Expression::Literal(Literal::Undefined),
        Expression::Literal(Literal::BigInt(n)) => Expression::Literal(Literal::BigInt(*n)),
        Expression::Literal(Literal::RegExp(pattern, flags)) => {
            Expression::Literal(Literal::RegExp(
                String::leak((*pattern).to_string()),
                String::leak((*flags).to_string()),
            ))
        }
        Expression::Identifier(name) => Expression::Identifier(String::leak((*name).to_string())),
        Expression::PrivateIdentifier(name) => {
            Expression::PrivateIdentifier(String::leak((*name).to_string()))
        }
        Expression::BinaryExpression(expr) => {
            Expression::BinaryExpression(Box::new(BinaryExpression {
                operator: expr.operator.clone(),
                left: clone_expression(&expr.left),
                right: clone_expression(&expr.right),
            }))
        }
        Expression::UnaryExpression(expr) => {
            Expression::UnaryExpression(Box::new(UnaryExpression {
                operator: expr.operator.clone(),
                argument: clone_expression(&expr.argument),
                prefix: expr.prefix,
            }))
        }
        Expression::AssignmentExpression(expr) => {
            Expression::AssignmentExpression(Box::new(AssignmentExpression {
                operator: expr.operator.clone(),
                left: clone_expression(&expr.left),
                right: clone_expression(&expr.right),
            }))
        }
        Expression::MemberExpression(expr) => {
            Expression::MemberExpression(Box::new(MemberExpression {
                object: clone_expression(&expr.object),
                property: clone_expression(&expr.property),
                computed: expr.computed,
                optional: expr.optional,
            }))
        }
        Expression::CallExpression(expr) => Expression::CallExpression(Box::new(CallExpression {
            callee: clone_expression(&expr.callee),
            arguments: expr.arguments.iter().map(clone_expression).collect(),
            optional: expr.optional,
        })),
        Expression::NewExpression(expr) => Expression::NewExpression(Box::new(CallExpression {
            callee: clone_expression(&expr.callee),
            arguments: expr.arguments.iter().map(clone_expression).collect(),
            optional: expr.optional,
        })),
        Expression::FunctionExpression(func) => {
            Expression::FunctionExpression(Box::new(clone_function_declaration(func)))
        }
        Expression::ArrowFunctionExpression(func) => {
            Expression::ArrowFunctionExpression(Box::new(clone_function_declaration(func)))
        }
        Expression::UpdateExpression(expr) => {
            Expression::UpdateExpression(Box::new(UpdateExpression {
                operator: expr.operator.clone(),
                argument: clone_expression(&expr.argument),
                prefix: expr.prefix,
            }))
        }
        Expression::SequenceExpression(seq) => {
            Expression::SequenceExpression(seq.iter().map(clone_expression).collect())
        }
        Expression::ConditionalExpression {
            test,
            consequent,
            alternate,
        } => Expression::ConditionalExpression {
            test: Box::new(clone_expression(test)),
            consequent: Box::new(clone_expression(consequent)),
            alternate: Box::new(clone_expression(alternate)),
        },
        Expression::ArrayExpression(values) => Expression::ArrayExpression(
            values
                .iter()
                .map(|value| value.as_ref().map(clone_expression))
                .collect(),
        ),
        Expression::ObjectExpression(props) => Expression::ObjectExpression(
            props
                .iter()
                .map(|prop| {
                    let key = clone_object_key(&prop.key);
                    let kind = match &prop.kind {
                        ObjectPropertyKind::Value(value) => {
                            ObjectPropertyKind::Value(clone_expression(value))
                        }
                        ObjectPropertyKind::Getter(func) => {
                            ObjectPropertyKind::Getter(clone_function_declaration(func))
                        }
                        ObjectPropertyKind::Setter(func) => {
                            ObjectPropertyKind::Setter(clone_function_declaration(func))
                        }
                    };
                    ObjectProperty {
                        key,
                        value: clone_expression(&prop.value),
                        shorthand: prop.shorthand,
                        computed: prop.computed,
                        method: prop.method,
                        kind,
                    }
                })
                .collect(),
        ),
        Expression::ClassExpression(expr) => {
            let id = expr.id.map(|value| {
                let leaked: &'static mut str = String::leak(value.to_string());
                leaked as &'static str
            });
            let super_class = expr.super_class.as_ref().map(clone_expression);
            let body = expr
                .body
                .iter()
                .map(|element| match element {
                    ClassElement::Constructor {
                        function: func,
                        is_default,
                    } => ClassElement::Constructor {
                        function: clone_function_declaration(func),
                        is_default: *is_default,
                    },
                    ClassElement::Method {
                        key,
                        value,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Method {
                            key,
                            value: clone_function_declaration(value),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Getter {
                        key,
                        body,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Getter {
                            key,
                            body: clone_function_declaration(body),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Setter {
                        key,
                        body,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Setter {
                            key,
                            body: clone_function_declaration(body),
                            is_static: *is_static,
                        }
                    }
                    ClassElement::Field {
                        key,
                        initializer,
                        is_static,
                    } => {
                        let key = clone_object_key(key);
                        ClassElement::Field {
                            key,
                            initializer: initializer.as_ref().map(clone_expression),
                            is_static: *is_static,
                        }
                    }
                })
                .collect();
            Expression::ClassExpression(Box::new(ClassDeclaration {
                id,
                super_class,
                body,
            }))
        }
        Expression::ThisExpression => Expression::ThisExpression,
        Expression::SuperExpression => Expression::SuperExpression,
        Expression::SpreadElement(expr) => {
            Expression::SpreadElement(Box::new(clone_expression(expr)))
        }
        Expression::TemplateLiteral(parts) => Expression::TemplateLiteral(
            parts
                .iter()
                .map(|part| match part {
                    TemplatePart::String(s) => TemplatePart::String(String::leak(s.to_string())),
                    TemplatePart::Expr(expr) => TemplatePart::Expr(clone_expression(expr)),
                })
                .collect(),
        ),
        Expression::YieldExpression { argument, delegate } => Expression::YieldExpression {
            argument: argument.as_ref().map(|e| Box::new(clone_expression(e))),
            delegate: *delegate,
        },
        Expression::AwaitExpression(expr) => {
            Expression::AwaitExpression(Box::new(clone_expression(expr)))
        }
        Expression::TaggedTemplateExpression(tag, parts) => Expression::TaggedTemplateExpression(
            Box::new(clone_expression(tag)),
            parts
                .iter()
                .map(|part| match part {
                    TemplatePart::String(s) => TemplatePart::String(String::leak(s.to_string())),
                    TemplatePart::Expr(expr) => TemplatePart::Expr(clone_expression(expr)),
                })
                .collect(),
        ),
    }
}

impl Drop for Interpreter {
    fn drop(&mut self) {
        self.global_env.borrow_mut().variables.clear();
    }
}

fn js_strict_eq(left: &JsValue, right: &JsValue) -> bool {
    let left = resolve_indirect_value(left);
    let right = resolve_indirect_value(right);
    match (&left, &right) {
        (JsValue::Undefined, JsValue::Undefined) => true,
        (JsValue::Null, JsValue::Null) => true,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        (JsValue::Number(a), JsValue::Number(b)) => a == b,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
        (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
        (JsValue::Function(a), JsValue::Function(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn js_abstract_eq(left: &JsValue, right: &JsValue) -> bool {
    let left = resolve_indirect_value(left);
    let right = resolve_indirect_value(right);
    match (&left, &right) {
        (JsValue::Null, JsValue::Undefined) | (JsValue::Undefined, JsValue::Null) => true,
        (JsValue::Number(a), JsValue::String(b)) => *a == b.parse::<f64>().unwrap_or(f64::NAN),
        (JsValue::String(a), JsValue::Number(b)) => a.parse::<f64>().unwrap_or(f64::NAN) == *b,
        (JsValue::Boolean(a), _) => {
            js_abstract_eq(&JsValue::Number(if *a { 1.0 } else { 0.0 }), &right)
        }
        (_, JsValue::Boolean(b)) => {
            js_abstract_eq(&left, &JsValue::Number(if *b { 1.0 } else { 0.0 }))
        }
        _ => js_strict_eq(&left, &right),
    }
}

fn extract_for_binding<'a>(stmt: &'a Statement<'a>) -> Option<(&'a Expression<'a>, bool)> {
    match stmt {
        Statement::VariableDeclaration(decl) => decl
            .declarations
            .first()
            .map(|declarator| (&declarator.id, true)),
        Statement::ExpressionStatement(expr) => Some((expr, false)),
        _ => None,
    }
}
