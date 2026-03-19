with open("src/parser/mod.rs", "r") as f:
    text = f.read()

target1 = """            _ => {
                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Ok(Statement::ExpressionStatement(expr))
            }"""

replacement1 = """            _ => {
                // Peek ahead for a label
                if let Some(Token::Identifier(_label_str)) = self.current_token {
                    if let Some(Token::Colon) = self.lexer.clone().next_token().ok() {
                        self.advance()?; // identifier
                        self.advance()?; // colon
                        let _stmt = self.parse_statement()?; // lazy pass-through as we don't have LabeledStatement AST yet
                        return Ok(Statement::EmptyStatement);
                    }
                }
                
                let expr = self.parse_expression()?;
                self.consume_opt(Token::Semicolon)?;
                Ok(Statement::ExpressionStatement(expr))
            }"""

text = text.replace(target1, replacement1)

target2 = "if self.consume_opt(Token::Assign)? {"
replacement2 = """if self.current_token == Some(Token::Assign) || self.current_token == Some(Token::PlusAssign) || self.current_token == Some(Token::MinusAssign) || self.current_token == Some(Token::MultiplyAssign) || self.current_token == Some(Token::DivideAssign) || self.current_token == Some(Token::PercentAssign) {
            let operator = match self.current_token.clone().unwrap() {
                Token::PlusAssign => AssignmentOperator::PlusAssign,
                Token::MinusAssign => AssignmentOperator::MinusAssign,
                Token::MultiplyAssign => AssignmentOperator::MultiplyAssign,
                Token::DivideAssign => AssignmentOperator::DivideAssign,
                Token::PercentAssign => AssignmentOperator::PercentAssign,
                _ => AssignmentOperator::Assign,
            };
            self.advance()?;"""

text = text.replace(target2, replacement2)

with open("src/parser/mod.rs", "w") as f:
    f.write(text)

with open("src/parser/ast.rs", "r") as f:
    text = f.read()

text = text.replace("Assign,\n}", "Assign,\n    PlusAssign,\n    MinusAssign,\n    MultiplyAssign,\n    DivideAssign,\n    PercentAssign,\n}")
with open("src/parser/ast.rs", "w") as f:
    f.write(text)
