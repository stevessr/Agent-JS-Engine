import re

with open("src/engine/value.rs", "r") as f:
    content = f.read()

replacement = """pub fn add(&self, other: &JsValue) -> Result<JsValue, RuntimeError> {
        match (self, other) {
            (JsValue::String(s1), _) => {
                let s2 = other.as_string();
                if s1.len() + s2.len() > 500_000 { return Err(RuntimeError::ReferenceError("OOM Limit".into())); }
                Ok(JsValue::String(s1.clone() + &s2))
            },
            (_, JsValue::String(s2)) => {
                let s1 = self.as_string();
                if s1.len() + s2.len() > 500_000 { return Err(RuntimeError::ReferenceError("OOM Limit".into())); }
                Ok(JsValue::String(s1 + s2))
            },
            _ => Ok(JsValue::Number(self.as_number() + other.as_number())),
        }
    }"""

content = re.sub(
    r"pub fn add\(&self, other: &JsValue\) -> Result<JsValue, RuntimeError> \{.*?_ => Ok\(JsValue::Number\(self\.as_number\(\) \+ other\.as_number\(\)\)\),\n        \}",
    replacement,
    content,
    flags=re.DOTALL
)

with open("src/engine/value.rs", "w") as f:
    f.write(content)

with open("src/engine/interpreter.rs", "r") as f:
    interp = f.read()

interp_replacement = """Expression::UpdateExpression(update) => {
                let id = match &update.argument {
                    Expression::Identifier(name) => name,
                    _ => return Ok(JsValue::Undefined),
                };
                let current_val = env.borrow().get(id).unwrap_or(JsValue::Undefined).as_number();
                let new_val = if update.operator == ai_agent::parser::ast::UpdateOperator::Increment { current_val + 1.0 } else { current_val - 1.0 };
                env.borrow_mut().set(id, JsValue::Number(new_val)).ok();
                if update.prefix {
                    Ok(JsValue::Number(new_val))
                } else {
                    Ok(JsValue::Number(current_val))
                }
            }"""

interp = re.sub(
    r"Expression::UpdateExpression\(update\) => \{\s*// Dummy for now\s*Ok\(JsValue::Undefined\)\s*\}",
    interp_replacement,
    interp,
    flags=re.DOTALL
)

with open("src/engine/interpreter.rs", "w") as f:
    f.write(interp)

