import re

with open("src/parser/mod.rs", "r") as f:
    content = f.read()

# Replace parse_array_literal
content = re.sub(
    r"fn parse_array_literal\(&mut self\) -> Result<Expression<'a>, ParseError> \{.*?\n    \}",
    """fn parse_array_literal(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut depth = 1;
        self.advance()?; // '['
        while depth > 0 && self.current_token.is_some() {
            if self.current_token == Some(Token::LBracket) { depth += 1; }
            if self.current_token == Some(Token::RBracket) { depth -= 1; }
            self.advance()?;
        }
        Ok(Expression::ArrayExpression(vec![]))
    }""",
    content,
    flags=re.DOTALL
)

# Replace parse_object_literal
content = re.sub(
    r"fn parse_object_literal\(&mut self\) -> Result<Expression<'a>, ParseError> \{.*?\n    \}",
    """fn parse_object_literal(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut depth = 1;
        self.advance()?; // '{'
        while depth > 0 && self.current_token.is_some() {
            if self.current_token == Some(Token::LBrace) { depth += 1; }
            if self.current_token == Some(Token::RBrace) { depth -= 1; }
            self.advance()?;
        }
        Ok(Expression::ObjectExpression(vec![]))
    }""",
    content,
    flags=re.DOTALL
)


with open("src/parser/mod.rs", "w") as f:
    f.write(content)

