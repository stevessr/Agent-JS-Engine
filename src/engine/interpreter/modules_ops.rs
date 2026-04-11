use super::*;

impl Interpreter {
    pub(super) fn current_module_base_dir(&self) -> Result<PathBuf, RuntimeError> {
        if let Some(dir) = self.module_base_dirs.last() {
            return Ok(dir.clone());
        }
        std::env::current_dir()
            .map_err(|err| RuntimeError::ReferenceError(format!("failed to read cwd: {err}")))
    }

    pub(super) fn resolve_module_path(&self, source: &str) -> Result<PathBuf, RuntimeError> {
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

    pub(super) fn export_identifiers_from_pattern(
        &self,
        pattern: &Expression,
        names: &mut Vec<String>,
    ) {
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

    pub(super) fn write_module_export_value(&mut self, exported_name: &str, value: JsValue) {
        if let Some(exports) = self.module_exports_stack.last() {
            exports
                .borrow_mut()
                .insert(exported_name.to_string(), PropertyValue::Data(value));
        }
    }

    pub(super) fn write_module_export_binding(
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

    pub(super) fn write_module_export_namespace_binding(
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

    pub(super) fn read_namespace_export(
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

    pub(super) fn module_namespace_property_values(
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

    pub(super) fn eval_program_in_env(
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

    pub(super) fn load_module_namespace(&mut self, source: &str) -> Result<JsValue, RuntimeError> {
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
}
