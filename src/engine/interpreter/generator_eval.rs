use super::*;

impl Interpreter {
    pub(super) fn eval_generator_property_key(
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

    pub(super) fn eval_generator_array_expression(
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

    pub(super) fn eval_generator_call_arguments(
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

    pub(super) fn eval_generator_object_expression(
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

    pub(super) fn eval_generator_template_literal(
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

    pub(super) fn eval_generator_tagged_template_arguments(
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

    pub(super) fn eval_generator_member_expression_value(
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

    pub(super) fn eval_generator_call_target(
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

    pub(super) fn eval_tagged_template_target<'a>(
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

    pub(super) fn eval_generator_statement(
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

    pub(super) fn eval_generator_switch_statement(
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

    pub(super) fn eval_generator_switch_match(
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

    pub(super) fn eval_generator_switch_consequents(
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

    pub(super) fn eval_generator_update_expression(
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

    pub(super) fn yield_from_iterator(
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

    pub(super) fn eval_generator_expression(
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
                            UnaryOperator::Minus => match value {
                                JsValue::BigInt(n) => JsValue::BigInt(-n),
                                _ => JsValue::Number(-value.as_number()),
                            },
                            UnaryOperator::Plus => match value {
                                JsValue::BigInt(_) => {
                                    return Err(RuntimeError::TypeError(
                                        "cannot convert BigInt value to number".into(),
                                    ));
                                }
                                _ => JsValue::Number(value.as_number()),
                            },
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
                    Expression::MemberExpression(mem)
                        if self.member_private_name(mem).is_some() =>
                    {
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
                            let object =
                                self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
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
                                let current = interp.read_super_member_value(
                                    Rc::clone(&env_clone),
                                    &property_key,
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
                            let property_key =
                                self.member_property_key(mem, Rc::clone(&env_clone))?;
                            let current =
                                self.read_super_member_value(Rc::clone(&env_clone), &property_key)?;
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
                            move |interp: &mut Interpreter,
                                  object: JsValue,
                                  property_key: String| {
                                let current = interp.read_member_value(
                                    object.clone(),
                                    &property_key,
                                    None,
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
                            let object =
                                self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
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
                            let object =
                                self.eval_expression(&mem.object, Rc::clone(&env_clone))?;
                            let property_key =
                                self.member_property_key(mem, Rc::clone(&env_clone))?;
                            let current =
                                self.read_member_value(object.clone(), &property_key, None)?;
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
                    _ => Err(RuntimeError::SyntaxError(
                        "invalid assignment target".into(),
                    )),
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
                                            if is_super_call {
                                                env_for_this
                                                    .borrow()
                                                    .get("__new_target__")
                                                    .unwrap_or(JsValue::Undefined)
                                            } else {
                                                JsValue::Undefined
                                            },
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
                                                JsValue::Function(Rc::clone(&function)),
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
            | Expression::MetaProperty(_)
            | Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)) => {
                let value = self.eval_expression(&other, env)?;
                on_complete(self, value)
            }
        }
    }
}
