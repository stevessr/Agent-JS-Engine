use ai_agent::engine::Interpreter;
use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{Expression, Param};

#[test]
fn debug_function_params_are_stored() {
    let source = r#"
    function add(a, b) {
        return b;
    }
    add(1, 42);
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();
    let mut interpreter = Interpreter::new();
    let _ = interpreter.eval_program(&program);
    assert_eq!(interpreter.functions.len(), 1);
    assert!(matches!(
        interpreter.functions[0].params[0],
        Param {
            pattern: Expression::Identifier("a"),
            is_rest: false
        }
    ));
    assert!(matches!(
        interpreter.functions[0].params[1],
        Param {
            pattern: Expression::Identifier("b"),
            is_rest: false
        }
    ));
}
