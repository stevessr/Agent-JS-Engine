import re

with open("src/engine/interpreter.rs", "r") as f:
    content = f.read()

replacement = """
impl Drop for Interpreter {
    fn drop(&mut self) {
        self.global_env.borrow_mut().variables.clear();
    }
}

"""
if "impl Drop for Interpreter" not in content:
    content += replacement

with open("src/engine/interpreter.rs", "w") as f:
    f.write(content)

with open("src/engine/env.rs", "r") as f:
    env_content = f.read()

env_content = env_content.replace(
    "variables: HashMap<String, JsValue>,",
    "pub variables: HashMap<String, JsValue>,"
)

with open("src/engine/env.rs", "w") as f:
    f.write(env_content)

