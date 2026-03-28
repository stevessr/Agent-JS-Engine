use crate::engine::env::Environment;
use crate::engine::value::{
    get_object_property, has_object_property, object_with_proto, FunctionValue, JsValue,
};
use crate::parser::ast::*;
use std::cell::RefCell;
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    #[error("Reference Error: {0} is not defined")]
    ReferenceError(String),
    #[error("Type Error: {0}")]
    TypeError(String),
    #[error("Syntax Error: {0}")]
    SyntaxError(String),
    #[error("Return: {0:?}")]
    Return(JsValue),
    #[error("Throw: {0:?}")]
    Throw(JsValue),
    #[error("Break: {0:?}")]
    Break(Option<String>),
    #[error("Continue: {0:?}")]
    Continue(Option<String>),
    #[error("Timeout: Infinite loop or too many instructions")]
    Timeout,
}

pub struct Interpreter {
    pub global_env: Rc<RefCell<Environment>>,
    pub instruction_count: usize,
    pub functions: Vec<FunctionDeclaration<'static>>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            global_env: Rc::new(RefCell::new(Environment::new(None))),
            instruction_count: 0,
            functions: Vec::new(),
        }
    }

    fn member_property_key(
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
                other => Err(RuntimeError::TypeError(format!(
                    "invalid member property: {other:?}"
                ))),
            }
        }
    }

    fn to_int32(&self, value: &JsValue) -> i32 {
        self.to_uint32(value) as i32
    }

    fn to_uint32(&self, value: &JsValue) -> u32 {
        let number = value.as_number();
        if !number.is_finite() || number == 0.0 {
            return 0;
        }

        number.trunc().rem_euclid(4294967296.0) as u32
    }

    fn assignment_result(
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
                Ok(JsValue::Number(left.as_number() % right.as_number()))
            }
            AssignmentOperator::PowerAssign => {
                Ok(JsValue::Number(left.as_number().powf(right.as_number())))
            }
            AssignmentOperator::LogicAndAssign
            | AssignmentOperator::LogicOrAssign
            | AssignmentOperator::NullishAssign => Ok(right.clone()),
            AssignmentOperator::BitAndAssign => Ok(JsValue::Number(
                (self.to_int32(left) & self.to_int32(right)) as f64,
            )),
            AssignmentOperator::BitOrAssign => Ok(JsValue::Number(
                (self.to_int32(left) | self.to_int32(right)) as f64,
            )),
            AssignmentOperator::BitXorAssign => Ok(JsValue::Number(
                (self.to_int32(left) ^ self.to_int32(right)) as f64,
            )),
            AssignmentOperator::ShiftLeftAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) << shift) as f64))
            }
            AssignmentOperator::ShiftRightAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_int32(left) >> shift) as f64))
            }
            AssignmentOperator::UnsignedShiftRightAssign => {
                let shift = self.to_uint32(right) & 0x1f;
                Ok(JsValue::Number((self.to_uint32(left) >> shift) as f64))
            }
        }
    }

    fn write_member_value(
        &self,
        object: JsValue,
        property_key: &str,
        value: JsValue,
    ) -> Result<JsValue, RuntimeError> {
        match object {
            JsValue::Object(values) => {
                values.borrow_mut().insert(property_key.to_string(), value.clone());
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
            _ => Err(RuntimeError::TypeError("value is not an object".into())),
        }
    }

    fn check_timeout(&mut self) -> Result<(), RuntimeError> {
        self.instruction_count += 1;
        if self.instruction_count > 2_000 {
            return Err(RuntimeError::Timeout);
        }
        Ok(())
    }

    fn create_function_value(
        &mut self,
        func: &FunctionDeclaration,
        env: Rc<RefCell<Environment>>,
    ) -> JsValue {
        let id = self.functions.len();
        self.functions.push(clone_function_declaration(func));
        let prototype = object_with_proto(JsValue::Null);
        JsValue::Function(Rc::new(FunctionValue {
            id,
            env,
            prototype,
        }))
    }

    fn call_function_value(
        &mut self,
        function: Rc<FunctionValue>,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, RuntimeError> {
        let declaration = self
            .functions
            .get(function.id)
            .cloned()
            .ok_or_else(|| RuntimeError::TypeError("function body missing".into()))?;
        let call_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&function.env)))));
        call_env
            .borrow_mut()
            .define("this".to_string(), this_value);

        let mut arg_index = 0;
        for param in &declaration.params {
            match param {
                Param::Simple(name) => {
                    let value = args.get(arg_index).cloned().unwrap_or(JsValue::Undefined);
                    call_env.borrow_mut().define(name.to_string(), value);
                    arg_index += 1;
                }
                Param::Rest(name) => {
                    let rest: Vec<JsValue> = args[arg_index..].to_vec();
                    call_env.borrow_mut().define(
                        name.to_string(),
                        JsValue::Array(Rc::new(RefCell::new(rest))),
                    );
                    break;
                }
                Param::Default(name, default_expr) => {
                    let value = match args.get(arg_index) {
                        Some(JsValue::Undefined) | None => {
                            self.eval_expression(default_expr, Rc::clone(&call_env))?
                        }
                        Some(v) => v.clone(),
                    };
                    call_env.borrow_mut().define(name.to_string(), value);
                    arg_index += 1;
                }
            }
        }

        match self.eval_statement(&Statement::BlockStatement(declaration.body.clone()), call_env) {
            Ok(value) => Ok(value),
            Err(RuntimeError::Return(value)) => Ok(value),
            Err(error) => Err(error),
        }
    }

    pub fn eval_program(&mut self, program: &Program) -> Result<JsValue, RuntimeError> {
        let mut last_val = JsValue::Undefined;
        for stmt in &program.body {
            match self.eval_statement(stmt, Rc::clone(&self.global_env)) {
                Ok(val) => {
                    last_val = val;
                }
                Err(RuntimeError::Return(val)) => return Ok(val),
                Err(e) => return Err(e),
            }
        }
        Ok(last_val)
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
                    env.borrow_mut().define(d.id.to_string(), val.clone());
                }
                Ok(JsValue::Undefined)
            }
            Statement::ExpressionStatement(expr) => self.eval_expression(expr, env),
            Statement::BlockStatement(block) => {
                let block_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                let mut last_val = JsValue::Undefined;
                for s in &block.body {
                    self.check_timeout()?;
                    last_val = self.eval_statement(s, Rc::clone(&block_env))?;
                }
                Ok(last_val)
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
            Statement::WhileStatement(while_stmt) => {
                let mut last_val = JsValue::Undefined;
                loop {
                    self.check_timeout()?;
                    let test_val = self.eval_expression(&while_stmt.test, Rc::clone(&env))?;
                    if !test_val.is_truthy() {
                        break;
                    }
                    match self.eval_statement(&while_stmt.body, Rc::clone(&env)) {
                        Ok(val) => last_val = val,
                        Err(RuntimeError::Break(None)) => break,
                        Err(RuntimeError::Continue(None)) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(last_val)
            }
            Statement::DoWhileStatement(while_stmt) => {
                let mut last_val = JsValue::Undefined;
                loop {
                    self.check_timeout()?;
                    match self.eval_statement(&while_stmt.body, Rc::clone(&env)) {
                        Ok(val) => last_val = val,
                        Err(RuntimeError::Break(None)) => break,
                        Err(RuntimeError::Continue(None)) => {}
                        Err(e) => return Err(e),
                    }
                    let test_val = self.eval_expression(&while_stmt.test, Rc::clone(&env))?;
                    if !test_val.is_truthy() {
                        break;
                    }
                }
                Ok(last_val)
            }
            Statement::ForStatement(for_stmt) => {
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
                        Err(RuntimeError::Break(None)) => break,
                        Err(RuntimeError::Continue(None)) => {}
                        Err(e) => return Err(e),
                    }
                    if let Some(update) = &for_stmt.update {
                        self.eval_expression(update, Rc::clone(&for_env))?;
                    }
                }
                Ok(last_val)
            }
            Statement::ForInStatement(for_in) => {
                let right = self.eval_expression(&for_in.right, Rc::clone(&env))?;
                let keys: Vec<String> = match &right {
                    JsValue::Object(map) => map.borrow().keys().cloned().collect(),
                    JsValue::Array(arr) => (0..arr.borrow().len()).map(|i| i.to_string()).collect(),
                    _ => vec![],
                };
                let binding_name = extract_for_binding(&for_in.left);
                let mut last_val = JsValue::Undefined;
                for key in keys {
                    self.check_timeout()?;
                    let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                    if let Some(name) = &binding_name {
                        iter_env.borrow_mut().define(name.clone(), JsValue::String(key));
                    }
                    match self.eval_statement(&for_in.body, Rc::clone(&iter_env)) {
                        Ok(val) => last_val = val,
                        Err(RuntimeError::Break(None)) => break,
                        Err(RuntimeError::Continue(None)) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(last_val)
            }
            Statement::ForOfStatement(for_of) => {
                let right = self.eval_expression(&for_of.right, Rc::clone(&env))?;
                let items: Vec<JsValue> = match &right {
                    JsValue::Array(arr) => arr.borrow().clone(),
                    JsValue::String(s) => s.chars().map(|c| JsValue::String(c.to_string())).collect(),
                    _ => vec![],
                };
                let binding_name = extract_for_binding(&for_of.left);
                let mut last_val = JsValue::Undefined;
                for item in items {
                    self.check_timeout()?;
                    let iter_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&env)))));
                    if let Some(name) = &binding_name {
                        iter_env.borrow_mut().define(name.clone(), item);
                    }
                    match self.eval_statement(&for_of.body, Rc::clone(&iter_env)) {
                        Ok(val) => last_val = val,
                        Err(RuntimeError::Break(None)) => break,
                        Err(RuntimeError::Continue(None)) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(last_val)
            }
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
            Statement::LabeledStatement(labeled) => {
                match self.eval_statement(&labeled.body, Rc::clone(&env)) {
                    Err(RuntimeError::Break(Some(ref l))) if l == labeled.label => Ok(JsValue::Undefined),
                    other => other,
                }
            }            Statement::TryStatement(try_stmt) => {
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
                            if let Some(param) = handler.param {
                                let err_val = match e {
                                    RuntimeError::Throw(v) => v,
                                    err => JsValue::String(err.to_string()),
                                };
                                catch_env.borrow_mut().define(param.to_string(), err_val);
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
                Literal::BigInt(n) => Ok(JsValue::Number(*n as f64)),
                Literal::RegExp(_, _) => Ok(JsValue::Object(Rc::new(RefCell::new(std::collections::HashMap::new())))),
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
            Expression::AssignmentExpression(assign) => {
                match &assign.left {
                    Expression::Identifier(name) => {
                        let current = env.borrow().get(name).unwrap_or(JsValue::Undefined);
                        let should_assign = match assign.operator {
                            AssignmentOperator::LogicAndAssign => current.is_truthy(),
                            AssignmentOperator::LogicOrAssign => !current.is_truthy(),
                            AssignmentOperator::NullishAssign => {
                                matches!(current, JsValue::Undefined | JsValue::Null)
                            }
                            _ => true,
                        };

                        if !should_assign {
                            return Ok(current);
                        }

                        let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        if env.borrow_mut().set(name, value.clone()).is_err() {
                            env.borrow_mut().define(name.to_string(), value.clone());
                        }
                        Ok(value)
                    }
                    Expression::MemberExpression(mem) => {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                        let current = match &object {
                            JsValue::Object(values) => values
                                .borrow()
                                .get(&property_key)
                                .cloned()
                                .unwrap_or(JsValue::Undefined),
                            JsValue::Array(values) => {
                                let values = values.borrow();
                                if property_key == "length" {
                                    JsValue::Number(values.len() as f64)
                                } else {
                                    match property_key.parse::<usize>() {
                                        Ok(index) => values
                                            .get(index)
                                            .cloned()
                                            .unwrap_or(JsValue::Undefined),
                                        Err(_) => {
                                            return Err(RuntimeError::TypeError(
                                                "array assignment requires a non-negative integer index"
                                                    .into(),
                                            ))
                                        }
                                    }
                                }
                            }
                            _ => return Err(RuntimeError::TypeError("value is not an object".into())),
                        };

                        let should_assign = match assign.operator {
                            AssignmentOperator::LogicAndAssign => current.is_truthy(),
                            AssignmentOperator::LogicOrAssign => !current.is_truthy(),
                            AssignmentOperator::NullishAssign => {
                                matches!(current, JsValue::Undefined | JsValue::Null)
                            }
                            _ => true,
                        };

                        if !should_assign {
                            return Ok(current);
                        }

                        let right = self.eval_expression(&assign.right, env)?;
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        self.write_member_value(object, &property_key, value)
                    }
                    _ => Ok(JsValue::Undefined),
                }
            }
            Expression::BinaryExpression(bin) => {
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
                        Ok(JsValue::Number(left.as_number() % right.as_number()))
                    }
                    BinaryOperator::BitAnd => Ok(JsValue::Number(
                        (self.to_int32(&left) & self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::BitOr => Ok(JsValue::Number(
                        (self.to_int32(&left) | self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::BitXor => Ok(JsValue::Number(
                        (self.to_int32(&left) ^ self.to_int32(&right)) as f64,
                    )),
                    BinaryOperator::ShiftLeft => {
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) << shift) as f64))
                    }
                    BinaryOperator::ShiftRight => {
                        let shift = self.to_uint32(&right) & 0x1f;
                        Ok(JsValue::Number((self.to_int32(&left) >> shift) as f64))
                    }
                    BinaryOperator::LogicalShiftRight => {
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
                    BinaryOperator::Power => Ok(JsValue::Number(left.as_number().powf(right.as_number()))),
                    BinaryOperator::Instanceof => match (&left, &right) {
                        (JsValue::Object(object), JsValue::Function(function)) => {
                            let prototype = function.prototype.clone();
                            let mut current = object.borrow().get("__proto__").cloned();
                            while let Some(value) = current {
                                if js_strict_eq(&value, &prototype) {
                                    return Ok(JsValue::Boolean(true));
                                }
                                current = match value {
                                    JsValue::Object(proto) => proto.borrow().get("__proto__").cloned(),
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
                            _ => Err(RuntimeError::TypeError("right-hand side of 'in' is not an object".into())),
                        }
                    }
                    _ => Ok(JsValue::Undefined),
                }
            }
            Expression::UnaryExpression(unary) => {
                let arg = self.eval_expression(&unary.argument, env)?;
                match unary.operator {
                    UnaryOperator::Minus => Ok(JsValue::Number(-arg.as_number())),
                    UnaryOperator::Plus => Ok(JsValue::Number(arg.as_number())),
                    UnaryOperator::LogicNot => Ok(JsValue::Boolean(!arg.is_truthy())),
                    UnaryOperator::BitNot => {
                        Ok(JsValue::Number((!self.to_int32(&arg)) as f64))
                    }
                    UnaryOperator::Typeof => Ok(JsValue::String(arg.type_of())),
                    UnaryOperator::Void => Ok(JsValue::Undefined),
                    UnaryOperator::Delete => Ok(JsValue::Boolean(true)), // Stub for now
                }
            }
            Expression::ArrayExpression(elements) => {
                let mut values = Vec::new();
                for element in elements {
                    match element {
                        Some(Expression::SpreadElement(spread_expr)) => {
                            let spread_val = self.eval_expression(spread_expr, Rc::clone(&env))?;
                            match spread_val {
                                JsValue::Array(arr) => values.extend(arr.borrow().clone()),
                                other => values.push(other),
                            }
                        }
                        Some(expr) => values.push(self.eval_expression(expr, Rc::clone(&env))?),
                        None => values.push(JsValue::Undefined),
                    }
                }
                Ok(JsValue::Array(Rc::new(RefCell::new(values))))
            }
            Expression::ObjectExpression(properties) => {
                let mut values = std::collections::HashMap::new();
                for prop in properties {
                    if prop.method {
                        // method shorthand: treat as function value
                        let key = match &prop.key {
                            ObjectKey::Identifier(name) | ObjectKey::String(name) => (*name).to_string(),
                            ObjectKey::Number(n) => n.to_string(),
                            ObjectKey::Computed(expr) => self.eval_expression(expr, Rc::clone(&env))?.as_string(),
                        };
                        let val = self.eval_expression(&prop.value, Rc::clone(&env))?;
                        values.insert(key, val);
                    } else if let Expression::SpreadElement(spread_expr) = &prop.value {
                        // spread: { ...obj }
                        let spread_val = self.eval_expression(spread_expr, Rc::clone(&env))?;
                        if let JsValue::Object(map) = spread_val {
                            for (k, v) in map.borrow().iter() {
                                values.insert(k.clone(), v.clone());
                            }
                        }
                    } else {
                        let key = match &prop.key {
                            ObjectKey::Identifier(name) | ObjectKey::String(name) => (*name).to_string(),
                            ObjectKey::Number(n) => n.to_string(),
                            ObjectKey::Computed(expr) => self.eval_expression(expr, Rc::clone(&env))?.as_string(),
                        };
                        let val = self.eval_expression(&prop.value, Rc::clone(&env))?;
                        values.insert(key, val);
                    }
                }
                Ok(JsValue::Object(Rc::new(RefCell::new(values))))
            }
            Expression::MemberExpression(mem) => {
                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(mem, env)?;

                match object {
                    JsValue::Object(values) => Ok(get_object_property(&values, &property_key)),
                    JsValue::Array(values) => {
                        let values = values.borrow();
                        if property_key == "length" {
                            return Ok(JsValue::Number(values.len() as f64));
                        }
                        match property_key.parse::<usize>() {
                            Ok(index) => Ok(values.get(index).cloned().unwrap_or(JsValue::Undefined)),
                            Err(_) => Ok(JsValue::Undefined),
                        }
                    }
                    JsValue::String(s) => {
                        match property_key.as_str() {
                            "length" => Ok(JsValue::Number(s.chars().count() as f64)),
                            _ => {
                                if let Ok(index) = property_key.parse::<usize>() {
                                    Ok(s.chars().nth(index)
                                        .map(|c| JsValue::String(c.to_string()))
                                        .unwrap_or(JsValue::Undefined))
                                } else {
                                    Ok(JsValue::Undefined)
                                }
                            }
                        }
                    }
                    JsValue::Function(function) => Ok(match property_key.as_str() {
                        "prototype" => function.prototype.clone(),
                        _ => JsValue::Undefined,
                    }),
                    _ => Err(RuntimeError::TypeError("value is not an object".into())),
                }
            }
            Expression::CallExpression(call) => {
                let (callee, this_value) = match &call.callee {
                    Expression::MemberExpression(mem) => {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = self.member_property_key(mem, Rc::clone(&env))?;
                        let callee = match &object {
                            JsValue::Object(values) => get_object_property(values, &property_key),
                            _ => object.get_property(&property_key),
                        };
                        (callee, object)
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
                            match spread_val {
                                JsValue::Array(arr) => args.extend(arr.borrow().clone()),
                                other => args.push(other),
                            }
                        }
                        expr => args.push(self.eval_expression(expr, Rc::clone(&env))?),
                    }
                }

                match callee {
                    JsValue::Function(function) => self.call_function_value(function, this_value, args),
                    _ => Err(RuntimeError::TypeError("value is not callable".into())),
                }
            }
            Expression::UpdateExpression(update) => {
                match &update.argument {
                    Expression::Identifier(name) => {
                        let current_val = env.borrow().get(name).unwrap_or(JsValue::Undefined).as_number();
                        let new_val = if update.operator == UpdateOperator::PlusPlus { current_val + 1.0 } else { current_val - 1.0 };
                        env.borrow_mut().set(name, JsValue::Number(new_val)).ok();
                        if update.prefix { Ok(JsValue::Number(new_val)) } else { Ok(JsValue::Number(current_val)) }
                    }
                    Expression::MemberExpression(mem) => {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = if mem.computed {
                            self.eval_expression(&mem.property, Rc::clone(&env))?.as_string()
                        } else if let Expression::Identifier(name) = &mem.property {
                            name.to_string()
                        } else {
                            return Ok(JsValue::Undefined);
                        };
                        let current_val = match &object {
                            JsValue::Object(map) => map.borrow().get(&property_key).cloned().unwrap_or(JsValue::Undefined).as_number(),
                            JsValue::Array(arr) => arr.borrow().get(property_key.parse::<usize>().unwrap_or(usize::MAX)).cloned().unwrap_or(JsValue::Undefined).as_number(),
                            _ => f64::NAN,
                        };
                        let new_val = if update.operator == UpdateOperator::PlusPlus { current_val + 1.0 } else { current_val - 1.0 };
                        self.write_member_value(object, &property_key, JsValue::Number(new_val))?;
                        if update.prefix { Ok(JsValue::Number(new_val)) } else { Ok(JsValue::Number(current_val)) }
                    }
                    _ => Ok(JsValue::Undefined),
                }
            }
            Expression::ArrowFunctionExpression(func) => {
                Ok(self.create_function_value(func, Rc::clone(&env)))
            }
            Expression::ClassExpression(_) => Ok(JsValue::Undefined),
            Expression::FunctionExpression(func) => {
                Ok(self.create_function_value(func, Rc::clone(&env)))
            }
            Expression::ThisExpression => Ok(env.borrow().get("this").unwrap_or(JsValue::Undefined)),
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
                            match spread_val {
                                JsValue::Array(arr) => args.extend(arr.borrow().clone()),
                                other => args.push(other),
                            }
                        }
                        expr => args.push(self.eval_expression(expr, Rc::clone(&env))?),
                    }
                }

                match callee {
                    JsValue::Function(function) => {
                        let instance = object_with_proto(function.prototype.clone());
                        let result = self.call_function_value(Rc::clone(&function), instance.clone(), args)?;
                        match result {
                            JsValue::Object(_) => Ok(result),
                            _ => Ok(instance),
                        }
                    }
                    _ => Err(RuntimeError::TypeError("value is not a constructor".into())),
                }
            },
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
            Expression::YieldExpression(_) => Ok(JsValue::Undefined),
            Expression::AwaitExpression(expr) => self.eval_expression(expr, env),
            Expression::TaggedTemplateExpression(tag, parts) => {
                // Evaluate the template parts and call the tag function
                let tag_val = self.eval_expression(tag, Rc::clone(&env))?;
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
                match tag_val {
                    JsValue::Function(function) => {
                        let declaration = self
                            .functions
                            .get(function.id)
                            .cloned()
                            .ok_or_else(|| RuntimeError::TypeError("tag function body missing".into()))?;
                        let call_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(&function.env)))));
                        for (i, param) in declaration.params.iter().enumerate() {
                            let value = call_args.get(i).cloned().unwrap_or(JsValue::Undefined);
                            call_env.borrow_mut().define(param.name().to_string(), value);
                        }
                        match self.eval_statement(&Statement::BlockStatement(declaration.body.clone()), call_env) {
                            Ok(v) => Ok(v),
                            Err(RuntimeError::Return(v)) => Ok(v),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err(RuntimeError::TypeError("tag is not callable".into())),
                }
            }
        }
    }
}

