use super::*;

impl Interpreter {
    pub(super) fn invoke_callable(
        &mut self,
        callee: JsValue,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, RuntimeError> {
        match callee {
            JsValue::Function(function) => {
                self.call_function_value(function, this_value, args, false, JsValue::Undefined)
            }
            JsValue::NativeFunction(function) => (function.func)(self, &this_value, &args),
            JsValue::BuiltinFunction(function) => {
                self.invoke_builtin_function(function.as_ref(), this_value, args)
            }
            _ => Err(RuntimeError::TypeError("value is not callable".into())),
        }
    }

    pub(super) fn invoke_builtin_function(
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

    pub(super) fn collect_delegate_yields(
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

    pub(super) fn collect_iterable_items(
        &mut self,
        value: JsValue,
    ) -> Result<Vec<JsValue>, RuntimeError> {
        let mut cursor = self.begin_iteration(value)?;
        let mut items = Vec::new();
        loop {
            match self.iterator_step(&mut cursor, false)? {
                IteratorStep::Yield(value) => items.push(value),
                IteratorStep::Complete(_) => return Ok(items),
            }
        }
    }

    pub(super) fn collect_for_in_keys_from_object(
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

    pub(super) fn collect_for_in_keys(&self, value: JsValue) -> Result<Vec<String>, RuntimeError> {
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
}
