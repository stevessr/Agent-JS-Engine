use super::*;

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

    pub(super) fn generator_result_object(value: JsValue, done: bool) -> JsValue {
        let mut result = HashMap::new();
        result.insert("value".to_string(), PropertyValue::Data(value));
        result.insert(
            "done".to_string(),
            PropertyValue::Data(JsValue::Boolean(done)),
        );
        JsValue::Object(Rc::new(RefCell::new(result)))
    }

    pub(super) fn promise_with_state(state: PromiseState) -> JsValue {
        JsValue::Promise(Rc::new(RefCell::new(PromiseValue {
            state,
            reactions: Vec::new(),
        })))
    }

    pub(super) fn pending_promise() -> Rc<RefCell<PromiseValue>> {
        Rc::new(RefCell::new(PromiseValue {
            state: PromiseState::Pending,
            reactions: Vec::new(),
        }))
    }

    pub(super) fn resolved_promise(value: JsValue) -> JsValue {
        Self::promise_with_state(PromiseState::Fulfilled(value))
    }

    pub(super) fn rejected_promise(reason: JsValue) -> JsValue {
        Self::promise_with_state(PromiseState::Rejected(reason))
    }

    pub(super) fn async_generator_result_value(
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

    pub(super) fn to_rejection_value(&self, error: RuntimeError) -> JsValue {
        match error {
            RuntimeError::Throw(value) => value,
            other => JsValue::String(other.to_string()),
        }
    }

    pub(super) fn await_value(&mut self, value: JsValue) -> Result<JsValue, RuntimeError> {
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

    pub(super) fn get_dispose_method(
        &mut self,
        value: &JsValue,
        property_key: &str,
    ) -> Result<Option<JsValue>, RuntimeError> {
        let property = match value {
            JsValue::Object(map) => get_property_value(map, property_key),
            JsValue::Function(function) => get_property_value(&function.properties, property_key),
            _ => None,
        };
        match property {
            Some(PropertyValue::Data(method)) => {
                if matches!(method, JsValue::Undefined) {
                    Ok(None)
                } else {
                    Ok(Some(method))
                }
            }
            Some(PropertyValue::Accessor { getter, .. }) => match getter {
                Some(getter) => self
                    .invoke_callable(getter, value.clone(), vec![])
                    .map(Some),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }

    pub(super) fn dispose_resource(
        &mut self,
        resource: ResourceRecord,
    ) -> Result<(), RuntimeError> {
        let method = if resource.is_await {
            self.get_dispose_method(&resource.value, "asyncDispose")?
                .or_else(|| {
                    self.get_dispose_method(&resource.value, "dispose")
                        .ok()
                        .flatten()
                })
        } else {
            self.get_dispose_method(&resource.value, "dispose")?
        };

        let Some(method) = method else {
            return Err(RuntimeError::TypeError(
                "Disposable value must have a callable dispose method".into(),
            ));
        };

        let result = self.invoke_callable(method, resource.value, vec![])?;
        if resource.is_await {
            self.await_value(result)?;
        }
        Ok(())
    }

    pub(super) fn dispose_env_resources(
        &mut self,
        env: Rc<RefCell<Environment>>,
        completion: Result<JsValue, RuntimeError>,
    ) -> Result<JsValue, RuntimeError> {
        let resources = {
            let mut env = env.borrow_mut();
            std::mem::take(&mut env.resources)
        };
        let mut completion = completion;
        for resource in resources.into_iter().rev() {
            match self.dispose_resource(resource) {
                Ok(()) => {}
                Err(error) => completion = Err(error),
            }
        }
        completion
    }

    pub(super) fn attach_promise_reaction(
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

    pub(super) fn settle_promise(
        &mut self,
        promise: Rc<RefCell<PromiseValue>>,
        state: PromiseState,
    ) {
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

    pub(super) fn resolve_promise_value(
        &mut self,
        promise: Rc<RefCell<PromiseValue>>,
        value: JsValue,
    ) {
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

    pub(super) fn process_then_reaction(
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

    pub(super) fn process_finally_reaction(
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

    pub(super) fn run_microtask(&mut self, task: Microtask) {
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

    pub(super) fn drain_microtasks(&mut self) -> Result<(), RuntimeError> {
        while let Some(task) = self.microtask_queue.pop_front() {
            self.run_microtask(task);
        }
        Ok(())
    }

    pub(super) fn drain_microtasks_until_promise_settled(
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

    pub(super) fn invoke_getter(
        &mut self,
        getter: JsValue,
        this_value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        self.invoke_callable(getter, this_value, vec![])
    }

    pub(super) fn create_generator_iterator(&self, state: Rc<RefCell<GeneratorState>>) -> JsValue {
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

    pub(super) fn begin_iteration(
        &mut self,
        value: JsValue,
    ) -> Result<IteratorCursor, RuntimeError> {
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

    pub(super) fn iterator_step(
        &mut self,
        cursor: &mut IteratorCursor,
        await_result: bool,
    ) -> Result<IteratorStep, RuntimeError> {
        self.iterator_resume(cursor, ResumeAction::Next(JsValue::Undefined), await_result)
    }

    pub(super) fn iterator_resume(
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

    pub(super) fn close_iterator(
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

    pub(super) fn map_generator_execution(
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

    pub(super) fn continue_generator_execution(
        &mut self,
        exec: GeneratorExecution,
        on_complete: GeneratorValueContinuation,
    ) -> Result<GeneratorExecution, RuntimeError> {
        self.map_generator_execution(exec, on_complete, Rc::new(|_, error| Err(error)))
    }

    pub(super) fn yield_generator_value(
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
}
