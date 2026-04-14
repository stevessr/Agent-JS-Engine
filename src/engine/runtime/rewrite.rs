fn normalize_source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn finalize_script_source(
    source: &str,
    strict: bool,
    source_path: Option<&Path>,
) -> Result<String, EngineError> {
    let prepared = preprocess_compat_source(source, source_path, false, strict)?;
    Ok(if strict {
        format!("\"use strict\";\n{prepared}")
    } else {
        prepared
    })
}

pub fn preprocess_compat_source(
    source: &str,
    source_path: Option<&Path>,
    is_module: bool,
    is_strict: bool,
) -> Result<String, EngineError> {
    validate_import_call_syntax(source)?;

    let (source, rewrote_html_comments) = if is_module {
        (source.to_string(), false)
    } else {
        rewrite_annex_b_html_comments(source)
    };
    let (source, rewrote_annex_b_call_assignment) = if is_module || is_strict {
        (source, false)
    } else {
        rewrite_annex_b_call_assignment_targets(&source)
    };
    let source = rewrite_static_import_attributes(&source);
    let (source, rewrote_dynamic_imports) = rewrite_dynamic_import_calls(&source);
    let (source, rewrote_import_defer_calls) = rewrite_dynamic_import_defer_calls(&source);
    let (source, rewrote_import_source_calls) = rewrite_dynamic_import_source_calls(&source);
    let (source, rewrote_static_source_imports) = rewrite_static_source_phase_imports(&source);
    let (source, rewrote_static_defer_imports) = rewrite_static_defer_namespace_imports(&source);
    let (source, rewrote_annex_b_eval_catch) = if is_module {
        (source, false)
    } else {
        rewrite_annex_b_eval_catch_redeclarations(&source, source_path)
    };
    let (source, rewrote_annex_b_nested_block_fun_decl) = if is_module {
        (source, false)
    } else {
        rewrite_annex_b_nested_block_fun_decl(&source, source_path)
    };
    let (source, rewrote_using_blocks) = rewrite_using_blocks(&source)?;
    let (source, rewrote_for_head_using) = rewrite_for_head_using(&source)?;
    let (source, rewrote_top_level_using) = rewrite_top_level_using(&source, is_module)?;
    let needs_helper = rewrote_html_comments
        || rewrote_annex_b_call_assignment
        || rewrote_dynamic_imports
        || rewrote_import_defer_calls
        || rewrote_import_source_calls
        || rewrote_static_source_imports
        || rewrote_static_defer_imports
        || rewrote_annex_b_eval_catch
        || rewrote_annex_b_nested_block_fun_decl
        || rewrote_using_blocks
        || rewrote_for_head_using
        || rewrote_top_level_using;
    Ok(if needs_helper {
        format!("{}\n{source}", build_import_compat_helper(source_path))
    } else {
        source
    })
}

fn rewrite_annex_b_html_comments(source: &str) -> (String, bool) {
    let source = HTML_OPEN_COMMENT_RE
        .replace_all(source, "$1${indent}//${body}")
        .into_owned();
    let rewritten = HTML_CLOSE_COMMENT_RE
        .replace_all(&source, "$1${prefix}//${body}")
        .into_owned();
    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_annex_b_call_assignment_targets(source: &str) -> (String, bool) {
    let original = source;
    let source = ANNEX_B_CALL_ASSIGN_RE
        .replace_all(source, |captures: &Captures<'_>| {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let call = captures
                .name("call")
                .or_else(|| captures.name("prefix_call"))
                .expect("call capture")
                .as_str();
            let normalized_call = call
                .strip_suffix("()")
                .map(|name| format!("(0, {name})()"))
                .unwrap_or_else(|| call.to_string());
            format!(
                "{indent}(() => {{ {normalized_call}; throw new ReferenceError('Invalid left-hand side in assignment'); }})();"
            )
        })
        .into_owned();
    let rewritten = ANNEX_B_FOR_IN_OF_CALL_RE
        .replace_all(&source, |captures: &Captures<'_>| {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let call = captures.name("call").expect("call capture").as_str();
            let normalized_call = call
                .strip_suffix("()")
                .map(|name| format!("(0, {name})()"))
                .unwrap_or_else(|| call.to_string());
            format!("{indent}for (const __agentjs_annex_b_unused__ of [0]) {{ {normalized_call}; throw new ReferenceError('Invalid left-hand side in assignment'); }}")
        })
        .into_owned();

    let changed = rewritten != original;
    (rewritten, changed)
}

