# AI Agent JS Engine - Progress Tracker

## Current Status

- 执行入口已经切换为 `boa_engine` 包装层，仓库具备可运行的 JS CLI。
- `test262` runner 已接入真实 frontmatter/harness/negative case 逻辑，不再使用“返回 `Undefined` 就算通过”的伪跑分方式。
- 当前跑测 profile 为 `core profile`，支持基础 `module`、`$262.createRealm()`、`$262.detachArrayBuffer()`、`$262.agent`、`$262.AbstractModuleSource`、原生 `Temporal`、基础 `Intl`/`intl402` 顶层样本，以及通过兼容层支持 `dynamic import` 第二参数、`json-modules`、`import-text`、`import-bytes`、一部分 `import-defer` 和一部分 `source-phase-imports`；仍会跳过 `staging` 和更重的 `intl402` / 高级模块扩展。

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
   - runner 现在会放行 `import-defer` 的动态 catch、dynamic import valid syntax、dynamic sync module graph、整个静态 syntax 目录，以及 `deferred-namespace-object` / `evaluation-triggers` 大部分子组 / `syntax-error` / `resolution-error` / `evaluation-sync` / `module-throws` / 同步 `get-*` re-entrancy 错误组
   - 修正 async module harness 拼接：在 module case 中把 `$DONE` 显式挂到 `globalThis`，让 `asyncHelpers.js` 在 ESM 里也能工作
   - 给 deferred namespace 加了最小 recursion / re-entry 防护，避免自引用 import defer 直接把 runtime 带进无限递归
   - 验证：
     - `TEST262_FILTER='import-defer'`：`146 / 146` 执行通过，`77` 个更重 case 继续跳过
     - `test/language/import/import-defer/deferred-namespace-object/`：`4 / 4` 通过
     - `test/language/import/import-defer/evaluation-triggers/`：`63 / 63` 执行通过，`3` 个已知边角 case 继续跳过
     - `test/language/expressions/dynamic-import/import-defer/sync/`：`1 / 1` 通过
     - `test/language/import/import-defer/errors/syntax-error/import-defer-of-syntax-error-fails.js`：`1 / 1` 通过
     - `test/language/import/import-defer/errors/resolution-error/import-defer-of-missing-module-fails.js`：`1 / 1` 通过
     - `test/language/import/import-defer/evaluation-sync/`：`2 / 2` 通过
     - `test/language/import/import-defer/errors/module-throws/`：`3 / 3` 通过
     - `test/language/import/import-defer/errors/get-*.js` 同步子组：`4 / 4` 通过
14. 启用 Boa 原生 `Temporal`：
   - 在 `Cargo.toml` 为 `boa_engine` 打开 `temporal` feature
   - 确认现有 `Context::builder().build()` / `create_realm()` 路径会自动初始化 Temporal builtins，无需额外 provider wiring
   - runner 不再对 `test/built-ins/Temporal` 做整目录硬编码跳过
   - 增加 runtime smoke tests，覆盖普通脚本、路径脚本、module 和 `$262.agent` worker 中的 `Temporal` 可见性
   - 验证：
     - `cargo test --test isolated_test`：`25 passed, 1 ignored`
     - `TEST262_FILTER='Temporal' cargo test --test test262_runner -- --ignored --exact test262_core_profile`：通过
     - `TEST262_FILTER='test/built-ins/Temporal/' cargo test --test test262_runner -- --ignored --exact test262_core_profile`：通过
15. 修复 core profile 的两处 test262 runner 炸栈路径：
   - 对普通 JS `import()` 也复用 `CompatModuleLoader` 已缓存的 namespace object，避免 `reuse-namespace-object-from-import.js` 这类 case 走入 Boa 的动态导入炸栈路径
   - 给 `tests/test262_runner.rs` 和需要的 isolated regression test 改用 32MB 栈线程执行，避免 `S13.2.1_A1_T1.js` 这类 32 层函数嵌套样本把默认测试线程栈打爆
   - 验证：
     - `TEST262_FILTER='test/language/expressions/dynamic-import/' cargo test --test test262_runner -- --ignored --exact test262_core_profile`：通过
     - `TEST262_FILTER='test/language/statements/function/S13.2.1_A1_T1.js' cargo test --test test262_runner -- --ignored --exact test262_core_profile`：通过
     - `cargo test --test test262_runner -- --ignored --exact test262_core_profile`：通过
16. 启用最小 `Intl` / `intl402` 子集：
   - 在 `Cargo.toml` 为 `boa_engine` 打开 `intl_bundled` feature，使用 Boa 自带 ICU/provider 数据，避免额外 provider wiring
   - 增加 `Intl` smoke tests，验证脚本和 module 入口都能构造 `Intl.NumberFormat` / `Intl.Collator`
   - runner 当前先放行 `test/intl402/*.js` 顶层基础样本，保守跳过更重的分目录 case
   - 验证：
     - `cargo test --test isolated_test engine_exposes_intl_in_eval_scripts -- --exact`：通过
     - `cargo test --test isolated_test engine_exposes_intl_in_file_scripts -- --exact`：通过
     - `cargo test --test isolated_test engine_exposes_intl_in_modules -- --exact`：通过
     - `test/intl402/*.js` 顶层 22 个样本逐个运行：全部通过

## Next Steps

- [ ] 继续补 `import-defer` 的 deferred namespace / evaluation 语义，以及更完整的 `source-phase-imports`。
- [ ] 继续扩充其余 host hooks，例如 `gc` 等较少见的测试接口。
- [ ] 评估是否启用 `Intl` 特性，拉高 `intl402` 覆盖；当前已优先启用 `Temporal`，可沿同样流程逐步验证更重特性。
- [ ] 逐步把当前仓库自研 parser/interpreter 与新运行时能力对齐，而不是长期完全依赖外部内核。
