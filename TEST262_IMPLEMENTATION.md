# Test262 实现文档

## 目标与现状

本仓库当前的 `test262` 支持目标，不是做一个“只跑少量 smoke case”的伪集成，而是让真实的 `test262` 用例通过统一的 runner、真实的 frontmatter / harness / negative case 逻辑，以及项目运行时里的最小宿主环境来执行。

当前实现已经具备以下状态：

- `core profile` 可执行。
- `Intl / intl402` 已整块放开。
- `staging` 已整块放开。
- `Temporal` 已原生启用并接入。
- `import-attributes`、`import-bytes`、`import-defer`、`source-phase-imports` 都已经接入当前运行时路径。
- `$262` 已提供一批关键宿主接口，包括 `createRealm()`、`detachArrayBuffer()`、`gc()`、`agent` 和 `AbstractModuleSource`。
- 整轮长跑验证已迁移到 GitHub Actions，避免继续依赖本机跑完全量 suite。

> 说明：这不等于“对 ECMAScript 所有边角语义都已经完美实现”。它表示当前 runner 和 runtime 已经能够真实执行 `core profile`，并把大量过往被整体跳过的大块能力逐步放开。

---

## 关键文件

### Runner

- `tests/test262_runner.rs`

### Runtime / Host hooks

- `src/engine/runtime.rs`

### Smoke tests

- `tests/isolated_test.rs`

### CI / 长跑验证

- `.github/workflows/test262-core-profile.yml`
- `run_test262.sh`

---

## Runner 架构

`tests/test262_runner.rs` 当前承担 4 件事：

1. **发现 test262 case**
2. **解析 frontmatter metadata**
3. **拼接 harness + test source**
4. **执行 case 并汇总统计**

### 1. Case 发现

Runner 会遍历 `test262/test`，过滤出：

- 后缀为 `.js`
- 非 `*_FIXTURE.js`

当前实现分两层：

- `discover_case_paths()`：只发现路径，支持 `TEST262_FILTER`、`TEST262_OFFSET`、`TEST262_MAX_CASES`
- `discover_cases()`：只对选中的路径读取源码并解析 metadata

这样做的目的是降低内存占用：

- 不再把 5 万多个测试源码全量常驻内存
- 对分片、单目录、单文件验证尤其有效

### 2. Metadata 解析

metadata 通过 frontmatter 里的 YAML 提取：

- `includes`
- `flags`
- `features`
- `negative.phase`
- `negative.type`

核心逻辑在：

- `extract_metadata()`
- `Test262Metadata`
- `NegativeMetadata`

### 3. Harness 拼接

Runner 使用：

- `sta.js`
- `assert.js`
- `doneprintHandle.js`（async case）
- metadata 指定的 harness

核心逻辑在：

- `HarnessCache::load()`
- `build_source()`

`HarnessCache` 现在按 `harness/` 相对路径建索引（而不是仅文件名），因此 `sm/non262-Reflect-shell.js` 这类带目录 include 可以被稳定加载。

对于 `async + module` case，会显式把 `$DONE` 暴露到 `globalThis`，避免 ESM 场景下 harness 不可见。

### 4. Case 执行与统计

每个 case 最终会走：

- `JsEngine::eval_script_with_options(...)`
- 或 `JsEngine::eval_module_with_options(...)`

执行结果按三类统计：

- `Passed`
- `Failed`
- `Skipped`

当前 `Skipped` 分支已经基本不再作为主策略使用；大量历史 allowlist / feature gating 已被删除，runner 更接近“按真实能力直接跑”。

核心逻辑在：

- `run_case()`
- `run_core_profile_once()`
- `parse_summary_from_output()`
- `test262_core_profile()`

---

## Runtime 入口与上下文构建

当前运行时有三条主要入口：

- `eval_with_options()`
- `eval_script_with_options()`
- `eval_module_with_options()`

位置：

- `src/engine/runtime.rs:435`
- `src/engine/runtime.rs:472`
- `src/engine/runtime.rs:521`

这三条路径都会：

1. 构造 Boa `Context`
2. 安装宿主全局对象
3. 在需要时安装 `test262` 宿主对象 `$262`
4. 执行脚本 / module
5. drain job queue

这意味着：

- 普通 CLI eval
- 路径脚本执行
- module 执行
- test262 runner 执行

最终都复用同一套运行时与宿主语义。

---

## `$262` 宿主对象实现

`$262` 通过：

- `install_test262_globals()`
- `build_test262_object()`

注入到全局环境。

关键位置：

- `src/engine/runtime.rs:2126`
- `src/engine/runtime.rs:2137`

当前已实现的重要接口：

### `evalScript()`

- 跨 realm 执行脚本
- 通过 `with_realm(...)` 切换执行环境

