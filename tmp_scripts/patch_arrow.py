import re

with open("src/parser/ast.rs", "r") as f:
    text = f.read()

# Add ArrowFunctionExpression
text = text.replace("ThisExpression,", "ThisExpression,\n    ArrowFunctionExpression(Box<FunctionDeclaration<'a>>),")

with open("src/parser/ast.rs", "w") as f:
    f.write(text)

with open("src/parser/mod.rs", "r") as f:
    text = f.read()

# In parse_primary, replace the LParen case:
target = """            Some(Token::LParen) => {
                self.advance()?;
                let expr = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                Ok(expr)
            }"""

replacement = """            Some(Token::LParen) => {
                self.advance()?;
                if self.current_token == Some(Token::RParen) {
                    self.advance()?; // consume )
                    if self.current_token == Some(Token::Arrow) {
                        self.advance()?; // consume =>
                        // Very simple dummy body
                        while self.current_token != Some(Token::LBrace) && self.current_token != None {
                            self.advance()?;
                        }
                        if self.current_token == Some(Token::LBrace) {
                            let mut depth = 1;
                            self.advance()?;
                            while depth > 0 && self.current_token != None {
                                if self.current_token == Some(Token::LBrace) { depth += 1; }
                                if self.current_token == Some(Token::RBrace) { depth -= 1; }
                                self.advance()?;
                            }
                        } else {
                            // expression body, consume it (lazy approach, let's just parse_assignment_expression)
                            self.parse_assignment_expression().ok();
                        }
                        return Ok(Expression::ArrowFunctionExpression(Box::new(FunctionDeclaration { id: None, params: vec![], body: BlockStatement { body: vec![] } })));
                    } else {
                        return Err(ParseError::UnexpectedToken { expected: "Arrow".into(), found: None });
                    }
                }
                
                let expr = self.parse_expression()?;
                self.consume_opt(Token::RParen)?;
                
                // could be (a,b) => ...
                if self.current_token == Some(Token::Arrow) {
                    self.advance()?;
                    // skip body
                    while self.current_token != Some(Token::LBrace) && self.current_token != None {
                            self.advance()?;
                        }
                        if self.current_token == Some(Token::LBrace) {
                            let mut depth = 1;
                            self.advance()?;
                            while depth > 0 && self.current_token != None {
                                if self.current_token == Some(Token::LBrace) { depth += 1; }
                                if self.current_token == Some(Token::RBrace) { depth -= 1; }
                                self.advance()?;
                            }
                        } else {
                            self.parse_assignment_expression().ok();
                        }
                    return Ok(Expression::ArrowFunctionExpression(Box::new(FunctionDeclaration { id: None, params: vec![], body: BlockStatement { body: vec![] } })));
                }
                Ok(expr)
            }"""

text = text.replace(target, replacement)

with open("src/parser/mod.rs", "w") as f:
    f.write(text)


with open("src/engine/interpreter.rs", "r") as f:
    text = f.read()

text = text.replace("Expression::ClassExpression(_) => { Ok(JsValue::Undefined) }", "Expression::ArrowFunctionExpression(_) => { Ok(JsValue::Undefined) }\n            Expression::ClassExpression(_) => { Ok(JsValue::Undefined) }")

with open("src/engine/interpreter.rs", "w") as f:
    f.write(text)

