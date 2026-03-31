# JavaScript Syntax Support

This document describes the JavaScript syntax features supported by the Agent-JS-Engine handwritten lexer/parser/interpreter.

## ES2015+ Features

### ✅ Fully Supported

#### Variables & Declarations
- `let`, `const`, `var` declarations
- Block scoping
- Temporal dead zone (TDZ) for `let`/`const`

#### Functions
- Arrow functions: `() => expr`, `(x) => { ... }`
- Async functions: `async function f() { ... }`
- Generator functions: `function* gen() { yield x; }`
- **Async generators**: `async function* gen() { yield x; }`
- Default parameters: `function f(x = 5) { ... }`
- Rest parameters: `function f(...args) { ... }`
- Method shorthand: `{ method() { ... } }`

#### Classes
- Class declarations and expressions
- Constructor methods
- Instance and static methods
- Getters and setters
- Static blocks: `static { ... }`
- Private identifiers: `#privateField`
- Class field initializers
- `extends` for inheritance
- `super` keyword

#### Destructuring
- Array destructuring: `const [a, b] = arr`
- Object destructuring: `const {x, y} = obj`
- Nested destructuring
- Default values: `const {x = 5} = obj`
- Rest elements: `const [first, ...rest] = arr`
- Computed property names: `const {[key]: value} = obj`

#### Template Literals
- Basic templates: `` `hello ${name}` ``
- Multi-line strings
- Tagged templates: `` tag`hello ${name}` ``
- Nested expressions

#### Operators
- Spread operator: `[...arr]`, `{...obj}`
- Nullish coalescing: `x ?? y`
- Optional chaining: `obj?.prop`, `obj?.[expr]`, `func?.()`
- Exponentiation: `x ** y`
- Logical assignment: `x &&= y`, `x ||= y`, `x ??= y`

#### Numeric Literals
- **BigInt literals**: `123n`, `0xFFn`, `0o77n`, `0b1010n`
- **Numeric separators**: `1_000_000`, `0xFF_FF`
- Binary: `0b1010`
- Octal: `0o777`
- Hexadecimal: `0xFF`

#### BigInt Runtime (handwritten interpreter)
- BigInt literal evaluation returns a dedicated `BigInt` runtime value
- Supported operations: unary `-`, `+`, `-`, `*`, `<`, `<=`, `>`, `>=`, `==`, `===`
- Mixing `BigInt` with `Number` in arithmetic throws `TypeError`
- Not yet supported: `/`, `%`, `**` for BigInt

#### Control Flow
- `for...of` loops
- `for...in` loops
- `for await...of` loops
- `try...catch...finally` with optional catch binding

#### Modules
- `import` declarations
- `export` declarations (named, default, namespace)
- Dynamic `import()`
- Import assertions/attributes (runtime support via boa_engine)

#### Other
- Computed property names: `{ [expr]: value }`
- Property shorthand: `{ x, y }`
- Regular expressions: `/pattern/flags`
- `yield` and `yield*` expressions
- `await` expressions

### ⚠️ Partially Supported

#### String Escapes
- Basic escapes work: `\n`, `\t`, `\\`, `\'`, `\"`
- Unicode/hex escapes are lexed but not processed (handled by runtime)

#### Regex
- Regex literals are tokenized
- Context-aware detection is basic (may have edge cases)

### ❌ Not Yet Supported

#### Stage 3+ Features
- Decorators
- Pattern matching
- Pipeline operator
- Record & Tuple
- Explicit resource management (`using` declarations)

#### Advanced Syntax
- JSX/TSX
- TypeScript type annotations
- Import assertions in handwritten parser (runtime supports via boa_engine)

## Runtime vs Parser Support

The project uses two execution paths:

1. **Handwritten Parser + Interpreter** (`src/lexer`, `src/parser`, `src/engine/interpreter.rs`)
   - Experimental implementation
   - Supports features listed above
   - Used for learning and future independence

2. **Boa Engine Runtime** (`src/engine/runtime.rs`)
   - Production runtime
   - Full ES2024+ compliance
   - Supports Temporal, Intl, advanced modules, etc.
   - Used for actual script execution

## Testing

Run parser tests:
```bash
cargo test --test parser_test
cargo test --test bigint_test
cargo test --test async_generator_test
```

Run interpreter tests:
```bash
cargo test --test interpreter_test
cargo test --test bigint_interpreter_test
```

Run test262 conformance:
```bash
./run_test262.sh
```

## Examples

See the `examples/` directory for sample JavaScript files demonstrating supported features.
