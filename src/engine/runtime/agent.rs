impl AgentRuntime {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            next_agent_id: AtomicUsize::new(0),
            state: Mutex::new(AgentRuntimeState::default()),
        }
    }

    fn monotonic_now(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64() * 1000.0
    }

    fn sleep(&self, milliseconds: u64) {
        thread::sleep(Duration::from_millis(milliseconds));
    }

    fn start_worker(self: &Arc<Self>, source: String) -> JsResult<()> {
        let mailbox = Arc::new(AgentMailbox::new(
            self.next_agent_id.fetch_add(1, Ordering::SeqCst),
        ));
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .agents
            .push(mailbox.clone());

        let (started_tx, started_rx) = mpsc::channel();
        let runtime = Arc::clone(self);
        thread::spawn(move || run_worker_agent(runtime, mailbox, source, started_tx));

        match started_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(JsNativeError::typ().with_message(message).into()),
            Err(_) => Err(JsNativeError::typ()
                .with_message("failed to start test262 agent")
                .into()),
        }
    }

    fn broadcast(&self, buffer: SharedArrayBuffer) {
        let agents = self
            .state
            .lock()
            .expect("agent runtime mutex poisoned")
            .agents
            .clone();
        let completion = Arc::new(BroadcastCompletion::new(agents.len()));

        for agent in agents {
            agent.enqueue(BroadcastEnvelope {
                buffer: buffer.clone(),
                completion: completion.clone(),
            });
        }

        completion.wait();
    }

    fn push_report(&self, report: String) {
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .reports
            .push_back(report);
    }

    fn pop_report(&self) -> Option<String> {
        self.state
            .lock()
            .expect("agent runtime mutex poisoned")
            .reports
            .pop_front()
    }

    fn unregister_agent(&self, id: usize) {
        let mut state = self.state.lock().expect("agent runtime mutex poisoned");
        state.agents.retain(|agent| agent.id != id);
    }
}

impl AgentMailbox {
    fn new(id: usize) -> Self {
        Self {
            id,
            state: Mutex::new(AgentMailboxState::default()),
            condvar: Condvar::new(),
        }
    }

    fn enqueue(&self, envelope: BroadcastEnvelope) {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        if state.closed {
            drop(state);
            envelope.completion.acknowledge();
            return;
        }

        state.pending = Some(envelope);
        self.condvar.notify_all();
    }

    fn receive(&self) -> Option<BroadcastEnvelope> {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        loop {
            if let Some(envelope) = state.pending.take() {
                return Some(envelope);
            }
            if state.closed {
                return None;
            }
            state = self
                .condvar
                .wait(state)
                .expect("agent mailbox mutex poisoned");
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("agent mailbox mutex poisoned");
        state.closed = true;
        if let Some(envelope) = state.pending.take() {
            drop(state);
            envelope.completion.acknowledge();
            self.condvar.notify_all();
            return;
        }
        self.condvar.notify_all();
    }
}

impl BroadcastCompletion {
    fn new(remaining: usize) -> Self {
        Self {
            remaining: Mutex::new(remaining),
            condvar: Condvar::new(),
        }
    }

    fn acknowledge(&self) {
        let mut remaining = self
            .remaining
            .lock()
            .expect("broadcast completion mutex poisoned");
        if *remaining == 0 {
            return;
        }
        *remaining -= 1;
        if *remaining == 0 {
            self.condvar.notify_all();
        }
    }

    fn wait(&self) {
        let mut remaining = self
            .remaining
            .lock()
            .expect("broadcast completion mutex poisoned");
        while *remaining != 0 {
            remaining = self
                .condvar
                .wait(remaining)
                .expect("broadcast completion mutex poisoned");
        }
    }
}

fn install_host_globals(context: &mut Context) -> boa_engine::JsResult<()> {
    ensure_host_hooks_context(context);
    context.register_global_builtin_callable(
        js_string!("print"),
        1,
        NativeFunction::from_fn_ptr(host_print),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_get_cached_import__"),
        3,
        NativeFunction::from_fn_ptr(host_get_cached_import),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_assert_import_source__"),
        2,
        NativeFunction::from_fn_ptr(host_assert_import_source),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_dynamic_import_defer__"),
        2,
        NativeFunction::from_fn_ptr(host_dynamic_import_defer),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_register_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_register_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_invoke__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_invoke),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_wrap_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_wrap_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_dynamic_import__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_dynamic_import),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_can_parse_script__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_can_parse_script),
    )?;
    install_console_object(context)?;
    install_disposable_stack_builtins(context)?;
    install_finalization_registry_builtin(context)?;
    install_array_buffer_detached_getter(context)?;
    install_array_buffer_immutable_hooks(context)?;
    install_data_view_immutable_hooks(context)?;
    install_promise_then_hook(context)?;
    install_string_replace_guard(context)?;
    install_string_match_guards(context)?;
    install_reg_exp_legacy_accessors(context)?;
    install_reg_exp_compile_guard(context)?;
    install_reg_exp_escape(context)?;
    install_array_from_async_builtin(context)?;
    install_array_flat_undefined_fix(context)?;
    install_uint8array_base_encoding_builtins(context)?;
    install_promise_keyed_builtins(context)?;
    install_intl_date_time_format_polyfill(context)?;
    Ok(())
}

fn install_child_realm_host_globals(context: &mut Context) -> boa_engine::JsResult<()> {
    ensure_host_hooks_context(context);
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_register_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_register_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_invoke__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_invoke),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_wrap_callable__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_wrap_callable),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_dynamic_import__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_dynamic_import),
    )?;
    context.register_global_builtin_callable(
        js_string!("__agentjs_shadowrealm_can_parse_script__"),
        1,
        NativeFunction::from_fn_ptr(host_shadow_realm_can_parse_script),
    )?;
    // Child realms need the same observable realm-local constructors/prototypes
    // for cross-realm Test262 coverage. Keep this list targeted to the built-ins
    // whose constructors or statics are exercised through $262.createRealm().
    install_disposable_stack_builtins(context)?;
    install_finalization_registry_builtin(context)?;
    install_reg_exp_escape(context)?;
    install_iterator_helpers(context)?;
    Ok(())
}
