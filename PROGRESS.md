# AI Agent JS Engine - Progress Tracker

## Current Status

- 执行入口已经切换为 `boa_engine` 包装层，仓库具备可运行的 JS CLI。
- `test262` runner 已接入真实 frontmatter/harness/negative case 逻辑，不再使用“返回 `Undefined` 就算通过”的伪跑分方式。
- 当前跑测 profile 为 `core profile`，支持基础 `module`、`$262.createRealm()` 和 `$262.detachArrayBuffer()`，仍会跳过 `intl402`、`Temporal`、`staging`、部分高级模块扩展，以及 `$262.agent`。

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
5. 扩充 `$262` 宿主对象，补上 `createRealm()`、跨 realm `evalScript()` 和 `detachArrayBuffer()`，并解除对应测试跳过。
6. 新增 `run_test262.sh`，自动 sparse clone `test262` 的 `test/` 和 `harness/`。
7. 增加基础 smoke tests，确保运行时、`print()` 管道、简单模块导入、`createRealm()` 和 `detachArrayBuffer()` 正常工作。

## Next Steps

- [ ] 继续补高级模块特性，如 `import attributes` / `import defer` / `source phase imports`。
- [ ] 继续扩充 `$262` 宿主对象，优先实现 `agent` 等剩余高频测试钩子。
- [ ] 评估是否启用 `Intl` 特性，拉高 `intl402` 覆盖。
- [ ] 逐步把当前仓库自研 parser/interpreter 与新运行时能力对齐，而不是长期完全依赖外部内核。
