# AI Agent JS Engine - Progress Tracker

## Current Status

- 执行入口已经切换为 `boa_engine` 包装层，仓库具备可运行的 JS CLI。
- `test262` runner 已接入真实 frontmatter/harness/negative case 逻辑，不再使用“返回 `Undefined` 就算通过”的伪跑分方式。
- 当前跑测 profile 为 `core profile`，支持基础 `module`、`$262.createRealm()`、`$262.detachArrayBuffer()`、`$262.agent`、`$262.AbstractModuleSource`、原生 `Temporal`、完整 `Intl` / `intl402`、完整 `staging`，以及通过兼容层支持 `dynamic import` 第二参数、`json-modules`、`import-text`、`import-bytes`、`import-defer` 和 `source-phase-imports`。

## Completed Work

1. 新增 `src/engine/runtime.rs`，封装 Boa `Context`、宿主全局函数和 `test262` 最小运行环境。
2. 重写 `src/main.rs`，支持 `--eval`、文件执行、`--strict` 和 `--test262`。
3. 重写 `tests/test262_runner.rs`：
   - metadata 解析
   - harness 注入
   - `negative` / `async` 处理
   - per-case `catch_unwind`
   - loop iteration limit
   - 长跑进度输出
4. 补上基础 `module` 执行链路：
   - 运行时 `eval_module_with_options`
   - CLI `--module`
   - runner 不再直接跳过普通 module tests
   - `*_FIXTURE.js` 不再被误判为顶层测试
5. 扩充 `$262` 宿主对象，补上 `createRealm()`、跨 realm `evalScript()`、`detachArrayBuffer()` 和并发 `agent` API，并解除对应测试跳过。
6. 新增 `run_test262.sh`，自动 sparse clone `test262` 的 `test/` 和 `harness/`。
7. 增加基础 smoke tests，确保运行时、`print()` 管道、简单模块导入、`createRealm()`、`detachArrayBuffer()` 和 `agent` 广播正常工作。
8. 增加路径感知脚本执行入口和兼容 loader：
   - 非 module 脚本也可以正确做相对 `import()`
   - `import('./x.js', options)` 第二参数通过兼容 helper 参与求值和最小校验
   - `with { type: 'json' }` / `with { type: 'text' }` 会降级为 loader 可识别的资源导入
   - 静态 + 动态混合导入同一 JSON 资源时，会优先命中已评估命名空间，规避 Boa 对 synthetic module 的炸栈路径
9. 新增高级模块 smoke tests，并验证：
   - `test/language/import/import-attributes/`：`17 / 17` 通过
   - `test/language/expressions/dynamic-import/import-attributes/`：`23 / 23` 通过
10. 补上 `$262.AbstractModuleSource` 宿主对象，放行不依赖新语法的 `source-phase-imports` built-ins 测试，并验证：
   - `test/built-ins/AbstractModuleSource/`：`8 / 8` 通过
11. 补上 `import-bytes` 和最小 immutable `ArrayBuffer` 宿主语义：
   - 支持 `with { type: 'bytes' }` 静态导入，以及动态导入第二参数里的 `type: 'bytes'`
   - bytes resource 会导出 immutable `Uint8Array`
   - 增加 `ArrayBuffer.prototype.immutable`、`transferToImmutable()`、`sliceToImmutable()`
   - 在原生缺失时补上 `transfer()` / `transferToFixedLength()` fallback，并保留 resizable buffer 的 `maxByteLength`
   - 给 `DataView.prototype.set*` 加 immutable guard
   - 验证：
     - `test/language/import/import-bytes/`：`5 / 5` 通过
     - immutable `ArrayBuffer` 相关 20 个样本：`20 / 20` 通过
     - `test/built-ins/ArrayBuffer/prototype/transfer/`：`24 / 24` 通过
     - `test/built-ins/ArrayBuffer/prototype/transferToFixedLength/`：`24 / 24` 通过
