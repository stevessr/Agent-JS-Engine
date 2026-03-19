import re

with open("src/engine/interpreter.rs", "r") as f:
    content = f.read()

content = content.replace("                    _ => unreachable!(),", "                    _ => Ok(JsValue::Undefined),")

with open("src/engine/interpreter.rs", "w") as f:
    f.write(content)

