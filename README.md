# Agent-JS-Engine

一个基于 Rust 的轻量级 JavaScript 执行引擎包装层，默认使用纯 Rust 的 `boa_engine` 作为运行时内核，并保留仓库内原有的手写 lexer/parser/interpreter 实验实现。

## 当前架构

- `src/engine/runtime.rs`
  轻量运行时封装。负责创建 Boa `Context`、注册宿主函数、执行脚本、收集 `print()` 输出，并为 `test262` 注入最小宿主对象。
- `src/main.rs`
  CLI 入口。支持 `--eval` 和直接执行 JS 文件。
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
cargo run -- [--strict] [--test262] <file.js>
cargo run -- [--strict] [--test262] --eval "print('hi')"
```

- `--strict`: 在脚本顶部注入 `"use strict"`.
- `--test262`: 注入最小 `test262` 宿主对象 `$262`.

## Test262 策略

当前 runner 关注可稳定验证的 core profile：

- 注入 `sta.js`、`assert.js` 和 metadata 指定的 harness 文件
- 支持 `onlyStrict`、`raw`、`async`、`negative`
- 跳过 `staging`、`intl402`、`built-ins/Temporal`、`module` 以及部分依赖复杂宿主钩子的 `$262.*` 用例
- 为每个 case 设置 loop iteration limit，避免单例卡死整轮跑测

这套策略的目标是先把“真实可执行的 ES 核心能力”稳定拉到 60% 以上，再逐步补齐模块和更复杂宿主能力。