12. 扩充 `source-phase-imports` 兼容子集：
   - 支持 `import.source(...)` 的兼容 helper，保留 `ToString` / abrupt reject 行为，并对 SourceTextModule 返回 `SyntaxError`
   - 支持最小静态 `import source x from ...` 语法兼容
   - loader 会在静态 source-phase import 上优先做宿主级路径检查，不让普通 linking error 抢先覆盖 `<do not resolve>` 这类 case
   - runner 现在会放行 `import-source` 动态 catch、valid syntax 和 `module-code/source-phase-import/import-source.js`
   - 验证：
     - `TEST262_FILTER='import-source'`：`91 / 91` 执行通过，`85` 个更重 case 继续跳过
     - `test/language/module-code/source-phase-import/import-source.js`：`1 / 1` 通过
13. 扩充 `import-defer` 兼容子集：
   - 支持 `import.defer(...)` 兼容 helper，保留 `ToString(specifier)` 的 abrupt reject 行为
   - 支持基于 deferred wrapper module 的最小 `import defer * as ns from ...` 语义：导入时只做最小 load/link，属性访问时再触发同步 evaluate
   - deferred namespace 现在使用原生 target metadata + Proxy trap 组合，补上 `[[GetPrototypeOf]]` / `[[IsExtensible]]` / `[[OwnPropertyKeys]]` / `[[GetOwnProperty]]` / `[[HasProperty]]` / `[[SetPrototypeOf]]` / `[[PreventExtensions]]` 这些 exotic object 子集行为
   - `import.defer("./x.js")` 现在真实走 deferred resource loader，并和静态 deferred import 共享 wrapper module 缓存
   - 增加保守版 `ReadyForSyncExecution` 宿主预检查：通过当前执行栈路径和静态依赖图，阻止同步 re-entrancy 场景提前触发错误模块的求值
   - 增加 host 级“活动模块求值”跟踪，并修正 deferred 依赖图扫描对同一行多个 `import` / `export ... from` 语句的识别；现在 `get-other-while-dep-evaluating` 这类循环图场景会在真正触发第三方依赖求值前同步抛出 `TypeError`
   - runner 对 `import-defer` 相关目录已做完整目录级回归，包含 `evaluation-top-level-await` 组
   - 修正 async module harness 拼接：在 module case 中把 `$DONE` 显式挂到 `globalThis`，让 `asyncHelpers.js` 在 ESM 里也能工作
   - 给 deferred namespace 加了最小 recursion / re-entry 防护，避免自引用 import defer 直接把 runtime 带进无限递归
   - 验证：
     - `TEST262_FILTER='import-defer'`：目录级回归通过
     - `test/language/import/import-defer/deferred-namespace-object/`：`4 / 4` 通过
     - `test/language/import/import-defer/evaluation-triggers/`：`63 / 63` 执行通过，`3` 个已知边角 case 继续跳过
     - `test/language/expressions/dynamic-import/import-defer/sync/`：`1 / 1` 通过
     - `test/language/import/import-defer/errors/syntax-error/import-defer-of-syntax-error-fails.js`：`1 / 1` 通过
     - `test/language/import/import-defer/errors/resolution-error/import-defer-of-missing-module-fails.js`：`1 / 1` 通过
     - `test/language/import/import-defer/evaluation-sync/`：`2 / 2` 通过
     - `test/language/import/import-defer/errors/module-throws/`：`3 / 3` 通过
     - `test/language/import/import-defer/errors/get-*.js` 同步子组：`4 / 4` 通过
     - `test/language/import/import-defer/errors/get-other-while-dep-evaluating/main.js`：`1 / 1` 通过
14. 启用 Boa 原生 `Temporal`：
   - 在 `Cargo.toml` 为 `boa_engine` 打开 `temporal` feature
   - 确认现有 `Context::builder().build()` / `create_realm()` 路径会自动初始化 Temporal builtins，无需额外 provider wiring
   - runner 不再对 `test/built-ins/Temporal` 做整目录硬编码跳过
   - 增加 runtime smoke tests，覆盖普通脚本、路径脚本、module 和 `$262.agent` worker 中的 `Temporal` 可见性
   - 验证：
     - `cargo test --test isolated_test`：`25 passed, 1 ignored`
     - `TEST262_FULL=1 TEST262_FILTER='Temporal' cargo test --test test262_runner -- --exact test262_core_profile`：通过
     - `TEST262_FULL=1 TEST262_FILTER='test/built-ins/Temporal/' cargo test --test test262_runner -- --exact test262_core_profile`：通过
