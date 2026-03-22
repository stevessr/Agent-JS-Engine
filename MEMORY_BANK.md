# Memory Bank & Architecture Map

## 项目目标
开发一个面向 AI Agent 的轻量级 JavaScript 执行引擎，Rust 实现。
当前阶段目标：通过真实 `test262` core profile 的 60% 以上用例，并提供可直接运行的 CLI。

## 架构决策与设计哲学
1. **轻量级 & 零拷贝**:
   - 避免使用大而全的依赖。
   - 尽量使用生命周期注解 (`&'a str`) 和 `Cow`，减少 `String` 拷贝，将解析时的字符串持有开销降至最低。
   - 减少 `Rc<RefCell<T>>` 的滥用，初期使用精简的作用域树或 Arena 分配器来管理对象，实现纯净的所有权转移。
2. **纯 Rust 内核优先**:
   - 不引入 V8/QuickJS 等 C/C++ 绑定。
   - 当前运行时采用纯 Rust 的 `boa_engine`，同时保留仓库自研 lexer/parser/interpreter 作为后续演进方向。
3. **数据驱动开发**:
   - 使用 Test262 的子集持续验证功能，任何功能的实现均需有匹配的 Test262 测试通过。

## 目录模块说明
- `src/lexer`: 词法分析，产出按需切片的 `Token` 流。
- `src/parser`: 递归下降分析，构建纯 Rust 结构体的 `AST` (抽象语法树)。
- `src/engine`: 解释器/执行环境封装，包含执行上下文、作用域解析与闭包处理。
- `src/gc`: 垃圾回收管理。暂时采用简单复制或者基于 Arena 的引用计数。
- `src/utils`: 工具与内存抽象。
- `tests/`: Test262 的执行脚手架。

## 进展追踪
- [x] 工程初始化 & 骨架设计
- [x] 基础 Lexer (var, let, 操作符, 单行/多行注释忽略)
- [x] 基础 Parser (变量声明, 二元表达式)
- [x] 基础 Engine/Interpreter (基本值类型、环境字典建模和简单表达式求值)
- [x] 基础 Test262 Runner (支持 metadata/harness/negative/async)
- [x] CLI 执行入口 (`cargo run -- --eval "1 + 2"`)
- [ ] 模块执行与更完整的宿主 `$262`
- [ ] `intl402` / `Temporal` 支持策略
- [ ] 自研 parser/interpreter 与当前运行时对齐
