use thiserror::Error;
use std::iter::Peekable;
use std::str::CharIndices;

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    Var, Let, Const, If, Else, Function, Return, Throw, Try, Catch, Finally,
    For, While, Do, Break, Continue, New, This, Typeof, Void, Delete, Switch,
    Case, Default, In, Instanceof, Class, Extends, Super, Yield, Await, Async,
    Import, Export, True, False, Null, Undefined, Debugger, With,
    Identifier(&'a str), Number(f64), String(&'a str), Template(&'a str), Regex(&'a str, &'a str),
    Plus, Minus, Asterisk, Slash, Percent,
    PlusPlus, MinusMinus, Power, LeftShift, RightShift, UnsignedRightShift,
    EqEq, EqEqEq, NotEq, NotEqEq, Less, LessEq, Greater, GreaterEq,
    LogicNot, LogicAnd, LogicOr, BitNot, BitAnd, BitOr, BitXor,
    Assign, PlusAssign, MinusAssign, MultiplyAssign, DivideAssign, PercentAssign,
    PowerAssign, LeftShiftAssign, RightShiftAssign, UnsignedRightShiftAssign,
    BitAndAssign, BitOrAssign, BitXorAssign, LogicAndAssign, LogicOrAssign, NullishAssign,
    Nullish, OptionalChain, Arrow, Semicolon, Comma, Dot, DotDotDot, Colon, Question,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Eof,
}

#[derive(Error, Debug, PartialEq)]
pub enum LexerError {
    #[error("Unexpected character: {0}")]
    UnexpectedCharacter(char),
    #[error("Unterminated string")]
    UnterminatedString,
    #[error("Unterminated multi-line comment")]
    UnterminatedComment,
    #[error("Unterminated template literal")]
    UnterminatedTemplate,
    #[error("Unterminated regular expression")]
    UnterminatedRegex,
}