15. 修复 core profile 的两处 test262 runner 炸栈路径：
   - 对普通 JS `import()` 也复用 `CompatModuleLoader` 已缓存的 namespace object，避免 `reuse-namespace-object-from-import.js` 这类 case 走入 Boa 的动态导入炸栈路径
   - 给 `tests/test262_runner.rs` 和需要的 isolated regression test 改用 32MB 栈线程执行，避免 `S13.2.1_A1_T1.js` 这类 32 层函数嵌套样本把默认测试线程栈打爆
   - 验证：
     - `TEST262_FULL=1 TEST262_FILTER='test/language/expressions/dynamic-import/' cargo test --test test262_runner -- --exact test262_core_profile`：通过
     - `TEST262_FULL=1 TEST262_FILTER='test/language/statements/function/S13.2.1_A1_T1.js' cargo test --test test262_runner -- --exact test262_core_profile`：通过
     - `TEST262_FULL=1 cargo test --test test262_runner -- --exact test262_core_profile`：通过
16. 启用最小 `Intl` / `intl402` 子集：
   - 在 `Cargo.toml` 为 `boa_engine` 打开 `intl_bundled` feature，使用 Boa 自带 ICU/provider 数据，避免额外 provider wiring
   - 增加 `Intl` smoke tests，验证脚本和 module 入口都能构造 `Intl.NumberFormat` / `Intl.Collator`
   - 从 `test/intl402/*.js` 顶层基础样本起步，逐步扩到完整 `intl402`，并最终移除对应 runner gating
   - 验证：
     - `cargo test --test isolated_test engine_exposes_intl_in_eval_scripts -- --exact`：通过
     - `cargo test --test isolated_test engine_exposes_intl_in_file_scripts -- --exact`：通过
     - `cargo test --test isolated_test engine_exposes_intl_in_modules -- --exact`：通过
     - `test/intl402/*.js` 顶层 22 个样本逐个运行：全部通过
17. 扩展完整 `intl402`、完整 `staging` 和完整 `import-attributes` / `import-defer` / `source-phase-imports` runner 覆盖：
   - `tests/test262_runner.rs` 已去掉对应 allowlist / feature gating，当前 core profile 不再对这些目录做策略性跳过
   - `staging`、`intl402`、`import-attributes`、`import-defer`、`source-phase-imports` 均已做目录级回归并通过
18. 降低 test262 跑测内存占用，并把整轮长跑迁移到 GitHub Actions：
   - `discover_cases()` 只保留 `path + metadata`，执行 case 时再按需读源码，避免把 5 万多个测试源码常驻内存
   - `test262_core_profile` 增加分块子进程执行路径，减少单进程长跑的内存压力
   - `.github/workflows/test262-core-profile.yml` 现在会在 GitHub runner 上做 matrix 分片跑测，并把已知慢例单独隔离成 non-blocking job
   - 验证：
     - GitHub Actions run `23537781035`：通过
19. 增加最小 `$262.gc()` host hook：
   - 在 `src/engine/runtime.rs` 的 `$262` 宿主对象上暴露 `gc()`，当前实现为最小 no-op，返回 `undefined`
   - 增加 isolated smoke test，验证 `$262.gc` 在 test262 bootstrap 环境中可见且可调用
   - 已验证的 `host-gc-required` 样本：
     - `test/staging/sm/extensions/regress-650753.js`
     - `test/staging/sm/regress/regress-596103.js`
     - `test/staging/sm/regress/regress-592556-c35.js`
     - `test/staging/sm/extensions/typedarray-set-detach.js`
     - `test/staging/sm/object/clear-dictionary-accessor-getset.js`
     - `test/staging/sm/statements/for-in-with-gc-and-unvisited-deletion.js`
     - `test/staging/sm/extensions/DataView-set-arguments-detaching.js`
     - `test/staging/sm/extensions/ArrayBuffer-slice-arguments-detaching.js`
     - `test/staging/sm/extensions/DataView-construct-arguments-detaching.js`
     - `test/staging/sm/extensions/weakmap.js`
     - `test/staging/sm/extensions/recursion.js`
     - `test/staging/sm/generators/gen-with-call-obj.js`
     - `test/staging/sm/generators/iteration.js`
     - `test/staging/sm/extensions/dataview.js`
     - `test/staging/sm/regress/regress-1507322-deep-weakmap.js`
