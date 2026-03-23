# Agent-JS-Engine

一个基于 Rust 的轻量级 JavaScript 执行引擎包装层，默认使用纯 Rust 的 `boa_engine` 作为运行时内核，并保留仓库内原有的手写 lexer/parser/interpreter 实验实现。

## 当前架构

- `src/engine/runtime.rs`
  轻量运行时封装。负责创建 Boa `Context`、注册宿主函数、执行脚本、收集 `print()` 输出、处理 `import()` / 资源模块兼容层，并为 `test262` 注入最小宿主对象。
- `src/main.rs`
  CLI 入口。支持 `--eval`、直接执行 JS 文件，以及 `--module`。
- `tests/test262_runner.rs`
  `test262` core profile runner。支持 frontmatter 解析、harness 注入、negative case 判定、async `$DONE` 处理、跳过不支持目录以及进度输出。
- `src/lexer` / `src/parser` / `src/engine/interpreter.rs`
  仓库原有的手写实现，当前保留用于后续自研内核迭代。

## 快速开始

```bash
# 构建
cargo build

# 执行文件
cargo run -- examples/demo.js

# 直接执行一段 JS
cargo run -- --eval "1 + 2"

# 拉取并运行 test262 core profile
./run_test262.sh
```

## CLI

```bash
cargo run -- [--strict] [--test262] [--module] <file.js>
cargo run -- [--strict] [--test262] [--module] --eval "print('hi')"
```

- `--strict`: 在脚本顶部注入 `"use strict"`.
- `--test262`: 注入最小 `test262` 宿主对象 `$262`.
- `--module`: 以 ECMAScript module 方式执行。

## Test262 策略

当前 runner 关注可稳定验证的 core profile：

- 注入 `sta.js`、`assert.js` 和 metadata 指定的 harness 文件
- 支持 `onlyStrict`、`raw`、`async`、`negative`、基础 `module`、`$262.createRealm()`、跨 realm `evalScript`、`$262.detachArrayBuffer()`、`$262.agent` 和 `$262.AbstractModuleSource`
- 支持基于兼容层的高级模块子集：`dynamic import` 第二参数、`json-modules`、`import-text`、`import-bytes`，并在已缓存模块上复用 dynamic import namespace object
- 支持 `import-defer` 的兼容子集：`import.defer(...)` 动态调用，以及基于 deferred wrapper module 的最小 `import defer * as ns from ...` 语义；当前可覆盖 dynamic syntax / abrupt-reject / dynamic sync module graph / static syntax / deferred namespace exotic object / evaluation-triggers 大部分子组 / resolution-error / evaluation-sync / module-throws / 同步 re-entrancy 错误组
- 支持 `source-phase-imports` 的兼容子集：`import.source(...)`、最小静态 `import source x from ...`、以及对应的 test262 `import-source` 动态 catch / valid syntax / module-code parse 组
- 补上最小 immutable `ArrayBuffer` 宿主语义：`immutable` getter、`transferToImmutable()`、`sliceToImmutable()`、`transfer()` / `transferToFixedLength()` fallback，以及 `DataView.prototype.set*` 的 immutable guard
- 自动排除 `*_FIXTURE.js` 依赖文件，避免把模块夹具误记为顶层测试
- 跳过 `staging`、`intl402` 以及暂未落地的 `import-defer` 延迟求值语义和更完整的 `source-phase-imports`
- 为每个 case 设置 loop iteration limit，避免单例卡死整轮跑测；test runner 本身会在更大的线程栈上执行，减少深递归/深嵌套样本导致的测试线程炸栈

已验证样本：

- `test/language/import/import-attributes/`: `17 / 17` 通过
- `test/language/expressions/dynamic-import/import-attributes/`: `23 / 23` 通过
- `test/language/import/import-bytes/`: `5 / 5` 通过
- `test/built-ins/AbstractModuleSource/`: `8 / 8` 通过
- `import-defer` 子集样本：`146 / 146` 执行通过，另有 `77` 个更重的 async/TLA / deferred evaluation case 继续跳过
- `import-source` 子集样本：`91 / 91` 执行通过，另有 `85` 个更重的 `source-phase-imports` case 仍按策略跳过
- immutable `ArrayBuffer` 相关 20 个样本：`20 / 20` 通过
- `test/built-ins/ArrayBuffer/prototype/transfer/`: `24 / 24` 通过
- `test/built-ins/ArrayBuffer/prototype/transferToFixedLength/`: `24 / 24` 通过

这套策略的目标是先把“真实可执行的 ES 核心能力”稳定拉到 60% 以上，再逐步补齐剩余高级模块语义和宿主能力。
