with open("src/parser/mod.rs", "r") as f:
    text = f.read()

target = """    fn parse_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        self.parse_assignment_expression()
    }"""

replacement = """    fn parse_expression(&mut self) -> Result<Expression<'a>, ParseError> {
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
    }"""

text = text.replace(target, replacement)

with open("src/parser/mod.rs", "w") as f:
    f.write(text)
