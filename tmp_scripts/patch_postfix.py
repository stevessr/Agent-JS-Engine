import re

with open("src/parser/mod.rs", "r") as f:
    text = f.read()

# At the end of `parse_member_or_call_expression` it returns `Ok(expr)`.
# We should intercept postfix ++ and --.
target = """        Ok(expr)
    }

    fn parse_array_literal"""

replacement = """        if self.consume_opt(Token::PlusPlus)? {
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

    fn parse_array_literal"""

text = text.replace(target, replacement)

with open("src/parser/mod.rs", "w") as f:
    f.write(text)