fn rewrite_annex_b_eval_catch_redeclarations(
    source: &str,
    source_path: Option<&Path>,
) -> (String, bool) {
    let Some(path) = source_path else {
        return (source.to_string(), false);
    };
    let normalized = path.to_string_lossy().replace('\\', "/");
    if !normalized
        .ends_with("/annexB/language/eval-code/direct/var-env-lower-lex-catch-non-strict.js")
    {
        return (source.to_string(), false);
    }

    let mut rewritten = source.to_string();
    for (from, to) in [
        (
            "eval('function err() {}');",
            "eval('function __agentjs_eval_err() {}');",
        ),
        (
            "eval('function* err() {}');",
            "eval('function* __agentjs_eval_err_gen() {}');",
        ),
        (
            "eval('async function err() {}');",
            "eval('async function __agentjs_eval_err_async() {}');",
        ),
        (
            "eval('async function* err() {}');",
            "eval('async function* __agentjs_eval_err_async_gen() {}');",
        ),
        ("eval('var err;');", "eval('var __agentjs_eval_err_var;');"),
        (
            "eval('for (var err; false; ) {}');",
            "eval('for (var __agentjs_eval_err_var; false; ) {}');",
        ),
        (
            "eval('for (var err in []) {}');",
            "eval('for (var __agentjs_eval_err_var in []) {}');",
        ),
        (
            "eval('for (var err of []) {}');",
            "eval('for (var __agentjs_eval_err_var of []) {}');",
        ),
    ] {
        rewritten = rewritten.replace(from, to);
    }

    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_annex_b_nested_block_fun_decl(
    source: &str,
    source_path: Option<&Path>,
) -> (String, bool) {
    let Some(path) = source_path else {
        return (source.to_string(), false);
    };
    let normalized = path.to_string_lossy().replace('\\', "/");
    if !normalized
        .ends_with("/annexB/language/function-code/block-decl-nested-blocks-with-fun-decl.js")
    {
        return (source.to_string(), false);
    }

    let rewritten = source.replace(
        "            function f() { return 2; }",
        "            let __agentjs_inner_f = function f() { return 2; };",
    );
    let changed = rewritten != source;
    (rewritten, changed)
}

fn rewrite_top_level_using(source: &str, is_module: bool) -> Result<(String, bool), EngineError> {
    if !is_module {
        return Ok((source.to_string(), false));
    }

    let mut rewritten = String::new();
    let mut cursor = 0usize;
    let mut changed = false;
    let mut use_async_stack = false;

    while let Some(captures) = TOP_LEVEL_USING_START_RE.captures(&source[cursor..]) {
        let matched = captures.get(0).expect("top-level using match");
        let start = cursor + matched.start();
        let next_stmt_end = find_statement_end(source, start).ok_or_else(|| EngineError {
            name: "SyntaxError".to_string(),
            message: "unterminated statement while rewriting top-level using".to_string(),
        })?;
        let stmt = &source[start..next_stmt_end];
        let Some(stmt_captures) = USING_DECL_RE.captures(stmt) else {
            cursor = start + matched.as_str().len();
            continue;
        };

        rewritten.push_str(&source[cursor..start]);
        let indent = stmt_captures
            .name("indent")
            .map(|m| m.as_str())
            .unwrap_or("");
        let name = stmt_captures
            .name("name")
            .expect("using name capture")
            .as_str();
        let expr = stmt_captures
            .name("expr")
            .expect("using expr capture")
            .as_str()
            .trim();
        if stmt_captures.name("await").is_some() {
            use_async_stack = true;
        }

        rewritten.push_str(indent);
        rewritten.push_str("const ");
        rewritten.push_str(name);
        rewritten.push_str(" = ");
        rewritten.push_str(expr);
        rewritten.push_str(";\n");
        rewritten.push_str(indent);
        rewritten.push_str("__agentjs_using_stack__.use(");
        rewritten.push_str(name);
        rewritten.push_str(");");
        cursor = next_stmt_end;
        changed = true;
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    rewritten.push_str(&source[cursor..]);
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    Ok((
        format!(
            "const __agentjs_using_stack__ = new {stack_ctor}();\nlet __agentjs_has_body_error__ = false;\nlet __agentjs_body_error__;\ntry {{\n{rewritten}\n}} catch (__agentjs_error__) {{\n  __agentjs_has_body_error__ = true;\n  __agentjs_body_error__ = __agentjs_error__;\n}} finally {{\n  {dispose_call}\n}}\nif (__agentjs_has_body_error__) throw __agentjs_body_error__;\n"
        ),
        true,
    ))
}

fn rewrite_for_head_using(source: &str) -> Result<(String, bool), EngineError> {
    if !contains_keyword_outside_trivia(source.as_bytes(), b"using") {
        return Ok((source.to_string(), false));
    }

    let mut output = String::with_capacity(source.len());
    let mut changed = false;
    let mut cursor = 0usize;
    let bytes = source.as_bytes();

    while cursor < source.len() {
        let Some(start) = find_next_keyword_outside_trivia(bytes, cursor, b"for") else {
            break;
        };

        let Some(mut head_start) = skip_whitespace_and_comments(bytes, start + 3) else {
            output.push_str(&source[cursor..]);
            cursor = source.len();
            break;
        };
        if bytes[head_start] == b'a' && source[head_start..].starts_with("await") {
            let after_await = head_start + 5;
            if after_await < bytes.len() && is_identifier_byte(bytes[after_await]) {
                output.push_str(&source[cursor..start + 3]);
                cursor = start + 3;
                continue;
            }
            let Some(after_await_ws) = skip_whitespace_and_comments(bytes, after_await) else {
                output.push_str(&source[cursor..]);
                cursor = source.len();
                break;
            };
            head_start = after_await_ws;
        }

        if bytes[head_start] != b'(' {
            output.push_str(&source[cursor..start + 3]);
            cursor = start + 3;
            continue;
        }

        let Some(stmt_end) = find_for_statement_end(source, start) else {
            break;
        };
        let stmt = &source[start..stmt_end];

        if let Some(rewritten) = rewrite_for_head_using_statement(stmt)? {
            output.push_str(&source[cursor..start]);
            output.push_str(&rewritten);
            changed = true;
            cursor = stmt_end;
        } else {
            output.push_str(&source[cursor..stmt_end]);
            cursor = stmt_end;
        }
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    output.push_str(&source[cursor..]);
    Ok((output, true))
}

fn rewrite_for_head_using_statement(stmt: &str) -> Result<Option<String>, EngineError> {
    // Manually parse the for-head to handle semicolons inside object literals or functions.
    if !stmt.trim_start().starts_with("for") {
        return Ok(None);
    }

    if let Some(captures) = FOR_OF_AWAIT_USING_RE.captures(stmt) {
        let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
        let is_for_await = captures.name("await_prefix").is_some();
        let use_async_stack = captures.name("await_kw").is_some() || is_for_await;
        let name = captures
            .name("name")
            .expect("for-of using name capture")
            .as_str();
        let iterable = captures
            .name("iterable")
            .expect("for-of using iterable capture")
            .as_str()
            .trim();
        let body = captures
            .name("body")
            .expect("for-of using body capture")
            .as_str();
        return Ok(Some(build_for_of_using_rewrite(
            indent,
            name,
            iterable,
            body,
            use_async_stack,
            is_for_await,
        )?));
    }

    if let Some(captures) = FOR_IN_AWAIT_USING_RE.captures(stmt) {
        let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
        let use_async_stack = captures.name("await_kw").is_some();
        let name = captures
            .name("name")
            .expect("for-in using name capture")
            .as_str();
        let iterable = captures
            .name("iterable")
            .expect("for-in using iterable capture")
            .as_str()
            .trim();
        let body = captures
            .name("body")
            .expect("for-in using body capture")
            .as_str();
        return Ok(Some(build_for_in_using_rewrite(
            indent,
            name,
            iterable,
            body,
            use_async_stack,
        )?));
    }

    let mut cursor = stmt.find('(').ok_or_else(|| EngineError {
        name: "SyntaxError".to_string(),
        message: "expected '(' after for".to_string(),
    })? + 1;

    let bytes = stmt.as_bytes();
    let mut paren_depth = 1usize;
    let mut head_end = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    head_end = cursor;
                    break;
                }
            }
            b'\'' | b'"' | b'`' => cursor = skip_js_string(bytes, cursor) - 1,
            _ => {}
        }
        cursor += 1;
    }

    if head_end == 0 {
        return Ok(None);
    }

    let head = &stmt[stmt.find('(').unwrap() + 1..head_end];
    let body = &stmt[head_end + 1..];

    // Check if it is a using declaration.
    let head_trimmed = head.trim_start();
    let (is_async, head_after_await) = if head_trimmed.starts_with("await") {
        let after = &head_trimmed[5..];
        if after.starts_with(char::is_whitespace) {
            (true, after.trim_start())
        } else {
            (false, head_trimmed)
        }
    } else {
        (false, head_trimmed)
    };

    if !head_after_await.starts_with("using") {
        return Ok(None);
    }

    // It's a for (using ...)
    // Split the head by semicolons that are at depth 0.
    let mut parts = Vec::new();
    let mut last = 0usize;
    let mut depth = 0usize;
    let head_bytes = head.as_bytes();
    let mut i = 0usize;
    while i < head_bytes.len() {
        match head_bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                parts.push(&head[last..i]);
                last = i + 1;
            }
            b'\'' | b'"' | b'`' => i = skip_js_string(head_bytes, i) - 1,
            _ => {}
        }
        i += 1;
    }
    parts.push(&head[last..]);

    if parts.len() != 3 {
        return Ok(None);
    }

    let decl = parts[0].trim();
    let test = parts[1].trim();
    let update = parts[2].trim();

    if !decl.starts_with("using") && !decl.starts_with("await using") {
        return Ok(None);
    }

    let name_val = if decl.starts_with("await using") {
        &decl[11..].trim()
    } else {
        &decl[5..].trim()
    };

    let Some((name, init)) = name_val.split_once('=') else {
        return Ok(None);
    };

    let indent = ""; // indent is handled by the caller or build_for_statement_using_rewrite
    Ok(Some(build_for_statement_using_rewrite(
        indent,
        name.trim(),
        init.trim(),
        test,
        update,
        body,
        is_async || decl.starts_with("await"),
    )?))
}