### `createRealm()`

- 创建新 realm
- 给新 realm 重新安装宿主全局和 test262 全局

位置：
- `src/engine/runtime.rs:2781`

### `detachArrayBuffer()`

- 对接 test262 需要的 ArrayBuffer detach 宿主语义

位置：
- `src/engine/runtime.rs:2796` 附近

### `gc()`

- 当前实现为最小 no-op
- 满足 `host-gc-required` 样本对宿主接口存在性的要求
- 返回 `undefined`

位置：
- `src/engine/runtime.rs:2792`

### `$262.agent`

支持的主接口：

- `start`
- `broadcast`
- `getReport`
- `sleep`
- `monotonicNow`

以及 worker 侧：

- `receiveBroadcast`
- `report`
- `leaving`

这部分是 test262 并发 / agent 相关样本的基础。

### `$262.AbstractModuleSource`

提供 source-phase-imports 相关 built-ins 测试所需的宿主对象。

---

## 兼容层：为什么需要它

Boa 0.21 提供了大量原生语义，但并不自动等于本仓库对 `test262` 新特性目录的可运行性。

所以 runtime 里额外做了一层**兼容 source rewrite + loader 扩展**，用于承接：

- `import-attributes`
- `import-bytes`
- `import-text`
- `import-defer`
- `source-phase-imports`
- `dynamic import` 第二参数

核心位置：

- `preprocess_compat_source()` — `src/engine/runtime.rs:603`
- `build_import_compat_helper()` — `src/engine/runtime.rs:875`
- `CompatModuleLoader` — `src/engine/runtime.rs` 前部

### dynamic import compatibility

- 对 `import(specifier, options)` 做最小重写
- 支持 `with { type: 'json' | 'text' | 'bytes' }`
- 在已缓存模块上复用 namespace object，避免部分路径进入 Boa 原生动态导入炸栈路径

### import-defer compatibility

- 支持 `import.defer(...)`
- 支持最小静态 `import defer * as ns from ...`
- 通过 deferred wrapper module + proxy 模拟最小 exotic behavior

### source-phase-imports compatibility

- 支持 `import.source(...)`
- 支持最小静态 `import source x from ...`
- 对不支持 SourceTextModule 的路径明确给 `SyntaxError`

---

## 已放开的能力块

### 1. `Temporal`

- 通过 `boa_engine` 的 `temporal` feature 启用
- 当前运行时无需额外 provider wiring
- `Context::builder().build()` 即可自动初始化 Temporal builtins

### 2. `Intl / intl402`

- 通过 `boa_engine` 的 `intl_bundled` feature 启用
- 使用 Boa 自带 ICU/provider 数据
- 不需要手动额外注入 buffer provider

### 3. `staging`

- 现已整块放开
- 不再依赖细粒度 allowlist

### 4. `import-attributes`

- 目录级验证通过后，相关 gating 已移除
- 包括：
  - `language/import/import-attributes`
  - `language/expressions/dynamic-import/import-attributes`
  - `language/module-code/import-attributes`

### 5. `import-defer` / `source-phase-imports`

- 相关目录复查通过后，历史 runner feature gating 已删除
- 目前 runner 直接按真实能力执行，而不是靠策略性跳过

---

## 内存与稳定性优化

为了避免本机 OOM 和长跑系统卡死，目前 test262 长跑已经做了几层优化。

### 1. 只保留 `path + metadata`

`discover_cases()` 不再把测试源码字符串常驻在 `Vec<TestCase>` 中。

### 2. 执行时按需读源码

`run_case()` 在真正执行某个 case 之前才读取文件内容。

### 3. 大栈线程执行

`test262_core_profile()` 运行在显式更大的测试线程栈上，避免：

- 深嵌套函数样本
- 深递归样本

直接打爆默认 test 线程栈。

### 4. GitHub Actions 分片长跑

整轮 `core profile` 长跑已经迁移到 GitHub Actions：

- `isolated-smoke`
- `core-profile-shards`（matrix）
- `aggregate`
- `known-slow-tail-case`

这让长跑不再依赖本机资源。

---

## GitHub Actions 设计

workflow 文件：

- `.github/workflows/test262-core-profile.yml`

当前策略：

### 自动触发条件

只在这些文件改动时自动触发：

- `Cargo.toml`
- `Cargo.lock`
- `run_test262.sh`
- `src/**/*.rs`
- `tests/**/*.rs`
- workflow 本身

这样可以避免无关提交消耗整轮 CI。

### 分片矩阵

- 主体按 offset / max_cases 分片
- 对慢尾部区间做了更细粒度拆分
- 对已知特别慢的单 case（`replace-flags.js`）单独隔离为 `known-slow-tail-case`

### Artifact 汇总

