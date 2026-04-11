use super::*;

impl Interpreter {
    pub(super) fn check_timeout(&mut self) -> Result<(), RuntimeError> {
        self.instruction_count += 1;
        if self.instruction_count > 2_000 {
            return Err(RuntimeError::Timeout);
        }
        Ok(())
    }

    pub(super) fn create_function_value(
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

    pub(super) fn create_arrow_function_value(
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

    pub(super) fn create_method_function_value(
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

    pub(super) fn create_accessor_function_value(
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

    pub(super) fn create_function_value_with_meta(
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

    pub(super) fn current_home_object_super_property_base(
        &self,
        home_object: &JsValue,
    ) -> Option<JsValue> {
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

    pub(super) fn bind_function_super_context(
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

    pub(super) fn initialize_instance_fields_for_function(
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

    pub(super) fn maybe_initialize_current_instance_fields(
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

    pub(super) fn class_public_key_to_string(
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

    pub(super) fn build_class_value(
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
        if let Some(class_name) = class_decl.id {
            static_env
                .borrow_mut()
                .define(class_name.to_string(), class_value.clone());
        }
        if let Some(super_binding) = &super_value {
            static_env
                .borrow_mut()
                .define("super".to_string(), super_binding.clone());
        }
        static_env
            .borrow_mut()
            .define("__home_object__".to_string(), class_value.clone());
        static_env.borrow_mut().define(
            "__super_property_base__".to_string(),
            super_value.clone().unwrap_or(JsValue::Null),
        );

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
                            Some(expr) => self.eval_expression(expr, Rc::clone(&static_env))?,
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
                ClassElement::StaticBlock(block) => {
                    self.eval_statement(
                        &Statement::BlockStatement(block.clone()),
                        Rc::clone(&static_env),
                    )?;
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

    pub(super) fn call_function_value(
        &mut self,
        function: Rc<FunctionValue>,
        this_value: JsValue,
        args: Vec<JsValue>,
        is_construct_call: bool,
        new_target: JsValue,
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
        let resolved_new_target = if function.uses_lexical_this {
            function
                .env
                .borrow()
                .get("__new_target__")
                .unwrap_or(JsValue::Undefined)
        } else {
            new_target
        };
        call_env
            .borrow_mut()
            .define("this".to_string(), initial_this);
        call_env
            .borrow_mut()
            .define("__constructor_this__".to_string(), this_value.clone());
        call_env
            .borrow_mut()
            .define("__new_target__".to_string(), resolved_new_target);
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
}