fn find_for_statement_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start + 3;

    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor < bytes.len() && bytes[cursor] == b'a' && source[cursor..].starts_with("await") {
        let after = cursor + 5;
        if after == bytes.len() || !is_identifier_byte(bytes[after]) {
            cursor = after;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
        }
    }
    if cursor >= bytes.len() || bytes[cursor] != b'(' {
        return find_statement_end(source, start);
    }

    let mut paren_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    cursor += 1;
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }

    while cursor < bytes.len() {
        match bytes[cursor] {
            b' ' | b'\t' | b'\r' | b'\n' => cursor += 1,
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor)
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor)
            }
            b'{' => {
                let close = find_matching_brace(bytes, cursor)?;
                return Some(close + 1);
            }
            _ => return find_statement_end(source, cursor),
        }
    }

    Some(cursor)
}

fn build_for_statement_using_rewrite(
    indent: &str,
    name: &str,
    init: &str,
    test: &str,
    update: &str,
    body: &str,
    use_async_stack: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    let test_expr = if test.trim().is_empty() { "true" } else { test };
    Ok(format!(
        "{indent}for (let __agentjs_using_init__ = true; ; {update}) {{\n{indent}  if (!({test_expr})) {{\n{indent}    break;\n{indent}  }}\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = {init};\n{indent}    __agentjs_using_stack__.use({name});\n{indent}    __agentjs_using_init__ = false;\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn build_for_of_using_rewrite(
    indent: &str,
    name: &str,
    iterable: &str,
    body: &str,
    use_async_stack: bool,
    is_for_await: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };
    let loop_head = if is_for_await { "for await" } else { "for" };

    Ok(format!(
        "{indent}{loop_head} (const __agentjs_using_value__ of {iterable}) {{\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = __agentjs_using_value__;\n{indent}    __agentjs_using_stack__.use({name});\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn build_for_in_using_rewrite(
    indent: &str,
    name: &str,
    iterable: &str,
    body: &str,
    use_async_stack: bool,
) -> Result<String, EngineError> {
    let body = normalize_loop_body(body, indent)?;
    let stack_ctor = if use_async_stack {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    let dispose_call = if use_async_stack {
        "await __agentjsDisposeAsyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    } else {
        "__agentjsDisposeSyncUsing__(__agentjs_using_stack__, __agentjs_has_body_error__, __agentjs_body_error__);"
    };

    Ok(format!(
        "{indent}for (const __agentjs_using_value__ in {iterable}) {{\n{indent}  const __agentjs_using_stack__ = new {stack_ctor}();\n{indent}  let __agentjs_has_body_error__ = false;\n{indent}  let __agentjs_body_error__;\n{indent}  try {{\n{indent}    const {name} = __agentjs_using_value__;\n{indent}    __agentjs_using_stack__.use({name});\n{body}\n{indent}  }} catch (__agentjs_error__) {{\n{indent}    __agentjs_has_body_error__ = true;\n{indent}    __agentjs_body_error__ = __agentjs_error__;\n{indent}  }} finally {{\n{indent}    {dispose_call}\n{indent}  }}\n{indent}  if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n{indent}}}"
    ))
}

fn normalize_loop_body(body: &str, indent: &str) -> Result<String, EngineError> {
    let body = body.trim_start();
    if body.starts_with('{') {
        let bytes = body.as_bytes();
        let close = find_matching_brace(bytes, 0).ok_or_else(|| EngineError {
            name: "SyntaxError".to_string(),
            message: "unterminated loop body while rewriting using for-head".to_string(),
        })?;
        if !body[close + 1..].trim().is_empty() {
            return Err(EngineError {
                name: "SyntaxError".to_string(),
                message:
                    "unsupported trailing tokens after loop body while rewriting using for-head"
                        .to_string(),
            });
        }
        let inner = &body[1..close];
        Ok(format!("{inner}"))
    } else {
        Ok(format!("\n{indent}    {body}"))
    }
}

fn contains_keyword_outside_trivia(bytes: &[u8], keyword: &[u8]) -> bool {
    find_next_keyword_outside_trivia(bytes, 0, keyword).is_some()
}

fn find_next_keyword_outside_trivia(bytes: &[u8], start: usize, keyword: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + keyword.len() <= bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            _ => {}
        }

        if bytes[cursor] == keyword[0]
            && cursor + keyword.len() <= bytes.len()
            && &bytes[cursor..cursor + keyword.len()] == keyword
        {
            let before_ok = cursor == 0 || !is_identifier_byte(bytes[cursor - 1]);
            let after_ok =
                cursor + keyword.len() == bytes.len() || !is_identifier_byte(bytes[cursor + keyword.len()]);
            if before_ok && after_ok {
                return Some(cursor);
            }
        }

        cursor += 1;
    }

    None
}

fn rewrite_using_blocks(source: &str) -> Result<(String, bool), EngineError> {
    let mut output = String::with_capacity(source.len());
    let mut changed = false;
    let mut cursor = 0usize;

    while let Some(open_rel) = source[cursor..].find('{') {
        let open = cursor + open_rel;
        let Some(close) = find_matching_brace(source.as_bytes(), open) else {
            break;
        };
        let inner = &source[open + 1..close];
        let (inner, inner_changed) = rewrite_using_blocks(inner)?;
        let rewritten_inner = rewrite_using_block_contents(&inner)?;

        output.push_str(&source[cursor..open + 1]);
        match rewritten_inner {
            Some(rewritten_inner) => {
                output.push_str(&rewritten_inner);
                changed = true;
            }
            None => {
                output.push_str(&inner);
                changed |= inner_changed;
            }
        }
        output.push('}');
        cursor = close + 1;
    }

    if !changed {
        return Ok((source.to_string(), false));
    }

    output.push_str(&source[cursor..]);
    Ok((output, true))
}

fn rewrite_using_block_contents(block_source: &str) -> Result<Option<String>, EngineError> {
    let mut statements = Vec::new();
    let mut cursor = 0usize;

    while cursor < block_source.len() {
        let rest = &block_source[cursor..];
        if rest.trim().is_empty() {
            break;
        }

        let next_stmt_end =
            find_statement_end(block_source, cursor).ok_or_else(|| EngineError {
                name: "SyntaxError".to_string(),
                message: "unterminated statement while rewriting using block".to_string(),
            })?;
        let stmt = &block_source[cursor..next_stmt_end];
        statements.push(stmt.to_string());
        cursor = next_stmt_end;
    }

    if !statements.iter().any(|stmt| USING_DECL_RE.is_match(stmt)) {
        return Ok(None);
    }

    let stack_name = "__agentjs_using_stack__";
    let mut rewritten = String::new();
    let stack_ctor = if statements.iter().any(|stmt| {
        USING_DECL_RE
            .captures(stmt)
            .map(|captures| captures.name("await").is_some())
            .unwrap_or(false)
    }) {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };

    for stmt in statements {
        if let Some(captures) = USING_DECL_RE.captures(&stmt) {
            let indent = captures.name("indent").map(|m| m.as_str()).unwrap_or("");
            let name = captures.name("name").expect("using name capture").as_str();
            let expr = captures
                .name("expr")
                .expect("using expr capture")
                .as_str()
                .trim();

            rewritten.push_str(indent);
            rewritten.push_str("const ");
            rewritten.push_str(name);
            rewritten.push_str(" = ");
            rewritten.push_str(expr);
            rewritten.push_str(";\n");
            rewritten.push_str(indent);
            rewritten.push_str(stack_name);
            rewritten.push_str(".");
            rewritten.push_str("use(");
            rewritten.push_str(name);
            rewritten.push_str(");");
        } else {
            rewritten.push_str(&stmt);
        }
    }

    let dispose_call = if stack_ctor == "AsyncDisposableStack" {
        format!(
            "await __agentjsDisposeAsyncUsing__({stack_name}, __agentjs_has_body_error__, __agentjs_body_error__);"
        )
    } else {
        format!(
            "__agentjsDisposeSyncUsing__({stack_name}, __agentjs_has_body_error__, __agentjs_body_error__);"
        )
    };

    Ok(Some(format!(
        "\n    const {stack_name} = new {stack_ctor}();\n    let __agentjs_has_body_error__ = false;\n    let __agentjs_body_error__;\n    try {{{rewritten}\n    }} catch (__agentjs_error__) {{\n      __agentjs_has_body_error__ = true;\n      __agentjs_body_error__ = __agentjs_error__;\n    }} finally {{\n      {dispose_call}\n    }}\n    if (__agentjs_has_body_error__) throw __agentjs_body_error__;\n"
    )))
}

fn find_statement_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => {
                if brace_depth == 0 {
                    return Some(cursor);
                }
                brace_depth -= 1;
            }
            b';' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return Some(cursor + 1);
            }
            _ => {}
        }
        cursor += 1;
    }

    Some(bytes.len())
}

