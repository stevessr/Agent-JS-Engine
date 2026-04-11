fn install_iterator_helpers(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        'use strict';
        (() => {
          // Check if Iterator already exists and has helpers
          if (typeof globalThis.Iterator === 'function' && 
              typeof Iterator.prototype.map === 'function') {
            return;
          }

          // Get the %IteratorPrototype%
          const IteratorPrototype = Object.getPrototypeOf(
            Object.getPrototypeOf([][Symbol.iterator]())
          );

          // Helper to define non-enumerable method
          function defineMethod(obj, name, fn, length) {
            Object.defineProperty(obj, name, {
              value: fn,
              writable: true,
              enumerable: false,
              configurable: true
            });
            // For symbols, the name should be the description wrapped in brackets
            let nameValue = name;
            if (typeof name === 'symbol') {
              const desc = name.description;
              nameValue = desc ? '[' + desc + ']' : '';
            }
            Object.defineProperty(fn, 'name', {
              value: nameValue,
              writable: false,
              enumerable: false,
              configurable: true
            });
            Object.defineProperty(fn, 'length', {
              value: length !== undefined ? length : fn.length,
              writable: false,
              enumerable: false,
              configurable: true
            });
          }

          // Helper to make a function non-constructible
          function makeNonConstructible(impl, name, length) {
            // Arrow functions have no [[Construct]], wrap with Proxy for 'this' support
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const proxy = new Proxy(arrowWrapper, {
              apply(target, thisArg, args) {
                return impl.apply(thisArg, args);
              }
            });
            // For symbols, the name should be the description wrapped in brackets
            let nameValue = name;
            if (typeof name === 'symbol') {
              const desc = name.description;
              nameValue = desc ? '[' + desc + ']' : '';
            }
            Object.defineProperty(proxy, 'name', {
              value: nameValue,
              writable: false,
              enumerable: false,
              configurable: true
            });
            Object.defineProperty(proxy, 'length', {
              value: length,
              writable: false,
              enumerable: false,
              configurable: true
            });
            return proxy;
          }

          // Helper to define non-enumerable, non-constructible method
          function defineNonConstructibleMethod(obj, name, fn, length) {
            const wrapped = makeNonConstructible(fn, name, length);
            Object.defineProperty(obj, name, {
              value: wrapped,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }

          // Helper to get iterator record - per spec, doesn't validate callability
          function GetIteratorDirect(obj) {
            if (typeof obj !== 'object' || obj === null) {
              throw new TypeError('Iterator must be an object');
            }
            const nextMethod = obj.next;
            // Note: We do NOT check if nextMethod is callable here
            // That check happens when next() is actually called
            return { iterator: obj, nextMethod, done: false };
          }

          // Helper to call next method with validation
          function IteratorNext(iteratorRecord) {
            const nextMethod = iteratorRecord.nextMethod;
            if (typeof nextMethod !== 'function') {
              throw new TypeError('Iterator next method must be callable');
            }
            return nextMethod.call(iteratorRecord.iterator);
          }

          function getIntrinsicIteratorPrototype(newTarget) {
            if (newTarget === undefined || newTarget === Iterator) {
              return Iterator.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherIterator = otherGlobal && otherGlobal.Iterator;
              const otherProto = otherIterator && otherIterator.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return Iterator.prototype;
          }

          function iteratorFromConstructInput(obj) {
            const iteratorMethod = obj[Symbol.iterator];
            if (iteratorMethod !== undefined && iteratorMethod !== null) {
              if (typeof iteratorMethod !== 'function') {
                throw new TypeError('Symbol.iterator is not callable');
              }
              const iterator = iteratorMethod.call(obj);
              return GetIteratorDirect(iterator);
            }
            if (typeof obj !== 'object' || obj === null) {
              throw new TypeError('obj is not iterable');
            }
            return GetIteratorDirect(obj);
          }

          // Iterator constructor - abstract class
          function Iterator(_iterable) {
            if (new.target === undefined) {
              throw new TypeError('Constructor Iterator requires "new"');
            }
            if (new.target === Iterator) {
              throw new TypeError('Abstract class Iterator not directly constructable');
            }
            const proto = getIntrinsicIteratorPrototype(new.target);
            return Object.create(proto);
          }

          // SetterThatIgnoresPrototypeProperties per spec
          // 1. If this is not Object, throw TypeError
          // 2. If this is home (IteratorPrototype), throw TypeError
          // 3. If desc is undefined, CreateDataPropertyOrThrow
          // 4. Otherwise, Set(this, p, v, true)
          function SetterThatIgnoresPrototypeProperties(home, p, v) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Cannot set property on non-object');
            }
            if (this === home) {
              throw new TypeError('Cannot set property on prototype');
            }
            const desc = Object.getOwnPropertyDescriptor(this, p);
            if (desc === undefined) {
              Object.defineProperty(this, p, {
                value: v,
                writable: true,
                enumerable: true,
                configurable: true
              });
            } else {
              this[p] = v;
            }
          }

          // Iterator.prototype.constructor should be an accessor property per spec
          Object.defineProperty(IteratorPrototype, 'constructor', {
            get() { return Iterator; },
            set(v) { SetterThatIgnoresPrototypeProperties.call(this, IteratorPrototype, 'constructor', v); },
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype[@@toStringTag] should be an accessor property per spec
          Object.defineProperty(IteratorPrototype, Symbol.toStringTag, {
            get() { return 'Iterator'; },
            set(v) { SetterThatIgnoresPrototypeProperties.call(this, IteratorPrototype, Symbol.toStringTag, v); },
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype setup
          const IteratorHelperPrototype = Object.create(IteratorPrototype);

          Object.defineProperty(IteratorHelperPrototype, Symbol.toStringTag, {
            value: 'Iterator Helper',
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Create a helper iterator wrapper
          // underlyingRecord is the result of GetIteratorDirect (has .iterator, .nextMethod)
          function createIteratorHelper(underlyingRecord, nextImpl, returnImpl) {
            const helper = Object.create(IteratorHelperPrototype);
            const state = { 
              underlyingRecord: underlyingRecord,
              done: false,
              executing: false,
              nextImpl,
              returnImpl
            };
            
            helper.next = function() {
              if (state.executing) {
                throw new TypeError('Generator is already executing');
              }
              if (state.done) {
                return { value: undefined, done: true };
              }
              state.executing = true;
              try {
                return state.nextImpl(state);
              } catch (e) {
                state.done = true;
                // NOTE: We do NOT close the iterator here
                // IfAbruptCloseIterator is handled by each method's nextImpl
                // when their callback/mapper/predicate throws
                throw e;
              } finally {
                state.executing = false;
              }
            };
            
            helper.return = function(value) {
              if (state.executing) {
                throw new TypeError('Generator is already executing');
              }
              if (state.done) {
                // Already closed, don't forward return again
                return { value: value, done: true };
              }
              state.done = true;
              if (state.returnImpl) {
                return state.returnImpl(state, value);
              }
              const underlying = state.underlyingRecord.iterator;
              if (underlying && typeof underlying.return === 'function') {
                return underlying.return(value);
              }
              return { value: value, done: true };
            };
            
            return helper;
          }

          // Iterator.prototype.map
          defineNonConstructibleMethod(IteratorPrototype, 'map', function map(mapper) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.map called on non-object');
            }
            if (typeof mapper !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('mapper must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            return createIteratorHelper(iterated, (state) => {
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Wrap mapper call - close iterator if it throws
              let mapped;
              try {
                mapped = mapper(next.value, counter++);
              } catch (e) {
                // IfAbruptCloseIterator
                if (typeof state.underlyingRecord.iterator.return === 'function') {
                  try { state.underlyingRecord.iterator.return(); } catch (_) {}
                }
                throw e;
              }
              return { value: mapped, done: false };
            });
          }, 1);

          // Iterator.prototype.filter
          defineNonConstructibleMethod(IteratorPrototype, 'filter', function filter(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.filter called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            return createIteratorHelper(iterated, (state) => {
              while (true) {
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                // Wrap predicate call - close iterator if it throws
                let result;
                try {
                  result = predicate(next.value, counter++);
                } catch (e) {
                  // IfAbruptCloseIterator
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw e;
                }
                if (result) {
                  return { value: next.value, done: false };
                }
              }
            });
          }, 1);

          // Helper to close iterator if it has a return method
          function IteratorClose(iteratorRecord, error) {
            const iterator = iteratorRecord.iterator;
            if (typeof iterator.return === 'function') {
              try {
                iterator.return();
              } catch (e) {
                if (error) throw error;
                throw e;
              }
            }
            if (error) throw error;
          }

          // Iterator.prototype.take
          defineNonConstructibleMethod(IteratorPrototype, 'take', function take(limit) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.take called on non-object');
            }
            // Steps 2-5: Validate arguments BEFORE GetIteratorDirect
            // But if validation fails, close the iterator (this) directly
            let numLimit;
            try {
              numLimit = Number(limit);
            } catch (e) {
              // ToNumber threw - close iterator before re-throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw e;
            }
            if (Number.isNaN(numLimit)) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be a number');
            }
            const intLimit = Math.trunc(numLimit);
            if (intLimit < 0) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be non-negative');
            }
            // Step 6: NOW get the iterator
            const iterated = GetIteratorDirect(this);
            let remaining = intLimit;
            
            return createIteratorHelper(iterated, (state) => {
              if (remaining <= 0) {
                state.done = true;
                // Close underlying iterator
                if (typeof state.underlyingRecord.iterator.return === 'function') {
                  state.underlyingRecord.iterator.return();
                }
                return { value: undefined, done: true };
              }
              remaining--;
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Per spec: Yield(? IteratorValue(next)) - must read .value
              const value = next.value;
              return { value: value, done: false };
            });
          }, 1);

          // Iterator.prototype.drop
          defineNonConstructibleMethod(IteratorPrototype, 'drop', function drop(limit) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.drop called on non-object');
            }
            // Steps 2-5: Validate arguments BEFORE GetIteratorDirect
            // But if validation fails, close the iterator (this) directly
            let numLimit;
            try {
              numLimit = Number(limit);
            } catch (e) {
              // ToNumber threw - close iterator before re-throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw e;
            }
            if (Number.isNaN(numLimit)) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be a number');
            }
            const intLimit = Math.trunc(numLimit);
            if (intLimit < 0) {
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new RangeError('limit must be non-negative');
            }
            // Step 6: NOW get the iterator
            const iterated = GetIteratorDirect(this);
            let remaining = intLimit;
            
            return createIteratorHelper(iterated, (state) => {
              // Skip the first 'limit' values
              while (remaining > 0) {
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                // Per spec: during drop, must read .value for side effects
                const _ = next.value;
                remaining--;
              }
              const next = IteratorNext(state.underlyingRecord);
              if (next.done) {
                state.done = true;
                return { value: undefined, done: true };
              }
              // Per spec: Yield(? IteratorValue(next)) - must read .value
              const value = next.value;
              return { value: value, done: false };
            });
          }, 1);

          // Iterator.prototype.flatMap
          defineNonConstructibleMethod(IteratorPrototype, 'flatMap', function flatMap(mapper) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.flatMap called on non-object');
            }
            if (typeof mapper !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('mapper must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            // Store inner iterator in an object so returnImpl can access it
            const innerState = { iterator: null };
            
            const helper = createIteratorHelper(iterated, (state) => {
              while (true) {
                // If we have an inner iterator, consume it first
                if (innerState.iterator !== null) {
                  const innerNext = innerState.iterator.next();
                  if (!innerNext.done) {
                    return { value: innerNext.value, done: false };
                  }
                  innerState.iterator = null;
                }
                
                // Get next from outer iterator
                const next = IteratorNext(state.underlyingRecord);
                if (next.done) {
                  state.done = true;
                  return { value: undefined, done: true };
                }
                
                // Map and get inner iterator - GetIteratorFlattenable semantics
                // Wrap mapper call - close iterator if it throws
                let mapped;
                try {
                  mapped = mapper(next.value, counter++);
                } catch (e) {
                  // IfAbruptCloseIterator
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw e;
                }
                if (typeof mapped !== 'object' || mapped === null) {
                  // IfAbruptCloseIterator for validation error
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw new TypeError('flatMap mapper must return an iterable or iterator');
                }
                
                // Check Symbol.iterator property
                const iteratorMethod = mapped[Symbol.iterator];
                if (iteratorMethod !== undefined && iteratorMethod !== null) {
                  // Has non-null/undefined @@iterator - must be callable
                  if (typeof iteratorMethod !== 'function') {
                    if (typeof state.underlyingRecord.iterator.return === 'function') {
                      try { state.underlyingRecord.iterator.return(); } catch (_) {}
                    }
                    throw new TypeError('Symbol.iterator is not a function');
                  }
                  innerState.iterator = iteratorMethod.call(mapped);
                } else if (typeof mapped.next === 'function') {
                  // Fallback to using object directly as iterator
                  innerState.iterator = mapped;
                } else {
                  if (typeof state.underlyingRecord.iterator.return === 'function') {
                    try { state.underlyingRecord.iterator.return(); } catch (_) {}
                  }
                  throw new TypeError('flatMap mapper must return an iterable or iterator');
                }
              }
            }, (state, value) => {
              // Custom return: close inner iterator first, then outer
              if (innerState.iterator !== null && typeof innerState.iterator.return === 'function') {
                try {
                  innerState.iterator.return();
                } catch (_) {}
              }
              innerState.iterator = null;
              const underlying = state.underlyingRecord.iterator;
              if (underlying && typeof underlying.return === 'function') {
                return underlying.return(value);
              }
              return { value: value, done: true };
            });
            return helper;
          }, 1);

          // Iterator.prototype.forEach
          defineNonConstructibleMethod(IteratorPrototype, 'forEach', function forEach(fn) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.forEach called on non-object');
            }
            if (typeof fn !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('callback must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return undefined;
              }
              try {
                fn(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
            }
          }, 1);

          // Iterator.prototype.some
          defineNonConstructibleMethod(IteratorPrototype, 'some', function some(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.some called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return false;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return true;
              }
            }
          }, 1);

          // Iterator.prototype.every
          defineNonConstructibleMethod(IteratorPrototype, 'every', function every(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.every called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return true;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (!result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return false;
              }
            }
          }, 1);

          // Iterator.prototype.find
          defineNonConstructibleMethod(IteratorPrototype, 'find', function find(predicate) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.find called on non-object');
            }
            if (typeof predicate !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('predicate must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return undefined;
              }
              let result;
              try {
                result = predicate(next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
              if (result) {
                // Close iterator
                if (typeof iterated.iterator.return === 'function') {
                  iterated.iterator.return();
                }
                return next.value;
              }
            }
          }, 1);

          // Iterator.prototype.reduce
          defineNonConstructibleMethod(IteratorPrototype, 'reduce', function reduce(reducer, ...args) {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.reduce called on non-object');
            }
            if (typeof reducer !== 'function') {
              // Close iterator before throwing
              if (typeof this.return === 'function') {
                try { this.return(); } catch (_) {}
              }
              throw new TypeError('reducer must be a function');
            }
            const iterated = GetIteratorDirect(this);
            let counter = 0;
            let accumulator;
            
            if (args.length === 0) {
              // No initial value - use first element
              const first = IteratorNext(iterated);
              if (first.done) {
                throw new TypeError('Reduce of empty iterator with no initial value');
              }
              accumulator = first.value;
              counter = 1;
            } else {
              accumulator = args[0];
            }
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return accumulator;
              }
              try {
                accumulator = reducer(accumulator, next.value, counter++);
              } catch (e) {
                IteratorClose(iterated, e);
              }
            }
          }, 1);

          // Iterator.prototype.toArray
          defineNonConstructibleMethod(IteratorPrototype, 'toArray', function toArray() {
            if (typeof this !== 'object' || this === null) {
              throw new TypeError('Iterator.prototype.toArray called on non-object');
            }
            const iterated = GetIteratorDirect(this);
            const result = [];
            
            while (true) {
              const next = IteratorNext(iterated);
              if (next.done) {
                return result;
              }
              result.push(next.value);
            }
          }, 0);

          // Iterator.prototype[Symbol.iterator] - must preserve primitive this values
          // Use a special implementation that doesn't box primitives
          const iteratorSymbolImpl = {
            [Symbol.iterator]() { return this; }
          };
          Object.defineProperty(IteratorPrototype, Symbol.iterator, {
            value: iteratorSymbolImpl[Symbol.iterator],
            writable: true,
            enumerable: false,
            configurable: true
          });
          // The function's name should be [Symbol.iterator]
          Object.defineProperty(IteratorPrototype[Symbol.iterator], 'name', {
            value: '[Symbol.iterator]',
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Iterator.prototype[Symbol.dispose] (for using)
          defineNonConstructibleMethod(IteratorPrototype, Symbol.dispose, function() {
            if (typeof this.return === 'function') {
              this.return();
            }
          }, 0);

          const WrapForValidIteratorPrototype = Object.create(IteratorPrototype);

          function createIteratorWrapper(iteratorRecord) {
            let returnMethodInitialized = false;
            let cachedReturn;
            const helper = createIteratorHelper(
              iteratorRecord,
              (state) => IteratorNext(state.underlyingRecord),
              (state, value) => {
                if (!returnMethodInitialized) {
                  const returnMethod = state.underlyingRecord.iterator.return;
                  cachedReturn = typeof returnMethod === 'function' ? returnMethod : undefined;
                  returnMethodInitialized = true;
                }
                if (cachedReturn === undefined) {
                  return { value, done: true };
                }
                const result = cachedReturn.call(state.underlyingRecord.iterator, value);
                if (typeof result !== 'object' || result === null) {
                  throw new TypeError('Iterator result must be an object');
                }
                return result;
              }
            );
            const sourceProto = Object.getPrototypeOf(iteratorRecord.iterator);
            const keepSourcePrototype =
              sourceProto !== null &&
              sourceProto !== Object.prototype &&
              sourceProto !== IteratorPrototype &&
              typeof iteratorRecord.iterator.throw === 'function';
            if (keepSourcePrototype) {
              Object.setPrototypeOf(helper, sourceProto);
            } else {
              Object.setPrototypeOf(helper, WrapForValidIteratorPrototype);
            }
            return helper;
          }
          // Iterator.from static method (non-constructible)
          defineNonConstructibleMethod(Iterator, 'from', function from(obj) {
            if (typeof obj !== 'object' && typeof obj !== 'string') {
              throw new TypeError('Iterator.from requires an object or string');
            }
            if (obj === null) {
              throw new TypeError('Iterator.from requires an object or string');
            }

            let iteratorRecord;
            const iteratorMethod = obj[Symbol.iterator];
            if (iteratorMethod !== undefined && iteratorMethod !== null) {
              if (typeof iteratorMethod !== 'function') {
                throw new TypeError('Symbol.iterator is not callable');
              }
              const iterator = iteratorMethod.call(obj);
              iteratorRecord = GetIteratorDirect(iterator);
            } else {
              if (typeof obj !== 'object' || obj === null) {
                throw new TypeError('obj is not iterable');
              }
              iteratorRecord = GetIteratorDirect(obj);
            }

            const helper = createIteratorWrapper(iteratorRecord);
            return helper;
          }, 1);

          // Iterator.concat static method (iterator-sequencing proposal)
          // https://tc39.es/proposal-iterator-sequencing/
          defineNonConstructibleMethod(Iterator, 'concat', function concat(...items) {
            const iterables = [];
            // 2. For each element item of items, do
            for (let i = 0; i < items.length; i++) {
              const item = items[i];
              // a. If item is not an Object, throw a TypeError exception.
              if (typeof item !== 'object' || item === null) {
                throw new TypeError('Iterator.concat: argument is not an object');
              }
              // b. Let method be ? GetMethod(item, @@iterator).
              const method = item[Symbol.iterator];
              // c. If method is undefined, throw a TypeError exception.
              if (method === undefined) {
                throw new TypeError('Iterator.concat: argument is not iterable');
              }
              if (typeof method !== 'function') {
                throw new TypeError('Iterator.concat: @@iterator is not callable');
              }
              // d. Append the Record { [[OpenMethod]]: method, [[Iterable]]: item } to iterables.
              iterables.push({ openMethod: method, iterable: item });
            }
            
            // 3. Let closure be a new Abstract Closure with no parameters
            // 4. Let gen be CreateIteratorFromClosure(closure, "Iterator Helper", ...)
            // 5. Return gen.
            
            // Create generator-like state
            let currentIndex = 0;
            let currentIterator = null;
            let started = false;
            let executing = false;
            let closed = false;
            
            // Helper to close iterator
            function closeIterator(iterator) {
              if (iterator !== null) {
                const returnMethod = iterator.return;
                if (typeof returnMethod === 'function') {
                  try {
                    returnMethod.call(iterator);
                  } catch (e) {
                    // Ignore errors on cleanup
                  }
                }
              }
            }
            
            const helper = Object.create(IteratorHelperPrototype);
            
            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (closed) {
                return { value: undefined, done: true };
              }
              executing = true;
              try {
                while (true) {
                  // If we have a current iterator, try to get next from it
                  if (currentIterator !== null) {
                    const result = currentIterator.next();
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                    const done = !!result.done;
                    if (!done) {
                      // Access value after done
                      const value = result.value;
                      return { value, done: false };
                    }
                    // Current iterator is done, move to next
                    currentIterator = null;
                  }
                  
                  // Move to next iterable
                  if (currentIndex >= iterables.length) {
                    return { value: undefined, done: true };
                  }
                  
                  const record = iterables[currentIndex++];
                  const iter = record.openMethod.call(record.iterable);
                  if (typeof iter !== 'object' || iter === null) {
                    throw new TypeError('Iterator.concat: iterator method did not return an object');
                  }
                  currentIterator = iter;
                  started = true;
                }
              } finally {
                executing = false;
              }
            }
            
            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (closed) {
                return { value: undefined, done: true };
              }
              executing = true;
              try {
                // Only forward return if we've started and have a current iterator
                if (started && currentIterator !== null) {
                  const returnMethod = currentIterator.return;
                  if (typeof returnMethod === 'function') {
                    const result = returnMethod.call(currentIterator);
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                  }
                }
                currentIterator = null;
                closed = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }
            
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);
            
            return helper;
          }, 0);

          function closeInputIterator(inputIterRecord, error) {
            const iterator = inputIterRecord && inputIterRecord.iterator ? inputIterRecord.iterator : inputIterRecord;
            let returnMethod;
            try {
              returnMethod = iterator?.return;
            } catch (e) {
              if (error !== undefined) {
                throw error;
              }
              throw e;
            }
            if (typeof returnMethod === 'function') {
              if (error !== undefined) {
                try {
                  returnMethod.call(iterator);
                } catch (_) {}
              } else {
                const result = returnMethod.call(iterator);
                if (typeof result !== 'object' || result === null) {
                  throw new TypeError('Iterator result must be an object');
                }
              }
            }
            if (error !== undefined) {
              throw error;
            }
          }

          function closeIteratorList(iteratorList, startIndex, skipIndex, error, shouldThrow = true) {
            let closeError = null;
            for (let i = iteratorList.length - 1; i >= startIndex; i--) {
              if (i === skipIndex || iteratorList[i] === null) {
                continue;
              }
              const record = iteratorList[i];
              const iter = record && record.iterator ? record.iterator : record;
              let returnMethod;
              try {
                returnMethod = iter?.return;
              } catch (e) {
                if (error === undefined && closeError === null) {
                  closeError = e;
                }
                iteratorList[i] = null;
                continue;
              }
              if (typeof returnMethod === 'function') {
                if (error !== undefined) {
                  try {
                    returnMethod.call(iter);
                  } catch (_) {}
                } else {
                  try {
                    const result = returnMethod.call(iter);
                    if (typeof result !== 'object' || result === null) {
                      throw new TypeError('Iterator result must be an object');
                    }
                  } catch (e) {
                    if (closeError === null) {
                      closeError = e;
                    }
                  }
                }
              }
              iteratorList[i] = null;
            }
            if (error !== undefined) {
              if (shouldThrow) {
                throw error;
              }
              return error;
            }
            if (closeError !== null) {
              throw closeError;
            }
          }

          // Iterator.zip static method (joint-iteration proposal)
          // https://tc39.es/proposal-joint-iteration/
          defineNonConstructibleMethod(Iterator, 'zip', function zip(iterables, options) {
            if (typeof iterables !== 'object' || iterables === null) {
              throw new TypeError('Iterator.zip: iterables is not an object');
            }

            let mode = 'shortest';
            let paddingOption = undefined;

            if (options !== undefined) {
              if (typeof options !== 'object' || options === null) {
                throw new TypeError('Iterator.zip: options is not an object');
              }
              const modeOption = options.mode;
              if (modeOption === undefined) {
                mode = 'shortest';
              } else if (modeOption === 'longest' || modeOption === 'strict' || modeOption === 'shortest') {
                mode = modeOption;
              } else {
                throw new TypeError('Iterator.zip: mode must be "shortest", "longest", or "strict"');
              }
              if (mode === 'longest') {
                paddingOption = options.padding;
              }
            }

            const iters = [];
            const openIters = [];

            function closeZipIterators(skipIndex, error) {
              closeIteratorList(openIters, 0, skipIndex, error);
            }

            function getIteratorFlattenable(value) {
              if (typeof value !== 'object' || value === null) {
                throw new TypeError('Iterator.zip: iterable element is not an object');
              }
              const iterMethod = value[Symbol.iterator];
              let iter;
              if (iterMethod === undefined) {
                iter = value;
              } else {
                if (typeof iterMethod !== 'function') {
                  throw new TypeError('Iterator.zip: @@iterator is not callable');
                }
                iter = iterMethod.call(value);
                if (typeof iter !== 'object' || iter === null) {
                  throw new TypeError('Iterator.zip: iterator is not an object');
                }
              }
              return GetIteratorDirect(iter);
            }

            const inputIterMethod = iterables[Symbol.iterator];
            if (typeof inputIterMethod !== 'function') {
              throw new TypeError('Iterator.zip: iterables is not iterable');
            }
            const inputIter = inputIterMethod.call(iterables);
            if (typeof inputIter !== 'object' || inputIter === null) {
              throw new TypeError('Iterator.zip: iterables iterator is not an object');
            }
            const inputIterRecord = GetIteratorDirect(inputIter);

            try {
              while (true) {
                let next;
                try {
                  next = IteratorNext(inputIterRecord);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  throw e;
                }
                if (typeof next !== 'object' || next === null) {
                  const error = new TypeError('Iterator.zip: iterator result is not an object');
                  closeIteratorList(openIters, 0, -1, error, false);
                  throw error;
                }
                const done = !!next.done;
                if (done) {
                  break;
                }
                const value = next.value;
                try {
                  const iterRecord = getIteratorFlattenable(value);
                  iters.push({ iterator: iterRecord.iterator, nextMethod: iterRecord.nextMethod, done: false });
                  openIters.push(iterRecord.iterator);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  closeInputIterator(inputIterRecord, e);
                }
              }
            } catch (e) {
              throw e;
            }

            const padding = new Array(iters.length).fill(undefined);
            if (mode === 'longest' && paddingOption !== undefined) {
              if (typeof paddingOption !== 'object' || paddingOption === null) {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding is not an object'));
              }
              const paddingIterMethod = paddingOption[Symbol.iterator];
              if (typeof paddingIterMethod !== 'function') {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding is not iterable'));
              }
              let paddingIter;
              try {
                paddingIter = paddingIterMethod.call(paddingOption);
              } catch (e) {
                closeZipIterators(-1, e);
              }
              if (typeof paddingIter !== 'object' || paddingIter === null) {
                closeZipIterators(-1, new TypeError('Iterator.zip: padding iterator is not an object'));
              }
              const paddingIterRecord = GetIteratorDirect(paddingIter);
              let usingIterator = true;
              let completionError;
              for (let i = 0; i < iters.length; i++) {
                if (!usingIterator) {
                  padding[i] = undefined;
                  continue;
                }
                try {
                  const next = IteratorNext(paddingIterRecord);
                  if (typeof next !== 'object' || next === null) {
                    throw new TypeError('Iterator.zip: padding iterator result is not an object');
                  }
                  const done = !!next.done;
                  if (done) {
                    usingIterator = false;
                    padding[i] = undefined;
                  } else {
                    padding[i] = next.value;
                  }
                } catch (e) {
                  completionError = e;
                  break;
                }
              }
              if (completionError !== undefined) {
                closeIteratorList(openIters, 0, -1, completionError, false);
                throw completionError;
              }
              if (usingIterator) {
                try {
                  closeInputIterator(paddingIterRecord);
                } catch (e) {
                  closeIteratorList(openIters, 0, -1, e, false);
                  throw e;
                }
              }
            }

            let executing = false;
            let allDone = iters.length === 0;
            let hasYielded = false;

            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }

              executing = true;
              try {
                const iterCount = iters.length;
                const results = [];

                for (let i = 0; i < iterCount; i++) {
                  const iter = iters[i];
                  if (iter.done) {
                    if (mode === 'longest') {
                      results.push(padding[i]);
                    }
                    continue;
                  }

                  if (typeof iter.nextMethod !== 'function') {
                    allDone = true;
                    closeZipIterators(i, new TypeError('Iterator next method is not callable'));
                  }

                  let result;
                  try {
                    result = iter.nextMethod.call(iter.iterator);
                  } catch (e) {
                    allDone = true;
                    closeZipIterators(i, e);
                  }

                  if (typeof result !== 'object' || result === null) {
                    allDone = true;
                    closeZipIterators(i, new TypeError('Iterator result must be an object'));
                  }

                  const done = !!result.done;
                  if (done) {
                    iter.done = true;
                    openIters[i] = null;

                    if (mode === 'shortest') {
                      allDone = true;
                      closeZipIterators(i);
                      return { value: undefined, done: true };
                    }

                    if (mode === 'strict') {
                      if (i !== 0) {
                        allDone = true;
                        closeZipIterators(-1, new TypeError('Iterator.zip: iterators have different lengths (strict mode)'));
                      }
                      for (let k = 1; k < iterCount; k++) {
                        const other = iters[k];
                        if (typeof other.nextMethod !== 'function') {
                          allDone = true;
                          closeZipIterators(-1, new TypeError('Iterator next method is not callable'));
                        }
                        let otherResult;
                        try {
                          otherResult = other.nextMethod.call(other.iterator);
                        } catch (e) {
                          allDone = true;
                          closeZipIterators(k, e);
                        }
                        if (typeof otherResult !== 'object' || otherResult === null) {
                          allDone = true;
                          closeZipIterators(k, new TypeError('Iterator result must be an object'));
                        }
                        if (!!otherResult.done) {
                          other.done = true;
                          openIters[k] = null;
                        } else {
                          allDone = true;
                          closeZipIterators(-1, new TypeError('Iterator.zip: iterators have different lengths (strict mode)'));
                        }
                      }
                      allDone = true;
                      return { value: undefined, done: true };
                    }

                    results.push(padding[i]);
                  } else {
                    results.push(result.value);
                  }
                }

                if (mode === 'longest') {
                  let allIteratorsDone = true;
                  for (let i = 0; i < iterCount; i++) {
                    if (!iters[i].done) {
                      allIteratorsDone = false;
                      break;
                    }
                  }
                  if (allIteratorsDone) {
                    allDone = true;
                    return { value: undefined, done: true };
                  }
                }

                hasYielded = true;
                return { value: results, done: false };
              } finally {
                executing = false;
              }
            }

            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }
              if (!hasYielded) {
                allDone = true;
              } else {
                executing = true;
              }
              try {
                closeZipIterators(-1);
                allDone = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }

            const helper = Object.create(IteratorHelperPrototype);
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);

            return helper;
          }, 1);

          // Iterator.zipKeyed static method (joint-iteration proposal)
          // https://tc39.es/proposal-joint-iteration/
          defineNonConstructibleMethod(Iterator, 'zipKeyed', function zipKeyed(iterables, options) {
            if (typeof iterables !== 'object' || iterables === null) {
              throw new TypeError('Iterator.zipKeyed: iterables is not an object');
            }

            let mode = 'shortest';
            let paddingOption = undefined;

            if (options !== undefined) {
              if (typeof options !== 'object' || options === null) {
                throw new TypeError('Iterator.zipKeyed: options is not an object');
              }
              const modeOption = options.mode;
              if (modeOption === undefined) {
                mode = 'shortest';
              } else if (modeOption === 'longest' || modeOption === 'strict' || modeOption === 'shortest') {
                mode = modeOption;
              } else {
                throw new TypeError('Iterator.zipKeyed: mode must be "shortest", "longest", or "strict"');
              }
              if (mode === 'longest') {
                paddingOption = options.padding;
              }
            }

            const iters = [];
            const openIters = [];
            const keys = [];

            function closeZipKeyedIterators(skipIndex, error) {
              closeIteratorList(openIters, 0, skipIndex, error);
            }

            const allKeys = Reflect.ownKeys(iterables);
            for (const key of allKeys) {
              let desc;
              try {
                desc = Reflect.getOwnPropertyDescriptor(iterables, key);
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
              if (desc === undefined || desc.enumerable !== true) {
                continue;
              }
              let value;
              try {
                value = iterables[key];
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
              if (value === undefined) {
                continue;
              }
              if (typeof value !== 'object' || value === null) {
                closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterable element is not an object'));
              }
              try {
                const iterMethod = value[Symbol.iterator];
                let iter;
                if (iterMethod === undefined || iterMethod === null) {
                  iter = value;
                } else {
                  if (typeof iterMethod !== 'function') {
                    closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: @@iterator is not callable'));
                  }
                  iter = iterMethod.call(value);
                  if (typeof iter !== 'object' || iter === null) {
                    closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterator is not an object'));
                  }
                }
                const nextMethod = iter.next;
                keys.push(key);
                iters.push({ key, iterator: iter, nextMethod, done: false });
                openIters.push(iter);
              } catch (e) {
                closeZipKeyedIterators(-1, e);
              }
            }

            const paddingValues = Object.create(null);
            if (mode === 'longest' && paddingOption !== undefined) {
              if (typeof paddingOption !== 'object' || paddingOption === null) {
                throw new TypeError('Iterator.zipKeyed: padding is not an object');
              }
              for (const key of keys) {
                try {
                  paddingValues[key] = paddingOption[key];
                } catch (e) {
                  closeZipKeyedIterators(-1, e);
                }
              }
            }

            function getPaddingValue(key) {
              if (mode !== 'longest') {
                return undefined;
              }
              if (paddingOption === undefined) {
                return undefined;
              }
              return paddingValues[key];
            }

            let executing = false;
            let allDone = iters.length === 0;
            let hasYielded = false;

            function nextImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }

              executing = true;
              try {
                const iterCount = iters.length;
                const resultObj = Object.create(null);

                for (let i = 0; i < iterCount; i++) {
                  const { key, iterator, nextMethod } = iters[i];

                  if (iters[i].done) {
                    if (mode === 'longest') {
                      resultObj[key] = getPaddingValue(key);
                    }
                    continue;
                  }

                  if (typeof nextMethod !== 'function') {
                    allDone = true;
                    closeZipKeyedIterators(i, new TypeError('Iterator next method is not callable'));
                  }

                  let result;
                  try {
                    result = nextMethod.call(iterator);
                  } catch (e) {
                    allDone = true;
                    closeZipKeyedIterators(i, e);
                  }

                  if (typeof result !== 'object' || result === null) {
                    allDone = true;
                    closeZipKeyedIterators(i, new TypeError('Iterator result must be an object'));
                  }

                  const done = !!result.done;
                  if (done) {
                    iters[i].done = true;
                    openIters[i] = null;

                    if (mode === 'shortest') {
                      allDone = true;
                      closeZipKeyedIterators(i);
                      return { value: undefined, done: true };
                    } else if (mode === 'strict') {
                      if (i !== 0) {
                        allDone = true;
                        closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterators have different lengths (strict mode)'));
                      }

                      for (let k = 1; k < iterCount; k++) {
                        const kNextMethod = iters[k].nextMethod;
                        if (typeof kNextMethod !== 'function') {
                          allDone = true;
                          closeZipKeyedIterators(-1, new TypeError('Iterator next method is not callable'));
                        }

                        let kResult;
                        try {
                          kResult = kNextMethod.call(iters[k].iterator);
                        } catch (e) {
                          allDone = true;
                          closeZipKeyedIterators(k, e);
                        }

                        if (typeof kResult !== 'object' || kResult === null) {
                          allDone = true;
                          closeZipKeyedIterators(k, new TypeError('Iterator result must be an object'));
                        }

                        if (kResult.done) {
                          iters[k].done = true;
                          openIters[k] = null;
                        } else {
                          allDone = true;
                          closeZipKeyedIterators(-1, new TypeError('Iterator.zipKeyed: iterators have different lengths (strict mode)'));
                        }
                      }

                      allDone = true;
                      return { value: undefined, done: true };
                    } else {
                      resultObj[key] = getPaddingValue(key);
                    }
                  } else {
                    resultObj[key] = result.value;
                  }
                }

                if (mode === 'longest') {
                  let allItersDone = true;
                  for (let i = 0; i < iterCount; i++) {
                    if (!iters[i].done) {
                      allItersDone = false;
                      break;
                    }
                  }
                  if (allItersDone) {
                    allDone = true;
                    return { value: undefined, done: true };
                  }
                }

                hasYielded = true;
                return { value: resultObj, done: false };
              } finally {
                executing = false;
              }
            }

            function returnImpl() {
              if (executing) {
                throw new TypeError('Generator is already executing');
              }
              if (allDone) {
                return { value: undefined, done: true };
              }
              if (!hasYielded) {
                allDone = true;
              } else {
                executing = true;
              }
              try {
                closeZipKeyedIterators(-1);
                allDone = true;
                return { value: undefined, done: true };
              } finally {
                executing = false;
              }
            }

            const helper = Object.create(IteratorHelperPrototype);
            defineNonConstructibleMethod(helper, 'next', nextImpl, 0);
            defineNonConstructibleMethod(helper, 'return', returnImpl, 0);

            return helper;
          }, 1);

          // Set up Iterator constructor
          Object.setPrototypeOf(Iterator, Function.prototype);
          Iterator.prototype = IteratorPrototype;
          
          Object.defineProperty(Iterator, 'prototype', {
            writable: false,
            enumerable: false,
            configurable: false
          });

          Object.defineProperty(Iterator, 'name', {
            value: 'Iterator',
            writable: false,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(Iterator, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true
          });

          // Expose Iterator globally
          Object.defineProperty(globalThis, 'Iterator', {
            value: Iterator,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

