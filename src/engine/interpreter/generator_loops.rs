use super::*;

impl Interpreter {
    pub(super) fn expression_contains_yield(&self, expr: &Expression) -> bool {
        match expr {
            Expression::YieldExpression { .. } => true,
            Expression::Literal(_)
            | Expression::Identifier(_)
            | Expression::ThisExpression
            | Expression::SuperExpression
            | Expression::MetaProperty(_)
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

    pub(super) fn statement_contains_yield(&self, stmt: &Statement) -> bool {
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

    pub(super) fn eval_generator_block(
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

    pub(super) fn eval_generator_variable_declaration(
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

    pub(super) fn eval_generator_assign_array_pattern(
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

    pub(super) fn eval_generator_assign_object_property(
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

    pub(super) fn eval_generator_assign_object_pattern(
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

    pub(super) fn eval_generator_assign_pattern(
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

    pub(super) fn eval_generator_sequence(
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

    pub(super) fn run_generator_finalizer(
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

    pub(super) fn eval_generator_catch_body(
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

    pub(super) fn eval_generator_catch_clause(
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

    pub(super) fn eval_generator_for_in_iteration_body(
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

    pub(super) fn eval_generator_for_of_iteration_body(
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

    pub(super) fn eval_generator_while_loop(
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

    pub(super) fn eval_generator_for_loop(
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

    pub(super) fn eval_generator_for_body(
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

    pub(super) fn eval_generator_for_update(
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

    pub(super) fn eval_generator_for_in_loop(
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

    pub(super) fn eval_generator_for_of_loop(
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
}
