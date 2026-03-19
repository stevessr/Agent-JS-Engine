import re

with open("src/engine/interpreter.rs", "r") as f:
    content = f.read()

content = content.replace("if self.instruction_count > 100_000 {", "if self.instruction_count > 2_000 {")

with open("src/engine/interpreter.rs", "w") as f:
    f.write(content)

