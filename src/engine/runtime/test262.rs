fn install_test262_globals(
    context: &mut Context,
    install_shadow_realm: bool,
) -> boa_engine::JsResult<()> {
    // Add formatDurationFormatPattern and partitionDurationFormatPattern to global object for DurationFormat tests
    context.eval(Source::from_bytes(
        r#"
        if (typeof globalThis.partitionDurationFormatPattern !== 'function') {
          globalThis.partitionDurationFormatPattern = function(df, duration) { return df.formatToParts(duration); };
        }
        if (typeof globalThis.formatDurationFormatPattern !== 'function') {
          globalThis.formatDurationFormatPattern = function(df, duration) { return df.format(duration); };
        }
        "#,
    ))?;

    let test262 = build_test262_object(
        context.realm().clone(),
        context.global_object(),
        true,
        context,
    );

    context.register_global_property(js_string!("$262"), test262, Attribute::all())?;
    if install_shadow_realm {
        install_shadow_realm_polyfill(context)?;
    }
    Ok(())
}

fn install_shadow_realm_polyfill(context: &mut Context) -> boa_engine::JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          if (typeof globalThis.ShadowRealm === 'function') {
            return;
          }
          if (typeof globalThis.$262 !== 'object' || globalThis.$262 === null || typeof globalThis.$262.createRealm !== 'function') {
            return;
          }

          var stateKey = Symbol.for('@@agentjs.shadowrealm.state');
          var nextTempId = 0;

          function getIntrinsicShadowRealmPrototype(newTarget) {
            if (newTarget === undefined || newTarget === ShadowRealm) {
              return ShadowRealm.prototype;
            }
            var proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              var otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              var otherShadowRealm = otherGlobal && otherGlobal.ShadowRealm;
              var otherProto = otherShadowRealm && otherShadowRealm.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return ShadowRealm.prototype;
          }

          function requireShadowRealm(value, name, TypeErrorCtor) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !Object.prototype.hasOwnProperty.call(value, stateKey)) {
              throw new TypeErrorCtor(name + ' called on incompatible receiver');
            }
            return value[stateKey];
          }

          function isPrimitive(value) {
            return value === null || (typeof value !== 'object' && typeof value !== 'function');
          }

          function defineNameAndLength(wrapper, target) {
            var length = 0;
            var name = '';

            try {
              if (Object.prototype.hasOwnProperty.call(target, 'length')) {
                var targetLength = target.length;
                if (typeof targetLength === 'number') {
                  if (targetLength === Infinity) {
                    length = Infinity;
                  } else if (targetLength === -Infinity) {
                    length = 0;
                  } else {
                    var coerced = Math.trunc(targetLength);
                    if (!Number.isFinite(coerced)) {
                      coerced = 0;
                    }
                    length = Math.max(coerced, 0);
                  }
                }
              }

              var targetName = target.name;
              if (typeof targetName === 'string') {
                name = targetName;
              }
            } catch {
              throw new TypeError('WrappedFunctionCreate failed');
            }

            Object.defineProperty(wrapper, 'length', {
              value: length,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(wrapper, 'name', {
              value: name,
              writable: false,
              enumerable: false,
              configurable: true,
            });
          }

          function evalInBridge(state, sourceText) {
            var tempName = '__agentjs_shadowrealm_source_' + (nextTempId++) + '__';
            state.bridge.global[tempName] = sourceText;
            try {
              return state.bridge.evalScript('(0, eval)(globalThis[' + JSON.stringify(tempName) + '])');
            } finally {
              try {
                delete state.bridge.global[tempName];
              } catch (e) {}
            }
          }

          function createTargetRealmCallable(callable, state) {
            var callableId = __agentjs_shadowrealm_register_callable__(callable);
            var source =
              '(function(callableId) {' +
              '  return function() {' +
              '    var invokeArgs = [callableId];' +
              '    for (var i = 0; i < arguments.length; i++) {' +
              '      invokeArgs.push(arguments[i]);' +
              '    }' +
              '    var result;' +
              '    try {' +
              '      result = __agentjs_shadowrealm_invoke__.apply(undefined, invokeArgs);' +
              '    } catch (e) {' +
              '      throw new TypeError();' +
              '    }' +
              '    if (result === null || (typeof result !== "object" && typeof result !== "function")) {' +
              '      return result;' +
              '    }' +
              '    if (typeof result === "function") {' +
              '      var nestedId = __agentjs_shadowrealm_register_callable__(result);' +
              '      return function() {' +
              '        var nestedArgs = [nestedId];' +
              '        for (var j = 0; j < arguments.length; j++) {' +
              '          nestedArgs.push(arguments[j]);' +
              '        }' +
              '        try {' +
              '          return __agentjs_shadowrealm_invoke__.apply(undefined, nestedArgs);' +
              '        } catch (e) {' +
              '          throw new TypeError();' +
              '        }' +
              '      };' +
              '    }' +
              '    throw new TypeError();' +
              '  };' +
              '})(' + String(callableId) + ')';
            return evalInBridge(state, source);
          }

          function convertArgumentsForTarget(args, state) {
            var converted = [];
            for (var i = 0; i < args.length; i++) {
              var arg = args[i];
              if (isPrimitive(arg)) {
                converted.push(arg);
                continue;
              }
              if (typeof arg === 'function') {
                converted.push(createTargetRealmCallable(arg, state));
                continue;
              }
              throw new TypeError('ShadowRealm wrapped functions only accept primitives or callables');
            }
            return converted;
          }

          function wrapValueFromTarget(value, wrapperCarrier, TypeErrorCtor) {
            if (isPrimitive(value)) {
              return value;
            }
            if (typeof value === 'function') {
              try {
                return __agentjs_shadowrealm_wrap_callable__(wrapperCarrier, value);
              } catch (e) {
                throw new TypeErrorCtor('WrappedFunctionCreate failed');
              }
            }
            throw new TypeErrorCtor('ShadowRealm values must be primitive or callable');
          }

          function createWrappedFunction(targetCallable, state) {
            var targetId = __agentjs_shadowrealm_register_callable__(targetCallable);
            var wrapped = function() {
              var convertedArgs = convertArgumentsForTarget(Array.prototype.slice.call(arguments), state);
              var invokeArgs = [targetId];
              for (var i = 0; i < convertedArgs.length; i++) {
                invokeArgs.push(convertedArgs[i]);
              }
              var result;
              try {
                result = __agentjs_shadowrealm_invoke__.apply(undefined, invokeArgs);
              } catch (e) {
                throw new TypeError('Wrapped function invocation failed');
              }
              return wrapValueFromTarget(result, state);
            };
            defineNameAndLength(wrapped, targetCallable);
            return wrapped;
          }

          function ShadowRealm() {
            if (new.target === undefined) {
              throw new TypeError('Constructor ShadowRealm requires new');
            }

            var realm = Object.create(getIntrinsicShadowRealmPrototype(new.target));
            Object.defineProperty(realm, stateKey, {
              value: {
                bridge: $262.createRealm(),
              },
              writable: false,
              enumerable: false,
              configurable: false,
            });
            return realm;
          }

          var shadowRealmMethods = {
            evaluate(sourceText) {
              var CallerTypeError = TypeError;
              var CallerSyntaxError = SyntaxError;
              var RealmCarrier = function() {};
              var state = requireShadowRealm(this, 'ShadowRealm.prototype.evaluate', CallerTypeError);
              if (typeof sourceText !== 'string') {
                throw new CallerTypeError('ShadowRealm.prototype.evaluate requires a string');
              }
              var initialParseValid = __agentjs_shadowrealm_can_parse_script__(sourceText);

              var result;
              try {
                result = evalInBridge(state, sourceText);
              } catch (e) {
                if (!initialParseValid && e && e.name === 'SyntaxError') {
                  throw new CallerSyntaxError('Invalid ShadowRealm source text');
                }
                throw new CallerTypeError('ShadowRealm.prototype.evaluate failed');
              }
              return wrapValueFromTarget(result, RealmCarrier, CallerTypeError);
            },

            importValue(specifier, exportName) {
              var CallerTypeError = TypeError;
              var RealmCarrier = function() {};
              var state = requireShadowRealm(this, 'ShadowRealm.prototype.importValue', CallerTypeError);
              var specifierString = String(specifier);
              if (typeof exportName !== 'string') {
                throw new CallerTypeError('ShadowRealm.prototype.importValue exportName must be a string');
              }

              var promise;
              try {
                promise = __agentjs_shadowrealm_dynamic_import__(specifierString);
              } catch (e) {
                return Promise.reject(new CallerTypeError('ShadowRealm.prototype.importValue failed'));
              }

              return promise.then(
                function(namespace) {
                  if (!Object.prototype.hasOwnProperty.call(namespace, exportName)) {
                    throw new CallerTypeError('Requested export was not found');
                  }
                  return wrapValueFromTarget(
                    namespace[exportName],
                    RealmCarrier,
                    CallerTypeError
                  );
                },
                function() {
                  throw new CallerTypeError('ShadowRealm.prototype.importValue failed');
                }
              );
            },
          };

          var shadowRealmPrototype = Object.create(Object.prototype);
          Object.defineProperties(shadowRealmPrototype, {
            constructor: {
              value: ShadowRealm,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            evaluate: {
              value: shadowRealmMethods.evaluate,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            importValue: {
              value: shadowRealmMethods.importValue,
              writable: true,
              enumerable: false,
              configurable: true,
            },
          });
          Object.defineProperty(shadowRealmPrototype, Symbol.toStringTag, {
            value: 'ShadowRealm',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(ShadowRealm, 'prototype', {
            value: shadowRealmPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(globalThis, 'ShadowRealm', {
            value: ShadowRealm,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn build_test262_object(
    target_realm: Realm,
    target_global: JsObject,
    expose_host_hooks: bool,
    context: &mut Context,
) -> JsObject {
    let eval_realm = target_realm.clone();
    let eval_script = build_builtin_function(
        context,
        js_string!("evalScript"),
        1,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, target_realm, context| eval_script_in_realm(args, target_realm, context),
            eval_realm,
        ),
    );
    let create_realm = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("createRealm"),
            0,
            NativeFunction::from_fn_ptr(host_create_realm),
        )
    });
    let detach_array_buffer = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("detachArrayBuffer"),
            1,
            NativeFunction::from_fn_ptr(host_detach_array_buffer),
        )
    });
    let gc = expose_host_hooks.then(|| {
        build_builtin_function(
            context,
            js_string!("gc"),
            0,
            NativeFunction::from_fn_ptr(host_gc),
        )
    });
    let abstract_module_source =
        expose_host_hooks.then(|| build_abstract_module_source_constructor(context));
    let agent = build_agent_object(context);

    let is_html_dda = JsObject::with_is_html_dda_proto(target_realm.intrinsics());

    let realm = context.realm().clone();
    let mut object = ObjectInitializer::new(context);
    object.property(js_string!("global"), target_global, Attribute::all());
    object.property(js_string!("evalScript"), eval_script, Attribute::all());
    object.property(js_string!("IsHTMLDDA"), is_html_dda, Attribute::all());
    if let Some(create_realm) = create_realm {
        object.property(js_string!("createRealm"), create_realm, Attribute::all());
    }
    if let Some(detach_array_buffer) = detach_array_buffer {
        object.property(
            js_string!("detachArrayBuffer"),
            detach_array_buffer,
            Attribute::all(),
        );
    }
    if let Some(gc) = gc {
        object.property(js_string!("gc"), gc, Attribute::all());
    }
    if let Some(abstract_module_source) = abstract_module_source {
        object.property(
            js_string!("AbstractModuleSource"),
            abstract_module_source,
            Attribute::all(),
        );
    }
    if let Some(agent) = agent {
        object.property(js_string!("agent"), agent, Attribute::all());
    }

    let object = object.build();
    let can_parse_realm = target_realm.clone();
    let can_parse = build_builtin_function(
        context,
        js_string!("__agentjsCanParseScript__"),
        1,
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, target_realm, context| {
                let source = script_source_from_args(args, context)?;
                with_realm(context, target_realm.clone(), |context| {
                    Ok(
                        Script::parse(Source::from_bytes(source.as_str()), None, context)
                            .is_ok()
                            .into(),
                    )
                })
            },
            can_parse_realm,
        ),
    );
    object
        .define_property_or_throw(
            js_string!("__agentjsCanParseScript__"),
            PropertyDescriptor::builder()
                .value(can_parse)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("defining internal parse helper on test262 realm wrapper must succeed");
    object
}

fn build_agent_object(context: &mut Context) -> Option<JsObject> {
    if context.has_data::<WorkerAgentContext>() {
        Some(build_worker_agent_object(context))
    } else if context.has_data::<AgentRuntimeContext>() {
        Some(build_main_agent_object(context))
    } else {
        None
    }
}

fn build_main_agent_object(context: &mut Context) -> JsObject {
    let start = build_builtin_function(
        context,
        js_string!("start"),
        1,
        NativeFunction::from_fn_ptr(host_agent_start),
    );
    let broadcast = build_builtin_function(
        context,
        js_string!("broadcast"),
        1,
        NativeFunction::from_fn_ptr(host_agent_broadcast),
    );
    let get_report = build_builtin_function(
        context,
        js_string!("getReport"),
        0,
        NativeFunction::from_fn_ptr(host_agent_get_report),
    );
    let sleep = build_builtin_function(
        context,
        js_string!("sleep"),
        1,
        NativeFunction::from_fn_ptr(host_agent_sleep),
    );
    let monotonic_now = build_builtin_function(
        context,
        js_string!("monotonicNow"),
        0,
        NativeFunction::from_fn_ptr(host_agent_monotonic_now),
    );

    let mut object = ObjectInitializer::new(context);
    object.property(js_string!("start"), start, Attribute::all());
    object.property(js_string!("broadcast"), broadcast, Attribute::all());
    object.property(js_string!("getReport"), get_report, Attribute::all());
    object.property(js_string!("sleep"), sleep, Attribute::all());
    object.property(js_string!("monotonicNow"), monotonic_now, Attribute::all());
    object.build()
}

fn build_worker_agent_object(context: &mut Context) -> JsObject {
    let receive_broadcast = build_builtin_function(
        context,
        js_string!("receiveBroadcast"),
        1,
        NativeFunction::from_fn_ptr(host_worker_receive_broadcast),
    );
    let report = build_builtin_function(
        context,
        js_string!("report"),
        1,
        NativeFunction::from_fn_ptr(host_worker_report),
    );
    let sleep = build_builtin_function(
        context,
        js_string!("sleep"),
        1,
        NativeFunction::from_fn_ptr(host_agent_sleep),
    );
    let leaving = build_builtin_function(
        context,
        js_string!("leaving"),
        0,
        NativeFunction::from_fn_ptr(host_worker_leaving),
    );
    let monotonic_now = build_builtin_function(
        context,
        js_string!("monotonicNow"),
        0,
        NativeFunction::from_fn_ptr(host_agent_monotonic_now),
    );

    let mut object = ObjectInitializer::new(context);
    object.property(
        js_string!("receiveBroadcast"),
        receive_broadcast,
        Attribute::all(),
    );
    object.property(js_string!("report"), report, Attribute::all());
    object.property(js_string!("sleep"), sleep, Attribute::all());
    object.property(js_string!("leaving"), leaving, Attribute::all());
    object.property(js_string!("monotonicNow"), monotonic_now, Attribute::all());
    object.build()
}

fn build_builtin_function(
    context: &mut Context,
    name: boa_engine::JsString,
    length: usize,
    body: NativeFunction,
) -> JsObject {
    FunctionObjectBuilder::new(context.realm(), body)
        .name(name)
        .length(length)
        .constructor(false)
        .build()
        .into()
}

fn build_abstract_module_source_constructor(context: &mut Context) -> JsObject {
    let constructor = build_builtin_function(
        context,
        js_string!("AbstractModuleSource"),
        0,
        NativeFunction::from_fn_ptr(host_abstract_module_source_constructor),
    );

    let prototype = ObjectInitializer::new(context).build();
    let to_string_tag = build_builtin_function(
        context,
        js_string!("get [Symbol.toStringTag]"),
        0,
        NativeFunction::from_fn_ptr(host_abstract_module_source_to_string_tag),
    );

    prototype
        .define_property_or_throw(
            js_string!("constructor"),
            PropertyDescriptor::builder()
                .value(constructor.clone())
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("AbstractModuleSource.prototype.constructor definition must succeed");
    prototype
        .define_property_or_throw(
            JsSymbol::to_string_tag(),
            PropertyDescriptor::builder()
                .get(to_string_tag)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .expect("AbstractModuleSource.prototype[@@toStringTag] definition must succeed");

    constructor
        .define_property_or_throw(
            js_string!("prototype"),
            PropertyDescriptor::builder()
                .value(prototype)
                .writable(false)
                .enumerable(false)
                .configurable(false),
            context,
        )
        .expect("AbstractModuleSource.prototype definition must succeed");

    constructor
}

