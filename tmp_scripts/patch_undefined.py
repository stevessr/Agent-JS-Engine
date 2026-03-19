with open("src/parser/mod.rs", "r") as f:
    text = f.read()

text = text.replace("Some(Token::Null) => { self.advance()?; Ok(Expression::Literal(Literal::Null)) }",
                    "Some(Token::Null) => { self.advance()?; Ok(Expression::Literal(Literal::Null)) }\n            Some(Token::Undefined) => { self.advance()?; Ok(Expression::Identifier(\"undefined\")) }")

with open("src/parser/mod.rs", "w") as f:
    f.write(text)