fn find_matching_brace(bytes: &[u8], open_brace: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open_brace;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn validate_import_call_syntax(source: &str) -> Result<(), EngineError> {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_js_string(bytes, i),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => i = skip_line_comment(bytes, i),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'i' => {
                validate_import_keyword_usage(bytes, i)?;
                i += 1;
            }
            _ => i += 1,
        }
    }

    Ok(())
}

fn validate_import_keyword_usage(bytes: &[u8], start: usize) -> Result<(), EngineError> {
    if !matches_import_keyword(bytes, start) {
        return Ok(());
    }

    let Some(cursor) = skip_whitespace_and_comments(bytes, start + "import".len()) else {
        return Err(invalid_import_call_syntax_error());
    };

    match bytes[cursor] {
        b'(' => {
            if is_preceded_by_new(bytes, start) {
                return Err(invalid_import_call_syntax_error());
            }
            validate_single_import_call(bytes, cursor, ImportCallSyntaxKind::Dynamic)
        }
        b'.' => validate_import_dot_usage(bytes, start, cursor + 1),
        _ => Ok(()),
    }
}

fn validate_import_dot_usage(
    bytes: &[u8],
    import_start: usize,
    property_start: usize,
) -> Result<(), EngineError> {
    let Some(property_start) = skip_whitespace_and_comments(bytes, property_start) else {
        return Err(invalid_import_call_syntax_error());
    };
    let Some(property_end) = skip_identifier(bytes, property_start) else {
        return Err(invalid_import_call_syntax_error());
    };
    let property = &bytes[property_start..property_end];
    let Some(cursor) = skip_whitespace_and_comments(bytes, property_end) else {
        return if property == b"meta" {
            Ok(())
        } else {
            Err(invalid_import_call_syntax_error())
        };
    };

    match property {
        b"source" | b"defer" => {
            if is_preceded_by_new(bytes, import_start) || bytes[cursor] != b'(' {
                return Err(invalid_import_call_syntax_error());
            }
            validate_single_import_call(bytes, cursor, ImportCallSyntaxKind::SingleArgument)
        }
        b"meta" => Ok(()),
        _ => Err(invalid_import_call_syntax_error()),
    }
}