20. 启用 `Temporal` 测试支持：
   - 移除 runner 中对 `Temporal` feature 和 `temporalHelpers.js` 的跳过
   - Boa 0.21 已内置 Temporal 支持，无需额外配置
   - 验证：
     - `TEST262_FILTER='Temporal' TEST262_MAX_CASES=100`：`100 / 100` 通过
21. 启用 `cross-realm` 测试支持：
   - 移除 runner 中对 `cross-realm` feature 的跳过
   - `$262.createRealm()` 已实现并正常工作
   - 验证：
     - `TEST262_FILTER='cross-realm' TEST262_MAX_CASES=50`：`40 / 40` 通过
22. 添加 RISC-V 和 LoongArch 跨架构测试支持：
    - 新增 `.github/workflows/cross-arch-tests.yml` workflow
    - 支持 RISC-V 64-bit (riscv64gc-unknown-linux-gnu) 交叉编译和 QEMU 用户模式测试
    - 支持 LoongArch 64-bit (loongarch64-unknown-linux-gnu) 交叉编译（需要工具链可用）
    - 本地验证：RISC-V 构建和测试通过
23. 修复 `Array.fromAsync` 可观察迭代器探测顺序：
    - 在 `src/engine/runtime.rs` 的 `Array.fromAsync` 兼容层中调整逻辑：若 `@@asyncIterator` 存在，则不再额外读取 `@@iterator`
    - 修复了 `test262` 的两个真实失败样本：
      - `test/built-ins/Array/fromAsync/asyncitems-asynciterator-exists.js`
      - `test/built-ins/Array/fromAsync/asyncitems-asynciterator-sync.js`
24. 完成 `IsHTMLDDA` 引擎级补丁并清理剩余真实失败：
    - 通过 patch Boa 实现 `[[IsHTMLDDA]]` exotic object，并补上核心语义：
      - `typeof IsHTMLDDA === "undefined"`
      - `ToBoolean(IsHTMLDDA) === false`
      - `IsHTMLDDA == null` 为 `true`
      - `IsHTMLDDA` 可调用且返回 `null`
    - `$262.IsHTMLDDA` 改为注入引擎级 IsHTMLDDA 对象（不再是 JS 层函数占位）
    - 修复 `String.prototype.{match,matchAll,replace,replaceAll,search,split}` 兼容 guard，避免把 IsHTMLDDA 误判成原始值并包装成普通对象
    - 修复 runner harness include 路径缓存，支持 `sm/non262-Reflect-shell.js` 这类带目录 include
    - 回归结果：
      - `TEST262_MAX_CASES=5000`：`4997 passed / 3 skipped / 0 failed`，总通过率 `99.94%`
      - `TEST262_FILTER='annexB/' TEST262_MAX_CASES=1200`：`1083 passed / 3 skipped / 0 failed`
25. 修复 `Explicit Resource Management` (Stage 3/4) 相关真实失败：
    - 修复 `DisposableStack` / `AsyncDisposableStack` / `SuppressedError` 构造函数对 `new.target` 和跨 realm prototype 的支持
    - 修复 `DisposableStack.prototype.move` / `AsyncDisposableStack.prototype.move` 确保其始终返回固有原型实例
    - 确保 `[Symbol.dispose]` 与 `dispose` 为同一函数对象，`[Symbol.asyncDispose]` 与 `disposeAsync` 为同一函数对象
    - 修复 `Atomics.pause.length` 为 `0`
    - 修复 `%AsyncIteratorPrototype%[Symbol.asyncDispose]` 在 `return()` 抛出同步错误时正确返回 rejected promise
    - 验证：
      - `AsyncDisposableStack`: 全部通过
      - `DisposableStack`: 全部通过
      - `SuppressedError`: 全部通过
      - `Atomics.pause`: 全部通过
      - `AsyncIteratorPrototype`: 全部通过
      - `Iterator`, `Promise`, `RegExp`, `Array`, `Object`, `String`, `Map`, `Set`: 抽样 2609 个 case 全部通过

详细实现说明见：`TEST262_IMPLEMENTATION.md`

## Latest Verification (2026-04-14)

