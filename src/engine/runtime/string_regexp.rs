fn install_promise_then_hook(context: &mut Context) -> boa_engine::JsResult<()> {
    let prototype = context.intrinsics().constructors().promise().prototype();
    let original_symbol = promise_then_original_symbol(context)?;
    if prototype.has_own_property(original_symbol.clone(), context)? {
        return Ok(());
    }

    let original = prototype.get(js_string!("then"), context)?;
    if original.as_callable().is_none() {
        return Ok(());
    }

    prototype.define_property_or_throw(
        original_symbol,
        PropertyDescriptor::builder()
            .value(original)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    Ok(())
}

fn install_string_replace_guard(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = String.prototype;
          const originalKey = "__agentjs_original_String_replace__";
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.replace;
          if (typeof original !== 'function') {
            return;
          }

          const isHtmlDdaLike = (value) =>
            typeof value === 'undefined' && value !== undefined;
          const isObjectLike = (value) =>
            (typeof value === 'object' && value !== null) ||
            typeof value === 'function' ||
            isHtmlDdaLike(value);

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const replaceFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              const replaceValue = args.length > 1 ? args[1] : undefined;
              const input = String(thisArg);
              if (
                typeof replaceValue === 'string' &&
                replaceValue.includes('$') &&
                input.length > 0 &&
                replaceValue.length > 0
              ) {
                const estimatedLength = BigInt(input.length) * BigInt(replaceValue.length);
                if (estimatedLength > 1073741824n) {
                  throw new ReferenceError('OOM Limit');
                }
              }

              let effectiveSearchValue = searchValue;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                const searchString = `${searchValue}`;
                effectiveSearchValue = {
                  [Symbol.toPrimitive]() {
                    return searchString;
                  },
                  toString() {
                    return searchString;
                  },
                  valueOf() {
                    return searchString;
                  },
                };
              }

              return original.call(thisArg, effectiveSearchValue, replaceValue);
            }
          });

          Object.defineProperty(replaceFn, 'name', {
            value: 'replace',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(replaceFn, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, 'replace', {
            value: replaceFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_string_match_guards(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = String.prototype;
          const originalMatchKey = "__agentjs_original_String_match__";
          const originalMatchAllKey = "__agentjs_original_String_matchAll__";
          const originalSearchKey = "__agentjs_original_String_search__";
          const originalReplaceAllKey = "__agentjs_original_String_replaceAll__";
          const originalSplitKey = "__agentjs_original_String_split__";
          if (
            Object.prototype.hasOwnProperty.call(proto, originalMatchKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalMatchAllKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalSearchKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalReplaceAllKey) &&
            Object.prototype.hasOwnProperty.call(proto, originalSplitKey)
          ) {
            return;
          }

          const originalMatch = proto.match;
          const originalMatchAll = proto.matchAll;
          const originalSearch = proto.search;
          const originalReplaceAll = proto.replaceAll;
          const originalSplit = proto.split;
          if (
            typeof originalMatch !== "function" ||
            typeof originalMatchAll !== "function" ||
            typeof originalSearch !== "function" ||
            typeof originalReplaceAll !== "function" ||
            typeof originalSplit !== "function"
          ) {
            return;
          }

          const isHtmlDdaLike = (value) =>
            typeof value === "undefined" && value !== undefined;
          const isObjectLike = (value) =>
            (typeof value === "object" && value !== null) ||
            typeof value === "function" ||
            isHtmlDdaLike(value);

          Object.defineProperty(proto, originalMatchKey, {
            value: originalMatch,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalMatchAllKey, {
            value: originalMatchAll,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalSearchKey, {
            value: originalSearch,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalReplaceAllKey, {
            value: originalReplaceAll,
            writable: false,
            enumerable: false,
            configurable: false,
          });
          Object.defineProperty(proto, originalSplitKey, {
            value: originalSplit,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const createPrimitivePatternWrapper = (value) => {
            const patternString = `${value}`;
            return {
              [Symbol.toPrimitive]() {
                return patternString;
              },
              toString() {
                return patternString;
              },
              valueOf() {
                return patternString;
              },
            };
          };

          const matchFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const regexp = args.length > 0 ? args[0] : undefined;
              if (regexp !== undefined && regexp !== null && !isObjectLike(regexp)) {
                return originalMatch.call(thisArg, createPrimitivePatternWrapper(regexp));
              }
              return originalMatch.call(thisArg, regexp);
            },
          });

          Object.defineProperty(matchFn, "name", {
            value: "match",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(matchFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "match", {
            value: matchFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const matchAllFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const regexp = args.length > 0 ? args[0] : undefined;
              if (regexp !== undefined && regexp !== null && !isObjectLike(regexp)) {
                return originalMatchAll.call(thisArg, createPrimitivePatternWrapper(regexp));
              }
              return originalMatchAll.call(thisArg, regexp);
            },
          });

          Object.defineProperty(matchAllFn, "name", {
            value: "matchAll",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(matchAllFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "matchAll", {
            value: matchAllFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const searchFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                return originalSearch.call(thisArg, createPrimitivePatternWrapper(searchValue));
              }
              return originalSearch.call(thisArg, searchValue);
            },
          });

          Object.defineProperty(searchFn, "name", {
            value: "search",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(searchFn, "length", {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "search", {
            value: searchFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const replaceAllFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const searchValue = args.length > 0 ? args[0] : undefined;
              const replaceValue = args.length > 1 ? args[1] : undefined;
              let effectiveSearchValue = searchValue;
              if (
                searchValue !== undefined &&
                searchValue !== null &&
                !isObjectLike(searchValue)
              ) {
                effectiveSearchValue = createPrimitivePatternWrapper(searchValue);
              }
              return originalReplaceAll.call(thisArg, effectiveSearchValue, replaceValue);
            },
          });

          Object.defineProperty(replaceAllFn, "name", {
            value: "replaceAll",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(replaceAllFn, "length", {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "replaceAll", {
            value: replaceAllFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const splitFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              const separator = args.length > 0 ? args[0] : undefined;
              const limit = args.length > 1 ? args[1] : undefined;
              let effectiveSeparator = separator;
              if (
                separator !== undefined &&
                separator !== null &&
                !isObjectLike(separator)
              ) {
                effectiveSeparator = createPrimitivePatternWrapper(separator);
              }
              return originalSplit.call(thisArg, effectiveSeparator, limit);
            },
          });

          Object.defineProperty(splitFn, "name", {
            value: "split",
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(splitFn, "length", {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, "split", {
            value: splitFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_reg_exp_legacy_accessors(context: &mut Context) -> JsResult<()> {
    let regexp_ctor = context.intrinsics().constructors().regexp().constructor();
    let receiver_check = "if (this !== RegExp) { throw new TypeError('RegExp legacy static accessor called on incompatible receiver'); }";

    for i in 1..=9 {
        let name = format!("${i}");
        let getter = context.eval(Source::from_bytes(&format!(
            "(function() {{ {receiver_check} return ''; }})"
        )))?;
        let getter = getter.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create RegExp legacy getter")
        })?;
        regexp_ctor.define_property_or_throw(
            JsString::from(name.as_str()),
            PropertyDescriptor::builder()
                .get(getter)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    for (full, short) in [
        ("lastMatch", "$&"),
        ("lastParen", "$+"),
        ("leftContext", "$`"),
        ("rightContext", "$'"),
    ] {
        let getter = context.eval(Source::from_bytes(&format!(
            "(function() {{ {receiver_check} return ''; }})"
        )))?;
        let getter = getter.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create RegExp legacy getter")
        })?;
        for name in [full, short] {
            regexp_ctor.define_property_or_throw(
                js_string!(name),
                PropertyDescriptor::builder()
                    .get(getter.clone())
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }

    let input_getter = context.eval(Source::from_bytes(&format!(
        "(function() {{ {receiver_check} return ''; }})"
    )))?;
    let input_getter = input_getter
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp input getter"))?;
    let input_setter = context.eval(Source::from_bytes(&format!(
        "(function(_value) {{ {receiver_check} return undefined; }})"
    )))?;
    let input_setter = input_setter
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("failed to create RegExp input setter"))?;
    for name in ["input", "$_"] {
        regexp_ctor.define_property_or_throw(
            js_string!(name),
            PropertyDescriptor::builder()
                .get(input_getter.clone())
                .set(input_setter.clone())
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    Ok(())
}

fn install_reg_exp_compile_guard(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = RegExp.prototype;
          const originalKey = "__agentjs_original_RegExp_compile__";
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.compile;
          if (typeof original !== 'function') {
            return;
          }

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(proto, 'compile', {
            value: function compile(pattern, flags) {
              if (typeof this === 'object' && this !== null && Object.getPrototypeOf(this) !== proto) {
                throw new TypeError('RegExp.prototype.compile called on incompatible receiver');
              }
              return original.call(this, pattern, flags);
            },
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_reg_exp_escape(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof RegExp !== 'function' || typeof RegExp.escape === 'function') {
            return;
          }

          const syntaxCharacters = new Set(['^', '$', '\\', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|']);
          const otherPunctuators = new Set([',', '-', '=', '<', '>', '#', '&', '!', '%', ':', ';', '@', '~', "'", '`', '"']);
          const controlEscapeNames = new Map([
            [0x0009, 't'],
            [0x000A, 'n'],
            [0x000B, 'v'],
            [0x000C, 'f'],
            [0x000D, 'r'],
          ]);

          function isAsciiLetterOrDigit(cp) {
            return (
              (cp >= 0x30 && cp <= 0x39) ||
              (cp >= 0x41 && cp <= 0x5A) ||
              (cp >= 0x61 && cp <= 0x7A)
            );
          }

          function toHex(value, width) {
            return value.toString(16).padStart(width, '0');
          }

          function unicodeEscape(codeUnit) {
            return '\\u' + toHex(codeUnit, 4);
          }

          function isWhiteSpaceOrLineTerminator(cp) {
            if (
              cp === 0x0009 ||
              cp === 0x000A ||
              cp === 0x000B ||
              cp === 0x000C ||
              cp === 0x000D ||
              cp === 0x0020 ||
              cp === 0x00A0 ||
              cp === 0x1680 ||
              (cp >= 0x2000 && cp <= 0x200A) ||
              cp === 0x2028 ||
              cp === 0x2029 ||
              cp === 0x202F ||
              cp === 0x205F ||
              cp === 0x3000 ||
              cp === 0xFEFF
            ) {
              return true;
            }
            return false;
          }

          function encodeForRegExpEscape(cp) {
            const ch = String.fromCodePoint(cp);
            if (syntaxCharacters.has(ch) || cp === 0x002F) {
              return '\\' + ch;
            }

            const controlEscape = controlEscapeNames.get(cp);
            if (controlEscape !== undefined) {
              return '\\' + controlEscape;
            }

            if (
              otherPunctuators.has(ch) ||
              isWhiteSpaceOrLineTerminator(cp) ||
              (cp >= 0xD800 && cp <= 0xDFFF)
            ) {
              if (cp <= 0xFF) {
                return '\\x' + toHex(cp, 2);
              }
              if (cp <= 0xFFFF) {
                return unicodeEscape(cp);
              }
              const high = Math.floor((cp - 0x10000) / 0x400) + 0xD800;
              const low = ((cp - 0x10000) % 0x400) + 0xDC00;
              return unicodeEscape(high) + unicodeEscape(low);
            }

            return ch;
          }

          function regExpEscape(string) {
            if (typeof string !== 'string') {
              throw new TypeError('RegExp.escape requires a string argument');
            }

            let escaped = '';
            let isFirst = true;
            for (const ch of string) {
              const cp = ch.codePointAt(0);
              if (isFirst && isAsciiLetterOrDigit(cp)) {
                escaped += '\\x' + toHex(cp, 2);
              } else {
                escaped += encodeForRegExpEscape(cp);
              }
              isFirst = false;
            }
            return escaped;
          }

          const escapeFn = new Proxy(() => {}, {
            apply(_target, _thisArg, args) {
              const input = args.length > 0 ? args[0] : undefined;
              return regExpEscape(input);
            }
          });

          Object.defineProperty(escapeFn, 'name', {
            value: 'escape',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(escapeFn, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(RegExp, 'escape', {
            value: escapeFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

