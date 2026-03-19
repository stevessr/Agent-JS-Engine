import re

with open("src/engine/interpreter.rs", "r") as f:
    text = f.read()

# Add handling in eval_expression
target = "Expression::NewExpression(new_exp) => {"

replacement = """Expression::UpdateExpression(update) => {
                // Dummy for now
                Ok(JsValue::Undefined)
            }
            Expression::FunctionExpression(func) => {
                Ok(JsValue::Undefined)
            }
            Expression::ThisExpression => {
                Ok(JsValue::Undefined)
            }
            Expression::SequenceExpression(seq) => {
                let mut res = JsValue::Undefined;
                for expr in seq {
                    res = self.eval_expression(expr)?;
                }
                Ok(res)
            }
            Expression::ConditionalExpression { test, consequent, alternate } => {
                let cond = self.eval_expression(test)?;
                if cond.is_truthy() {
                    self.eval_expression(consequent)
                } else {
                    self.eval_expression(alternate)
                }
            }
            Expression::NewExpression(new_exp) => {"""

text = text.replace(target, replacement)

with open("src/engine/interpreter.rs", "w") as f:
    f.write(text)
