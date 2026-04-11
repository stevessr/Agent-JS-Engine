use super::*;

impl Interpreter {
    pub(super) fn read_member_value(
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

    pub(super) fn to_int32(&self, value: &JsValue) -> i32 {
        self.to_uint32(value) as i32
    }

    pub(super) fn to_uint32(&self, value: &JsValue) -> u32 {
        let number = value.as_number();
        if !number.is_finite() || number == 0.0 {
            return 0;
        }

        number.trunc().rem_euclid(4294967296.0) as u32
    }

    pub(super) fn bigint_unsupported_binary_operation(
        &self,
        left: &JsValue,
        right: &JsValue,
        message: &'static str,
    ) -> Result<(), RuntimeError> {
        if matches!(left, JsValue::BigInt(_)) || matches!(right, JsValue::BigInt(_)) {
            return Err(RuntimeError::TypeError(message.into()));
        }
        Ok(())
    }

    pub(super) fn assignment_result(
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
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt remainder is not supported yet",
                )?;
                Ok(JsValue::Number(left.as_number() % right.as_number()))
            }
            AssignmentOperator::PowerAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt exponentiation is not supported yet",
                )?;
                Ok(JsValue::Number(left.as_number().powf(right.as_number())))
            }
            AssignmentOperator::LogicAndAssign
            | AssignmentOperator::LogicOrAssign
            | AssignmentOperator::NullishAssign => Ok(right.clone()),
            AssignmentOperator::BitAndAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt bitwise operations are not supported yet",
                )?;
                Ok(JsValue::Number(
                    (self.to_int32(left) & self.to_int32(right)) as f64,
                ))
            }
            AssignmentOperator::BitOrAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt bitwise operations are not supported yet",
                )?;
                Ok(JsValue::Number(
                    (self.to_int32(left) | self.to_int32(right)) as f64,
                ))
            }
            AssignmentOperator::BitXorAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt bitwise operations are not supported yet",
                )?;
                Ok(JsValue::Number(
                    (self.to_int32(left) ^ self.to_int32(right)) as f64,
                ))
            }
            AssignmentOperator::ShiftLeftAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt shift operations are not supported yet",
                )?;
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) << shift) as f64))
            }
            AssignmentOperator::ShiftRightAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt shift operations are not supported yet",
                )?;
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) >> shift) as f64))
            }
            AssignmentOperator::UnsignedShiftRightAssign => {
                self.bigint_unsupported_binary_operation(
                    left,
                    right,
                    "BigInt shift operations are not supported yet",
                )?;
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_uint32(left) >> shift) as f64))
            }
        }
    }

    pub(super) fn eval_binary_operation(
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
                if matches!(left, JsValue::BigInt(_)) || matches!(right, JsValue::BigInt(_)) {
                    Err(RuntimeError::TypeError(
                        "BigInt exponentiation is not supported yet".into(),
                    ))
                } else {
                    Ok(JsValue::Number(left.as_number().powf(right.as_number())))
                }
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

    pub(super) fn write_member_value(
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

    pub(super) fn write_own_member_value(
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

    pub(super) fn get_property_value_from_base(
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

    pub(super) fn current_super_property_base(
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

    pub(super) fn current_super_receiver(&self, env: &Rc<RefCell<Environment>>) -> JsValue {
        env.borrow().get("this").unwrap_or(JsValue::Undefined)
    }

    pub(super) fn read_super_member_value(
        &mut self,
        env: Rc<RefCell<Environment>>,
        property_key: &str,
    ) -> Result<JsValue, RuntimeError> {
        let receiver = self.current_super_receiver(&env);
        let base = self.current_super_property_base(&env)?;
        self.read_member_value(base, property_key, Some(receiver))
    }

    pub(super) fn write_super_member_value(
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

    pub(super) fn should_apply_assignment(
        &self,
        operator: &AssignmentOperator,
        current: &JsValue,
    ) -> bool {
        match operator {
            AssignmentOperator::LogicAndAssign => current.is_truthy(),
            AssignmentOperator::LogicOrAssign => !current.is_truthy(),
            AssignmentOperator::NullishAssign => {
                matches!(current, JsValue::Undefined | JsValue::Null)
            }
            _ => true,
        }
    }

    pub(super) fn collect_with_scope_bindings(&self, object: &JsValue) -> Vec<(String, JsValue)> {
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

    pub(super) fn read_property_for_pattern(
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

    pub(super) fn enumerable_keys_for_pattern(
        &self,
        source: &JsValue,
    ) -> Result<Vec<String>, RuntimeError> {
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

    pub(super) fn object_rest_for_pattern(
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

    pub(super) fn assign_identifier(
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

    pub(super) fn assign_pattern(
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

    pub(super) fn bind_parameters(
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

    pub(super) fn sync_with_scope_bindings(
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
}