每个 shard 上传：

- `summary.json`

`aggregate` job 负责汇总：

- `Total cases`
- `Executed`
- `Passed`
- `Skipped`
- `Total pass rate`
- `Executed pass rate`

### 已知成功 run

- GitHub Actions run `23537781035`：通过

---

## `$262.gc()` 当前语义边界

当前 `$262.gc()` 是：

- 存在
- 可调用
- 返回 `undefined`
- 不保证真实强制回收

这在当前已验证的 `host-gc-required` staging 样本里已经足够。

已验证样本包括：

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

这不代表未来所有依赖真实 GC 时序的样本都一定完全等价；但至少当前这批 test262 样本在这个最小 hook 上已经可执行。

---

## 当前仍需谨慎表述的边界

下面这些点应谨慎表述，避免文档过度承诺：

1. **并不表示所有 JS / TC39 提案都完整实现**
   - 当前结论聚焦于仓库当前接入的 `core profile` 和已验证目录

2. **`import-defer` / `source-phase-imports` 仍是兼容层承接，不是完全原生实现**
   - 虽然 runner 现在已经不再对这些目录做策略性跳过
   - 但实现方式仍以宿主兼容为主

3. **`$262.gc()` 是最小宿主接口，不是强语义 GC 控制器**

4. **GitHub Actions 成功并不等于本机长跑永远安全**
   - 长跑默认应交给 CI
   - 本机更适合跑小样本 / 小目录验证

5. **仍有 3 个 Annex B 边界语义缺口**
   - `IsHTMLDDA` 已完成引擎级 `[[IsHTMLDDA]]` exotic object 支持并放开相关测试
   - 余下 3 个 Annex B 边界 case 仍依赖更底层 parser/runtime 语义，当前保持 skip

---

## 推荐开发与验证流程

### 本地

用于快速验证：

- isolated smoke tests
- 单目录验证
- 单文件验证

例如：

```bash
cargo test --test isolated_test
TEST262_FILTER='test/staging/sm/extensions/regress-650753.js' cargo test --test test262_runner -- --ignored --exact test262_core_profile
```

### CI

用于长跑：

- GitHub Actions `test262 core profile`
- matrix shards
- aggregate 汇总

避免继续在本机直接跑整轮 `core profile`。

---

## 总结

当前仓库的 `test262` 实现已经从“少量 smoke + 大量策略性跳过”推进到：

- `core profile` 真实执行
- `Temporal` 放开
- `Intl / intl402` 放开
- `staging` 放开
- `$262` 关键宿主对象与接口到位
- 长跑验证迁移到 GitHub Actions
- `Temporal` 测试已启用
- `cross-realm` 测试已启用
- RISC-V 和 LoongArch 跨架构测试支持
- `Array.fromAsync` 兼容层已修正 `@@asyncIterator` / `@@iterator` 可观察探测顺序
- `IsHTMLDDA` 已切到引擎级 exotic object（含 `typeof` / `ToBoolean` / `== null` / 可调用返回 `null`）

最近一次本地分片回归（`TEST262_MAX_CASES=5000`）结果：

- Passed: `4997`
- Skipped: `3`（`Annex B edge semantics: 3`）
- Failed: `0`

后续若继续推进，最有价值的方向是：

- 更深层 `import-defer` / `source-phase-imports` 语义收敛
- 继续扩宿主接口（如更多 `$262` / GC 边角能力）
- 逐步把当前自研 parser / interpreter 与这套运行时能力重新对齐

---

## 跨架构测试支持

### RISC-V 64-bit

通过 QEMU 用户模式模拟支持 RISC-V 64-bit 架构测试：

```bash
# 安装交叉编译工具链
sudo apt-get install gcc-riscv64-linux-gnu g++-riscv64-linux-gnu qemu-user-static

# 添加 Rust 目标
rustup target add riscv64gc-unknown-linux-gnu

# 构建
CC_riscv64gc_unknown_linux_gnu=riscv64-linux-gnu-gcc cargo build --release --target riscv64gc-unknown-linux-gnu

# 运行测试
qemu-riscv64 -L /usr/riscv64-linux-gnu ./target/riscv64gc-unknown-linux-gnu/release/ai-agent -p "1 + 2"
```

### LoongArch 64-bit

LoongArch 支持依赖于交叉编译工具链的可用性，目前在大多数发行版中尚未广泛可用。

GitHub Actions workflow 会在工具链可用时自动尝试构建和测试。

### CI Workflow

跨架构测试通过 `.github/workflows/cross-arch-tests.yml` 配置，包含：

- x86_64 原生测试（基线）
- RISC-V 64 交叉编译和 QEMU 测试
- LoongArch 64 交叉编译和 QEMU 测试（条件执行）
