import re

with open("src/parser/mod.rs", "r") as f:
    text = f.read()

# Make parse_primary handle 'this' and 'function'
primary_match = re.search(r'fn parse_primary\(&mut self\) -> Result<Expression<\'a>, ParseError> \{.*?_ => Err\(ParseError::UnexpectedToken', text, re.DOTALL)
if primary_match:
    primary = primary_match.group(0)
    primary = primary.replace("Some(Token::Null) => { self.advance()?; Ok(Expression::Literal(Literal::Null)) }",
                              "Some(Token::Null) => { self.advance()?; Ok(Expression::Literal(Literal::Null)) }\n            Some(Token::This) => { self.advance()?; Ok(Expression::ThisExpression) }\n            Some(Token::Function) => { let func = self.parse_function_declaration()?; Ok(Expression::FunctionExpression(Box::new(func))) }")
    text = text.replace(primary_match.group(0), primary)

# Now about UpdateExpression (++ / -- preceding an expression, handled around parse_unary_expression)
# let's just do an update to parse_unary_expression
text = text.replace("""fn parse_unary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        if let Some(op) = self.current_token.clone() {""", """fn parse_unary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        if let Some(op) = self.current_token.clone() {
            if op == Token::PlusPlus || op == Token::MinusMinus {
                let operator = if op == Token::PlusPlus { UpdateOperator::PlusPlus } else { UpdateOperator::MinusMinus };
                self.advance()?;
                let argument = self.parse_unary_expression()?;
                return Ok(Expression::UpdateExpression(Box::new(UpdateExpression {
                    operator,
                    argument,
                    prefix: true,
                })));
            }
""")

with open("src/parser/mod.rs", "w") as f:
    f.write(text)