#[derive(Clone)]
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') => {
                    if self.peek_n(1) == Some('/') {
                        self.advance();
                        self.advance();
                        while let Some(c) = self.advance() {
                            if c == '\n' || c == '\r' { break; }
                        }
                    } else if self.peek_n(1) == Some('*') {
                        self.advance();
                        self.advance();
                        let mut prev_ast = false;
                        while let Some(c) = self.advance() {
                            if prev_ast && c == '/' { break; }
                            prev_ast = c == '*';
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Token<'a>, LexerError> {
        self.skip_whitespace_and_comments();
        if self.pos >= self.input.len() {
            return Ok(Token::Eof);
        }

        let start = self.pos;
        let c = self.peek().unwrap();

        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            return Ok(self.lex_identifier());
        }

        if c.is_ascii_digit() || (c == '.' && self.peek_n(1).map_or(false, |next_c| next_c.is_ascii_digit())) {
            return Ok(self.lex_number());
        }

        if c == '"' || c == '\'' {
            return self.lex_string(c);
        }

        self.advance(); // consume first char of operator

        match c {
            '+' => {
                if self.peek() == Some('+') { self.advance(); Ok(Token::PlusPlus) }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::PlusAssign) }
                else { Ok(Token::Plus) }
            }
            '-' => {
                if self.peek() == Some('-') { self.advance(); Ok(Token::MinusMinus) }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::MinusAssign) }
                else { Ok(Token::Minus) }
            }
            '*' => {
                if self.peek() == Some('*') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::PowerAssign) }
                    else { Ok(Token::Power) }
                }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::MultiplyAssign) }
                else { Ok(Token::Asterisk) }
            }
            '/' => {
                if self.peek() == Some('=') { self.advance(); Ok(Token::DivideAssign) }
                else { Ok(Token::Slash) }
            }
            '%' => {
                if self.peek() == Some('=') { self.advance(); Ok(Token::PercentAssign) }
                else { Ok(Token::Percent) }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::EqEqEq) }
                    else { Ok(Token::EqEq) }
                }
                else if self.peek() == Some('>') { self.advance(); Ok(Token::Arrow) }
                else { Ok(Token::Assign) }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::NotEqEq) }
                    else { Ok(Token::NotEq) }
                }
                else { Ok(Token::LogicNot) }
            }
            '<' => {
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::LeftShiftAssign) }
                    else { Ok(Token::LeftShift) }
                }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::LessEq) }
                else { Ok(Token::Less) }
            }
            '>' => {
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        if self.peek() == Some('=') { self.advance(); Ok(Token::UnsignedRightShiftAssign) }
                        else { Ok(Token::UnsignedRightShift) }
                    }
                    else if self.peek() == Some('=') { self.advance(); Ok(Token::RightShiftAssign) }
                    else { Ok(Token::RightShift) }
                }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::GreaterEq) }
                else { Ok(Token::Greater) }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::LogicAndAssign) }
                    else { Ok(Token::LogicAnd) }
                }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::BitAndAssign) }
                else { Ok(Token::BitAnd) }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::LogicOrAssign) }
                    else { Ok(Token::LogicOr) }
                }
                else if self.peek() == Some('=') { self.advance(); Ok(Token::BitOrAssign) }
                else { Ok(Token::BitOr) }
            }
            '^' => {
                if self.peek() == Some('=') { self.advance(); Ok(Token::BitXorAssign) }
                else { Ok(Token::BitXor) }
            }
            '~' => Ok(Token::BitNot),
            '?' => {
                if self.peek() == Some('?') {
                    self.advance();
                    if self.peek() == Some('=') { self.advance(); Ok(Token::NullishAssign) }
                    else { Ok(Token::Nullish) }
                }
                else if self.peek() == Some('.') {
                    // Could be optional chain
                    let next = self.peek_n(1);
                    if let Some(n) = next {
                        if !n.is_ascii_digit() {
                            self.advance();
                            return Ok(Token::OptionalChain);
                        }
                    }
                    Ok(Token::Question)
                }
                else { Ok(Token::Question) }
            }
            '.' => {
                if self.peek() == Some('.') && self.peek_n(1) == Some('.') {
                    self.advance(); self.advance();
                    Ok(Token::DotDotDot)
                } else {
                    Ok(Token::Dot)
                }
            }
            ';' => Ok(Token::Semicolon),
            ',' => Ok(Token::Comma),
            ':' => Ok(Token::Colon),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '{' => Ok(Token::LBrace),
            '}' => Ok(Token::RBrace),
            '[' => Ok(Token::LBracket),
            ']' => Ok(Token::RBracket),
            _ => Err(LexerError::UnexpectedCharacter(c)),
        }
    }

    fn lex_identifier(&mut self) -> Token<'a> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                self.advance();
            } else {
                break;
            }
        }
        let term = &self.input[start..self.pos];
        match term {
            "var" => Token::Var, "let" => Token::Let, "const" => Token::Const,
            "if" => Token::If, "else" => Token::Else, "function" => Token::Function,
            "return" => Token::Return, "throw" => Token::Throw, "try" => Token::Try,
            "catch" => Token::Catch, "finally" => Token::Finally, "for" => Token::For,
            "while" => Token::While, "do" => Token::Do, "break" => Token::Break,
            "continue" => Token::Continue, "new" => Token::New, "this" => Token::This,
            "typeof" => Token::Typeof, "void" => Token::Void, "delete" => Token::Delete,
            "switch" => Token::Switch, "case" => Token::Case, "default" => Token::Default,
            "in" => Token::In, "instanceof" => Token::Instanceof, "class" => Token::Class,
            "extends" => Token::Extends, "super" => Token::Super, "yield" => Token::Yield,
            "await" => Token::Await, "async" => Token::Async, "import" => Token::Import,
            "export" => Token::Export, "true" => Token::True, "false" => Token::False,
            "null" => Token::Null, "undefined" => Token::Undefined, "debugger" => Token::Debugger,
            "with" => Token::With,
            _ => Token::Identifier(term),
        }
    }

    fn lex_number(&mut self) -> Token<'a> {
        let start = self.pos;
        if self.peek() == Some('0') {
            let next = self.peek_n(1);
            if next == Some('x') || next == Some('X') {
                self.advance(); self.advance(); // consume 0x
                while let Some(c) = self.peek() {
                    if c.is_ascii_hexdigit() { self.advance(); } else { break; }
                }
                let val = i64::from_str_radix(&self.input[start+2..self.pos], 16).unwrap_or(0);
                return Token::Number(val as f64);
            } else if next == Some('o') || next == Some('O') {
                self.advance(); self.advance(); // consume 0o
                while let Some(c) = self.peek() {
                    if c >= '0' && c <= '7' { self.advance(); } else { break; }
                }
                let val = i64::from_str_radix(&self.input[start+2..self.pos], 8).unwrap_or(0);
                return Token::Number(val as f64);
            } else if next == Some('b') || next == Some('B') {
                self.advance(); self.advance(); // consume 0b
                while let Some(c) = self.peek() {
                    if c == '0' || c == '1' { self.advance(); } else { break; }
                }
                let val = i64::from_str_radix(&self.input[start+2..self.pos], 2).unwrap_or(0);
                return Token::Number(val as f64);
            }
        }

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || (c == '-' || c == '+') { // simple float handling
                self.advance();
            } else {
                break;
            }
        }
        let term = &self.input[start..self.pos];
        let val = term.parse::<f64>().unwrap_or(f64::NAN);
        Token::Number(val)
    }

    fn lex_string(&mut self, quote: char) -> Result<Token<'a>, LexerError> {
        self.advance(); // consume opening quote
        let start = self.pos;
        let mut escaped = false;
        while let Some(c) = self.advance() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == quote {
                let s = &self.input[start..self.pos - 1];
                return Ok(Token::String(s));
            }
        }
        Err(LexerError::UnterminatedString)
    }
}
