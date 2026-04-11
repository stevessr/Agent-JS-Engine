use super::*;

impl Interpreter {
    pub(super) fn member_property_key(
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

    pub(super) fn property_key_from_value(&self, property: JsValue) -> String {
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

    pub(super) fn member_private_name<'a>(&self, mem: &'a MemberExpression<'a>) -> Option<&'a str> {
        if mem.computed {
            None
        } else if let Expression::PrivateIdentifier(name) = &mem.property {
            Some(*name)
        } else {
            None
        }
    }

    pub(super) fn object_identity(value: &JsValue) -> Option<usize> {
        match value {
            JsValue::Object(map) => Some(Rc::as_ptr(map) as usize),
            JsValue::Function(function) => Some(Rc::as_ptr(function) as usize),
            JsValue::Array(values) => Some(Rc::as_ptr(values) as usize),
            JsValue::EnvironmentObject(env) => Some(Rc::as_ptr(env) as usize),
            _ => None,
        }
    }

    pub(super) fn current_private_brand(&self, env: &Rc<RefCell<Environment>>) -> Option<usize> {
        env.borrow()
            .get("__private_brand__")
            .and_then(|value| match value {
                JsValue::Number(n) if n >= 0.0 => Some(n as usize),
                _ => None,
            })
    }

    pub(super) fn brand_object(&mut self, object: &JsValue, brand: usize) {
        if let Some(id) = Self::object_identity(object) {
            self.object_private_brands
                .entry(id)
                .or_default()
                .insert(brand);
        }
    }

    pub(super) fn object_has_brand(&self, object: &JsValue, brand: usize) -> bool {
        Self::object_identity(object)
            .and_then(|id| self.object_private_brands.get(&id))
            .is_some_and(|brands| brands.contains(&brand))
    }

    pub(super) fn get_private_slot(
        &self,
        object: &JsValue,
        brand: usize,
        name: &str,
    ) -> Option<PrivateSlot> {
        Self::object_identity(object)
            .and_then(|id| self.object_private_slots.get(&id))
            .and_then(|slots| slots.get(&(brand, name.to_string())).cloned())
    }

    pub(super) fn set_private_slot(
        &mut self,
        object: &JsValue,
        brand: usize,
        name: &str,
        value: JsValue,
    ) {
        if let Some(id) = Self::object_identity(object) {
            self.object_private_slots
                .entry(id)
                .or_default()
                .insert((brand, name.to_string()), PrivateSlot::Data(value));
        }
    }

    pub(super) fn private_definition<'a>(
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

    pub(super) fn private_member_kind<'a>(
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

    pub(super) fn read_private_member_value(
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

    pub(super) fn write_private_member_value(
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

    pub(super) fn has_private_member_brand(
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

    pub(super) fn declare_private_name(
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

    pub(super) fn ensure_private_name_declared(
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

    pub(super) fn validate_private_names_in_function(
        &self,
        function: &FunctionDeclaration,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        for param in &function.params {
            self.validate_private_names_in_expression(&param.pattern, declared_names)?;
        }
        self.validate_private_names_in_block(&function.body, declared_names)
    }

    pub(super) fn validate_private_names_in_block(
        &self,
        block: &BlockStatement,
        declared_names: &HashSet<String>,
    ) -> Result<(), RuntimeError> {
        for statement in &block.body {
            self.validate_private_names_in_statement(statement, declared_names)?;
        }
        Ok(())
    }

    pub(super) fn validate_private_names_in_statement(
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

    pub(super) fn validate_private_names_in_expression(
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
            Expression::MetaProperty(_) => Ok(()),
            Expression::Literal(_)
            | Expression::Identifier(_)
            | Expression::ThisExpression
            | Expression::SuperExpression => Ok(()),
        }
    }

    pub(super) fn validate_private_class_elements(
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
                ClassElement::Constructor { .. } | ClassElement::StaticBlock(_) => continue,
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
                ClassElement::StaticBlock(block) => {
                    self.validate_private_names_in_block(block, &declared_names)?;
                }
            }
        }

        Ok(())
    }
}
