import re

# 1. FIX INTERPRETER missing env arg
with open("src/engine/interpreter.rs", "r") as f:
    itxt = f.read()

itxt = itxt.replace("res = self.eval_expression(expr)?;", "res = self.eval_expression(expr, env.clone())?;")
itxt = itxt.replace("self.eval_expression(test)?;", "self.eval_expression(test, env.clone())?;")
itxt = itxt.replace("self.eval_expression(consequent)", "self.eval_expression(consequent, env.clone())")
itxt = itxt.replace("self.eval_expression(alternate)", "self.eval_expression(alternate, env.clone())")

with open("src/engine/interpreter.rs", "w") as f:
    f.write(itxt)

# 2. FIX AST double derive
with open("src/parser/ast.rs", "r") as f:
    atxt = f.read()

atxt = atxt.replace("#[derive(Debug, Clone, PartialEq)]\npub enum UpdateOperator", "#[derive(PartialEq)]\npub enum UpdateOperator")
atxt = atxt.replace("pub struct UnaryExpression", "#[derive(Debug, Clone)]\npub struct UnaryExpression")

with open("src/parser/ast.rs", "w") as f:
    f.write(atxt)