fn clone_function_declaration(func: &FunctionDeclaration<'_>) -> FunctionDeclaration<'static> {
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
    }
}

fn clone_param(param: &Param<'_>) -> Param<'static> {
    match param {
        Param::Simple(name) => Param::Simple(String::leak(name.to_string())),
        Param::Rest(name) => Param::Rest(String::leak(name.to_string())),
        Param::Default(name, expr) => Param::Default(String::leak(name.to_string()), clone_expression(expr)),
    }
}

fn clone_block_statement(block: &BlockStatement<'_>) -> BlockStatement<'static> {
    BlockStatement {
        body: block.body.iter().map(clone_statement).collect(),
    }
}

fn clone_statement(stmt: &Statement<'_>) -> Statement<'static> {
    match stmt {
        Statement::ExpressionStatement(expr) => Statement::ExpressionStatement(clone_expression(expr)),
        Statement::BlockStatement(block) => Statement::BlockStatement(clone_block_statement(block)),
        Statement::IfStatement(stmt) => Statement::IfStatement(IfStatement {
            test: clone_expression(&stmt.test),
            consequent: Box::new(clone_statement(&stmt.consequent)),
            alternate: stmt.alternate.as_ref().map(|alt| Box::new(clone_statement(alt))),
        }),
        Statement::WhileStatement(stmt) => Statement::WhileStatement(WhileStatement {
            test: clone_expression(&stmt.test),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::ForStatement(stmt) => Statement::ForStatement(ForStatement {
            init: stmt.init.as_ref().map(|init| Box::new(clone_statement(init))),
            test: stmt.test.as_ref().map(clone_expression),
            update: stmt.update.as_ref().map(clone_expression),
            body: Box::new(clone_statement(&stmt.body)),
        }),
        Statement::TryStatement(stmt) => Statement::TryStatement(TryStatement {
            block: clone_block_statement(&stmt.block),
            handler: stmt.handler.as_ref().map(|handler| {
                let param = handler.param.map(|value| {
                    let leaked: &'static mut str = String::leak(value.to_string());
                    leaked as &'static str
                });
                CatchClause {
                    param,
                    body: clone_block_statement(&handler.body),
                }
            }),
            finalizer: stmt.finalizer.as_ref().map(clone_block_statement),
        }),
        Statement::ThrowStatement(expr) => Statement::ThrowStatement(clone_expression(expr)),
        Statement::VariableDeclaration(decl) => Statement::VariableDeclaration(VariableDeclaration {
            kind: decl.kind.clone(),
            declarations: decl
                .declarations
                .iter()
                .map(|decl| VariableDeclarator {
                    id: String::leak(decl.id.to_string()),
                    init: decl.init.as_ref().map(clone_expression),
                })
                .collect(),
        }),
        Statement::FunctionDeclaration(func) => {
            Statement::FunctionDeclaration(clone_function_declaration(func))
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
        }),
        Statement::SwitchStatement(stmt) => Statement::SwitchStatement(SwitchStatement {
            discriminant: clone_expression(&stmt.discriminant),
            cases: stmt.cases.iter().map(|case| SwitchCase {
                test: case.test.as_ref().map(clone_expression),
                consequent: case.consequent.iter().map(clone_statement).collect(),
            }).collect(),
        }),
        Statement::BreakStatement(label) => Statement::BreakStatement(
            label.map(|s| { let l: &'static mut str = String::leak(s.to_string()); l as &'static str })
        ),
        Statement::ContinueStatement(label) => Statement::ContinueStatement(
            label.map(|s| { let l: &'static mut str = String::leak(s.to_string()); l as &'static str })
        ),
        Statement::LabeledStatement(stmt) => Statement::LabeledStatement(LabeledStatement {
            label: { let l: &'static mut str = String::leak(stmt.label.to_string()); l as &'static str },
            body: Box::new(clone_statement(&stmt.body)),
        }),
    }
}

fn clone_expression(expr: &Expression<'_>) -> Expression<'static> {
    match expr {
        Expression::Literal(Literal::Number(n)) => Expression::Literal(Literal::Number(*n)),
        Expression::Literal(Literal::String(s)) => {
            Expression::Literal(Literal::String(String::leak((*s).to_string())))
        }
        Expression::Literal(Literal::Boolean(b)) => Expression::Literal(Literal::Boolean(*b)),
        Expression::Literal(Literal::Null) => Expression::Literal(Literal::Null),
        Expression::Literal(Literal::Undefined) => Expression::Literal(Literal::Undefined),
        Expression::Literal(Literal::BigInt(n)) => Expression::Literal(Literal::BigInt(*n)),
        Expression::Literal(Literal::RegExp(pattern, flags)) => Expression::Literal(Literal::RegExp(
            String::leak((*pattern).to_string()),
            String::leak((*flags).to_string()),
        )),
        Expression::Identifier(name) => Expression::Identifier(String::leak((*name).to_string())),
        Expression::BinaryExpression(expr) => Expression::BinaryExpression(Box::new(BinaryExpression {
            operator: expr.operator.clone(),
            left: clone_expression(&expr.left),
            right: clone_expression(&expr.right),
        })),
        Expression::UnaryExpression(expr) => Expression::UnaryExpression(Box::new(UnaryExpression {
            operator: expr.operator.clone(),
            argument: clone_expression(&expr.argument),
            prefix: expr.prefix,
        })),
        Expression::AssignmentExpression(expr) => Expression::AssignmentExpression(Box::new(AssignmentExpression {
            operator: expr.operator.clone(),
            left: clone_expression(&expr.left),
            right: clone_expression(&expr.right),
        })),
        Expression::MemberExpression(expr) => Expression::MemberExpression(Box::new(MemberExpression {
            object: clone_expression(&expr.object),
            property: clone_expression(&expr.property),
            computed: expr.computed,
            optional: expr.optional,
        })),
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
        Expression::FunctionExpression(func) => {
            Expression::FunctionExpression(Box::new(clone_function_declaration(func)))
        }
        Expression::ArrowFunctionExpression(func) => {
            Expression::ArrowFunctionExpression(Box::new(clone_function_declaration(func)))
        }
        Expression::UpdateExpression(expr) => Expression::UpdateExpression(Box::new(UpdateExpression {
            operator: expr.operator.clone(),
            argument: clone_expression(&expr.argument),
            prefix: expr.prefix,
        })),
        Expression::SequenceExpression(seq) => {
            Expression::SequenceExpression(seq.iter().map(clone_expression).collect())
        }
        Expression::ConditionalExpression { test, consequent, alternate } => {
            Expression::ConditionalExpression {
                test: Box::new(clone_expression(test)),
                consequent: Box::new(clone_expression(consequent)),
                alternate: Box::new(clone_expression(alternate)),
            }
        }
        Expression::ArrayExpression(values) => {
            Expression::ArrayExpression(values.iter().map(|value| value.as_ref().map(clone_expression)).collect())
        }
        Expression::ObjectExpression(props) => Expression::ObjectExpression(
            props.iter().map(|prop| {
                let key = match &prop.key {
                    ObjectKey::Identifier(name) => ObjectKey::Identifier(String::leak((*name).to_string())),
                    ObjectKey::String(name) => ObjectKey::String(String::leak((*name).to_string())),
                    ObjectKey::Number(n) => ObjectKey::Number(*n),
                    ObjectKey::Computed(expr) => ObjectKey::Computed(Box::new(clone_expression(expr))),
                };
                ObjectProperty {
                    key,
                    value: clone_expression(&prop.value),
                    shorthand: prop.shorthand,
                    computed: prop.computed,
                    method: prop.method,
                }
            }).collect(),
        ),
        Expression::ClassExpression(expr) => {
            let id = expr.id.map(|value| {
                let leaked: &'static mut str = String::leak(value.to_string());
                leaked as &'static str
            });
            Expression::ClassExpression(Box::new(ClassDeclaration { id }))
        }
        Expression::ThisExpression => Expression::ThisExpression,
        Expression::SpreadElement(expr) => Expression::SpreadElement(Box::new(clone_expression(expr))),
        Expression::TemplateLiteral(parts) => Expression::TemplateLiteral(
            parts.iter().map(|part| match part {
                TemplatePart::String(s) => TemplatePart::String(String::leak(s.to_string())),
                TemplatePart::Expr(expr) => TemplatePart::Expr(clone_expression(expr)),
            }).collect()
        ),
        Expression::YieldExpression(expr) => Expression::YieldExpression(expr.as_ref().map(|e| Box::new(clone_expression(e)))),
        Expression::AwaitExpression(expr) => Expression::AwaitExpression(Box::new(clone_expression(expr))),
        Expression::TaggedTemplateExpression(tag, parts) => Expression::TaggedTemplateExpression(
            Box::new(clone_expression(tag)),
            parts.iter().map(|part| match part {
                TemplatePart::String(s) => TemplatePart::String(String::leak(s.to_string())),
                TemplatePart::Expr(expr) => TemplatePart::Expr(clone_expression(expr)),
            }).collect()
        ),
    }
}

impl Drop for Interpreter {
    fn drop(&mut self) {
        self.global_env.borrow_mut().variables.clear();
    }
}

fn js_strict_eq(left: &JsValue, right: &JsValue) -> bool {
    match (left, right) {
        (JsValue::Undefined, JsValue::Undefined) => true,
        (JsValue::Null, JsValue::Null) => true,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        (JsValue::Number(a), JsValue::Number(b)) => a == b,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
        (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
        (JsValue::Function(a), JsValue::Function(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn js_abstract_eq(left: &JsValue, right: &JsValue) -> bool {
    match (left, right) {
        (JsValue::Null, JsValue::Undefined) | (JsValue::Undefined, JsValue::Null) => true,
        (JsValue::Number(a), JsValue::String(b)) => *a == b.parse::<f64>().unwrap_or(f64::NAN),
        (JsValue::String(a), JsValue::Number(b)) => a.parse::<f64>().unwrap_or(f64::NAN) == *b,
        (JsValue::Boolean(a), _) => js_abstract_eq(&JsValue::Number(if *a { 1.0 } else { 0.0 }), right),
        (_, JsValue::Boolean(b)) => js_abstract_eq(left, &JsValue::Number(if *b { 1.0 } else { 0.0 })),
        _ => js_strict_eq(left, right),
    }
}

fn extract_for_binding(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            decl.declarations.first().map(|d| d.id.to_string())
        }
        Statement::ExpressionStatement(Expression::Identifier(name)) => Some(name.to_string()),
        _ => None,
    }
}
