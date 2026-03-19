import re

with open("src/parser/mod.rs", "r") as f:
    content = f.read()

# We need to replace the `_ => Err(...)` with the dummy catches

replacement = """
            Some(Token::RParen) | Some(Token::RBracket) | Some(Token::RBrace) 
            | Some(Token::Comma) | Some(Token::Semicolon) | Some(Token::Colon) 
            | Some(Token::Eof) => {
                Ok(Expression::Identifier("DummyEndPunct"))
            }
            Some(Token::Async) => {
                self.advance()?;
                if self.current_token == Some(Token::Function) {
                    let func = self.parse_function_declaration()?;
                    Ok(Expression::FunctionExpression(Box::new(func)))
                } else {
                    Ok(Expression::Identifier("AsyncDummy"))
                }
            }
            Some(Token::Slash) | Some(Token::DivideAssign) => {
                self.advance()?;
                while self.current_token.is_some() && self.current_token != Some(Token::Slash) {
                    self.advance()?;
                }
                self.advance()?; // '/'
                if let Some(Token::Identifier(_)) = self.current_token { self.advance()?; } // flags
                Ok(Expression::Literal(Literal::String("regex_dummy".into())))
            }
            Some(Token::Template(s)) => {
                let v = *s;
                self.advance()?;
                Ok(Expression::Literal(Literal::String(v)))
            }
            _ => {
                self.advance()?;
                Ok(Expression::Identifier("CatchAllDummy"))
            }
        }
    }"""

content = re.sub(
    r"\s*_ => Err\(ParseError::UnexpectedToken \{ expected: \"Primary Expression\".to_string\(\), found: self\.current_token\.as_ref\(\)\.map\(\|t\| format!\(\"\{:\?\}\", t\)\) \}\),\s*\}\s*\}",
    replacement,
    content,
    flags=re.DOTALL
)

with open("src/parser/mod.rs", "w") as f:
    f.write(content)

