use crate::engine::env::Environment;
use crate::engine::value::JsValue;
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
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            global_env: Rc::new(RefCell::new(Environment::new(None))),
            instruction_count: 0,
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
            Statement::FunctionDeclaration(_func) => {
                // A very simplified function representation via Environment.
                // We will store AST nodes in Environment instead in later patches, for now skip.
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
                let val = self.eval_expression(&assign.right, Rc::clone(&env))?;
                if let Expression::Identifier(name) = &assign.left {
                    // Try to set, if it fails, just define locally
                    if env.borrow_mut().set(name, val.clone()).is_err() {
                        env.borrow_mut().define(name.to_string(), val.clone());
                    }
                    Ok(val)
                } else {
                    Ok(JsValue::Undefined)
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
            Expression::ArrayExpression(_elements) => {
                // Return dummy array for now. Proper Array needs object/heap management.
                Ok(JsValue::Undefined)
            }
            Expression::ObjectExpression(_properties) => {
                // Return dummy object
                Ok(JsValue::Undefined)
            }
            Expression::MemberExpression(_mem) => {
                // Stub
                Ok(JsValue::Undefined)
            }
            Expression::CallExpression(_call) => {
                // Stub
                Ok(JsValue::Undefined)
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
            Expression::ArrowFunctionExpression(_) => Ok(JsValue::Undefined),
            Expression::ClassExpression(_) => Ok(JsValue::Undefined),
            Expression::FunctionExpression(_func) => Ok(JsValue::Undefined),
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

impl Drop for Interpreter {
    fn drop(&mut self) {
        self.global_env.borrow_mut().variables.clear();
    }
}
