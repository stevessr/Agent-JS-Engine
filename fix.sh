sed -i 's/AndAnd/LogicAnd/g' src/lexer/mod.rs
sed -i 's/OrOr/LogicOr/g' src/lexer/mod.rs
sed -i 's/Not,/LogicNot,/g' src/lexer/mod.rs
sed -i 's/Token::Not,/Token::LogicNot,/g' src/lexer/mod.rs
sed -i 's/Token::Not =>/Token::LogicNot =>/g' src/lexer/mod.rs
sed -i 's/Typeof,/Typeof, Void, Delete,/g' src/lexer/mod.rs
sed -i 's/"typeof" => Token::Typeof,/"typeof" => Token::Typeof, "void" => Token::Void, "delete" => Token::Delete,/g' src/lexer/mod.rs
