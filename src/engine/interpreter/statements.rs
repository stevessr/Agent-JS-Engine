use super::*;

impl Interpreter {
    pub(super) fn eval_while_statement(
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

    pub(super) fn eval_for_statement(
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

    pub(super) fn eval_for_in_statement(
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

    pub(super) fn eval_for_of_statement(
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
                    self.assign_pattern(&d.id, val.clone(), Rc::clone(&env), true)?;
                    if matches!(decl.kind, VariableKind::Using | VariableKind::AwaitUsing) {
                        env.borrow_mut().resources.push(ResourceRecord {
                            value: val,
                            is_await: matches!(decl.kind, VariableKind::AwaitUsing),
                        });
                    }
                }
                Ok(JsValue::Undefined)
            }
            Statement::ExpressionStatement(expr) => self.eval_expression(expr, env),
            Statement::BlockStatement(block) => {
                let block_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                let mut last_val = JsValue::Undefined;
                let result = (|| {
                    for s in &block.body {
                        self.check_timeout()?;
                        last_val = self.eval_statement(s, Rc::clone(&block_env))?;
                    }
                    Ok(last_val)
                })();
                self.dispose_env_resources(block_env, result)
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
                Literal::BigInt(n) => Ok(JsValue::BigInt(*n)),
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
                _ => Err(RuntimeError::SyntaxError(
                    "invalid assignment target".into(),
                )),
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
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt remainder is not supported yet",
                        )?;
                        Ok(JsValue::Number(left.as_number() % right.as_number()))
                    }
                    BinaryOperator::BitAnd => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt bitwise operations are not supported yet",
                        )?;
                        Ok(JsValue::Number(
                            (self.to_int32(&left) & self.to_int32(&right)) as f64,
                        ))
                    }
                    BinaryOperator::BitOr => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt bitwise operations are not supported yet",
                        )?;
                        Ok(JsValue::Number(
                            (self.to_int32(&left) | self.to_int32(&right)) as f64,
                        ))
                    }
                    BinaryOperator::BitXor => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt bitwise operations are not supported yet",
                        )?;
                        Ok(JsValue::Number(
                            (self.to_int32(&left) ^ self.to_int32(&right)) as f64,
                        ))
                    }
                    BinaryOperator::ShiftLeft => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt shift operations are not supported yet",
                        )?;
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) << shift) as f64))
                    }
                    BinaryOperator::ShiftRight => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt shift operations are not supported yet",
                        )?;
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) >> shift) as f64))
                    }
                    BinaryOperator::LogicalShiftRight => {
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt shift operations are not supported yet",
                        )?;
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
                        self.bigint_unsupported_binary_operation(
                            &left,
                            &right,
                            "BigInt exponentiation is not supported yet",
                        )?;
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
                        UnaryOperator::Minus => match arg {
                            JsValue::BigInt(n) => Ok(JsValue::BigInt(-n)),
                            _ => Ok(JsValue::Number(-arg.as_number())),
                        },
                        UnaryOperator::Plus => match arg {
                            JsValue::BigInt(_) => Err(RuntimeError::TypeError(
                                "cannot convert BigInt value to number".into(),
                            )),
                            _ => Ok(JsValue::Number(arg.as_number())),
                        },
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
                            if matches!(call.callee, Expression::SuperExpression) {
                                env.borrow()
                                    .get("__new_target__")
                                    .unwrap_or(JsValue::Undefined)
                            } else {
                                JsValue::Undefined
                            },
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
            Expression::MetaProperty(meta) => {
                if meta.meta == "new" && meta.property == "target" {
                    Ok(env
                        .borrow()
                        .get("__new_target__")
                        .unwrap_or(JsValue::Undefined))
                } else {
                    Err(RuntimeError::SyntaxError(
                        "unsupported meta property".into(),
                    ))
                }
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
                            JsValue::Function(Rc::clone(&function)),
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

    pub(super) fn resume_generator(
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

    pub(super) fn finish_generator_resume(
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
