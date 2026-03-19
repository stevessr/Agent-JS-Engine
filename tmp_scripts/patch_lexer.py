import re

with open("src/lexer/mod.rs", "r") as f:
    code = f.read()

# I will just write a new lexer entirely to handle all Javascript tokens correctly.
# It is important to support everything: regex, templates, etc but for now let's just do a good basic lexer.
