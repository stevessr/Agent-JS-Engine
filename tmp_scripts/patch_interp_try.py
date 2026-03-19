import re

with open("src/engine/interpreter.rs", "r") as f:
    content = f.read()

replacement = """                    Err(RuntimeError::Return(v)) => Err(RuntimeError::Return(v)),
                    Err(RuntimeError::Timeout) => Err(RuntimeError::Timeout),"""

content = content.replace("                    Err(RuntimeError::Return(v)) => Err(RuntimeError::Return(v)),", replacement)

with open("src/engine/interpreter.rs", "w") as f:
    f.write(content)