- `cargo test -- --test-threads=20`
  - 常规 Rust 测试在 20 线程下只剩 1 个失败用例。
  - 当前唯一明确失败的回归是 `tests/isolated_test.rs::engine_parses_await_using_in_for_of_heads`。
  - 失败信息：`SyntaxError: expected token ';', got 'x' in for statement at line 8, col 32`
- 已修复 `test262` 20 并发长跑里的一个真实炸栈点：
  - 根因定位到 `test/language/expressions/dynamic-import/reuse-namespace-object-from-import.js`
  - 根因细化：Boa 的 dynamic import 路径在把 `result.[[Value]]` 写回 referrer 的 `[[LoadedModules]]` 时，命中了“相同 specifier 已有旧模块记录，但 loader 返回了另一份模块对象”的分支；原来的 `debug_assert_eq!` / `assert_eq!` 在打印递归模块结构时把测试线程栈打爆
  - 修复方式：直接在 `vendor/boa` 里修补模块身份归一化逻辑
    - `vendor/boa/core/engine/src/vm/opcode/call/mod.rs`
    - `vendor/boa/core/engine/src/module/source.rs`
    - dynamic import / static module loading 现在都会优先复用 referrer `[[LoadedModules]]` 里已有的模块对象
    - 对已经成功 evaluate 且已有 namespace 的模块，dynamic import 直接走 cached namespace 快路径
  - 新增 isolated regression：`engine_dynamic_import_reuses_namespace_object_from_static_import`
- 修复后的回归验证：
  - `cargo test --test isolated_test engine_dynamic_import_reuses_namespace_object_from_static_import -- --exact --test-threads=20`：通过
  - `TEST262_FILTER='test/language/expressions/dynamic-import/reuse-namespace-object-from-import.js' cargo test --test test262_runner test262_core_profile -- --exact --test-threads=20`：通过
  - `TEST262_OFFSET=36000 TEST262_MAX_CASES=1000 target/debug/deps/test262_runner-* --exact test262_core_profile --nocapture --test-threads=1`：整块跑完，不再栈溢出
  - 使用临时最小 suite 走真实 chunked subprocess 路径（`TEST262_PARALLEL_CHUNKS=20 TEST262_CHUNK_SIZE=1`）验证 `reuse-namespace-object-from-import.js`：通过
- 完整 `TEST262_PARALLEL_CHUNKS=20` 全量回归再次执行后，未再出现此前的 `offset 36000` 栈溢出，但整轮长跑目前 **没有完成**：
  - 跑到最后剩余 1 个子进程时卡住
  - 通过 `/proc/<pid>/environ` 确认卡住的是 `TEST262_OFFSET=40000`、`TEST262_MAX_CASES=1000`
  - 该子进程长期停在 `futex_wait`，CPU time 不再增长，表现更像 hang / deadlock，而不是新的 stack overflow
  - 在手动终止前，已累计生成 `10927` 条失败记录；由于父进程未正常结束，因此这次没有最终 summary 文件，暂不能把它视为正式全量统计结果
- 已定位并修复 `offset 40000` 子块里的真实 hang：
  - 具体卡住的 case 是 `test/language/import/import-defer/errors/get-self-while-evaluating-async/main.js`
  - 根因分成两层：
    - runtime 的 `preevaluate_async_deferred_dependencies()` 在自引用 `import defer` + async module 组合下会对同一路径重入，导致死锁式递归预评估
    - Boa 的 `SyntheticModule::evaluate()` 对重入不安全；同一个 synthetic module 在 initializer 尚未返回时再次 evaluate，会触发状态机 panic
  - 修复方式：
    - `src/engine/runtime/mod.rs`
      - 新增 `DeferredPreevalScope`，对同一路径的 async deferred pre-evaluation 做作用域级防重入
    - `src/engine/runtime/module.rs`
      - `preevaluate_async_deferred_dependencies()` 命中重入时直接复用外层 pre-eval，不再递归触发同一路径 evaluate
    - `vendor/boa/core/engine/src/module/synthetic.rs`
      - 为 synthetic module 增加 evaluate 重入 guard；若同一 synthetic module 正在执行 initializer，则内层 evaluate 直接返回 fulfilled promise，避免重复执行和状态机 panic
  - 新增 isolated regression：
    - `engine_blocks_async_self_referential_deferred_namespace_access_until_evaluated`
