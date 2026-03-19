import re

with open("src/parser/ast.rs", "r") as f:
    content = f.read()

content = re.sub(
    r"pub enum BinaryOperator \{.*?\n\}",
    """pub enum BinaryOperator {
    #[default]
    Plus,
    Minus,
    Multiply,
    Divide,
    EqEq,
    EqEqEq,
    NotEq,
    NotEqEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    LogicAnd,
    LogicOr,
    NullishCoalescing,
    Instanceof,
    In,
    Power,
    Percent,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    LogicalShiftRight,
}""",
    content,
    flags=re.DOTALL
)

with open("src/parser/ast.rs", "w") as f:
    f.write(content)

with open("src/parser/mod.rs", "r") as f:
    content = f.read()

# Refactor parse_binary_expression to just match and map
replacement = """fn parse_binary_expression(&mut self) -> Result<Expression<'a>, ParseError> {
        let mut left = self.parse_unary_expression()?;
        loop {
            let operator = match &self.current_token {
                Some(Token::Plus) => BinaryOperator::Plus,
                Some(Token::Minus) => BinaryOperator::Minus,
                Some(Token::Multiply) => BinaryOperator::Multiply,
                Some(Token::Divide) => BinaryOperator::Divide,
                Some(Token::StrictEqual) => BinaryOperator::EqEqEq,
                Some(Token::Equal) => BinaryOperator::EqEq,
                Some(Token::NotEqual) => BinaryOperator::NotEq,
                Some(Token::StrictNotEqual) => BinaryOperator::NotEqEq,
                Some(Token::LessThan) => BinaryOperator::Less,
                Some(Token::LessThanOrEqual) => BinaryOperator::LessEq,
                Some(Token::GreaterThan) => BinaryOperator::Greater,
                Some(Token::GreaterThanOrEqual) => BinaryOperator::GreaterEq,
                Some(Token::LogicalAnd) => BinaryOperator::LogicAnd,
                Some(Token::LogicalOr) => BinaryOperator::LogicOr,
                Some(Token::NullishCoalescing) => BinaryOperator::NullishCoalescing,
                Some(Token::Instanceof) => BinaryOperator::Instanceof,
                Some(Token::In) => BinaryOperator::In,
                Some(Token::Power) => BinaryOperator::Power,
                Some(Token::Percent) => BinaryOperator::Percent,
                Some(Token::BitwiseAnd) => BinaryOperator::BitAnd,
                Some(Token::BitwiseOr) => BinaryOperator::BitOr,
                Some(Token::BitwiseXor) => BinaryOperator::BitXor,
                Some(Token::ShiftLeft) => BinaryOperator::ShiftLeft,
                Some(Token::ShiftRight) => BinaryOperator::ShiftRight,
                Some(Token::LogicalShiftRight) => BinaryOperator::LogicalShiftRight,
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

with open("src/engine/interpreter.rs", "r") as f:
    content = f.read()

# Add to interpreter dummy handlers
content = re.sub(
    r"BinaryOperator::LogicAnd => todo!\(\),\n\s*BinaryOperator::LogicOr => todo!\(\),",
    """BinaryOperator::LogicAnd => todo!(),
                BinaryOperator::LogicOr => todo!(),
                _ => JsValue::Undefined, // Nullish, Instanceof, In, etc.""",
    content
)

with open("src/engine/interpreter.rs", "w") as f:
    f.write(content)

