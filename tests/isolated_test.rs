use ai_agent::engine::JsEngine;

#[test]
fn engine_executes_basic_javascript() {
    let engine = JsEngine::new();
    let output = engine.eval("const answer = 40 + 2; answer;").unwrap();

    assert_eq!(output.value.as_deref(), Some("42"));
}

#[test]
fn engine_captures_print_output() {
    let engine = JsEngine::new();
    let output = engine.eval("print('hello');").unwrap();

    assert_eq!(output.printed, vec!["hello".to_string()]);
}
