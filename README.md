# AI Agent Execution Engine (Lightweight JS)

本仓库是为参与2026年“全国大学生计算机系统能力大赛-OS功能挑战赛道”开发的轻量级 JavaScript 执行引擎。

## 赛题背景
JavaScript 生态已成为 AI 时代的重要技术设施。在智能体（AI Agent）场景下，传统浏览器内置的重量级 JS 执行引擎过于臃肿。我们旨在提供一个能进行**短时、高频、即时执行**的轻便、极速的 JS 引擎系统。

## 核心架构设计

- **Lexer**: 零拷贝/低拷贝词法状态机 (`src/lexer`)
- **Parser**: 基于递归下降机制的轻量级 AST 构建 (`src/parser`)
- **Engine**: 指令解释器/运行期上下文管理 (`src/engine`)
- **GC**: 原生优化的微型垃圾回收器 (`src/gc`)

## 创新点
1. **绝对纯净的原生态**: 100% Rust 实现，无任何形式的套壳（不仅不依赖 V8，甚至不依赖底层 C 语言 JS 实现的二次绑定如 QuickJS FFI）。
2. **极简生命周期模型**: 借力 Rust 借用检查机制，大幅缩减 `Rc` 引用计数带来的运行时分发消耗。
3. **按需流式解析模型**: **(待完善)**

## 快速开始

```bash
# 构建项目
cargo build --release

# 运行引擎
cargo run

# 执行 Test262 测试套件跑分
cargo test --test test262_runner
```

## 测试进度
| 模块 | 测试通过率 | Test262 用例数 | 备注 |
| --- | --- | --- | --- |
| 整体通过率 | **0%** | TBD | （当前为基础空架子） |

> (本章节数据待实现引擎细节及 Test262 测试脚手架后自动刷新填充)
# Agent-JS-Engine
