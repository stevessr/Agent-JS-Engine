import re

with open("src/parser/ast.rs", "r") as f:
    text = f.read()

text = text.replace("pub struct FunctionDeclaration<'a> {", "#[derive(Debug, Clone)]\npub struct ClassDeclaration<'a> {\n    pub id: Option<&'a str>,\n}\npub struct FunctionDeclaration<'a> {")
text = text.replace("FunctionExpression(Box<FunctionDeclaration<'a>>),", "FunctionExpression(Box<FunctionDeclaration<'a>>),\n    ClassExpression(Box<ClassDeclaration<'a>>),")

with open("src/parser/ast.rs", "w") as f:
    f.write(text)

with open("src/parser/mod.rs", "r") as f:
    text = f.read()

text = text.replace("Some(Token::This) => { self.advance()?; Ok(Expression::ThisExpression) }",
                    "Some(Token::This) => { self.advance()?; Ok(Expression::ThisExpression) }\n            Some(Token::Class) => {\n                self.advance()?;\n                let mut id = None;\n                if let Some(Token::Identifier(name)) = self.current_token {\n                    id = Some(name);\n                    self.advance()?;\n                }\n                // skip everything between { and } for now to just pass the parser\n                while self.current_token != Some(Token::LBrace) && self.current_token != None {\n                    self.advance()?;\n                }\n                if self.current_token == Some(Token::LBrace) {\n                    let mut depth = 1;\n                    self.advance()?;\n                    while depth > 0 && self.current_token != None {\n                        if self.current_token == Some(Token::LBrace) { depth += 1; }\n                        if self.current_token == Some(Token::RBrace) { depth -= 1; }\n                        self.advance()?;\n                    }\n                }\n                Ok(Expression::ClassExpression(Box::new(ClassDeclaration { id })))\n            }")

with open("src/parser/mod.rs", "w") as f:
    f.write(text)


with open("src/engine/interpreter.rs", "r") as f:
    text = f.read()

text = text.replace("Expression::FunctionExpression(func) => {", "Expression::ClassExpression(_) => { Ok(JsValue::Undefined) }\n            Expression::FunctionExpression(func) => {")

with open("src/engine/interpreter.rs", "w") as f:
    f.write(text)
