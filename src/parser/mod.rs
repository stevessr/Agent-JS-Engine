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
    #[error("Rest element must be last in array binding pattern")]
    InvalidRestElement,
    #[error("Rest property must be last in object binding pattern")]
    InvalidRestProperty,
    #[error("Missing initializer in const declaration")]
    MissingConstInitializer,
    #[error("Invalid for-in/of binding")]
    InvalidForBinding,
    #[error("for-in/of declarations cannot have initializers")]
    InvalidForBindingInitializer,
    #[error("Invalid assignment target")]
    InvalidAssignmentTarget,
    #[error("Invalid update target")]
    InvalidUpdateTarget,
    #[error("Invalid private identifier usage: {0}")]
    InvalidPrivateIdentifierUsage(String),
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

    fn parse_identifier_name(&mut self) -> Result<&'a str, ParseError> {
        match self.current_token.clone() {
            Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                let name = Self::token_as_identifier_name(&token).unwrap();
                self.advance()?;
                Ok(name)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "IdentifierName".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
        }
    }

    fn current_identifier_name_is(&self, expected: &str) -> bool {
        match &self.current_token {
            Some(token) => Self::token_as_identifier_name(token) == Some(expected),
            None => false,
        }
    }

    fn token_as_identifier_name(token: &Token<'a>) -> Option<&'a str> {
        match token {
            Token::Identifier(name) => Some(*name),
            Token::Var => Some("var"),
            Token::Let => Some("let"),
            Token::Const => Some("const"),
            Token::If => Some("if"),
            Token::Else => Some("else"),
            Token::Function => Some("function"),
            Token::Return => Some("return"),
            Token::Throw => Some("throw"),
            Token::Try => Some("try"),
            Token::Catch => Some("catch"),
            Token::Finally => Some("finally"),
            Token::For => Some("for"),
            Token::While => Some("while"),
            Token::Do => Some("do"),
            Token::Break => Some("break"),
            Token::Continue => Some("continue"),
            Token::New => Some("new"),
            Token::This => Some("this"),
            Token::Typeof => Some("typeof"),
            Token::Void => Some("void"),
            Token::Delete => Some("delete"),
            Token::Switch => Some("switch"),
            Token::Case => Some("case"),
            Token::Default => Some("default"),
            Token::In => Some("in"),
            Token::Instanceof => Some("instanceof"),
            Token::Class => Some("class"),
            Token::Extends => Some("extends"),
            Token::Super => Some("super"),
            Token::Yield => Some("yield"),
            Token::Await => Some("await"),
            Token::Async => Some("async"),
            Token::Import => Some("import"),
            Token::Export => Some("export"),
            Token::True => Some("true"),
            Token::False => Some("false"),
            Token::Null => Some("null"),
            Token::Undefined => Some("undefined"),
            Token::Debugger => Some("debugger"),
            Token::With => Some("with"),
            _ => None,
        }
    }

    fn is_simple_assignment_target(expr: &Expression<'a>) -> bool {
        matches!(
            expr,
            Expression::Identifier(_) | Expression::MemberExpression(_)
        )
    }

    fn is_private_member_expression(expr: &Expression<'a>) -> bool {
        matches!(
            expr,
            Expression::MemberExpression(member)
                if !member.computed && matches!(member.property, Expression::PrivateIdentifier(_))
        )
    }

    fn is_for_in_of_target(expr: &Expression<'a>) -> bool {
        matches!(
            expr,
            Expression::Identifier(_)
                | Expression::MemberExpression(_)
                | Expression::ArrayExpression(_)
                | Expression::ObjectExpression(_)
        )
    }

    fn is_assignment_target(expr: &Expression<'a>) -> bool {
        match expr {
            Expression::Identifier(_) | Expression::MemberExpression(_) => true,
            Expression::AssignmentExpression(assign)
                if matches!(assign.operator, AssignmentOperator::Assign) =>
            {
                Self::is_assignment_target(&assign.left)
            }
            Expression::ArrayExpression(elements) => elements.iter().all(|element| match element {
                None => true,
                Some(Expression::SpreadElement(inner)) => Self::is_assignment_target(inner),
                Some(element) => Self::is_assignment_target(element),
            }),
            Expression::ObjectExpression(properties) => properties.iter().all(|property| {
                if let Expression::SpreadElement(inner) = &property.value {
                    Self::is_assignment_target(inner)
                } else {
                    Self::is_assignment_target(&property.value)
                }
            }),
            Expression::SpreadElement(inner) => Self::is_assignment_target(inner),
            _ => false,
        }
    }

    fn validate_const_declaration(
        &self,
        decl: &VariableDeclaration<'a>,
        allow_missing_initializer: bool,
    ) -> Result<(), ParseError> {
        if matches!(decl.kind, VariableKind::Const)
            && decl
                .declarations
                .iter()
                .any(|declarator| declarator.init.is_none())
            && !allow_missing_initializer
        {
            return Err(ParseError::MissingConstInitializer);
        }
        Ok(())
    }

    fn validate_for_in_of_left(&self, left: &Statement<'a>) -> Result<(), ParseError> {
        match left {
            Statement::VariableDeclaration(decl) => {
                if decl.declarations.len() != 1 {
                    return Err(ParseError::InvalidForBinding);
                }
                if decl.declarations[0].init.is_some() {
                    return Err(ParseError::InvalidForBindingInitializer);
                }
                Ok(())
            }
            Statement::ExpressionStatement(expr) if Self::is_for_in_of_target(expr) => Ok(()),
            _ => Err(ParseError::InvalidForBinding),
        }
    }

    fn parse_module_name(&mut self) -> Result<&'a str, ParseError> {
        match self.current_token.clone() {
            Some(Token::String(name)) => {
                self.advance()?;
                Ok(name)
            }
            Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                let name = Self::token_as_identifier_name(&token).unwrap();
                self.advance()?;
                Ok(name)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "module name".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
        }
    }

    fn token_starts_method_key(token: &Token<'a>) -> bool {
        matches!(
            token,
            Token::String(_) | Token::Number(_) | Token::LBracket | Token::PrivateIdentifier(_)
        ) || Self::token_as_identifier_name(token).is_some()
    }

    fn looks_like_parenthesized_async_arrow(&self) -> bool {
        let mut lookahead = self.lexer.clone();
        if !matches!(lookahead.next_token().ok(), Some(Token::LParen)) {
            return false;
        }

        let mut depth = 1usize;
        while let Ok(token) = lookahead.next_token() {
            match token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(lookahead.next_token().ok(), Some(Token::Arrow));
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
        }

        false
    }

    fn looks_like_parenthesized_arrow(&self) -> bool {
        if self.current_token != Some(Token::LParen) {
            return false;
        }

        let mut lookahead = self.lexer.clone();
        let mut depth = 1usize;
        while let Ok(token) = lookahead.next_token() {
            match token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(lookahead.next_token().ok(), Some(Token::Arrow));
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
        }

        false
    }

    fn parse_parenthesized_arrow_expression(
        &mut self,
        is_async: bool,
    ) -> Result<Expression<'a>, ParseError> {
        let params = self.parse_parameter_list()?;
        if self.current_token != Some(Token::Arrow) {
            return Err(ParseError::UnexpectedToken {
                expected: "Arrow".into(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }
        self.advance()?;
        let body = self.parse_arrow_body()?;
        Ok(Expression::ArrowFunctionExpression(Box::new(
            FunctionDeclaration {
                id: None,
                params,
                body,
                is_generator: false,
                is_async,
            },
        )))
    }

    fn parse_grouped_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        self.consume_opt(Token::LParen)?;
        let expr = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;
        Ok(expr)
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
            Some(Token::Let | Token::Var | Token::Const) => {
                let decl = self.parse_variable_declaration()?;
                self.validate_const_declaration(&decl, false)?;
                Ok(Statement::VariableDeclaration(decl))
            }
            Some(Token::Import)
                if !matches!(
                    self.lexer.clone().next_token().ok(),
                    Some(Token::LParen) | Some(Token::Dot)
                ) =>
            {
                Ok(Statement::ImportDeclaration(
                    self.parse_import_declaration()?,
                ))
            }
            Some(Token::LBrace) => Ok(Statement::BlockStatement(self.parse_block_statement()?)),
            Some(Token::If) => Ok(Statement::IfStatement(self.parse_if_statement()?)),
            Some(Token::With) => Ok(Statement::WithStatement(self.parse_with_statement()?)),
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
            Some(Token::Async)
                if matches!(self.lexer.clone().next_token().ok(), Some(Token::Function)) =>
            {
                self.advance()?;
                let mut func = self.parse_function_declaration()?;
                func.is_async = true;
                Ok(Statement::FunctionDeclaration(func))
            }
            Some(Token::Function) => Ok(Statement::FunctionDeclaration(
                self.parse_function_declaration()?,
            )),
            Some(Token::Class) => Ok(Statement::ClassDeclaration(
                self.parse_class_declaration(true)?,
            )),
            Some(Token::Export) => self.parse_export_statement(),
            Some(Token::Debugger) => {
                self.advance()?;
                self.consume_opt(Token::Semicolon)?;
                Ok(Statement::EmptyStatement)
            }
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

    fn parse_with_statement(&mut self) -> Result<WithStatement<'a>, ParseError> {
        self.advance()?; // 'with'
        self.consume_opt(Token::LParen)?;
        let object = self.parse_expression()?;
        self.consume_opt(Token::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(WithStatement { object, body })
    }

    fn parse_import_declaration(&mut self) -> Result<ImportDeclaration<'a>, ParseError> {
        self.advance()?; // 'import'

        if let Some(Token::String(source)) = self.current_token {
            self.advance()?;
            self.consume_opt(Token::Semicolon)?;
            return Ok(ImportDeclaration {
                specifiers: vec![],
                source,
            });
        }

        let mut specifiers = Vec::new();

        if self.current_token == Some(Token::Asterisk) {
            self.advance()?;
            if !self.current_identifier_name_is("as") {
                return Err(ParseError::UnexpectedToken {
                    expected: "as".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
            self.advance()?;
            let local = self.parse_identifier_name()?;
            specifiers.push(ImportSpecifier::Namespace(local));
        } else if self.current_token == Some(Token::LBrace) {
            specifiers.extend(self.parse_named_import_specifiers()?);
        } else {
            let local = self.parse_identifier_name()?;
            specifiers.push(ImportSpecifier::Default(local));

            if self.consume_opt(Token::Comma)? {
                if self.current_token == Some(Token::Asterisk) {
                    self.advance()?;
                    if !self.current_identifier_name_is("as") {
                        return Err(ParseError::UnexpectedToken {
                            expected: "as".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                    self.advance()?;
                    let namespace = self.parse_identifier_name()?;
                    specifiers.push(ImportSpecifier::Namespace(namespace));
                } else if self.current_token == Some(Token::LBrace) {
                    specifiers.extend(self.parse_named_import_specifiers()?);
                }
            }
        }

        if !self.current_identifier_name_is("from") {
            return Err(ParseError::UnexpectedToken {
                expected: "from".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            });
        }
        self.advance()?;

        let source = match self.current_token {
            Some(Token::String(source)) => {
                self.advance()?;
                source
            }
            _ => {
                return Err(ParseError::UnexpectedToken {
                    expected: "String".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
        };

        self.consume_opt(Token::Semicolon)?;
        Ok(ImportDeclaration { specifiers, source })
    }

    fn parse_named_import_specifiers(&mut self) -> Result<Vec<ImportSpecifier<'a>>, ParseError> {
        self.consume_opt(Token::LBrace)?;
        let mut specifiers = Vec::new();

        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            let imported = self.parse_module_name()?;
            let local = if self.current_identifier_name_is("as") {
                self.advance()?;
                self.parse_identifier_name()?
            } else {
                imported
            };
            specifiers.push(ImportSpecifier::Named { imported, local });

            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }

        self.consume_opt(Token::RBrace)?;
        Ok(specifiers)
    }

    fn parse_export_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        self.advance()?; // 'export'

        if self.consume_opt(Token::Default)? {
            return self.parse_export_default_declaration();
        }

        if self.current_token == Some(Token::Asterisk) {
            self.advance()?;
            let exported = if self.current_identifier_name_is("as") {
                self.advance()?;
                Some(self.parse_identifier_name()?)
            } else {
                None
            };

            if !self.current_identifier_name_is("from") {
                return Err(ParseError::UnexpectedToken {
                    expected: "from".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
            self.advance()?;

            let source = match self.current_token {
                Some(Token::String(source)) => {
                    self.advance()?;
                    source
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "String".to_string(),
                        found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                    });
                }
            };
            self.consume_opt(Token::Semicolon)?;

            return Ok(Statement::ExportAllDeclaration(ExportAllDeclaration {
                exported,
                source,
            }));
        }

        if self.current_token == Some(Token::LBrace) {
            let specifiers = self.parse_export_specifiers()?;
            let source = if self.current_identifier_name_is("from") {
                self.advance()?;
                match self.current_token {
                    Some(Token::String(source)) => {
                        self.advance()?;
                        Some(source)
                    }
                    _ => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "String".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                }
            } else {
                None
            };
            self.consume_opt(Token::Semicolon)?;
            return Ok(Statement::ExportNamedDeclaration(ExportNamedDeclaration {
                declaration: None,
                specifiers,
                source,
            }));
        }

        let declaration = match self.current_token {
            Some(Token::Var | Token::Let | Token::Const) => {
                let decl = self.parse_variable_declaration()?;
                self.validate_const_declaration(&decl, false)?;
                Statement::VariableDeclaration(decl)
            }
            Some(Token::Function) => {
                Statement::FunctionDeclaration(self.parse_function_declaration()?)
            }
            Some(Token::Async)
                if matches!(self.lexer.clone().next_token().ok(), Some(Token::Function)) =>
            {
                self.advance()?;
                let mut func = self.parse_function_declaration()?;
                func.is_async = true;
                Statement::FunctionDeclaration(func)
            }
            Some(Token::Class) => Statement::ClassDeclaration(self.parse_class_declaration(true)?),
            _ => {
                return Err(ParseError::UnexpectedToken {
                    expected: "export declaration".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
            }
        };

        Ok(Statement::ExportNamedDeclaration(ExportNamedDeclaration {
            declaration: Some(Box::new(declaration)),
            specifiers: vec![],
            source: None,
        }))
    }

    fn parse_export_specifiers(&mut self) -> Result<Vec<ExportSpecifier<'a>>, ParseError> {
        self.consume_opt(Token::LBrace)?;
        let mut specifiers = Vec::new();

        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            let local = self.parse_module_name()?;
            let exported = if self.current_identifier_name_is("as") {
                self.advance()?;
                self.parse_module_name()?
            } else {
                local
            };
            specifiers.push(ExportSpecifier { local, exported });

            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }

        self.consume_opt(Token::RBrace)?;
        Ok(specifiers)
    }

    fn parse_export_default_declaration(&mut self) -> Result<Statement<'a>, ParseError> {
        let declaration = match self.current_token {
            Some(Token::Async)
                if matches!(self.lexer.clone().next_token().ok(), Some(Token::Function)) =>
            {
                self.advance()?;
                let mut func = self.parse_function_declaration()?;
                func.is_async = true;
                ExportDefaultKind::FunctionDeclaration(func)
            }
            Some(Token::Function) => {
                ExportDefaultKind::FunctionDeclaration(self.parse_function_declaration()?)
            }
            Some(Token::Class) => {
                ExportDefaultKind::ClassDeclaration(self.parse_class_declaration(false)?)
            }
            _ => {
                let expr = self.parse_assignment_expression()?;
                self.consume_opt(Token::Semicolon)?;
                ExportDefaultKind::Expression(expr)
            }
        };

        Ok(Statement::ExportDefaultDeclaration(
            ExportDefaultDeclaration { declaration },
        ))
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
        let is_await = self.consume_opt(Token::Await)?;
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
                if is_await {
                    return Err(ParseError::UnexpectedToken {
                        expected: "of".to_string(),
                        found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                    });
                }
                self.validate_for_in_of_left(&init)?;
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
                self.validate_for_in_of_left(&init)?;
                self.advance()?;
                let right = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Statement::ForOfStatement(ForOfStatement {
                    left: init,
                    right,
                    body,
                    is_await,
                }));
            }

            if is_await {
                return Err(ParseError::UnexpectedToken {
                    expected: "of".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
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
            if let Statement::VariableDeclaration(decl) = init.as_ref() {
                self.validate_const_declaration(decl, false)?;
            }
            Ok(Statement::ForStatement(ForStatement {
                init: Some(init),
                test,
                update,
                body,
            }))
        } else {
            if is_await {
                return Err(ParseError::UnexpectedToken {
                    expected: "for await initializer".to_string(),
                    found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                });
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
                    Some(Token::Identifier(_)) | Some(Token::LBracket) | Some(Token::LBrace) => {
                        Some(self.parse_binding_pattern()?)
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

    fn parse_parameter_list(&mut self) -> Result<Vec<Param<'a>>, ParseError> {
        self.consume_opt(Token::LParen)?;
        let mut params = Vec::new();
        while self.current_token != Some(Token::RParen) && self.current_token.is_some() {
            let param = self.parse_formal_parameter()?;
            let is_rest = param.is_rest;
            params.push(param);
            if is_rest || !self.consume_opt(Token::Comma)? {
                break;
            }
        }
        self.consume_opt(Token::RParen)?;
        Ok(params)
    }

    fn parse_formal_parameter(&mut self) -> Result<Param<'a>, ParseError> {
        let is_rest = self.consume_opt(Token::DotDotDot)?;
        let mut pattern = self.parse_binding_pattern()?;
        if !is_rest && self.current_token == Some(Token::Assign) {
            self.advance()?;
            let default = self.parse_assignment_expression()?;
            pattern = Expression::AssignmentExpression(Box::new(AssignmentExpression {
                operator: AssignmentOperator::Assign,
                left: pattern,
                right: default,
            }));
        }
        Ok(Param { pattern, is_rest })
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

        let params = self.parse_parameter_list()?;

        let body = self.parse_block_statement()?;

        Ok(FunctionDeclaration {
            id,
            params,
            body,
            is_generator,
            is_async: false,
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
                Some(Token::Identifier(_)) | Some(Token::LBracket) | Some(Token::LBrace) => {
                    self.parse_binding_pattern()?
                }
                _ => break, // just break if it's not a binding pattern (robustness)
            };

            let init = if self.current_token == Some(Token::Assign) {
                self.advance()?;
                Some(self.parse_expression()?)
            } else if self.current_token == Some(Token::PlusAssign)
                || self.current_token == Some(Token::MinusAssign)
                || self.current_token == Some(Token::MultiplyAssign)
                || self.current_token == Some(Token::DivideAssign)
                || self.current_token == Some(Token::PercentAssign)
                || self.current_token == Some(Token::PowerAssign)
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

    fn parse_binding_pattern(&mut self) -> Result<Expression<'a>, ParseError> {
        match self.current_token {
            Some(Token::Identifier(name)) => {
                self.advance()?;
                Ok(Expression::Identifier(name))
            }
            Some(Token::LBracket) => self.parse_array_binding_pattern(),
            Some(Token::LBrace) => self.parse_object_binding_pattern(),
            _ => Err(ParseError::UnexpectedToken {
                expected: "binding pattern".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
        }
    }

    fn parse_array_binding_pattern(&mut self) -> Result<Expression<'a>, ParseError> {
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
                let inner = self.parse_binding_pattern()?;
                let rest = Expression::SpreadElement(Box::new(inner));
                if self.current_token != Some(Token::RBracket) {
                    return Err(ParseError::InvalidRestElement);
                }
                rest
            } else {
                let mut pattern = self.parse_binding_pattern()?;
                if self.current_token == Some(Token::Assign) {
                    self.advance()?;
                    let default = self.parse_assignment_expression()?;
                    pattern = Expression::AssignmentExpression(Box::new(AssignmentExpression {
                        operator: AssignmentOperator::Assign,
                        left: pattern,
                        right: default,
                    }));
                }
                pattern
            };

            elements.push(Some(element));

            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }

        self.consume_opt(Token::RBracket)?;
        Ok(Expression::ArrayExpression(elements))
    }

    fn parse_object_binding_pattern(&mut self) -> Result<Expression<'a>, ParseError> {
        self.advance()?; // '{'
        let mut properties = Vec::new();

        while self.current_token.is_some() && self.current_token != Some(Token::RBrace) {
            if self.current_token == Some(Token::DotDotDot) {
                self.advance()?;
                let rest = self.parse_binding_pattern()?;
                if self.current_token != Some(Token::RBrace) {
                    return Err(ParseError::InvalidRestProperty);
                }
                properties.push(ObjectProperty {
                    key: ObjectKey::Identifier(""),
                    value: Expression::SpreadElement(Box::new(rest.clone())),
                    shorthand: false,
                    computed: false,
                    method: false,
                    kind: ObjectPropertyKind::Value(Expression::SpreadElement(Box::new(rest))),
                });
                if !self.consume_opt(Token::Comma)? {
                    break;
                }
                continue;
            }

            let (key, computed) = if self.current_token == Some(Token::LBracket) {
                self.advance()?;
                let expr = self.parse_expression()?;
                self.consume_opt(Token::RBracket)?;
                (ObjectKey::Computed(Box::new(expr)), true)
            } else {
                let key = match self.current_token.clone() {
                    Some(Token::String(name)) => {
                        self.advance()?;
                        ObjectKey::String(name)
                    }
                    Some(Token::Number(n)) => {
                        self.advance()?;
                        ObjectKey::Number(n)
                    }
                    Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                        let name = Self::token_as_identifier_name(&token).unwrap();
                        self.advance()?;
                        ObjectKey::Identifier(name)
                    }
                    Some(token) => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "object binding key".to_string(),
                            found: Some(format!("{:?}", token)),
                        });
                    }
                    None => return Err(ParseError::UnexpectedEOF),
                };
                (key, false)
            };

            let (value, shorthand) = if self.consume_opt(Token::Colon)? {
                let mut value = self.parse_binding_pattern()?;
                if self.current_token == Some(Token::Assign) {
                    self.advance()?;
                    let default = self.parse_assignment_expression()?;
                    value = Expression::AssignmentExpression(Box::new(AssignmentExpression {
                        operator: AssignmentOperator::Assign,
                        left: value,
                        right: default,
                    }));
                }
                (value, false)
            } else {
                let name = match &key {
                    ObjectKey::Identifier(name) => *name,
                    _ => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "Colon after object binding key".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                };
                let mut value = Expression::Identifier(name);
                if self.current_token == Some(Token::Assign) {
                    self.advance()?;
                    let default = self.parse_assignment_expression()?;
                    value = Expression::AssignmentExpression(Box::new(AssignmentExpression {
                        operator: AssignmentOperator::Assign,
                        left: value,
                        right: default,
                    }));
                }
                (value, true)
            };

            properties.push(ObjectProperty {
                key,
                value: value.clone(),
                shorthand,
                computed,
                method: false,
                kind: ObjectPropertyKind::Value(value),
            });

            if !self.consume_opt(Token::Comma)? {
                break;
            }
        }

        self.consume_opt(Token::RBrace)?;
        Ok(Expression::ObjectExpression(properties))
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
        if self.current_token == Some(Token::Yield) {
            self.advance()?;
            let delegate = self.consume_opt(Token::Asterisk)?;
            let argument = match self.current_token {
                Some(
                    Token::Semicolon
                    | Token::Comma
                    | Token::RParen
                    | Token::RBracket
                    | Token::RBrace,
                )
                | None => None,
                _ => Some(Box::new(self.parse_assignment_expression()?)),
            };
            return Ok(Expression::YieldExpression { argument, delegate });
        }

        let left = self.parse_conditional_expression()?;

        // single-param arrow: ident => body
        if self.current_token == Some(Token::Arrow) {
            if let Expression::Identifier(name) = left {
                self.advance()?;
                let body = self.parse_arrow_body()?;
                return Ok(Expression::ArrowFunctionExpression(Box::new(
                    FunctionDeclaration {
                        id: None,
                        params: vec![Param {
                            pattern: Expression::Identifier(name),
                            is_rest: false,
                        }],
                        body,
                        is_generator: false,
                        is_async: false,
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
            || self.current_token == Some(Token::PowerAssign)
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
                Token::PowerAssign => AssignmentOperator::PowerAssign,
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
            if matches!(operator, AssignmentOperator::Assign) {
                if !Self::is_assignment_target(&left) {
                    return Err(ParseError::InvalidAssignmentTarget);
                }
            } else if !Self::is_simple_assignment_target(&left) {
                return Err(ParseError::InvalidAssignmentTarget);
            }
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
                if !Self::is_simple_assignment_target(&argument) {
                    return Err(ParseError::InvalidUpdateTarget);
                }
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
                Token::Delete => None,
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

            if op == Token::Await {
                self.advance()?;
                let argument = self.parse_unary_expression()?;
                return Ok(Expression::AwaitExpression(Box::new(argument)));
            }

            if op == Token::Delete {
                self.advance()?;
                let argument = self.parse_unary_expression()?;
                if Self::is_private_member_expression(&argument) {
                    return Err(ParseError::InvalidPrivateIdentifierUsage(
                        "private fields cannot be deleted".to_string(),
                    ));
                }
                return Ok(Expression::UnaryExpression(Box::new(UnaryExpression {
                    operator: UnaryOperator::Delete,
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
                    Some(Token::PrivateIdentifier(name)) => {
                        self.advance()?;
                        expr = Expression::MemberExpression(Box::new(MemberExpression {
                            object: expr,
                            property: Expression::PrivateIdentifier(name),
                            computed: false,
                            optional: false,
                        }));
                    }
                    Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                        let id = Self::token_as_identifier_name(&token).unwrap();
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
                        Some(Token::PrivateIdentifier(name)) => {
                            return Err(ParseError::InvalidPrivateIdentifierUsage(format!(
                                "optional chaining cannot be used with private identifier '#{name}'"
                            )));
                        }
                        Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                            let id = Self::token_as_identifier_name(&token).unwrap();
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
            } else if matches!(self.current_token, Some(Token::Template(_, _))) {
                let parts = self.parse_template_parts()?;
                expr = Expression::TaggedTemplateExpression(Box::new(expr), parts);
            } else {
                break;
            }
        }

        if self.consume_opt(Token::PlusPlus)? {
            if !Self::is_simple_assignment_target(&expr) {
                return Err(ParseError::InvalidUpdateTarget);
            }
            expr = Expression::UpdateExpression(Box::new(UpdateExpression {
                operator: UpdateOperator::PlusPlus,
                argument: expr,
                prefix: false,
            }));
        } else if self.consume_opt(Token::MinusMinus)? {
            if !Self::is_simple_assignment_target(&expr) {
                return Err(ParseError::InvalidUpdateTarget);
            }
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

            let mut method_is_async = false;
            let mut method_is_generator = false;
            if self.current_token == Some(Token::Async) {
                let mut lookahead = self.lexer.clone();
                let next = lookahead.next_token().ok();
                let next_next = lookahead.next_token().ok();
                let next_third = lookahead.next_token().ok();
                let is_async_method = matches!(
                    (&next, &next_next),
                    (Some(token), Some(Token::LParen)) if Self::token_starts_method_key(token)
                );
                let is_async_generator = matches!(
                    (&next, &next_next, &next_third),
                    (Some(Token::Asterisk), Some(token), Some(Token::LParen))
                        if Self::token_starts_method_key(token)
                );
                if is_async_method || is_async_generator {
                    method_is_async = true;
                    method_is_generator = is_async_generator;
                    self.advance()?;
                    if method_is_generator {
                        self.advance()?;
                    }
                }
            }

            if !method_is_generator && self.current_token == Some(Token::Asterisk) {
                let mut lookahead = self.lexer.clone();
                let next = lookahead.next_token().ok();
                let next_next = lookahead.next_token().ok();
                if matches!(
                    (&next, &next_next),
                    (Some(token), Some(Token::LParen)) if Self::token_starts_method_key(token)
                ) {
                    method_is_generator = true;
                    self.advance()?;
                }
            }

            let mut accessor_kind = None;
            if !method_is_generator
                && matches!(self.current_token, Some(Token::Identifier("get" | "set")))
            {
                let marker = if matches!(self.current_token, Some(Token::Identifier("get"))) {
                    "get"
                } else {
                    "set"
                };
                let mut lookahead = self.lexer.clone();
                if let Ok(next_token) = lookahead.next_token() {
                    let is_accessor = Self::token_starts_method_key(&next_token)
                        && matches!(lookahead.next_token().ok(), Some(Token::LParen));
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
                    Some(Token::String(name)) => {
                        self.advance()?;
                        ObjectKey::String(name)
                    }
                    Some(Token::Number(n)) => {
                        self.advance()?;
                        ObjectKey::Number(n)
                    }
                    Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                        let name = Self::token_as_identifier_name(&token).unwrap();
                        self.advance()?;
                        ObjectKey::Identifier(name)
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
                let mut func = self.parse_function_body_from_params()?;
                func.is_async = method_is_async;
                func.is_generator = method_is_generator;
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
        let params = self.parse_parameter_list()?;
        let body = self.parse_block_statement()?;
        Ok(FunctionDeclaration {
            id: None,
            params,
            body,
            is_generator: false,
            is_async: false,
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

            let mut method_is_async = false;
            let mut method_is_generator = false;
            if self.current_token == Some(Token::Async) {
                let mut lookahead = self.lexer.clone();
                let next = lookahead.next_token().ok();
                let next_next = lookahead.next_token().ok();
                let next_third = lookahead.next_token().ok();
                let is_async_method = matches!(
                    (&next, &next_next),
                    (Some(token), Some(Token::LParen)) if Self::token_starts_method_key(token)
                );
                let is_async_generator = matches!(
                    (&next, &next_next, &next_third),
                    (Some(Token::Asterisk), Some(token), Some(Token::LParen))
                        if Self::token_starts_method_key(token)
                );
                if is_async_method || is_async_generator {
                    method_is_async = true;
                    method_is_generator = is_async_generator;
                    self.advance()?;
                    if method_is_generator {
                        self.advance()?;
                    }
                }
            }

            if !method_is_generator && self.current_token == Some(Token::Asterisk) {
                let mut lookahead = self.lexer.clone();
                let next = lookahead.next_token().ok();
                let next_next = lookahead.next_token().ok();
                if matches!(
                    (&next, &next_next),
                    (Some(token), Some(Token::LParen)) if Self::token_starts_method_key(token)
                ) {
                    method_is_generator = true;
                    self.advance()?;
                }
            }

            let key = match self.current_token.clone() {
                Some(Token::PrivateIdentifier(name)) => {
                    self.advance()?;
                    ObjectKey::PrivateIdentifier(name)
                }
                Some(Token::String(name)) => {
                    self.advance()?;
                    ObjectKey::String(name)
                }
                Some(Token::Number(n)) => {
                    self.advance()?;
                    ObjectKey::Number(n)
                }
                Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                    let name = Self::token_as_identifier_name(&token).unwrap();
                    self.advance()?;
                    ObjectKey::Identifier(name)
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

            if matches!(key, ObjectKey::Identifier("get" | "set"))
                && self.current_token != Some(Token::LParen)
            {
                let accessor = match key {
                    ObjectKey::Identifier("get") => "get",
                    ObjectKey::Identifier("set") => "set",
                    _ => unreachable!(),
                };
                let actual_key = match self.current_token.clone() {
                    Some(Token::PrivateIdentifier(name)) => {
                        self.advance()?;
                        ObjectKey::PrivateIdentifier(name)
                    }
                    Some(Token::String(name)) => {
                        self.advance()?;
                        ObjectKey::String(name)
                    }
                    Some(Token::Number(n)) => {
                        self.advance()?;
                        ObjectKey::Number(n)
                    }
                    Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                        let name = Self::token_as_identifier_name(&token).unwrap();
                        self.advance()?;
                        ObjectKey::Identifier(name)
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
                let mut func = self.parse_function_body_from_params()?;
                func.is_async = method_is_async;
                func.is_generator = method_is_generator;
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
                        params: vec![Param {
                            pattern: Expression::Identifier("args"),
                            is_rest: true,
                        }],
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
                        is_async: false,
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
            Some(Token::PrivateIdentifier(id)) => {
                let v = *id;
                self.advance()?;
                Ok(Expression::PrivateIdentifier(v))
            }
            Some(Token::Class) => Ok(Expression::ClassExpression(Box::new(
                self.parse_class_declaration(false)?,
            ))),
            Some(Token::Function) => {
                let func = self.parse_function_declaration()?;
                Ok(Expression::FunctionExpression(Box::new(func)))
            }
            Some(Token::Import) => {
                self.advance()?;
                Ok(Expression::Identifier("import"))
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
                if self.looks_like_parenthesized_arrow() {
                    self.parse_parenthesized_arrow_expression(false)
                } else {
                    self.parse_grouped_expression()
                }
            }
            Some(Token::LBracket) => self.parse_array_literal(),
            Some(Token::LBrace) => self.parse_object_literal(),
            Some(Token::New) => {
                self.advance()?; // 'new'
                let mut callee = self.parse_primary()?;

                loop {
                    if self.consume_opt(Token::Dot)? {
                        match self.current_token.clone() {
                            Some(token) if Self::token_as_identifier_name(&token).is_some() => {
                                let id = Self::token_as_identifier_name(&token).unwrap();
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
            | Some(Token::Eof) => Err(ParseError::UnexpectedToken {
                expected: "primary expression".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
            Some(Token::Async) => {
                let mut lookahead = self.lexer.clone();
                let next = lookahead.next_token().ok();
                let next_next = lookahead.next_token().ok();

                if matches!(next, Some(Token::Function)) {
                    self.advance()?;
                    let mut func = self.parse_function_declaration()?;
                    func.is_async = true;
                    Ok(Expression::FunctionExpression(Box::new(func)))
                } else if matches!(next, Some(Token::Identifier(_)))
                    && matches!(next_next, Some(Token::Arrow))
                {
                    self.advance()?;
                    let name = match self.current_token {
                        Some(Token::Identifier(name)) => name,
                        _ => unreachable!(),
                    };
                    self.advance()?;
                    self.consume_opt(Token::Arrow)?;
                    let body = self.parse_arrow_body()?;
                    Ok(Expression::ArrowFunctionExpression(Box::new(
                        FunctionDeclaration {
                            id: None,
                            params: vec![Param {
                                pattern: Expression::Identifier(name),
                                is_rest: false,
                            }],
                            body,
                            is_generator: false,
                            is_async: true,
                        },
                    )))
                } else if self.looks_like_parenthesized_async_arrow() {
                    self.advance()?;
                    self.parse_parenthesized_arrow_expression(true)
                } else {
                    self.advance()?;
                    Ok(Expression::Identifier("async"))
                }
            }
            Some(Token::Regex(pattern, flags)) => {
                let pattern = *pattern;
                let flags = *flags;
                self.advance()?;
                Ok(Expression::Literal(Literal::RegExp(pattern, flags)))
            }
            Some(Token::Slash) | Some(Token::DivideAssign) => Err(ParseError::UnexpectedToken {
                expected: "primary expression".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
            Some(Token::Template(_, _)) => {
                Ok(Expression::TemplateLiteral(self.parse_template_parts()?))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "primary expression".to_string(),
                found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
            }),
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

    fn parse_template_parts(&mut self) -> Result<Vec<TemplatePart<'a>>, ParseError> {
        let mut parts = Vec::new();

        loop {
            match self.current_token.clone() {
                Some(Token::Template(chunk, is_tail)) => {
                    self.advance()?;
                    parts.push(TemplatePart::String(chunk));
                    if is_tail {
                        break;
                    }

                    let expr = self.parse_expression()?;
                    parts.push(TemplatePart::Expr(expr));

                    if !matches!(self.current_token, Some(Token::RBrace)) {
                        return Err(ParseError::UnexpectedToken {
                            expected: "RBrace".to_string(),
                            found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                        });
                    }
                    self.advance()?;
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "Template".to_string(),
                        found: self.current_token.as_ref().map(|t| format!("{:?}", t)),
                    });
                }
            }
        }

        Ok(parts)
    }
}
