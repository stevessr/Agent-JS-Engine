with open("src/parser/mod.rs", "r") as f:
    text = f.read()

target = """    fn parse_assignment_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let left = self.parse_binary_expression()?;
        
        if self.consume_opt(Token::Assign)? {"""

replacement = """    fn parse_conditional_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let left = self.parse_binary_expression()?;
        if self.consume_opt(Token::Question)? {
            let consequent = self.parse_assignment_expression()?;
            if self.current_token == Some(Token::Colon) {
                self.advance()?;
            } else {
                return Err(ParseError::UnexpectedToken { expected: "Colon".to_string(), found: self.current_token.as_ref().map(|t| format!("{:?}", t)) });
            }
            let alternate = self.parse_assignment_expression()?;
            Ok(Expression::ConditionalExpression {
                test: Box::new(left),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate)
            })
        } else {
            Ok(left)
        }
    }

    fn parse_assignment_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let left = self.parse_conditional_expression()?;
        
        if self.consume_opt(Token::Assign)? {"""

text = text.replace(target, replacement)

with open("src/parser/mod.rs", "w") as f:
    f.write(text)