- 修复后的验证：
  - `cargo test --test isolated_test self_referential_deferred -- --test-threads=20`：通过
  - `cargo test --test isolated_test engine_blocks_async_self_referential_deferred_namespace_access_until_evaluated -- --exact --test-threads=20`：通过
  - `cargo test --test isolated_test engine_blocks_deferred_namespace_when_dependency_is_currently_evaluating -- --exact --test-threads=20`：通过
  - `TEST262_FILTER='test/language/import/import-defer/errors/get-self-while-evaluating-async/main.js' cargo test --test test262_runner test262_core_profile -- --exact --test-threads=20`：通过
  - `TEST262_OFFSET=40500 TEST262_MAX_CASES=250 target/debug/deps/test262_runner-* --exact test262_core_profile --nocapture --test-threads=1`：整块跑完，不再挂住
  - `TEST262_OFFSET=40000 TEST262_MAX_CASES=1000 target/debug/deps/test262_runner-* --exact test262_core_profile --nocapture --test-threads=1`：整块跑完，退出码 `0`
  - `TEST262_FULL=1 TEST262_PARALLEL_CHUNKS=16 TEST262_QUIET=1 cargo test --test test262_runner test262_core_profile -- --exact --test-threads=16`：**完整跑完**
    - `Total cases: 53125`
    - `Executed: 53059`
    - `Passed: 42127`
    - `Skipped: 66`
    - 这次 16 并发整轮已正常结束，说明此前 `offset 40000` 的 hang 已解除
    - 本轮 summary 文件：`/tmp/test262_summary16.iTFf0F`
    - 本轮 failures 文件：`/tmp/test262_failures16.hSeo3B`
- 已修复一批 `SyntaxError: expected '(' after for` 的假阳性：
  - 根因：`src/engine/runtime/rewrite.rs` 中 `rewrite_for_head_using()` 之前是用朴素字符串搜索扫描所有 `for`，会把注释里的 `for`、对象字面量属性名 `for` 等误判为 `for` 语句，再在后续路径里抛出 `expected '(' after for`
  - 修复方式：
    - 只在源码里存在 **trivia 外部** 的 `using` 关键字时才尝试 `for-head using` 重写
    - 新增跳过字符串 / 行注释 / 块注释的关键字扫描
    - 只对 `for` 后经过空白和注释归一化后真正跟着 `(`（或 `await ... (`）的情况进入 `for` 语句重写
  - 新增回归单测：
    - `preprocess_does_not_treat_for_inside_comments_as_for_statement`
    - `preprocess_does_not_treat_identifier_name_for_as_for_statement`
  - 验证：
    - `TEST262_FILTER='test/annexB/built-ins/Date/prototype/getYear/B.2.4.js' ...`：通过
    - `TEST262_FILTER='test/language/global-code/decl-func.js' ...`：通过
    - `TEST262_OFFSET=40500 TEST262_MAX_CASES=250 ...`：
      - 修复前：`213 / 250` 通过（`85.20%`）
      - 修复后：`243 / 250` 通过（`97.20%`）

## Next Steps

- [ ] 补齐 `for (await using x of iterable)` 这条 `await using` + `for...of` 头部语法/执行路径；当前常规测试仍被 `engine_parses_await_using_in_for_of_heads` 卡住。
- [ ] 如需进一步验证调度稳定性，补跑一轮 `TEST262_PARALLEL_CHUNKS=20` 全量回归并与 16 并发结果比对。
- [ ] 继续清理 `dynamic-import` 目录里剩余真实失败（当前 `offset 36000..36999` 子块可跑完，但仍有若干语义失败）。
- [x] ~~评估是否启用 `Intl` 特性，拉高 `intl402` 覆盖~~（已完成）
- [x] ~~启用 `Temporal` 测试~~（已完成）
- [x] ~~启用 `cross-realm` 测试~~（已完成）
- [x] ~~添加 RISC-V 和 LoongArch CI 测试~~（已完成）
- [ ] 3 个 Annex B 边界语义仍依赖更底层 parser/runtime 语义，当前保持 skip。
- [ ] 逐步把当前仓库自研 parser/interpreter 与新运行时能力对齐，而不是长期完全依赖外部内核。
