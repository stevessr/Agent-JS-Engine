// Async Generator Examples

// Basic async generator
async function* asyncCounter(max) {
    for (let i = 0; i < max; i++) {
        yield i;
    }
}

// Async generator with await
async function* fetchItems(urls) {
    for (const url of urls) {
        // In real code, this would be an actual fetch
        yield `Data from ${url}`;
    }
}

// Async generator method in class
class DataStream {
    async *generate(count) {
        for (let i = 0; i < count; i++) {
            yield i * 2;
        }
    }
}

// Async generator expression
const asyncGen = async function* () {
    yield 1;
    yield 2;
    yield 3;
};

print("Async generator examples completed!");
