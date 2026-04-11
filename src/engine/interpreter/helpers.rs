use super::*;

pub(super) fn generator_state_from_receiver(
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

pub(super) fn generator_next_native(
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

pub(super) fn generator_return_native(
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

pub(super) fn generator_throw_native(
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

pub(super) fn clone_function_declaration(
    func: &FunctionDeclaration<'_>,
) -> FunctionDeclaration<'static> {
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

pub(super) fn clone_param(param: &Param<'_>) -> Param<'static> {
    Param {
        pattern: clone_expression(&param.pattern),
        is_rest: param.is_rest,
    }
}

pub(super) fn clone_object_key(key: &ObjectKey<'_>) -> ObjectKey<'static> {
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

pub(super) fn clone_block_statement(block: &BlockStatement<'_>) -> BlockStatement<'static> {
    BlockStatement {
        body: block.body.iter().map(clone_statement).collect(),
    }
}

pub(super) fn clone_statement(stmt: &Statement<'_>) -> Statement<'static> {
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
                    ClassElement::StaticBlock(block) => {
                        ClassElement::StaticBlock(clone_block_statement(block))
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
                                ClassElement::StaticBlock(block) => {
                                    ClassElement::StaticBlock(clone_block_statement(block))
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

pub(super) fn clone_expression(expr: &Expression<'_>) -> Expression<'static> {
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
        Expression::MetaProperty(meta) => Expression::MetaProperty(Box::new(MetaProperty {
            meta: String::leak(meta.meta.to_string()),
            property: String::leak(meta.property.to_string()),
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
                    ClassElement::StaticBlock(block) => {
                        ClassElement::StaticBlock(clone_block_statement(block))
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

pub(super) fn js_strict_eq(left: &JsValue, right: &JsValue) -> bool {
    let left = resolve_indirect_value(left);
    let right = resolve_indirect_value(right);
    match (&left, &right) {
        (JsValue::Undefined, JsValue::Undefined) => true,
        (JsValue::Null, JsValue::Null) => true,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        (JsValue::Number(a), JsValue::Number(b)) => a == b,
        (JsValue::BigInt(a), JsValue::BigInt(b)) => a == b,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
        (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
        (JsValue::Function(a), JsValue::Function(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

pub(super) fn js_abstract_eq(left: &JsValue, right: &JsValue) -> bool {
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

pub(super) fn extract_for_binding<'a>(
    stmt: &'a Statement<'a>,
) -> Option<(&'a Expression<'a>, bool)> {
    match stmt {
        Statement::VariableDeclaration(decl) => decl
            .declarations
            .first()
            .map(|declarator| (&declarator.id, true)),
        Statement::ExpressionStatement(expr) => Some((expr, false)),
        _ => None,
    }
}
