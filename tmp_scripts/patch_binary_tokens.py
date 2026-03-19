import re

with open("src/parser/mod.rs", "r") as f:
    content = f.read()

replacement = """fn parse_binary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut left = self.parse_unary_expression()?;
        loop {
            let operator = match &self.current_token {
                Some(Token::Plus) => BinaryOperator::Plus,
                Some(Token::Minus) => BinaryOperator::Minus,
                Some(Token::Asterisk) => BinaryOperator::Multiply,
                Some(Token::Slash) => BinaryOperator::Divide,
                Some(Token::EqEqEq) => BinaryOperator::EqEqEq,
                Some(Token::EqEq) => BinaryOperator::EqEq,
                Some(Token::NotEq) => BinaryOperator::NotEq,
                Some(Token::NotEqEq) => BinaryOperator::NotEqEq,
                Some(Token::Less) => BinaryOperator::Less,
                Some(Token::LessEq) => BinaryOperator::LessEq,
                Some(Token::Greater) => BinaryOperator::Greater,
                Some(Token::GreaterEq) => BinaryOperator::GreaterEq,
                Some(Token::LogicAnd) => BinaryOperator::LogicAnd,
                Some(Token::LogicOr) => BinaryOperator::LogicOr,
                Some(Token::Nullish) => BinaryOperator::NullishCoalescing,
                Some(Token::Instanceof) => BinaryOperator::Instanceof,
                Some(Token::In) => BinaryOperator::In,
                Some(Token::Power) => BinaryOperator::Power,
                Some(Token::Percent) => BinaryOperator::Percent,
                Some(Token::BitAnd) => BinaryOperator::BitAnd,
                Some(Token::BitOr) => BinaryOperator::BitOr,
                Some(Token::BitXor) => BinaryOperator::BitXor,
                Some(Token::LeftShift) => BinaryOperator::ShiftLeft,
                Some(Token::RightShift) => BinaryOperator::ShiftRight,
                Some(Token::UnsignedRightShift) => BinaryOperator::LogicalShiftRight,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_unary_expression()?;
            left = Expression::BinaryExpression(Box::new(BinaryExpression {
                operator,
                left,
                right,
            }));
        }
        Ok(left)
    }"""

content = re.sub(
    r"fn parse_binary_expression\(&mut self\) -> Result<Expression<'a>, ParseError> \{.*?\n        Ok\(left\)\n    \}",
    replacement,
    content,
    flags=re.DOTALL
)

with open("src/parser/mod.rs", "w") as f:
    f.write(content)

