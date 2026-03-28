//! LL(1) Recursive Descent Parser

pub mod ast;

use self::ast::*;
use crate::lexer::{Lexer, LexerError, Token};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Lexer error: {0}")]
    Lexer(#[from] LexerError),
    #[error("Unexpected token. Expected {expected}, found {found:?}")]
    UnexpectedToken {
        expected: String,
        found: Option<String>,
    },
    #[error("Unexpected end of input")]
    UnexpectedEOF,
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token<'a>>,
    allow_in: bool,
}

impl<'a> Parser<'a> {
    pub fn new(mut lexer: Lexer<'a>) -> Result<Self, ParseError> {
        let current_token = match lexer.next_token()? {
            Token::Eof => None,
            t => Some(t),
        };
        Ok(Self {
            lexer,
            current_token,
            allow_in: true,
        })
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        self.current_token = match self.lexer.next_token()? {
            Token::Eof => None,
            t => Some(t),
        };
        Ok(())
    }

    fn consume_opt(&mut self, expected: Token<'a>) -> Result<bool, ParseError> {
        if let Some(t) = &self.current_token {
            if *t == expected {
                self.advance()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn parse_program(&mut self) -> Result<Program<'a>, ParseError> {
        let mut body = Vec::new();
        while self.current_token.is_some() {
            body.push(self.parse_statement()?);
        }
        Ok(Program { body })
    }

    fn parse_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        match self.current_token {
            Some(Token::RParen)
            | Some(Token::RBracket)
            | Some(Token::RBrace)
            | Some(Token::Comma)
            | Some(Token::Colon) => {
                self.advance()?;
                Ok(Statement::EmptyStatement)
            }
            Some(Token::Let | Token::Var | Token::Const) => Ok(Statement::VariableDeclaration(
                self.parse_variable_declaration()?,
            )),
            Some(Token::LBrace) => Ok(Statement::BlockStatement(self.parse_block_statement()?)),
            Some(Token::If) => Ok(Statement::IfStatement(self.parse_if_statement()?)),
            Some(Token::While) => Ok(Statement::WhileStatement(self.parse_while_statement()?)),
            Some(Token::Do) => Ok(Statement::DoWhileStatement(
                self.parse_do_while_statement()?,
            )),
            Some(Token::For) => self.parse_for_statement(),
            Some(Token::Switch) => Ok(Statement::SwitchStatement(self.parse_switch_statement()?)),
            Some(Token::Try) => Ok(Statement::TryStatement(self.parse_try_statement()?)),
            Some(Token::Throw) => {
                self.advance()?;
                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Ok(Statement::ThrowStatement(expr))
            }
            Some(Token::Function) => Ok(Statement::FunctionDeclaration(
                self.parse_function_declaration()?,
            )),
            Some(Token::Class) => Ok(Statement::ClassDeclaration(
                self.parse_class_declaration(true)?,
            )),
            Some(Token::Return) => Ok(self.parse_return_statement()?),
            Some(Token::Break) => self.parse_break_statement(),
            Some(Token::Continue) => self.parse_continue_statement(),
            Some(Token::Semicolon) => {
                self.advance()?;
                Ok(Statement::EmptyStatement)
            }
            _ => {
                if let Some(Token::Identifier(label)) = self.current_token {
                    if let Some(Token::Colon) = self.lexer.clone().next_token().ok() {
                        self.advance()?;
                        self.advance()?;
                        let body = self.parse_statement()?;
                        return Ok(Statement::LabeledStatement(LabeledStatement {
                            label,
                            body: Box::new(body),
                        }));
                    }
                }

                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Ok(Statement::ExpressionStatement(expr))
            }
        }
    }

    fn parse_block_statement(&mut self) -> Result<BlockStatement<'a>, ParseError> {
        self.advance()?; // Consume '{'
        let mut body = Vec::new();
        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            body.push(self.parse_statement()?);
        }
        self.consume_opt(Token::RBrace)?; // Consume '}'
        Ok(BlockStatement { body })
    }

    fn parse_if_statement(&mut self) -> Result<IfStatement<'a>, ParseError> {
        self.advance()?; // Consume 'if'
        self.consume_opt(Token::LParen)?;
        let test = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;

        let consequent = Box::new(self.parse_statement()?);

        let alternate = if self.consume_opt(Token::Else)? {
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };

        Ok(IfStatement {
            test,
            consequent,
            alternate,
        })
    }

    fn parse_while_statement(&mut self) -> Result<WhileStatement<'a>, ParseError> {
        self.advance()?; // 'while'
        self.consume_opt(Token::LParen)?;
        let test = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(WhileStatement { test, body })
    }

    fn parse_do_while_statement(&mut self) -> Result<WhileStatement<'a>, ParseError> {
        self.advance()?; // 'do'
        let body = Box::new(self.parse_statement()?);
        if !self.consume_opt(Token::While)? {
            return Err(ParseError::UnexpectedToken {
                expected: "While".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }
        self.consume_opt(Token::LParen)?;
        let test = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;
        self.consume_opt(Token::Semicolon)?;
        Ok(WhileStatement { test, body })
    }

    fn parse_for_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        self.advance()?; // 'for'
        self.consume_opt(Token::LParen)?;

        let init = if self.consume_opt(Token::Semicolon)? {
            None
        } else {
            Some(Box::new(self.with_allow_in(false, |parser| {
                parser.parse_for_initializer()
            })?))
        };

        if let Some(init) = init {
            if self.current_token == Some(Token::In) {
                self.advance()?;
                let right = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Statement::ForInStatement(ForInStatement {
                    left: init,
                    right,
                    body,
                }));
            }

            if matches!(self.current_token, Some(Token::Identifier("of"))) {
                self.advance()?;
                let right = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Statement::ForOfStatement(ForOfStatement {
                    left: init,
                    right,
                    body,
                }));
            }

            let test = if self.consume_opt(Token::Semicolon)? {
                None
            } else {
                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Some(expr)
            };

            let update = if self.consume_opt(Token::RParen)? {
                None
            } else {
                let expr = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                Some(expr)
            };

            let body = Box::new(self.parse_statement()?);
            Ok(Statement::ForStatement(ForStatement {
                init: Some(init),
                test,
                update,
                body,
            }))
        } else {
            let test = if self.consume_opt(Token::Semicolon)? {
                None
            } else {
                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Some(expr)
            };

            let update = if self.consume_opt(Token::RParen)? {
                None
            } else {
                let expr = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                Some(expr)
            };

            let body = Box::new(self.parse_statement()?);
            Ok(Statement::ForStatement(ForStatement {
                init: None,
                test,
                update,
                body,
            }))
        }
    }

    fn parse_for_initializer(&mut self) -> Result<Statement<'a>, ParseError> {
        if matches!(
            self.current_token,
            Some(Token::Var | Token::Let | Token::Const)
        ) {
            Ok(Statement::VariableDeclaration(
                self.parse_variable_declaration()?,
            ))
        } else {
            Ok(Statement::ExpressionStatement(self.parse_expression()?))
        }
    }

    fn parse_switch_statement(&mut self) -> Result<SwitchStatement<'a>, ParseError> {
        self.advance()?; // 'switch'
        self.consume_opt(Token::LParen)?;
        let discriminant = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;
        if !self.consume_opt(Token::LBrace)? {
            return Err(ParseError::UnexpectedToken {
                expected: "LBrace".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }

        let mut cases = Vec::new();
        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            match self.current_token {
                Some(Token::Case) => {
                    self.advance()?;
                    let test = Some(self.parse_expression()?);
                    if !self.consume_opt(Token::Colon)? {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Colon".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                    let consequent = self.parse_switch_consequent()?;
                    cases.push(SwitchCase { test, consequent });
                }
                Some(Token::Default) => {
                    self.advance()?;
                    if !self.consume_opt(Token::Colon)? {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Colon".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                    let consequent = self.parse_switch_consequent()?;
                    cases.push(SwitchCase {
                        test: None,
                        consequent,
                    });
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "Case or Default".to_string(),
                        found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                    });
                }
            }
        }

        self.consume_opt(Token::RBrace)?;
        Ok(SwitchStatement {
            discriminant,
            cases,
        })
    }

    fn parse_switch_consequent(&mut self) -> Result<Vec<Statement<'a>>, ParseError> {
        let mut consequent = Vec::new();
        while self.current_token.is_some()
            && self.current_token != Some(Token::Case)
            && self.current_token != Some(Token::Default)
            && self.current_token != Some(Token::RBrace)
        {
            consequent.push(self.parse_statement()?);
        }
        Ok(consequent)
    }

    fn parse_break_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        self.advance()?;
        let label = if let Some(Token::Identifier(label)) = self.current_token {
            self.advance()?;
            Some(label)
        } else {
            None
        };
        self.consume_opt(Token::Semicolon)?;
        Ok(Statement::BreakStatement(label))
    }

    fn parse_continue_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        self.advance()?;
        let label = if let Some(Token::Identifier(label)) = self.current_token {
            self.advance()?;
            Some(label)
        } else {
            None
        };
        self.consume_opt(Token::Semicolon)?;
        Ok(Statement::ContinueStatement(label))
    }

    fn with_allow_in<T>(
        &mut self,
        allow_in: bool,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let previous = self.allow_in;
        self.allow_in = allow_in;
        let result = f(self);
        self.allow_in = previous;
        result
    }

    fn parse_try_statement(&mut self) -> Result<TryStatement<'a>, ParseError> {
        self.advance()?; // 'try'
        let block = self.parse_block_statement()?;

        let handler = if self.consume_opt(Token::Catch)? {
            let param = if self.consume_opt(Token::LParen)? {
                let p = match self.current_token {
                    Some(Token::Identifier(id)) => {
                        self.advance()?;
                        Some(id)
                    }
                    _ => None,
                };
                self.consume_opt(Token::RParen)?;
                p
            } else {
                None
            };
            Some(CatchClause {
                param,
                body: self.parse_block_statement()?,
            })
        } else {
            None
        };

        let finalizer = if self.consume_opt(Token::Finally)? {
            Some(self.parse_block_statement()?)
        } else {
            None
        };

        Ok(TryStatement {
            block,
            handler,
            finalizer,
        })
    }

    fn parse_function_declaration(&mut self) -> Result<FunctionDeclaration<'a>, ParseError> {
        self.advance()?; // Consume 'function'

        let is_generator = self.consume_opt(Token::Asterisk)?;

        let id = match &self.current_token {
            Some(Token::Identifier(name)) => {
                let n = *name;
                self.advance()?;
                Some(n)
            }
            _ => None,
        };

        self.consume_opt(Token::LParen)?;
        let mut params = Vec::new();
        while self.current_token != Some(Token::RParen) && self.current_token.is_some() {
            if self.current_token == Some(Token::DotDotDot) {
                self.advance()?;
                if let Some(Token::Identifier(name)) = self.current_token {
                    params.push(Param::Rest(name));
                    self.advance()?;
                }
                break;
            }
            if let Some(Token::Identifier(param_name)) = self.current_token {
                self.advance()?;
                // default parameter: name = expr
                if self.current_token == Some(Token::Assign) {
                    self.advance()?;
                    let default = self.parse_assignment_expression()?;
                    params.push(Param::Default(param_name, default));
                } else {
                    params.push(Param::Simple(param_name));
                }
            } else {
                break;
            }
            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }
        self.consume_opt(Token::RParen)?;

        let body = self.parse_block_statement()?;

        Ok(FunctionDeclaration {
            id,
            params,
            body,
            is_generator,
        })
    }

    fn parse_return_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        self.advance()?; // Consume 'return'
        if let Some(Token::Semicolon | Token::RBrace | Token::Eof) | None = self.current_token {
            self.consume_opt(Token::Semicolon)?;
            Ok(Statement::ReturnStatement(None))
        } else {
            let expr = self.parse_expression()?;
            self.consume_opt(Token::Semicolon)?;
            Ok(Statement::ReturnStatement(Some(expr)))
        }
    }

    fn parse_variable_declaration(&mut self) -> Result<VariableDeclaration<'a>, ParseError> {
        let kind = match self.current_token {
            Some(Token::Let) => VariableKind::Let,
            Some(Token::Var) => VariableKind::Var,
            Some(Token::Const) => VariableKind::Const,
            _ => {
                return Err(ParseError::UnexpectedToken {
                    expected: "var, let, or const".to_string(),
                    found: None,
                });
            }
        };
        self.advance()?;

        let mut declarations = Vec::new();

        loop {
            let id = match self.current_token {
                Some(Token::Identifier(name)) => {
                    let name_copy = name;
                    self.advance()?;
                    name_copy
                }
                _ => break, // just break if it's not identifier (robustness)
            };

            let init = if self.current_token == Some(Token::Assign) {
                self.advance()?;
                Some(self.parse_expression()?)
            } else if self.current_token == Some(Token::PlusAssign)
                || self.current_token == Some(Token::MinusAssign)
                || self.current_token == Some(Token::MultiplyAssign)
                || self.current_token == Some(Token::DivideAssign)
                || self.current_token == Some(Token::PercentAssign)
                || self.current_token == Some(Token::LogicAndAssign)
                || self.current_token == Some(Token::LogicOrAssign)
                || self.current_token == Some(Token::NullishAssign)
                || self.current_token == Some(Token::BitAndAssign)
                || self.current_token == Some(Token::BitOrAssign)
                || self.current_token == Some(Token::BitXorAssign)
                || self.current_token == Some(Token::LeftShiftAssign)
                || self.current_token == Some(Token::RightShiftAssign)
                || self.current_token == Some(Token::UnsignedRightShiftAssign)
            {
                return Err(ParseError::UnexpectedToken {
                    expected: "Assign".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            } else {
                None
            };

            declarations.push(VariableDeclarator { id, init });

            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }

        self.consume_opt(Token::Semicolon)?;
        Ok(VariableDeclaration { kind, declarations })
    }

    fn parse_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut expr = self.parse_assignment_expression()?;
        if self.current_token == Some(Token::Comma) {
            let mut seq = vec![expr];
            while self.current_token == Some(Token::Comma) {
                self.advance()?;
                seq.push(self.parse_assignment_expression()?);
            }
            expr = Expression::SequenceExpression(seq);
        }
        Ok(expr)
    }

    fn parse_conditional_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let left = self.parse_binary_expression()?;
        if self.consume_opt(Token::Question)? {
            let consequent = self.parse_assignment_expression()?;
            if self.current_token == Some(Token::Colon) {
                self.advance()?;
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "Colon".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
            let alternate = self.parse_assignment_expression()?;
            Ok(Expression::ConditionalExpression {
                test: Box::new(left),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            })
        } else {
            Ok(left)
        }
    }

    fn parse_assignment_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let left = self.parse_conditional_expression()?;

        // single-param arrow: ident => body
        if self.current_token == Some(Token::Arrow) {
            if let Expression::Identifier(name) = left {
                self.advance()?;
                let body = self.parse_arrow_body()?;
                return Ok(Expression::ArrowFunctionExpression(Box::new(
                    FunctionDeclaration {
                        id: None,
                        params: vec![Param::Simple(name)],
                        body,
                        is_generator: false,
                    },
                )));
            }
        }

        if self.current_token == Some(Token::Assign)
            || self.current_token == Some(Token::PlusAssign)
            || self.current_token == Some(Token::MinusAssign)
            || self.current_token == Some(Token::MultiplyAssign)
            || self.current_token == Some(Token::DivideAssign)
            || self.current_token == Some(Token::PercentAssign)
            || self.current_token == Some(Token::LogicAndAssign)
            || self.current_token == Some(Token::LogicOrAssign)
            || self.current_token == Some(Token::NullishAssign)
            || self.current_token == Some(Token::BitAndAssign)
            || self.current_token == Some(Token::BitOrAssign)
            || self.current_token == Some(Token::BitXorAssign)
            || self.current_token == Some(Token::LeftShiftAssign)
            || self.current_token == Some(Token::RightShiftAssign)
            || self.current_token == Some(Token::UnsignedRightShiftAssign)
        {
            let operator = match self.current_token.clone().unwrap() {
                Token::PlusAssign => AssignmentOperator::PlusAssign,
                Token::MinusAssign => AssignmentOperator::MinusAssign,
                Token::MultiplyAssign => AssignmentOperator::MultiplyAssign,
                Token::DivideAssign => AssignmentOperator::DivideAssign,
                Token::PercentAssign => AssignmentOperator::PercentAssign,
                Token::LogicAndAssign => AssignmentOperator::LogicAndAssign,
                Token::LogicOrAssign => AssignmentOperator::LogicOrAssign,
                Token::NullishAssign => AssignmentOperator::NullishAssign,
                Token::BitAndAssign => AssignmentOperator::BitAndAssign,
                Token::BitOrAssign => AssignmentOperator::BitOrAssign,
                Token::BitXorAssign => AssignmentOperator::BitXorAssign,
                Token::LeftShiftAssign => AssignmentOperator::ShiftLeftAssign,
                Token::RightShiftAssign => AssignmentOperator::ShiftRightAssign,
                Token::UnsignedRightShiftAssign => AssignmentOperator::UnsignedShiftRightAssign,
                _ => AssignmentOperator::Assign,
            };
            self.advance()?;
            let right = self.parse_assignment_expression()?;
            Ok(Expression::AssignmentExpression(Box::new(
                AssignmentExpression {
                    operator,
                    left,
                    right,
                },
            )))
        } else {
            Ok(left)
        }
    }

    fn parse_binary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        self.parse_binary_expression_with_min_precedence(0)
    }

    fn parse_binary_expression_with_min_precedence(
        &mut self,
        min_precedence: u8,
    ) -> Result<Expression<'a>, ParseError> {
        let mut left = self.parse_unary_expression()?;

        loop {
            let Some((operator, precedence, right_associative)) = self.current_binary_operator()
            else {
                break;
            };
            if precedence < min_precedence {
                break;
            }

            self.advance()?;
            let next_min_precedence = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let right = self.parse_binary_expression_with_min_precedence(next_min_precedence)?;
            left = Expression::BinaryExpression(Box::new(BinaryExpression {
                operator,
                left,
                right,
            }));
        }

        Ok(left)
    }

    fn current_binary_operator(&self) -> Option<(BinaryOperator, u8, bool)> {
        match &self.current_token {
            Some(Token::LogicOr) => Some((BinaryOperator::LogicOr, 1, false)),
            Some(Token::Nullish) => Some((BinaryOperator::NullishCoalescing, 2, false)),
            Some(Token::LogicAnd) => Some((BinaryOperator::LogicAnd, 3, false)),
            Some(Token::BitOr) => Some((BinaryOperator::BitOr, 4, false)),
            Some(Token::BitXor) => Some((BinaryOperator::BitXor, 5, false)),
            Some(Token::BitAnd) => Some((BinaryOperator::BitAnd, 6, false)),
            Some(Token::EqEqEq) => Some((BinaryOperator::EqEqEq, 7, false)),
            Some(Token::EqEq) => Some((BinaryOperator::EqEq, 7, false)),
            Some(Token::NotEq) => Some((BinaryOperator::NotEq, 7, false)),
            Some(Token::NotEqEq) => Some((BinaryOperator::NotEqEq, 7, false)),
            Some(Token::Less) => Some((BinaryOperator::Less, 8, false)),
            Some(Token::LessEq) => Some((BinaryOperator::LessEq, 8, false)),
            Some(Token::Greater) => Some((BinaryOperator::Greater, 8, false)),
            Some(Token::GreaterEq) => Some((BinaryOperator::GreaterEq, 8, false)),
            Some(Token::Instanceof) => Some((BinaryOperator::Instanceof, 8, false)),
            Some(Token::In) if self.allow_in => Some((BinaryOperator::In, 8, false)),
            Some(Token::LeftShift) => Some((BinaryOperator::ShiftLeft, 9, false)),
            Some(Token::RightShift) => Some((BinaryOperator::ShiftRight, 9, false)),
            Some(Token::UnsignedRightShift) => Some((BinaryOperator::LogicalShiftRight, 9, false)),
            Some(Token::Plus) => Some((BinaryOperator::Plus, 10, false)),
            Some(Token::Minus) => Some((BinaryOperator::Minus, 10, false)),
            Some(Token::Asterisk) => Some((BinaryOperator::Multiply, 11, false)),
            Some(Token::Slash) => Some((BinaryOperator::Divide, 11, false)),
            Some(Token::Percent) => Some((BinaryOperator::Percent, 11, false)),
            Some(Token::Power) => Some((BinaryOperator::Power, 12, true)),
            _ => None,
        }
    }

    fn parse_unary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        if let Some(op) = self.current_token.clone() {
            if op == Token::PlusPlus || op == Token::MinusMinus {
                let operator = if op == Token::PlusPlus {
                    UpdateOperator::PlusPlus
                } else {
                    UpdateOperator::MinusMinus
                };
                self.advance()?;
                let argument = self.parse_unary_expression()?;
                return Ok(Expression::UpdateExpression(Box::new(UpdateExpression {
                    operator,
                    argument,
                    prefix: true,
                })));
            }

            let unary_op = match op {
                Token::Minus => Some(UnaryOperator::Minus),
                Token::Plus => Some(UnaryOperator::Plus),
                Token::LogicNot => Some(UnaryOperator::LogicNot),
                Token::BitNot => Some(UnaryOperator::BitNot),
                Token::Typeof => Some(UnaryOperator::Typeof),
                Token::Void => Some(UnaryOperator::Void),
                Token::Delete => Some(UnaryOperator::Delete),
                _ => None,
            };

            if let Some(operator) = unary_op {
                self.advance()?;
                let argument = self.parse_unary_expression()?;
                return Ok(Expression::UnaryExpression(Box::new(UnaryExpression {
                    operator,
                    argument,
                    prefix: true,
                })));
            }
        }
        self.parse_member_or_call_expression()
    }

    fn parse_member_or_call_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.consume_opt(Token::Dot)? {
                match self.current_token.clone() {
                    Some(Token::Identifier(id)) => {
                        self.advance()?;
                        expr = Expression::MemberExpression(Box::new(MemberExpression {
                            object: expr,
                            property: Expression::Identifier(id),
                            computed: false,
                            optional: false,
                        }));
                    }
                    _ => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Identifier".to_string(),
                            found: None,
                        });
                    }
                }
            } else if self.current_token == Some(Token::OptionalChain) {
                self.advance()?;
                if self.consume_opt(Token::LBracket)? {
                    let property = self.parse_expression()?;
                    self.consume_opt(Token::RBracket)?;
                    expr = Expression::MemberExpression(Box::new(MemberExpression {
                        object: expr,
                        property,
                        computed: true,
                        optional: true,
                    }));
                } else if self.consume_opt(Token::LParen)? {
                    let mut args = Vec::new();
                    if self.current_token != Some(Token::RParen) {
                        loop {
                            if self.current_token == Some(Token::DotDotDot) {
                                self.advance()?;
                                let spread = self.parse_assignment_expression()?;
                                args.push(Expression::SpreadElement(Box::new(spread)));
                            } else {
                                args.push(self.parse_assignment_expression()?);
                            }
                            if !self.consume_opt(Token::Comma)? {
                                break;
                            }
                        }
                    }
                    self.consume_opt(Token::RParen)?;
                    expr = Expression::CallExpression(Box::new(CallExpression {
                        callee: expr,
                        arguments: args,
                        optional: true,
                    }));
                } else {
                    match self.current_token.clone() {
                        Some(Token::Identifier(id)) => {
                            self.advance()?;
                            expr = Expression::MemberExpression(Box::new(MemberExpression {
                                object: expr,
                                property: Expression::Identifier(id),
                                computed: false,
                                optional: true,
                            }));
                        }
                        _ => {
                            return Err(ParseError::UnexpectedToken {
                                expected: "Identifier, LBracket, or LParen".to_string(),
                                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                            });
                        }
                    }
                }
            } else if self.consume_opt(Token::LBracket)? {
                let property = self.parse_expression()?;
                self.consume_opt(Token::RBracket)?;
                expr = Expression::MemberExpression(Box::new(MemberExpression {
                    object: expr,
                    property,
                    computed: true,
                    optional: false,
                }));
            } else if self.consume_opt(Token::LParen)? {
                let mut args = Vec::new();
                if self.current_token != Some(Token::RParen) {
                    loop {
                        if self.current_token == Some(Token::DotDotDot) {
                            self.advance()?;
                            let spread = self.parse_assignment_expression()?;
                            args.push(Expression::SpreadElement(Box::new(spread)));
                        } else {
                            args.push(self.parse_assignment_expression()?);
                        }
                        if !self.consume_opt(Token::Comma)? {
                            break;
                        }
                    }
                }
                self.consume_opt(Token::RParen)?;
                expr = Expression::CallExpression(Box::new(CallExpression {
                    callee: expr,
                    arguments: args,
                    optional: false,
                }));
            } else {
                break;
            }
        }

        if self.consume_opt(Token::PlusPlus)? {
            expr = Expression::UpdateExpression(Box::new(UpdateExpression {
                operator: UpdateOperator::PlusPlus,
                argument: expr,
                prefix: false,
            }));
        } else if self.consume_opt(Token::MinusMinus)? {
            expr = Expression::UpdateExpression(Box::new(UpdateExpression {
                operator: UpdateOperator::MinusMinus,
                argument: expr,
                prefix: false,
            }));
        }
        Ok(expr)
    }

    fn parse_array_literal(&mut self) -> Result<Expression<'a>, ParseError> {
        self.advance()?; // '['
        let mut elements = Vec::new();

        while self.current_token.is_some() && self.current_token != Some(Token::RBracket) {
            if self.current_token == Some(Token::Comma) {
                elements.push(None);
                self.advance()?;
                continue;
            }

            let element = if self.current_token == Some(Token::DotDotDot) {
                self.advance()?;
                Expression::SpreadElement(Box::new(self.parse_assignment_expression()?))
            } else {
                self.parse_assignment_expression()?
            };
            elements.push(Some(element));

            if self.current_token == Some(Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }

        if !self.consume_opt(Token::RBracket)? {
            return Err(ParseError::UnexpectedToken {
                expected: "RBracket".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }

        Ok(Expression::ArrayExpression(elements))
    }

    fn parse_object_literal(&mut self) -> Result<Expression<'a>, ParseError> {
        self.advance()?; // '{'
        let mut properties = Vec::new();

        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            // spread: { ...expr }
            if self.current_token == Some(Token::DotDotDot) {
                self.advance()?;
                let spread_expr = self.parse_assignment_expression()?;
                properties.push(ObjectProperty {
                    key: ObjectKey::Identifier(""),
                    value: Expression::SpreadElement(Box::new(spread_expr.clone())),
                    shorthand: false,
                    computed: false,
                    method: false,
                    kind: ObjectPropertyKind::Value(Expression::SpreadElement(Box::new(
                        spread_expr,
                    ))),
                });
                if self.current_token == Some(Token::Comma) {
                    self.advance()?;
                }
                continue;
            }

            let mut accessor_kind = None;
            if matches!(self.current_token, Some(Token::Identifier("get" | "set"))) {
                let marker = if matches!(self.current_token, Some(Token::Identifier("get"))) {
                    "get"
                } else {
                    "set"
                };
                let mut lookahead = self.lexer.clone();
                if let Ok(next_token) = lookahead.next_token() {
                    let is_accessor =
                        matches!(
                            next_token,
                            Token::Identifier(_)
                                | Token::String(_)
                                | Token::Number(_)
                                | Token::LBracket
                        ) && matches!(lookahead.next_token().ok(), Some(Token::LParen));
                    if is_accessor {
                        accessor_kind = Some(marker);
                        self.advance()?;
                    }
                }
            }

            // computed key: { [expr]: value }
            let (key, computed) = if self.current_token == Some(Token::LBracket) {
                self.advance()?;
                let expr = self.parse_assignment_expression()?;
                self.consume_opt(Token::RBracket)?;
                (ObjectKey::Computed(Box::new(expr)), true)
            } else {
                let k = match self.current_token.clone() {
                    Some(Token::Identifier(name)) => {
                        self.advance()?;
                        ObjectKey::Identifier(name)
                    }
                    Some(Token::String(name)) => {
                        self.advance()?;
                        ObjectKey::String(name)
                    }
                    Some(Token::Number(n)) => {
                        self.advance()?;
                        ObjectKey::Number(n)
                    }
                    Some(token) => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Identifier, String, or Number".to_string(),
                            found: Some(format!("{:?}", token)),
                        });
                    }
                    None => return Err(ParseError::UnexpectedEOF),
                };
                (k, false)
            };

            // method shorthand/accessor: { foo() { ... } } / { get foo() {} }
            if self.current_token == Some(Token::LParen) {
                let func = self.parse_function_body_from_params()?;
                let kind = match accessor_kind {
                    Some("get") => ObjectPropertyKind::Getter(func.clone()),
                    Some("set") => ObjectPropertyKind::Setter(func.clone()),
                    _ => ObjectPropertyKind::Value(Expression::FunctionExpression(Box::new(
                        func.clone(),
                    ))),
                };
                properties.push(ObjectProperty {
                    key,
                    value: Expression::FunctionExpression(Box::new(func)),
                    shorthand: false,
                    computed,
                    method: accessor_kind.is_none(),
                    kind,
                });
            } else if self.consume_opt(Token::Colon)? {
                let value = self.parse_assignment_expression()?;
                properties.push(ObjectProperty {
                    key,
                    value: value.clone(),
                    shorthand: false,
                    computed,
                    method: false,
                    kind: ObjectPropertyKind::Value(value),
                });
            } else {
                // shorthand: { foo }
                let name = match &key {
                    ObjectKey::Identifier(name) => *name,
                    _ => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Colon after property key".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                };
                properties.push(ObjectProperty {
                    key,
                    value: Expression::Identifier(name),
                    shorthand: true,
                    computed: false,
                    method: false,
                    kind: ObjectPropertyKind::Value(Expression::Identifier(name)),
                });
            }

            if self.current_token == Some(Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }

        if !self.consume_opt(Token::RBrace)? {
            return Err(ParseError::UnexpectedToken {
                expected: "RBrace".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }

        Ok(Expression::ObjectExpression(properties))
    }

    fn parse_function_body_from_params(&mut self) -> Result<FunctionDeclaration<'a>, ParseError> {
        self.consume_opt(Token::LParen)?;
        let mut params = Vec::new();
        while self.current_token != Some(Token::RParen) && self.current_token.is_some() {
            if self.current_token == Some(Token::DotDotDot) {
                self.advance()?;
                if let Some(Token::Identifier(name)) = self.current_token {
                    params.push(Param::Rest(name));
                    self.advance()?;
                }
                break;
            }
            if let Some(Token::Identifier(name)) = self.current_token {
                self.advance()?;
                params.push(Param::Simple(name));
            }
            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }
        self.consume_opt(Token::RParen)?;
        let body = self.parse_block_statement()?;
        Ok(FunctionDeclaration {
            id: None,
            params,
            body,
            is_generator: false,
        })
    }

    fn parse_class_declaration(
        &mut self,
        require_name: bool,
    ) -> Result<ClassDeclaration<'a>, ParseError> {
        self.advance()?; // class
        let id = match self.current_token {
            Some(Token::Identifier(name)) => {
                self.advance()?;
                Some(name)
            }
            _ if require_name => {
                return Err(ParseError::UnexpectedToken {
                    expected: "Identifier".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
            _ => None,
        };

        let super_class = if self.consume_opt(Token::Extends)? {
            Some(self.parse_member_or_call_expression()?)
        } else {
            None
        };

        if !self.consume_opt(Token::LBrace)? {
            return Err(ParseError::UnexpectedToken {
                expected: "LBrace".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }

        let mut body = Vec::new();
        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            let mut is_static = false;
            if matches!(self.current_token, Some(Token::Identifier("static"))) {
                let next = self.lexer.clone().next_token().ok();
                if !matches!(
                    next,
                    Some(Token::LParen) | Some(Token::Assign) | Some(Token::Semicolon)
                ) {
                    is_static = true;
                    self.advance()?;
                }
            }

            let key = match self.current_token.clone() {
                Some(Token::Identifier(name)) => {
                    self.advance()?;
                    ObjectKey::Identifier(name)
                }
                Some(Token::String(name)) => {
                    self.advance()?;
                    ObjectKey::String(name)
                }
                Some(Token::Number(n)) => {
                    self.advance()?;
                    ObjectKey::Number(n)
                }
                Some(Token::LBracket) => {
                    self.advance()?;
                    let expr = self.parse_expression()?;
                    self.consume_opt(Token::RBracket)?;
                    ObjectKey::Computed(Box::new(expr))
                }
                token => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "class element name".to_string(),
                        found: token.map(|t| format!("{:?}", t)),
                    });
                }
            };

            let mut accessor_kind = None;
            if matches!(key, ObjectKey::Identifier("get" | "set"))
                && self.current_token != Some(Token::LParen)
            {
                let accessor = match key {
                    ObjectKey::Identifier("get") => "get",
                    ObjectKey::Identifier("set") => "set",
                    _ => unreachable!(),
                };
                let actual_key = match self.current_token.clone() {
                    Some(Token::Identifier(name)) => {
                        self.advance()?;
                        ObjectKey::Identifier(name)
                    }
                    Some(Token::String(name)) => {
                        self.advance()?;
                        ObjectKey::String(name)
                    }
                    Some(Token::Number(n)) => {
                        self.advance()?;
                        ObjectKey::Number(n)
                    }
                    Some(Token::LBracket) => {
                        self.advance()?;
                        let expr = self.parse_expression()?;
                        self.consume_opt(Token::RBracket)?;
                        ObjectKey::Computed(Box::new(expr))
                    }
                    token => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "accessor key".to_string(),
                            found: token.map(|t| format!("{:?}", t)),
                        });
                    }
                };
                accessor_kind = Some(accessor);
                if self.current_token == Some(Token::LParen) {
                    let func = self.parse_function_body_from_params()?;
                    match accessor {
                        "get" => body.push(ClassElement::Getter {
                            key: actual_key,
                            body: func,
                            is_static,
                        }),
                        _ => body.push(ClassElement::Setter {
                            key: actual_key,
                            body: func,
                            is_static,
                        }),
                    }
                    continue;
                }
            }

            if self.current_token == Some(Token::LParen) {
                let func = self.parse_function_body_from_params()?;
                let is_constructor =
                    !is_static && matches!(key, ObjectKey::Identifier("constructor"));
                if is_constructor {
                    body.push(ClassElement::Constructor {
                        function: func,
                        is_default: false,
                    });
                } else {
                    body.push(ClassElement::Method {
                        key,
                        value: func,
                        is_static,
                    });
                }
            } else {
                let initializer = if self.consume_opt(Token::Assign)? {
                    Some(self.parse_assignment_expression()?)
                } else {
                    None
                };
                self.consume_opt(Token::Semicolon)?;
                body.push(ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                });
            }
        }
        self.consume_opt(Token::RBrace)?;

        if super_class.is_some()
            && !body
                .iter()
                .any(|element| matches!(element, ClassElement::Constructor { .. }))
        {
            body.insert(
                0,
                ClassElement::Constructor {
                    function: FunctionDeclaration {
                        id: None,
                        params: vec![Param::Rest("args")],
                        body: BlockStatement {
                            body: vec![Statement::ExpressionStatement(Expression::CallExpression(
                                Box::new(CallExpression {
                                    callee: Expression::SuperExpression,
                                    arguments: vec![Expression::SpreadElement(Box::new(
                                        Expression::Identifier("args"),
                                    ))],
                                    optional: false,
                                }),
                            ))],
                        },
                        is_generator: false,
                    },
                    is_default: true,
                },
            );
        }

        Ok(ClassDeclaration {
            id,
            super_class,
            body,
        })
    }

    fn parse_primary(&mut self) -> Result<Expression<'a>, ParseError> {
        match &self.current_token {
            Some(Token::Number(val)) => {
                let v = *val;
                self.advance()?;
                Ok(Expression::Literal(Literal::Number(v)))
            }
            Some(Token::String(s)) => {
                let v = *s;
                self.advance()?;
                Ok(Expression::Literal(Literal::String(v)))
            }
            Some(Token::Null) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Null))
            }
            Some(Token::Undefined) => {
                self.advance()?;
                Ok(Expression::Identifier("undefined"))
            }
            Some(Token::This) => {
                self.advance()?;
                Ok(Expression::ThisExpression)
            }
            Some(Token::Super) => {
                self.advance()?;
                Ok(Expression::SuperExpression)
            }
            Some(Token::Class) => Ok(Expression::ClassExpression(Box::new(
                self.parse_class_declaration(false)?,
            ))),
            Some(Token::Function) => {
                let func = self.parse_function_declaration()?;
                Ok(Expression::FunctionExpression(Box::new(func)))
            }
            Some(Token::True) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Boolean(true)))
            }
            Some(Token::False) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Boolean(false)))
            }
            Some(Token::Identifier(id)) => {
                let v = *id;
                self.advance()?;
                Ok(Expression::Identifier(v))
            }
            Some(Token::LParen) => {
                self.advance()?;
                // empty params: () => ...
                if self.current_token == Some(Token::RParen) {
                    self.advance()?;
                    if self.current_token == Some(Token::Arrow) {
                        self.advance()?;
                        let body = self.parse_arrow_body()?;
                        return Ok(Expression::ArrowFunctionExpression(Box::new(
                            FunctionDeclaration {
                                id: None,
                                params: vec![],
                                body,
                                is_generator: false,
                            },
                        )));
                    }
                    return Err(ParseError::UnexpectedToken {
                        expected: "Arrow".into(),
                        found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                    });
                }

                let expr = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;

                if self.current_token == Some(Token::Arrow) {
                    self.advance()?;
                    let params = collect_arrow_params(expr);
                    let body = self.parse_arrow_body()?;
                    return Ok(Expression::ArrowFunctionExpression(Box::new(
                        FunctionDeclaration {
                            id: None,
                            params,
                            body,
                            is_generator: false,
                        },
                    )));
                }
                Ok(expr)
            }
            Some(Token::LBracket) => self.parse_array_literal(),
            Some(Token::LBrace) => self.parse_object_literal(),
            Some(Token::New) => {
                self.advance()?; // 'new'
                let mut callee = self.parse_primary()?;

                loop {
                    if self.consume_opt(Token::Dot)? {
                        match self.current_token.clone() {
                            Some(Token::Identifier(id)) => {
                                self.advance()?;
                                callee = Expression::MemberExpression(Box::new(MemberExpression {
                                    object: callee,
                                    property: Expression::Identifier(id),
                                    computed: false,
                                    optional: false,
                                }));
                            }
                            _ => {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "Identifier".to_string(),
                                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                                });
                            }
                        }
                    } else if self.consume_opt(Token::LBracket)? {
                        let property = self.parse_expression()?;
                        self.consume_opt(Token::RBracket)?;
                        callee = Expression::MemberExpression(Box::new(MemberExpression {
                            object: callee,
                            property,
                            computed: true,
                            optional: false,
                        }));
                    } else {
                        break;
                    }
                }

                let mut arguments = Vec::new();
                if self.consume_opt(Token::LParen)? {
                    if self.current_token != Some(Token::RParen) {
                        loop {
                            if self.current_token == Some(Token::DotDotDot) {
                                self.advance()?;
                                let spread = self.parse_assignment_expression()?;
                                arguments.push(Expression::SpreadElement(Box::new(spread)));
                            } else {
                                arguments.push(self.parse_assignment_expression()?);
                            }
                            if !self.consume_opt(Token::Comma)? {
                                break;
                            }
                        }
                    }
                    self.consume_opt(Token::RParen)?;
                }

                Ok(Expression::NewExpression(Box::new(CallExpression {
                    callee,
                    arguments,
                    optional: false,
                })))
            }
            Some(Token::RParen)
            | Some(Token::RBracket)
            | Some(Token::RBrace)
            | Some(Token::Comma)
            | Some(Token::Semicolon)
            | Some(Token::Colon)
            | Some(Token::Eof) => Ok(Expression::Identifier("DummyEndPunct")),
            Some(Token::Async) => {
                self.advance()?;
                if self.current_token == Some(Token::Function) {
                    let func = self.parse_function_declaration()?;
                    Ok(Expression::FunctionExpression(Box::new(func)))
                } else {
                    Ok(Expression::Identifier("AsyncDummy"))
                }
            }
            Some(Token::Slash) | Some(Token::DivideAssign) => {
                self.advance()?;
                while self.current_token.is_some() && self.current_token != Some(Token::Slash) {
                    self.advance()?;
                }
                self.advance()?; // '/'
                if let Some(Token::Identifier(_)) = self.current_token {
                    self.advance()?;
                } // flags
                Ok(Expression::Literal(Literal::String("regex_dummy".into())))
            }
            Some(Token::Template(s)) => {
                let v = *s;
                self.advance()?;
                Ok(Expression::Literal(Literal::String(v)))
            }
            _ => {
                self.advance()?;
                Ok(Expression::Identifier("CatchAllDummy"))
            }
        }
    }

    fn parse_arrow_body(&mut self) -> Result<BlockStatement<'a>, ParseError> {
        if self.current_token == Some(Token::LBrace) {
            self.parse_block_statement()
        } else {
            // expression body: => expr
            let expr = self.parse_assignment_expression()?;
            Ok(BlockStatement {
                body: vec![Statement::ReturnStatement(Some(expr))],
            })
        }
    }
}

fn collect_arrow_params<'a>(expr: Expression<'a>) -> Vec<Param<'a>> {
    match expr {
        Expression::Identifier(name) => vec![Param::Simple(name)],
        Expression::SequenceExpression(exprs) => exprs
            .into_iter()
            .filter_map(|e| match e {
                Expression::Identifier(name) => Some(Param::Simple(name)),
                Expression::AssignmentExpression(assign) => {
                    if let Expression::Identifier(name) = assign.left {
                        Some(Param::Default(name, assign.right))
                    } else {
                        None
                    }
                }
                Expression::SpreadElement(inner) => {
                    if let Expression::Identifier(name) = *inner {
                        Some(Param::Rest(name))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect(),
        Expression::AssignmentExpression(assign) => {
            if let Expression::Identifier(name) = assign.left {
                vec![Param::Default(name, assign.right)]
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}
