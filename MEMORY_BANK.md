# Memory Bank & Architecture Map

## 项目目标
开发一个面向 AI Agent 的轻量级原生 JavaScript 执行引擎，纯 Rust 实现，零套壳。
最终目标：通过 ECMAScript Test Suite (test262) 60% 核心测试用例。

## 架构决策与设计哲学
1. **轻量级 & 零拷贝**:
   - 避免使用大而全的依赖。
   - 尽量使用生命周期注解 (`&'a str`) 和 `Cow`，减少 `String` 拷贝，将解析时的字符串持有开销降至最低。
   - 减少 `Rc<RefCell<T>>` 的滥用，初期使用精简的作用域树或 Arena 分配器来管理对象，实现纯净的所有权转移。
2. **纯粹性 (Non-wrapped)**:
   - 绝对不引入 V8/QuickJS 绑定，完全自己实现词法分析、语法分析、字节码解释（或树转移）及内存管理。
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
- [x] 基础 Test262 Runner (成功 Clone 测试集并跑通 2% Baseline)
- [ ] 中阶 Parser: 块作用域、If 控制流、函数声明支持
- [ ] GC 骨架与对象分配

