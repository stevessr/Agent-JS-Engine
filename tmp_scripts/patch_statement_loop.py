import re

with open("src/parser/mod.rs", "r") as f:
    content = f.read()

replacement = """fn parse_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        match self.current_token {
            Some(Token::RParen) | Some(Token::RBracket) | Some(Token::RBrace) 
            | Some(Token::Comma) | Some(Token::Colon) => {
                // Stray punctuation recovery
                self.advance()?;
                Ok(Statement::EmptyStatement)
            }
            Some(Token::Let | Token::Var | Token::Const) => Ok(Statement::VariableDeclaration(self.parse_variable_declaration()?)),"""

content = re.sub(
    r"fn parse_statement\(&mut self\) -> Result<Statement<'a>, ParseError> \{\n        match self\.current_token \{\n            Some\(Token::Let \| Token::Var \| Token::Const\) => Ok\(Statement::VariableDeclaration\(self\.parse_variable_declaration\(\)\?\)\),",
    replacement,
    content,
    flags=re.DOTALL
)

with open("src/parser/mod.rs", "w") as f:
    f.write(content)

