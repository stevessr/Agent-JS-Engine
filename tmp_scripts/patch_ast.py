import re

with open("src/parser/ast.rs", "r") as f:
    content = f.read()

content = re.sub(
    r"pub enum AssignmentOperator {.*?PercentAssign,\n\}",
    """pub enum AssignmentOperator {
    Assign,
    PlusAssign,
    MinusAssign,
    MultiplyAssign,
    DivideAssign,
    PercentAssign,
}""",
    content,
    flags=re.DOTALL
)

with open("src/parser/ast.rs", "w") as f:
    f.write(content)