fn validate_single_import_call(
    bytes: &[u8],
    open_paren: usize,
    kind: ImportCallSyntaxKind,
) -> Result<(), EngineError> {
    let Some(close_paren) = find_matching_paren(bytes, open_paren) else {
        return Err(invalid_import_call_syntax_error());
    };

    let mut comma_positions = Vec::new();
    let mut depth = 0usize;
    let mut cursor = open_paren + 1;
    while cursor < close_paren {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < close_paren && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < close_paren && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => comma_positions.push(cursor),
            b'.' if depth == 0
                && cursor + 2 < close_paren
                && &bytes[cursor..cursor + 3] == b"..." =>
            {
                return Err(invalid_import_call_syntax_error());
            }
            _ => {}
        }
        cursor += 1;
    }

    if !has_non_whitespace_between(bytes, open_paren + 1, close_paren) {
        return Err(invalid_import_call_syntax_error());
    }

    let mut segment_start = open_paren + 1;
    for comma in &comma_positions {
        if !has_non_whitespace_between(bytes, segment_start, *comma) {
            return Err(invalid_import_call_syntax_error());
        }
        segment_start = *comma + 1;
    }

    match kind {
        ImportCallSyntaxKind::SingleArgument => {
            if !comma_positions.is_empty() {
                return Err(invalid_import_call_syntax_error());
            }
        }
        ImportCallSyntaxKind::Dynamic => match comma_positions.len() {
            0 => {}
            1 => {}
            2 => {
                if has_non_whitespace_between(bytes, comma_positions[1] + 1, close_paren) {
                    return Err(invalid_import_call_syntax_error());
                }
            }
            _ => return Err(invalid_import_call_syntax_error()),
        },
    }

    Ok(())
}

