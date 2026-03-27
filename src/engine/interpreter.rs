use crate::engine::env::Environment;
use crate::engine::value::{FunctionValue, JsValue};
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
    #[error("Return: {0:?}")] // Control flow for return
    Return(JsValue),
    #[error("Throw: {0:?}")] // Control flow for throw
    Throw(JsValue),
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
                    last_val = self.eval_statement(&while_stmt.body, Rc::clone(&env))?;
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
                    last_val = self.eval_statement(&for_stmt.body, Rc::clone(&for_env))?;
                    if let Some(update) = &for_stmt.update {
                        self.eval_expression(update, Rc::clone(&for_env))?;
                    }
                }
                Ok(last_val)
            }
            Statement::TryStatement(try_stmt) => {
                let res = self.eval_statement(
                    &Statement::BlockStatement(try_stmt.block.clone()),
                    Rc::clone(&env),
                );
                let mut final_val = match res {
                    Ok(val) => Ok(val),
                    Err(RuntimeError::Return(v)) => Err(RuntimeError::Return(v)),
                    Err(RuntimeError::Timeout) => Err(RuntimeError::Timeout),
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
                    let id = self.functions.len();
                    self.functions.push(clone_function_declaration(func));
                    let function = JsValue::Function(Rc::new(FunctionValue {
                        id,
                        env: Rc::clone(&env),
                    }));
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
            },
            Expression::Identifier(name) => {
                Ok(env.borrow().get(name).unwrap_or(JsValue::Undefined))
            }
            Expression::AssignmentExpression(assign) => {
                let right = self.eval_expression(&assign.right, Rc::clone(&env))?;
                match &assign.left {
                    Expression::Identifier(name) => {
                        let current = env.borrow().get(name).unwrap_or(JsValue::Undefined);
                        let value = self.assignment_result(&assign.operator, &current, &right)?;
                        if env.borrow_mut().set(name, value.clone()).is_err() {
                            env.borrow_mut().define(name.to_string(), value.clone());
                        }
                        Ok(value)
                    }
                    Expression::MemberExpression(mem) => {
                        let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                        let property_key = self.member_property_key(mem, env)?;
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
                    BinaryOperator::EqEq => Ok(JsValue::Boolean(left == right)), // Needs abstract equality
                    BinaryOperator::EqEqEq => Ok(JsValue::Boolean(left == right)),
                    BinaryOperator::NotEq => Ok(JsValue::Boolean(left != right)),
                    BinaryOperator::NotEqEq => Ok(JsValue::Boolean(left != right)),
                    BinaryOperator::Less => left.lt(&right),
                    BinaryOperator::LessEq => left.le(&right),
                    BinaryOperator::Greater => left.gt(&right),
                    BinaryOperator::GreaterEq => left.ge(&right),
                    _ => Ok(JsValue::Undefined),
                }
            }
            Expression::UnaryExpression(unary) => {
                let arg = self.eval_expression(&unary.argument, env)?;
                match unary.operator {
                    UnaryOperator::Minus => {
                        if let JsValue::Number(n) = arg {
                            Ok(JsValue::Number(-n))
                        } else {
                            Ok(JsValue::Number(f64::NAN))
                        }
                    }
                    UnaryOperator::Plus => {
                        // Cast to number
                        if let JsValue::Number(n) = arg {
                            Ok(JsValue::Number(n))
                        } else {
                            Ok(JsValue::Number(f64::NAN))
                        }
                    }
                    UnaryOperator::LogicNot => Ok(JsValue::Boolean(!arg.is_truthy())),
                    UnaryOperator::Typeof => Ok(JsValue::String(arg.type_of())),
                    UnaryOperator::Void => Ok(JsValue::Undefined),
                    UnaryOperator::Delete => Ok(JsValue::Boolean(true)), // Stub for now
                }
            }
            Expression::ArrayExpression(elements) => {
                let values = elements
                    .iter()
                    .map(|element| match element {
                        Some(expr) => self.eval_expression(expr, Rc::clone(&env)),
                        None => Ok(JsValue::Undefined),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(JsValue::Array(Rc::new(RefCell::new(values))))
            }
            Expression::ObjectExpression(properties) => {
                let mut values = std::collections::HashMap::new();
                for (key, value) in properties {
                    let key = match key {
                        ObjectKey::Identifier(name) | ObjectKey::String(name) => (*name).to_string(),
                    };
                    values.insert(key, self.eval_expression(value, Rc::clone(&env))?);
                }
                Ok(JsValue::Object(Rc::new(RefCell::new(values))))
            }
            Expression::MemberExpression(mem) => {
                let object = self.eval_expression(&mem.object, Rc::clone(&env))?;
                let property_key = self.member_property_key(mem, env)?;

                match object {
                    JsValue::Object(values) => Ok(values
                        .borrow()
                        .get(&property_key)
                        .cloned()
                        .unwrap_or(JsValue::Undefined)),
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
                    _ => Err(RuntimeError::TypeError("value is not an object".into())),
                }
            }
            Expression::CallExpression(call) => {
                let callee = self.eval_expression(&call.callee, Rc::clone(&env))?;
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.eval_expression(arg, Rc::clone(&env)))
                    .collect::<Result<Vec<_>, _>>()?;

                match callee {
                    JsValue::Function(function) => {
                        let declaration = self
                            .functions
                            .get(function.id)
                            .cloned()
                            .ok_or_else(|| RuntimeError::TypeError("function body missing".into()))?;
                        let call_env = Rc::new(RefCell::new(Environment::new(Some(Rc::clone(
                            &function.env,
                        )))));
                        for (index, param) in declaration.params.iter().enumerate() {
                            let value = args.get(index).cloned().unwrap_or(JsValue::Undefined);
                            call_env.borrow_mut().define(param.to_string(), value);
                        }
                        match self.eval_statement(
                            &Statement::BlockStatement(declaration.body.clone()),
                            call_env,
                        ) {
                            Ok(value) => Ok(value),
                            Err(RuntimeError::Return(value)) => Ok(value),
                            Err(error) => Err(error),
                        }
                    }
                    _ => Err(RuntimeError::TypeError("value is not callable".into())),
                }
            }
            Expression::UpdateExpression(update) => {
                let id = match &update.argument {
                    Expression::Identifier(name) => name,
                    _ => return Ok(JsValue::Undefined),
                };
                let current_val = env
                    .borrow()
                    .get(id)
                    .unwrap_or(JsValue::Undefined)
                    .as_number();
                let new_val = if update.operator == crate::parser::ast::UpdateOperator::PlusPlus {
                    current_val + 1.0
                } else {
                    current_val - 1.0
                };
                env.borrow_mut().set(id, JsValue::Number(new_val)).ok();
                if update.prefix {
                    Ok(JsValue::Number(new_val))
                } else {
                    Ok(JsValue::Number(current_val))
                }
            }
            Expression::ArrowFunctionExpression(func) => {
                let id = self.functions.len();
                self.functions.push(clone_function_declaration(func));
                Ok(JsValue::Function(Rc::new(FunctionValue {
                    id,
                    env: Rc::clone(&env),
                })))
            }
            Expression::ClassExpression(_) => Ok(JsValue::Undefined),
            Expression::FunctionExpression(func) => {
                let id = self.functions.len();
                self.functions.push(clone_function_declaration(func));
                Ok(JsValue::Function(Rc::new(FunctionValue {
                    id,
                    env: Rc::clone(&env),
                })))
            }
            Expression::ThisExpression => Ok(JsValue::Undefined),
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
            Expression::NewExpression(_new_exp) => Ok(JsValue::Undefined),
        }
    }
}

