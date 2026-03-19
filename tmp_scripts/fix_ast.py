with open("src/parser/ast.rs", "r") as f:
    text = f.read()

text = text.replace("#[derive(Debug, Clone)]\n#[derive(Debug, Clone)]\npub struct ClassDeclaration", "#[derive(Debug, Clone)]\npub struct ClassDeclaration")
text = text.replace("pub struct FunctionDeclaration", "#[derive(Debug, Clone)]\npub struct FunctionDeclaration")

with open("src/parser/ast.rs", "w") as f:
    f.write(text)