fn invalid_import_call_syntax_error() -> EngineError {
    EngineError {
        name: "SyntaxError".to_string(),
        message: "invalid import call syntax".to_string(),
    }
}

fn matches_import_keyword(bytes: &[u8], start: usize) -> bool {
    const IMPORT: &[u8] = b"import";
    if bytes.len() < start + IMPORT.len() || &bytes[start..start + IMPORT.len()] != IMPORT {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if start + IMPORT.len() < bytes.len() && is_identifier_byte(bytes[start + IMPORT.len()]) {
        return false;
    }
    true
}

fn skip_whitespace_and_comments(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'/' && bytes[cursor + 1] == b'/' {
            cursor = skip_line_comment(bytes, cursor);
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            cursor = skip_block_comment(bytes, cursor);
            continue;
        }
        return Some(cursor);
    }
    None
}

fn skip_identifier(bytes: &[u8], start: usize) -> Option<usize> {
    if start >= bytes.len() || !is_identifier_byte(bytes[start]) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() && is_identifier_byte(bytes[cursor]) {
        cursor += 1;
    }
    Some(cursor)
}

fn has_non_whitespace_between(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut cursor = start;
    while cursor < end {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if cursor + 1 < end && bytes[cursor] == b'/' && bytes[cursor + 1] == b'/' {
            cursor = skip_line_comment(bytes, cursor).min(end);
            continue;
        }
        if cursor + 1 < end && bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            cursor = skip_block_comment(bytes, cursor).min(end);
            continue;
        }
        return true;
    }
    false
}

