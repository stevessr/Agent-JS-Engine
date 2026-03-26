#[cfg(test)]
mod tests {
    use ai_agent::engine::interpreter::Interpreter;
    use ai_agent::engine::value::JsValue;
    use ai_agent::lexer::Lexer;
    use ai_agent::parser::Parser;

    fn eval(src: &str) -> JsValue {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer).unwrap();
        let program = parser.parse_program().unwrap();
        let mut interp = Interpreter::new();
        interp.eval_program(&program).unwrap()
    }

    #[test]
    fn interpreter_array_index_access() {
        assert_eq!(eval("const a = [1, 2, 3]; a[1];"), JsValue::Number(2.0));
    }

    #[test]
    fn interpreter_array_length() {
        assert_eq!(eval("const a = [10, 20]; a.length;"), JsValue::Number(2.0));
    }

    #[test]
    fn interpreter_object_dot_access() {
        assert_eq!(eval("const o = { x: 42 }; o.x;"), JsValue::Number(42.0));
    }

    #[test]
    fn interpreter_object_bracket_access() {
        assert_eq!(eval("const o = { key: 7 }; o['key'];"), JsValue::Number(7.0));
    }

    #[test]
    fn interpreter_string_length() {
        assert_eq!(eval("const s = 'hello'; s.length;"), JsValue::Number(5.0));
    }

    #[test]
    fn interpreter_nested_object_access() {
        assert_eq!(eval("const o = { inner: { v: 99 } }; o.inner.v;"), JsValue::Number(99.0));
    }

    #[test]
    fn interpreter_array_member_assign() {
        assert_eq!(eval("const a = [1, 2, 3]; a[0] = 99; a[0];"), JsValue::Number(99.0));
    }

    #[test]
    fn interpreter_object_member_assign() {
        assert_eq!(eval("const o = { x: 1 }; o.y = 5; o.y;"), JsValue::Number(5.0));
    }
}
