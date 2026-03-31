// Modern JavaScript Syntax Examples for Agent-JS-Engine

// BigInt literals
const bigNum = 123456789012345678901234567890n;
const hexBig = 0xFFFFFFFFFFFFFFFFn;
const binBig = 0b11111111111111111111111111111111n;

// Numeric separators
const million = 1_000_000;
const billion = 1_000_000_000;
const hex = 0xFF_FF_FF;
const bin = 0b1111_0000_1111_0000;

// Arrow functions
const add = (a, b) => a + b;
const square = x => x * x;
const greet = name => {
    return `Hello, ${name}!`;
};

// Async functions
async function fetchData() {
    return "data";
}

// Generator functions
function* range(start, end) {
    for (let i = start; i < end; i++) {
        yield i;
    }
}

// Async generators
async function* asyncRange(start, end) {
    for (let i = start; i < end; i++) {
        yield i;
    }
}

// Destructuring
const [first, second, ...rest] = [1, 2, 3, 4, 5];
const {x, y, z = 10} = {x: 1, y: 2};
const {a: renamed} = {a: 42};

// Template literals
const name = "World";
const message = `Hello, ${name}!`;
const multiline = `
    This is a
    multi-line string
`;

// Classes
class Animal {
    #privateField = "secret";
    
    constructor(name) {
        this.name = name;
    }
    
    speak() {
        return `${this.name} makes a sound`;
    }
    
    static create(name) {
        return new Animal(name);
    }
    
    get displayName() {
        return this.name.toUpperCase();
    }
    
    set displayName(value) {
        this.name = value.toLowerCase();
    }
}

class Dog extends Animal {
    constructor(name, breed) {
        super(name);
        this.breed = breed;
    }
    
    speak() {
        return `${this.name} barks`;
    }
}

// Optional chaining
const obj = {nested: {value: 42}};
const value = obj?.nested?.value;
const missing = obj?.missing?.value;

// Nullish coalescing
const defaultValue = null ?? "default";
const zero = 0 ?? "default"; // returns 0

// Spread operator
const arr1 = [1, 2, 3];
const arr2 = [...arr1, 4, 5];
const obj1 = {a: 1, b: 2};
const obj2 = {...obj1, c: 3};

// For-of loops
for (const item of [1, 2, 3]) {
    // process item
}

// Object shorthand
const px = 10, py = 20;
const point = {x: px, y: py};

// Computed properties
const key = "dynamicKey";
const computed = {
    [key]: "value",
    [`${key}2`]: "value2"
};

// Default parameters
function greetWithDefault(name = "Guest") {
    return `Hello, ${name}`;
}

// Rest parameters
function sum(...numbers) {
    return numbers.reduce((a, b) => a + b, 0);
}

print("All modern syntax features demonstrated!");