fn clone_function_declaration(func: &FunctionDeclaration<'_>) -> FunctionDeclaration<'static> {
    let id = func.id.map(|value| {
        let leaked: &'static mut str = String::leak(value.to_string());
        leaked as &'static str
    });
    let params = func
        .params
        .iter()
        .map(|param| {
            let leaked: &'static mut str = String::leak((*param).to_string());
            leaked as &'static str
        })
        .collect();
    FunctionDeclaration {
        id,
        params,
        body: clone_block_statement(&func.body),
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
        })),
        Expression::CallExpression(expr) => Expression::CallExpression(Box::new(CallExpression {
            callee: clone_expression(&expr.callee),
            arguments: expr.arguments.iter().map(clone_expression).collect(),
        })),
        Expression::NewExpression(expr) => Expression::NewExpression(Box::new(CallExpression {
            callee: clone_expression(&expr.callee),
            arguments: expr.arguments.iter().map(clone_expression).collect(),
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
        Expression::ObjectExpression(values) => Expression::ObjectExpression(
            values
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        ObjectKey::Identifier(name) => {
                            ObjectKey::Identifier(String::leak((*name).to_string()))
                        }
                        ObjectKey::String(name) => {
                            ObjectKey::String(String::leak((*name).to_string()))
                        }
                    };
                    (key, clone_expression(value))
                })
                .collect(),
        ),
        Expression::ClassExpression(expr) => {
            let id = expr.id.map(|value| {
                let leaked: &'static mut str = String::leak(value.to_string());
                leaked as &'static str
            });
            Expression::ClassExpression(Box::new(ClassDeclaration { id }))
        }
        Expression::ThisExpression => Expression::ThisExpression,
    }
}

impl Drop for Interpreter {
    fn drop(&mut self) {
        self.global_env.borrow_mut().variables.clear();
    }
}