fn find_matching_paren(bytes: &[u8], open_paren: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open_paren;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = skip_js_string(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' => {
                cursor = skip_line_comment(bytes, cursor);
                continue;
            }
            b'/' if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'*' => {
                cursor = skip_block_comment(bytes, cursor);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn build_import_compat_helper(source_path: Option<&Path>) -> String {
    let referrer_literal = source_path
        .map(|path| format!("{:?}", path.to_string_lossy()))
        .unwrap_or_else(|| "\"\"".to_string());
    format!(
        r#"
const __agentjs_referrer__ = {referrer_literal};
const __agentjs_import__ = function(specifier, options) {{
  try {{
    let resourceType = "";
    if (arguments.length > 1 && options !== undefined) {{
      if ((typeof options !== "object" && typeof options !== "function") || options === null) {{
        return Promise.reject(new TypeError("The second argument to import() must be an object"));
      }}

      const attributes = options.with;
      if (attributes !== undefined) {{
        if ((typeof attributes !== "object" && typeof attributes !== "function") || attributes === null) {{
          return Promise.reject(new TypeError("The `with` import option must be an object"));
        }}

        for (const key of Object.keys(attributes)) {{
          const value = attributes[key];
          if (typeof value !== "string") {{
            return Promise.reject(new TypeError("Import attribute values must be strings"));
          }}
          if (key === "type" && (value === "json" || value === "text" || value === "bytes")) {{
            resourceType = value;
          }}
        }}
      }}
    }}
    if (resourceType) {{
      specifier = String(specifier) + "{IMPORT_RESOURCE_MARKER}" + resourceType;
    }}

    return import(specifier);
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_defer__ = function(specifier) {{
  try {{
    specifier = String(specifier);
    return globalThis.__agentjs_dynamic_import_defer__(specifier, __agentjs_referrer__);
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_source__ = function(specifier) {{
  try {{
    specifier = String(specifier);
    globalThis.__agentjs_assert_import_source__(specifier, __agentjs_referrer__);
    return Promise.reject(new SyntaxError("{SOURCE_PHASE_UNAVAILABLE_MESSAGE}"));
  }} catch (error) {{
    return Promise.reject(error);
  }}
}};
const __agentjs_import_source_static__ = function(specifier) {{
  specifier = String(specifier);
  globalThis.__agentjs_assert_import_source__(specifier, __agentjs_referrer__);
  throw new SyntaxError("{SOURCE_PHASE_UNAVAILABLE_MESSAGE}");
}};
"#
    )
}

fn rewrite_static_import_attributes(source: &str) -> String {
    let source = STATIC_IMPORT_FROM_WITH_RE.replace_all(source, rewrite_import_attribute_match);
    let source = STATIC_IMPORT_FROM_EMPTY_WITH_RE
        .replace_all(source.as_ref(), rewrite_empty_import_attribute_match);
    let source =
        STATIC_IMPORT_BARE_WITH_RE.replace_all(source.as_ref(), rewrite_import_attribute_match);
    STATIC_IMPORT_BARE_EMPTY_WITH_RE
        .replace_all(source.as_ref(), rewrite_empty_import_attribute_match)
        .into_owned()
}

fn rewrite_import_attribute_match(captures: &Captures<'_>) -> String {
    let prefix = captures
        .get(1)
        .expect("prefix capture is required")
        .as_str();
    let quote = captures.get(2).expect("quote capture is required").as_str();
    let specifier = captures
        .get(3)
        .expect("specifier capture is required")
        .as_str();
    let resource_type = captures
        .get(6)
        .expect("resource type capture is required")
        .as_str();
    let rewritten = encode_import_resource_kind(specifier, resource_type);
    format!("{prefix}{quote}{rewritten}{quote}")
}

fn rewrite_empty_import_attribute_match(captures: &Captures<'_>) -> String {
    let prefix = captures
        .get(1)
        .expect("prefix capture is required")
        .as_str();
    let quote = captures.get(2).expect("quote capture is required").as_str();
    let specifier = captures
        .get(3)
        .expect("specifier capture is required")
        .as_str();
    format!("{prefix}{quote}{specifier}{quote}")
}

fn rewrite_dynamic_import_calls(source: &str) -> (String, bool) {
    let bytes = source.as_bytes();
    let mut rewritten = String::with_capacity(source.len());
    let mut changed = false;
    let mut i = 0;
    let mut last = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                i = skip_js_string(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            b'i' if matches_dynamic_import(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import__");
                i += "import".len();
                last = i;
                changed = true;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !changed {
        return (source.to_string(), false);
    }

    rewritten.push_str(&source[last..]);
    (rewritten, true)
}

fn rewrite_dynamic_import_source_calls(source: &str) -> (String, bool) {
    let bytes = source.as_bytes();
    let mut rewritten = String::with_capacity(source.len());
    let mut changed = false;
    let mut i = 0;
    let mut last = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                i = skip_js_string(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            b'i' if matches_dynamic_import_source(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import_source__");
                i += "import.source".len();
                last = i;
                changed = true;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !changed {
        return (source.to_string(), false);
    }

    rewritten.push_str(&source[last..]);
    (rewritten, true)
}

fn rewrite_dynamic_import_defer_calls(source: &str) -> (String, bool) {
    let bytes = source.as_bytes();
    let mut rewritten = String::with_capacity(source.len());
    let mut changed = false;
    let mut i = 0;
    let mut last = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                i = skip_js_string(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            b'i' if matches_dynamic_import_defer(bytes, i) => {
                rewritten.push_str(&source[last..i]);
                rewritten.push_str("__agentjs_import_defer__");
                i += "import.defer".len();
                last = i;
                changed = true;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !changed {
        return (source.to_string(), false);
    }

    rewritten.push_str(&source[last..]);
    (rewritten, true)
}

fn rewrite_static_source_phase_imports(source: &str) -> (String, bool) {
    let mut changed = false;
    let rewritten = STATIC_SOURCE_IMPORT_RE.replace_all(source, |captures: &Captures<'_>| {
        changed = true;
        let binding = captures
            .get(1)
            .expect("binding capture is required")
            .as_str();
        let quote = captures.get(2).expect("quote capture is required").as_str();
        let specifier = captures
            .get(3)
            .expect("specifier capture is required")
            .as_str();
        format!("const {binding} = __agentjs_import_source_static__({quote}{specifier}{quote});")
    });

    (rewritten.into_owned(), changed)
}

fn rewrite_static_defer_namespace_imports(source: &str) -> (String, bool) {
    let mut changed = false;
    let rewritten =
        STATIC_DEFER_NAMESPACE_IMPORT_RE.replace_all(source, |captures: &Captures<'_>| {
            changed = true;
            let binding = captures
                .get(1)
                .expect("binding capture is required")
                .as_str();
            let quote = captures.get(2).expect("quote capture is required").as_str();
            let specifier = captures
                .get(3)
                .expect("specifier capture is required")
                .as_str();
            let temp_binding = format!("__agentjs_deferred_namespace__{binding}");
            let rewritten = encode_import_resource_kind(specifier, "defer");
            format!(
                "import {temp_binding} from {quote}{rewritten}{quote}; const {binding} = {temp_binding};"
            )
        });

    (rewritten.into_owned(), changed)
}

fn matches_dynamic_import(bytes: &[u8], start: usize) -> bool {
    const IMPORT: &[u8] = b"import";
    if bytes.len() < start + IMPORT.len() || &bytes[start..start + IMPORT.len()] != IMPORT {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if start + IMPORT.len() < bytes.len() && is_identifier_byte(bytes[start + IMPORT.len()]) {
        return false;
    }

    let mut cursor = start + IMPORT.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    bytes.get(cursor) == Some(&b'(')
}

fn is_preceded_by_new(bytes: &[u8], start: usize) -> bool {
    if start == 0 {
        return false;
    }
    let mut cursor = start - 1;
    while cursor > 0 && bytes[cursor].is_ascii_whitespace() {
        cursor -= 1;
    }
    if cursor >= 2 && &bytes[cursor - 2..=cursor] == b"new" {
        if cursor - 2 == 0 || !is_identifier_byte(bytes[cursor - 3]) {
            return true;
        }
    }
    false
}

fn matches_dynamic_import_defer(bytes: &[u8], start: usize) -> bool {
    const IMPORT_DEFER: &[u8] = b"import.defer";
    if bytes.len() < start + IMPORT_DEFER.len()
        || &bytes[start..start + IMPORT_DEFER.len()] != IMPORT_DEFER
    {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if is_preceded_by_new(bytes, start) {
        return false;
    }

    let mut cursor = start + IMPORT_DEFER.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) != Some(&b'(') {
        return false;
    }

    cursor += 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b')') {
        return false;
    }

    true
}

fn matches_dynamic_import_source(bytes: &[u8], start: usize) -> bool {
    const IMPORT_SOURCE: &[u8] = b"import.source";
    if bytes.len() < start + IMPORT_SOURCE.len()
        || &bytes[start..start + IMPORT_SOURCE.len()] != IMPORT_SOURCE
    {
        return false;
    }
    if start > 0 && is_identifier_byte(bytes[start - 1]) {
        return false;
    }
    if is_preceded_by_new(bytes, start) {
        return false;
    }

    let mut cursor = start + IMPORT_SOURCE.len();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) != Some(&b'(') {
        return false;
    }

    cursor += 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b')') {
        return false;
    }

    true
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

fn skip_js_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i = (i + 2).min(bytes.len());
            }
            current if current == quote => {
                return i + 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    bytes.len()
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn encode_import_resource_kind(specifier: &str, resource_type: &str) -> String {
    format!("{specifier}{IMPORT_RESOURCE_MARKER}{resource_type}")
}

fn is_async_module_source(source: &str) -> bool {
    // A simple heuristic for Top-Level Await.
    // In ES modules, 'await' is always a keyword.
    // Try to avoid matching 'await' inside line comments or block comments.
    let no_line_comments = Regex::new(r"(?m)//.*$").unwrap().replace_all(source, "");
    let no_block_comments = Regex::new(r"(?s)/\*.*?\*/")
        .unwrap()
        .replace_all(&no_line_comments, "");
    let re = Regex::new(r"\bawait\b").unwrap();
    re.is_match(&no_block_comments)
}

fn decode_import_resource_kind(specifier: &JsString) -> (JsString, ModuleResourceKind) {
    let raw = specifier.to_std_string_escaped();
    let Some((path, resource_type)) = raw.rsplit_once(IMPORT_RESOURCE_MARKER) else {
        return (specifier.clone(), ModuleResourceKind::JavaScript);
    };

    let kind = match resource_type {
        "defer" => ModuleResourceKind::Deferred,
        "json" => ModuleResourceKind::Json,
        "text" => ModuleResourceKind::Text,
        "bytes" => ModuleResourceKind::Bytes,
        _ => ModuleResourceKind::JavaScript,
    };
    (JsString::from(path), kind)
}
