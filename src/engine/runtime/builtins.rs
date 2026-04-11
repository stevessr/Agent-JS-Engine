fn install_array_from_async_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Array.fromAsync === 'function') {
            return;
          }

          const intrinsicIteratorSymbol = Symbol.iterator;
          const intrinsicAsyncIteratorSymbol = Symbol.asyncIterator;

          function isConstructor(value) {
            if (typeof value !== 'function') {
              return false;
            }
            try {
              Reflect.construct(function() {}, [], value);
              return true;
            } catch {
              return false;
            }
          }

          function toLength(value) {
            if (typeof value === 'bigint') {
              throw new TypeError('Array.fromAsync length cannot be a BigInt');
            }
            const number = Number(value);
            if (!Number.isFinite(number)) {
              return number > 0 ? Number.MAX_SAFE_INTEGER : 0;
            }
            if (number <= 0) {
              return 0;
            }
            return Math.min(Math.floor(number), Number.MAX_SAFE_INTEGER);
          }

          function createArrayFromAsyncResult(receiver, lengthArgProvided, length) {
            if (isConstructor(receiver)) {
              return lengthArgProvided
                ? Reflect.construct(receiver, [length])
                : Reflect.construct(receiver, []);
            }
            // If receiver is not a constructor, create an intrinsic Array
            if (lengthArgProvided && length > 4294967295) {
              throw new RangeError('Invalid array length');
            }
            return lengthArgProvided ? new Array(length) : [];
          }

          function defineArrayFromAsyncValue(target, index, value) {
            const key = String(index);
            const existing = Object.getOwnPropertyDescriptor(target, key);
            if (existing && existing.configurable === false && existing.writable === false) {
              throw new TypeError('Cannot define Array.fromAsync result element');
            }
            Object.defineProperty(target, key, {
              value,
              writable: true,
              enumerable: true,
              configurable: true,
            });
          }

          function setArrayFromAsyncLength(target, length) {
            const descriptor = Object.getOwnPropertyDescriptor(target, 'length');
            if (descriptor && descriptor.writable === false && descriptor.value !== length) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
            if (!Reflect.set(target, 'length', length, target)) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
          }

          function getIntrinsicIteratorMethod(value) {
            if (value === null || value === undefined) {
              return undefined;
            }
            const iterator = intrinsicIteratorSymbol;
            if (iterator === undefined || iterator === null) {
              return undefined;
            }
            const method = value[iterator];
            if (method === undefined || method === null) {
              return undefined;
            }
            if (typeof method !== 'function') {
              throw new TypeError('Array.fromAsync iterator method must be callable');
            }
            return method;
          }

          function getIntrinsicAsyncIteratorMethod(value) {
            if (value === null || value === undefined) {
              return undefined;
            }
            const asyncIterator = intrinsicAsyncIteratorSymbol;
            if (asyncIterator === undefined || asyncIterator === null) {
              return undefined;
            }
            const method = value[asyncIterator];
            if (method === undefined || method === null) {
              return undefined;
            }
            if (typeof method !== 'function') {
              throw new TypeError('Array.fromAsync iterator method must be callable');
            }
            return method;
          }

          function getIteratorMethodPair(value) {
            const asyncMethod = getIntrinsicAsyncIteratorMethod(value);
            if (asyncMethod !== undefined) {
              // Spec order: if @@asyncIterator exists, do not probe @@iterator.
              return {
                asyncMethod,
                syncMethod: undefined,
              };
            }
            return {
              asyncMethod: undefined,
              syncMethod: getIntrinsicIteratorMethod(value),
            };
          }

          function createAsyncFromSyncIterator(syncIterator) {
            return {
              next() {
                return Promise.resolve(syncIterator.next());
              },
              return(value) {
                if (typeof syncIterator.return === 'function') {
                  return Promise.resolve(syncIterator.return(value));
                }
                return Promise.resolve({ done: true, value });
              }
            };
          }

          async function closeAsyncIterator(iterator) {
            if (iterator && typeof iterator.return === 'function') {
              await iterator.return();
            }
          }

          async function fillFromIterator(receiver, iterator, mapping, mapfn, thisArg, awaitValues) {
            const result = createArrayFromAsyncResult(receiver, false, 0);
            let index = 0;
            try {
              while (true) {
                const step = await iterator.next();
                if ((typeof step !== 'object' && typeof step !== 'function') || step === null) {
                  throw new TypeError('Array.fromAsync iterator result must be an object');
                }
                if (step.done) {
                  if (Object.getPrototypeOf(result) !== Object.prototype) {
                    setArrayFromAsyncLength(result, index);
                  } else {
                    result.length = index;
                  }
                  return result;
                }
                let nextValue = awaitValues ? await step.value : step.value;
                if (mapping) {
                  nextValue = await mapfn.call(thisArg, nextValue, index);
                }
                defineArrayFromAsyncValue(result, index, nextValue);
                index += 1;
              }
            } catch (error) {
              await closeAsyncIterator(iterator);
              throw error;
            }
          }

          async function fillFromArrayLike(receiver, arrayLike, mapping, mapfn, thisArg) {
            const length = toLength(arrayLike.length);
            const result = createArrayFromAsyncResult(receiver, true, length);
            for (let index = 0; index < length; index += 1) {
              let nextValue = await arrayLike[index];
              if (mapping) {
                nextValue = await mapfn.call(thisArg, nextValue, index);
              }
              defineArrayFromAsyncValue(result, index, nextValue);
            }
            if (!Reflect.set(result, 'length', length, result)) {
              throw new TypeError('Cannot set length on Array.fromAsync result');
            }
            return result;
          }

          async function arrayFromAsyncImpl(receiver, items, mapfn, thisArg) {
            const mapping = mapfn !== undefined;
            if (mapping && typeof mapfn !== 'function') {
              throw new TypeError('Array.fromAsync mapfn must be callable');
            }
            if (items === null || items === undefined) {
              throw new TypeError('Array.fromAsync requires an array-like or iterable input');
            }

            const { asyncMethod, syncMethod } = getIteratorMethodPair(items);
            if (asyncMethod !== undefined) {
              const asyncIterator = asyncMethod.call(items);
              return fillFromIterator(receiver, asyncIterator, mapping, mapfn, thisArg, false);
            }

            if (syncMethod !== undefined) {
              const syncIterator = syncMethod.call(items);
              return fillFromIterator(receiver, createAsyncFromSyncIterator(syncIterator), mapping, mapfn, thisArg, true);
            }

            return fillFromArrayLike(receiver, Object(items), mapping, mapfn, thisArg);
          }

          const fromAsync = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              // Avoid iterator-based destructuring so tests can safely mutate
              // ArrayIteratorPrototype.next without affecting argument reads.
              const items = args.length > 0 ? args[0] : undefined;
              const mapfn = args.length > 1 ? args[1] : undefined;
              const thisArgArg = args.length > 2 ? args[2] : undefined;
              return arrayFromAsyncImpl(thisArg, items, mapfn, thisArgArg);
            }
          });

          Object.defineProperty(fromAsync, 'name', {
            value: 'fromAsync',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(fromAsync, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Array, 'fromAsync', {
            value: fromAsync,
            writable: true,
            enumerable: false,
            configurable: true,
          });

        })();
        "#,
    ))?;
    Ok(())
}

fn install_uint8array_base_encoding_builtins(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          const Uint8ArrayCtor = globalThis.Uint8Array;
          if (typeof Uint8ArrayCtor !== 'function') {
            return;
          }

          const objectToString = Object.prototype.toString;
          const base64Tables = {
            base64: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/',
            base64url: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_',
          };
          const base64DecodeTables = {
            base64: Object.create(null),
            base64url: Object.create(null),
          };

          for (const name of Object.keys(base64Tables)) {
            const table = base64Tables[name];
            const decodeTable = base64DecodeTables[name];
            for (let i = 0; i < table.length; i++) {
              decodeTable[table[i]] = i;
            }
          }

          function isAsciiWhitespace(ch) {
            return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\f' || ch === '\r';
          }

          function requireString(value, name) {
            if (typeof value !== 'string') {
              throw new TypeError(name + ' requires a string');
            }
            return value;
          }

          function requireUint8Array(value, name) {
            if (objectToString.call(value) !== '[object Uint8Array]') {
              throw new TypeError(name + ' requires a Uint8Array receiver');
            }
            return value;
          }

          function requireAttachedUint8Array(value, name) {
            const view = requireUint8Array(value, name);
            if (view.buffer.detached) {
              throw new TypeError(name + ' called on detached ArrayBuffer');
            }
            return view;
          }

          function getBase64Alphabet(options) {
            let alphabet = 'base64';
            if (options !== undefined) {
              const candidate = options.alphabet;
              if (candidate !== undefined) {
                if (typeof candidate !== 'string') {
                  throw new TypeError('alphabet option must be a string');
                }
                if (candidate !== 'base64' && candidate !== 'base64url') {
                  throw new TypeError('alphabet option must be "base64" or "base64url"');
                }
                alphabet = candidate;
              }
            }
            return alphabet;
          }

          function getBase64LastChunkHandling(options) {
            let handling = 'loose';
            if (options !== undefined) {
              const candidate = options.lastChunkHandling;
              if (candidate !== undefined) {
                if (typeof candidate !== 'string') {
                  throw new TypeError('lastChunkHandling option must be a string');
                }
                if (candidate !== 'loose' && candidate !== 'strict' && candidate !== 'stop-before-partial') {
                  throw new TypeError('invalid lastChunkHandling option');
                }
                handling = candidate;
              }
            }
            return handling;
          }

          function getToBase64Options(options) {
            const alphabet = getBase64Alphabet(options);
            let omitPadding = false;
            if (options !== undefined) {
              omitPadding = Boolean(options.omitPadding);
            }
            return { alphabet, omitPadding };
          }

          function readBase64Value(ch, decodeTable) {
            const value = decodeTable[ch];
            if (value === undefined) {
              throw new SyntaxError('invalid base64 string');
            }
            return value;
          }

          function decodeBase64Quartet(chunk, decodeTable, strict) {
            const q0 = chunk[0];
            const q1 = chunk[1];
            const q2 = chunk[2];
            const q3 = chunk[3];

            if (q2 === '=') {
              if (q3 !== '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(q0, decodeTable);
              const b = readBase64Value(q1, decodeTable);
              if (strict && (b & 0x0F) !== 0) {
                throw new SyntaxError('invalid base64 padding bits');
              }
              return [((a << 2) | (b >> 4)) & 0xFF];
            }

            if (q3 === '=') {
              const a = readBase64Value(q0, decodeTable);
              const b = readBase64Value(q1, decodeTable);
              const c = readBase64Value(q2, decodeTable);
              if (strict && (c & 0x03) !== 0) {
                throw new SyntaxError('invalid base64 padding bits');
              }
              return [
                ((a << 2) | (b >> 4)) & 0xFF,
                ((b << 4) | (c >> 2)) & 0xFF,
              ];
            }

            const a = readBase64Value(q0, decodeTable);
            const b = readBase64Value(q1, decodeTable);
            const c = readBase64Value(q2, decodeTable);
            const d = readBase64Value(q3, decodeTable);
            return [
              ((a << 2) | (b >> 4)) & 0xFF,
              ((b << 4) | (c >> 2)) & 0xFF,
              ((c << 6) | d) & 0xFF,
            ];
          }

          function canSkipPartialBase64Chunk(chunk, decodeTable) {
            if (chunk.length === 0) {
              return true;
            }
            if (chunk.length === 1) {
              return decodeTable[chunk[0]] !== undefined;
            }
            if (chunk.length === 2) {
              return decodeTable[chunk[0]] !== undefined && decodeTable[chunk[1]] !== undefined;
            }
            if (chunk.length === 3) {
              if (chunk[2] === '=') {
                return decodeTable[chunk[0]] !== undefined && decodeTable[chunk[1]] !== undefined;
              }
              return (
                decodeTable[chunk[0]] !== undefined &&
                decodeTable[chunk[1]] !== undefined &&
                decodeTable[chunk[2]] !== undefined
              );
            }
            return false;
          }

          function decodeLooseFinalBase64Chunk(chunk, decodeTable) {
            if (chunk.length === 2) {
              if (chunk[0] === '=' || chunk[1] === '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(chunk[0], decodeTable);
              const b = readBase64Value(chunk[1], decodeTable);
              return [((a << 2) | (b >> 4)) & 0xFF];
            }
            if (chunk.length === 3) {
              if (chunk[0] === '=' || chunk[1] === '=' || chunk[2] === '=') {
                throw new SyntaxError('invalid base64 string');
              }
              const a = readBase64Value(chunk[0], decodeTable);
              const b = readBase64Value(chunk[1], decodeTable);
              const c = readBase64Value(chunk[2], decodeTable);
              return [
                ((a << 2) | (b >> 4)) & 0xFF,
                ((b << 4) | (c >> 2)) & 0xFF,
              ];
            }
            throw new SyntaxError('invalid base64 string');
          }

          function decodeBase64Into(string, alphabet, lastChunkHandling, maxLength, emitByte) {
            if (maxLength === 0) {
              return { read: 0, written: 0 };
            }

            const decodeTable = base64DecodeTables[alphabet];
            const chunk = [];
            const chunkEnds = [];
            let readBeforeChunk = 0;
            let written = 0;
            let sawPaddingQuartet = false;
            let pendingBytes = null;

            for (let i = 0; i < string.length; i++) {
              const ch = string[i];
              if (isAsciiWhitespace(ch)) {
                continue;
              }
              if (sawPaddingQuartet) {
                throw new SyntaxError('invalid base64 string');
              }

              chunk.push(ch);
              chunkEnds.push(i + 1);

              if (chunk.length === 4) {
                const bytes = decodeBase64Quartet(chunk, decodeTable, lastChunkHandling === 'strict');
                const paddedQuartet = chunk[2] === '=' || chunk[3] === '=';
                if (written + bytes.length > maxLength) {
                  return { read: readBeforeChunk, written };
                }
                if (paddedQuartet && written + bytes.length !== maxLength) {
                  pendingBytes = bytes;
                  readBeforeChunk = chunkEnds[3];
                  sawPaddingQuartet = true;
                } else {
                  for (let j = 0; j < bytes.length; j++) {
                    emitByte(bytes[j], written + j);
                  }
                  written += bytes.length;
                  readBeforeChunk = chunkEnds[3];
                }
                chunk.length = 0;
                chunkEnds.length = 0;

                if (written === maxLength) {
                  return { read: readBeforeChunk, written };
                }
              }
            }

            if (chunk.length === 0) {
              if (pendingBytes !== null) {
                for (let j = 0; j < pendingBytes.length; j++) {
                  emitByte(pendingBytes[j], written + j);
                }
                written += pendingBytes.length;
              }
              return { read: string.length, written };
            }

            if (lastChunkHandling === 'stop-before-partial') {
              if (!canSkipPartialBase64Chunk(chunk, decodeTable)) {
                throw new SyntaxError('invalid base64 string');
              }
              return { read: readBeforeChunk, written };
            }

            if (lastChunkHandling === 'strict') {
              throw new SyntaxError('invalid base64 string');
            }

            const bytes = decodeLooseFinalBase64Chunk(chunk, decodeTable);
            if (written + bytes.length > maxLength) {
              return { read: readBeforeChunk, written };
            }
            for (let j = 0; j < bytes.length; j++) {
              emitByte(bytes[j], written + j);
            }
            written += bytes.length;
            return { read: string.length, written };
          }

          function hexValue(ch) {
            const code = ch.charCodeAt(0);
            if (code >= 0x30 && code <= 0x39) {
              return code - 0x30;
            }
            if (code >= 0x41 && code <= 0x46) {
              return code - 0x41 + 10;
            }
            if (code >= 0x61 && code <= 0x66) {
              return code - 0x61 + 10;
            }
            return -1;
          }

          function decodeHexInto(string, maxLength, emitByte) {
            if ((string.length & 1) !== 0) {
              throw new SyntaxError('hex string must have even length');
            }

            let written = 0;
            for (let i = 0; i < string.length; i += 2) {
              if (written === maxLength) {
                return { read: i, written };
              }
              const hi = hexValue(string[i]);
              const lo = hexValue(string[i + 1]);
              if (hi < 0 || lo < 0) {
                throw new SyntaxError('invalid hex string');
              }
              emitByte((hi << 4) | lo, written);
              written += 1;
            }
            return { read: string.length, written };
          }

          function encodeBase64(view, options) {
            const table = base64Tables[options.alphabet];
            let result = '';
            let i = 0;
            while (i + 2 < view.length) {
              const a = view[i++];
              const b = view[i++];
              const c = view[i++];
              result += table[a >> 2];
              result += table[((a & 0x03) << 4) | (b >> 4)];
              result += table[((b & 0x0F) << 2) | (c >> 6)];
              result += table[c & 0x3F];
            }

            const remaining = view.length - i;
            if (remaining === 1) {
              const a = view[i];
              result += table[a >> 2];
              result += table[(a & 0x03) << 4];
              if (!options.omitPadding) {
                result += '==';
              }
            } else if (remaining === 2) {
              const a = view[i++];
              const b = view[i];
              result += table[a >> 2];
              result += table[((a & 0x03) << 4) | (b >> 4)];
              result += table[(b & 0x0F) << 2];
              if (!options.omitPadding) {
                result += '=';
              }
            }

            return result;
          }

          const staticMethods = {
            fromBase64(string, options) {
              requireString(string, 'Uint8Array.fromBase64');
              const alphabet = getBase64Alphabet(options);
              const lastChunkHandling = getBase64LastChunkHandling(options);
              const bytes = [];
              decodeBase64Into(string, alphabet, lastChunkHandling, Infinity, (byte) => {
                bytes.push(byte);
              });
              return new Uint8ArrayCtor(bytes);
            },

            fromHex(string) {
              requireString(string, 'Uint8Array.fromHex');
              const bytes = [];
              decodeHexInto(string, Infinity, (byte) => {
                bytes.push(byte);
              });
              return new Uint8ArrayCtor(bytes);
            },
          };

          const prototypeMethods = {
            setFromBase64(string, options) {
              const view = requireUint8Array(this, 'Uint8Array.prototype.setFromBase64');
              requireString(string, 'Uint8Array.prototype.setFromBase64');
              const alphabet = getBase64Alphabet(options);
              const lastChunkHandling = getBase64LastChunkHandling(options);
              if (view.buffer.detached) {
                throw new TypeError('Uint8Array.prototype.setFromBase64 called on detached ArrayBuffer');
              }
              return decodeBase64Into(string, alphabet, lastChunkHandling, view.length, (byte, index) => {
                view[index] = byte;
              });
            },

            setFromHex(string) {
              const view = requireAttachedUint8Array(this, 'Uint8Array.prototype.setFromHex');
              requireString(string, 'Uint8Array.prototype.setFromHex');
              return decodeHexInto(string, view.length, (byte, index) => {
                view[index] = byte;
              });
            },

            toBase64(options) {
              const view = requireUint8Array(this, 'Uint8Array.prototype.toBase64');
              const encodeOptions = getToBase64Options(options);
              if (view.buffer.detached) {
                throw new TypeError('Uint8Array.prototype.toBase64 called on detached ArrayBuffer');
              }
              return encodeBase64(view, encodeOptions);
            },

            toHex() {
              const view = requireAttachedUint8Array(this, 'Uint8Array.prototype.toHex');
              let result = '';
              for (let i = 0; i < view.length; i++) {
                const byte = view[i];
                result += (byte < 16 ? '0' : '') + byte.toString(16);
              }
              return result;
            },
          };

          Object.defineProperty(staticMethods.fromBase64, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(staticMethods.fromHex, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.setFromBase64, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.setFromHex, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.toBase64, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(prototypeMethods.toHex, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Uint8ArrayCtor, 'fromBase64', {
            value: staticMethods.fromBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor, 'fromHex', {
            value: staticMethods.fromHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(Uint8ArrayCtor.prototype, 'setFromBase64', {
            value: prototypeMethods.setFromBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'setFromHex', {
            value: prototypeMethods.setFromHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'toBase64', {
            value: prototypeMethods.toBase64,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(Uint8ArrayCtor.prototype, 'toHex', {
            value: prototypeMethods.toHex,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn install_array_flat_undefined_fix(context: &mut Context) -> JsResult<()> {
    let prototype = context.intrinsics().constructors().array().prototype();
    let original_symbol = array_flat_original_symbol(context)?;
    if prototype.has_own_property(original_symbol.clone(), context)? {
        return Ok(());
    }

    let original = prototype.get(js_string!("flat"), context)?;
    let callable = original.as_callable().ok_or_else(|| {
        JsNativeError::typ().with_message("missing Array.prototype.flat original callable")
    })?;

    prototype.define_property_or_throw(
        original_symbol,
        PropertyDescriptor::builder()
            .value(original)
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;

    let wrapped = build_builtin_function(
        context,
        js_string!("flat"),
        0,
        NativeFunction::from_copy_closure_with_captures(
            |this, args, callable, context| {
                if args.len() == 1 && args[0].is_undefined() {
                    callable.call(this, &[], context)
                } else {
                    callable.call(this, args, context)
                }
            },
            callable.clone(),
        ),
    );

    prototype.define_property_or_throw(
        js_string!("flat"),
        PropertyDescriptor::builder()
            .value(wrapped)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(())
}

fn install_atomics_pause(context: &mut Context) -> JsResult<()> {
    let atomics = context.global_object().get(js_string!("Atomics"), context)?;
    if let Some(atomics_obj) = atomics.as_object() {
        if !atomics_obj.has_own_property(js_string!("pause"), context)? {
            let pause_fn = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_atomics_pause))
                .name(js_string!("pause"))
                .length(0)
                .constructor(false)
                .build();
            atomics_obj.define_property_or_throw(
                js_string!("pause"),
                PropertyDescriptor::builder()
                    .value(pause_fn)
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }
    Ok(())
}

fn install_error_is_error(context: &mut Context) -> JsResult<()> {
    let error_ctor = context.intrinsics().constructors().error().constructor();
    if error_ctor.has_own_property(js_string!("isError"), context)? {
        return Ok(());
    }

    let is_error = build_builtin_function(
        context,
        js_string!("isError"),
        1,
        NativeFunction::from_fn_ptr(host_error_is_error),
    );

    error_ctor.define_property_or_throw(
        js_string!("isError"),
        PropertyDescriptor::builder()
            .value(is_error)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    Ok(())
}

fn install_promise_keyed_builtins(context: &mut Context) -> JsResult<()> {
    let promise = context
        .global_object()
        .get(js_string!("Promise"), context)?;
    if let Some(promise_obj) = promise.as_object() {
        if !promise_obj.has_own_property(js_string!("allKeyed"), context)? {
            promise_obj.define_property_or_throw(
                js_string!("allKeyed"),
                PropertyDescriptor::builder()
                    .value(
                        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_promise_all_keyed))
                            .name(js_string!("allKeyed"))
                            .length(1)
                            .constructor(false)
                            .build()
                    )
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
        if !promise_obj.has_own_property(js_string!("allSettledKeyed"), context)? {
            promise_obj.define_property_or_throw(
                js_string!("allSettledKeyed"),
                PropertyDescriptor::builder()
                    .value(
                        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_promise_all_settled_keyed))
                            .name(js_string!("allSettledKeyed"))
                            .length(1)
                            .constructor(false)
                            .build()
                    )
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
        }
    }
    Ok(())
}

fn install_disposable_stack_builtins(context: &mut Context) -> JsResult<()> {
    // 1. Symbol.dispose and Symbol.asyncDispose
    let symbol_ctor = context.intrinsics().constructors().symbol().constructor();
    let for_method = symbol_ctor.get(js_string!("for"), context)?;

    let symbol_obj = context
        .global_object()
        .get(js_string!("Symbol"), context)?
        .as_object()
        .expect("Global Symbol object is missing")
        .clone();

    let dispose_sym = if symbol_obj
        .get(js_string!("dispose"), context)?
        .is_undefined()
    {
        let sym = for_method
            .as_callable()
            .unwrap()
            .call(
                &symbol_ctor.clone().into(),
                &[js_string!("Symbol.dispose").into()],
                context,
            )?;
        symbol_obj.define_property_or_throw(
            js_string!("dispose"),
            PropertyDescriptor::builder()
                .value(sym.clone())
                .writable(false)
                .enumerable(false)
                .configurable(false),
            context,
        )?;
        sym
    } else {
        symbol_obj.get(js_string!("dispose"), context)?
    };

    let async_dispose_sym = if symbol_obj
        .get(js_string!("asyncDispose"), context)?
        .is_undefined()
    {
        let sym = for_method
            .as_callable()
            .unwrap()
            .call(
                &symbol_ctor.into(),
                &[js_string!("Symbol.asyncDispose").into()],
                context,
            )?;
        symbol_obj.define_property_or_throw(
            js_string!("asyncDispose"),
            PropertyDescriptor::builder()
                .value(sym.clone())
                .writable(false)
                .enumerable(false)
                .configurable(false),
            context,
        )?;
        sym
    } else {
        symbol_obj.get(js_string!("asyncDispose"), context)?
    };

    // 1.5 Fix Symbol.keyFor to return undefined for our well-known-like symbols
    let key_for_method = symbol_obj.get(js_string!("keyFor"), context)?;
    if key_for_method.is_callable() {
        let original_key_for = key_for_method.clone();
        let dispose_sym_val = symbol_obj.get(js_string!("dispose"), context)?;
        let async_dispose_sym_val = symbol_obj.get(js_string!("asyncDispose"), context)?;

        let new_key_for = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this: &BoaValue, args: &[BoaValue], captures: &(BoaValue, BoaValue, BoaValue), context: &mut Context| {
                    let symbol = args.get_or_undefined(0);
                    let (orig, d, ad) = captures;
                    if symbol == d || symbol == ad {
                        return Ok(BoaValue::undefined());
                    }
                    let orig_obj = orig.as_object().unwrap();
                    orig_obj.call(&BoaValue::undefined(), args, context)
                },
                (
                    original_key_for,
                    dispose_sym_val,
                    async_dispose_sym_val,
                ),
            )
        )
        .name(js_string!("keyFor"))
        .length(1)
        .constructor(false)
        .build();

        symbol_obj.define_property_or_throw(
            js_string!("keyFor"),
            PropertyDescriptor::builder()
                .value(new_key_for)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    // 2. SuppressedError
    let error_ctor = context.intrinsics().constructors().error().constructor();
    let error_proto = context.intrinsics().constructors().error().prototype();

    let suppressed_error_proto = JsObject::from_proto_and_data(error_proto, OrdinaryObject);
    suppressed_error_proto.define_property_or_throw(
        js_string!("name"),
        PropertyDescriptor::builder()
            .value(js_string!("SuppressedError"))
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    suppressed_error_proto.define_property_or_throw(
        js_string!("message"),
        PropertyDescriptor::builder()
            .value(js_string!(""))
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let suppressed_error_ctor =
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_suppressed_error_constructor))
            .name(js_string!("SuppressedError"))
            .length(3)
            .constructor(true)
            .build();

    suppressed_error_ctor.define_property_or_throw(
        js_string!("prototype"),
        PropertyDescriptor::builder()
            .value(suppressed_error_proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    suppressed_error_proto.define_property_or_throw(
        js_string!("constructor"),
        PropertyDescriptor::builder()
            .value(suppressed_error_ctor.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    suppressed_error_ctor.set_prototype(Some(error_ctor.into()));

    context.register_global_property(
        js_string!("SuppressedError"),
        suppressed_error_ctor,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;

    // 3. DisposableStack
    let disposable_stack_proto = JsObject::with_object_proto(context.intrinsics());
    let disposable_stack_ctor =
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_constructor))
            .name(js_string!("DisposableStack"))
            .length(0)
            .constructor(true)
            .build();

    disposable_stack_ctor.define_property_or_throw(
        js_string!("prototype"),
        PropertyDescriptor::builder()
            .value(disposable_stack_proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    disposable_stack_proto.define_property_or_throw(
        js_string!("constructor"),
        PropertyDescriptor::builder()
            .value(disposable_stack_ctor.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let dispose_sym_key = dispose_sym.to_property_key(context)?;

    disposable_stack_proto.define_property_or_throw(
        js_string!("use"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_use))
                    .name(js_string!("use"))
                    .length(1)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    disposable_stack_proto.define_property_or_throw(
        js_string!("adopt"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_adopt))
                    .name(js_string!("adopt"))
                    .length(2)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    disposable_stack_proto.define_property_or_throw(
        js_string!("defer"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_defer))
                    .name(js_string!("defer"))
                    .length(1)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    disposable_stack_proto.define_property_or_throw(
        js_string!("move"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_move))
                    .name(js_string!("move"))
                    .length(0)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let dispose_fn = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_dispose))
        .name(js_string!("dispose"))
        .length(0)
        .constructor(false)
        .build();

    disposable_stack_proto.define_property_or_throw(
        js_string!("dispose"),
        PropertyDescriptor::builder()
            .value(dispose_fn.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    disposable_stack_proto.define_property_or_throw(
        dispose_sym_key.clone(),
        PropertyDescriptor::builder()
            .value(dispose_fn)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    disposable_stack_proto.define_property_or_throw(
        js_string!("disposed"),
        PropertyDescriptor::builder()
            .get(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_disposable_stack_disposed_getter))
                    .name(js_string!("get disposed"))
                    .length(0)
                    .constructor(false)
                    .build()
            )
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    disposable_stack_proto.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("DisposableStack"))
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    context.register_global_property(
        js_string!("DisposableStack"),
        disposable_stack_ctor,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;

    // 4. AsyncDisposableStack
    let async_disposable_stack_proto = JsObject::with_object_proto(context.intrinsics());
    let async_disposable_stack_ctor =
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_constructor))
            .name(js_string!("AsyncDisposableStack"))
            .length(0)
            .constructor(true)
            .build();

    async_disposable_stack_ctor.define_property_or_throw(
        js_string!("prototype"),
        PropertyDescriptor::builder()
            .value(async_disposable_stack_proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    async_disposable_stack_proto.define_property_or_throw(
        js_string!("constructor"),
        PropertyDescriptor::builder()
            .value(async_disposable_stack_ctor.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let async_dispose_sym_key = async_dispose_sym.to_property_key(context)?;

    async_disposable_stack_proto.define_property_or_throw(
        js_string!("use"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_use))
                    .name(js_string!("use"))
                    .length(1)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    async_disposable_stack_proto.define_property_or_throw(
        js_string!("adopt"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_adopt))
                    .name(js_string!("adopt"))
                    .length(2)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    async_disposable_stack_proto.define_property_or_throw(
        js_string!("defer"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_defer))
                    .name(js_string!("defer"))
                    .length(1)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    async_disposable_stack_proto.define_property_or_throw(
        js_string!("move"),
        PropertyDescriptor::builder()
            .value(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_move))
                    .name(js_string!("move"))
                    .length(0)
                    .constructor(false)
                    .build()
            )
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    let dispose_async_fn = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_dispose_async))
        .name(js_string!("disposeAsync"))
        .length(0)
        .constructor(false)
        .build();

    async_disposable_stack_proto.define_property_or_throw(
        js_string!("disposeAsync"),
        PropertyDescriptor::builder()
            .value(dispose_async_fn.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    async_disposable_stack_proto.define_property_or_throw(
        async_dispose_sym_key.clone(),
        PropertyDescriptor::builder()
            .value(dispose_async_fn)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    async_disposable_stack_proto.define_property_or_throw(
        js_string!("disposed"),
        PropertyDescriptor::builder()
            .get(
                FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_disposable_stack_disposed_getter))
                    .name(js_string!("get disposed"))
                    .length(0)
                    .constructor(false)
                    .build()
            )
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    async_disposable_stack_proto.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("AsyncDisposableStack"))
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;

    context.register_global_property(
        js_string!("AsyncDisposableStack"),
        async_disposable_stack_ctor,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;

    // 5. Global helper functions
    context.register_global_property(
        js_string!("__agentjsDisposeSyncUsing__"),
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_dispose_sync_using))
            .name(js_string!("__agentjsDisposeSyncUsing__"))
            .length(3)
            .constructor(false)
            .build(),
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;
    context.register_global_property(
        js_string!("__agentjsDisposeAsyncUsing__"),
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_dispose_async_using))
            .name(js_string!("__agentjsDisposeAsyncUsing__"))
            .length(3)
            .constructor(false)
            .build(),
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;

    // 6. AsyncIteratorPrototype[Symbol.asyncDispose]
    let async_iterator_proto = context.intrinsics().objects().iterator_prototypes().async_iterator();
    if !async_iterator_proto.has_own_property(async_dispose_sym_key.clone(), context)? {
        let async_dispose_fn = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_async_iterator_dispose))
            .name(js_string!("[Symbol.asyncDispose]"))
            .length(0)
            .constructor(false)
            .build();
        async_iterator_proto.define_property_or_throw(
            async_dispose_sym_key,
            PropertyDescriptor::builder()
                .value(async_dispose_fn)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    // 7. IteratorPrototype[Symbol.dispose]
    let iterator_proto = context.intrinsics().objects().iterator_prototypes().iterator();
    if !iterator_proto.has_own_property(dispose_sym_key.clone(), context)? {
        let dispose_fn = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(host_iterator_dispose))
            .name(js_string!("[Symbol.dispose]"))
            .length(0)
            .constructor(false)
            .build();
        iterator_proto.define_property_or_throw(
            dispose_sym_key,
            PropertyDescriptor::builder()
                .value(dispose_fn)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )?;
    }

    Ok(())
}

fn install_finalization_registry_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r###"
        (() => {
          if (typeof globalThis.FinalizationRegistry === 'function') {
            return;
          }

          const registryState = new WeakMap();
          const emptyToken = Symbol('empty FinalizationRegistry unregister token');

          function canBeHeldWeakly(value) {
            if ((typeof value === 'object' && value !== null) || typeof value === 'function') {
              return true;
            }
            return typeof value === 'symbol' && Symbol.keyFor(value) === undefined;
          }

          function getIntrinsicFinalizationRegistryPrototype(newTarget) {
            if (newTarget === undefined || newTarget === FinalizationRegistry) {
              return FinalizationRegistry.prototype;
            }
            const proto = newTarget.prototype;
            if ((typeof proto === 'object' && proto !== null) || typeof proto === 'function') {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherFinalizationRegistry = otherGlobal && otherGlobal.FinalizationRegistry;
              const otherProto = otherFinalizationRegistry && otherFinalizationRegistry.prototype;
              if ((typeof otherProto === 'object' && otherProto !== null) || typeof otherProto === 'function') {
                return otherProto;
              }
            } catch {}
            return FinalizationRegistry.prototype;
          }

          function requireFinalizationRegistry(value, name) {
            if ((typeof value !== 'object' && typeof value !== 'function') || value === null || !registryState.has(value)) {
              throw new TypeError(name + ' called on incompatible receiver');
            }
            return registryState.get(value);
          }

          function FinalizationRegistry(cleanupCallback) {
            if (new.target === undefined) {
              throw new TypeError('Constructor FinalizationRegistry requires new');
            }
            if (typeof cleanupCallback !== 'function') {
              throw new TypeError('FinalizationRegistry cleanup callback must be callable');
            }

            const registry = Object.create(getIntrinsicFinalizationRegistryPrototype(new.target));
            registryState.set(registry, {
              cleanupCallback,
              cells: [],
              active: false,
            });
            return registry;
          }

          const finalizationRegistryMethods = {
            register(target, holdings, unregisterToken) {
              const state = requireFinalizationRegistry(this, 'FinalizationRegistry.prototype.register');
              if (!canBeHeldWeakly(target)) {
                throw new TypeError('FinalizationRegistry.prototype.register target must be weakly holdable');
              }
              if (Object.is(target, holdings)) {
                throw new TypeError('FinalizationRegistry target and holdings must not be the same');
              }
              if (unregisterToken !== undefined && !canBeHeldWeakly(unregisterToken)) {
                throw new TypeError('FinalizationRegistry unregisterToken must be weakly holdable');
              }
              state.cells.push({
                target,
                holdings,
                unregisterToken: unregisterToken === undefined ? emptyToken : unregisterToken,
              });
              return undefined;
            },

            unregister(unregisterToken) {
              const state = requireFinalizationRegistry(this, 'FinalizationRegistry.prototype.unregister');
              if (!canBeHeldWeakly(unregisterToken)) {
                throw new TypeError('FinalizationRegistry unregisterToken must be weakly holdable');
              }
              let removed = false;
              state.cells = state.cells.filter((cell) => {
                if (cell.unregisterToken !== emptyToken && Object.is(cell.unregisterToken, unregisterToken)) {
                  removed = true;
                  return false;
                }
                return true;
              });
              return removed;
            },
          };

          Object.defineProperty(finalizationRegistryMethods.register, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(finalizationRegistryMethods.unregister, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const finalizationRegistryPrototype = Object.create(Object.prototype);
          Object.defineProperties(finalizationRegistryPrototype, {
            constructor: {
              value: FinalizationRegistry,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            register: {
              value: finalizationRegistryMethods.register,
              writable: true,
              enumerable: false,
              configurable: true,
            },
            unregister: {
              value: finalizationRegistryMethods.unregister,
              writable: true,
              enumerable: false,
              configurable: true,
            },
          });
          Object.defineProperty(finalizationRegistryPrototype, Symbol.toStringTag, {
            value: 'FinalizationRegistry',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(FinalizationRegistry, 'prototype', {
            value: finalizationRegistryPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(globalThis, 'FinalizationRegistry', {
            value: FinalizationRegistry,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "###,
    ))?;
    Ok(())
}

fn install_bigint_to_locale_string(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const proto = BigInt.prototype;
          const originalKey = '__agentjs_original_BigInt_toLocaleString__';
          if (Object.prototype.hasOwnProperty.call(proto, originalKey)) {
            return;
          }

          const original = proto.toLocaleString;
          if (typeof original !== 'function') {
            return;
          }
          if (typeof Intl !== 'object' || Intl === null || typeof Intl.NumberFormat !== 'function') {
            return;
          }

          const IntrinsicNumberFormat = Intl.NumberFormat;

          Object.defineProperty(proto, originalKey, {
            value: original,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          const toLocaleStringFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              let value = thisArg;
              if (typeof value !== 'bigint') {
                value = BigInt.prototype.valueOf.call(value);
              }

              const locales = args.length > 0 ? args[0] : undefined;
              const options = args.length > 1 ? args[1] : undefined;
              return new IntrinsicNumberFormat(locales, options).format(value);
            },
          });

          Object.defineProperty(toLocaleStringFn, 'name', {
            value: 'toLocaleString',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(toLocaleStringFn, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(proto, 'toLocaleString', {
            value: toLocaleStringFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn normalize_builtin_function_to_string(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const originalFunctionToString = Function.prototype.toString;

          const isLikelyNativeSource = (source) =>
            typeof source === 'string' && source.includes('[native code]');

          const isSimpleIdentifierName = (name) =>
            typeof name === 'string' && /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name);

          const nativeSourceFor = (fn) => {
            const name = typeof fn.name === 'string' ? fn.name : '';
            if (isSimpleIdentifierName(name)) {
              return `function ${name}() { [native code] }`;
            }
            return 'function () { [native code] }';
          };

          const installNativeLikeToString = (fn) => {
            if (typeof fn !== 'function') {
              return;
            }

            let source;
            try {
              source = originalFunctionToString.call(fn);
            } catch {
              return;
            }

            if (isLikelyNativeSource(source)) {
              return;
            }

            const nativeSource = nativeSourceFor(fn);
            const nativeToString = new Proxy(() => {}, {
              apply() {
                return nativeSource;
              }
            });

            try {
              Object.defineProperty(nativeToString, 'name', {
                value: 'toString',
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(nativeToString, 'length', {
                value: 0,
                writable: false,
                enumerable: false,
                configurable: true,
              });
              Object.defineProperty(fn, 'toString', {
                value: nativeToString,
                writable: true,
                enumerable: false,
                configurable: true,
              });
            } catch {
              // Ignore non-configurable or non-extensible functions.
            }
          };

          const seen = new Set();
          const visit = (value) => {
            if (value === null) {
              return;
            }
            const type = typeof value;
            if (type !== 'object' && type !== 'function') {
              return;
            }
            if (seen.has(value)) {
              return;
            }
            seen.add(value);

            if (type === 'function') {
              installNativeLikeToString(value);
            }

            let descriptors;
            try {
              descriptors = Object.getOwnPropertyDescriptors(value);
            } catch {
              return;
            }

            for (const key of Reflect.ownKeys(descriptors)) {
              const desc = descriptors[key];
              if ('value' in desc) {
                visit(desc.value);
              } else {
                visit(desc.get);
                visit(desc.set);
              }
            }

            try {
              visit(Object.getPrototypeOf(value));
            } catch {
              // Ignore exotic prototype lookups.
            }
          };

          visit(globalThis);

          // Ensure important intrinsic-only prototypes are also reached even if not
          // directly enumerable from the global object graph.
          try {
            async function* __agentjs_async_gen__() {}
            const asyncGenProto = Object.getPrototypeOf(__agentjs_async_gen__.prototype);
            visit(Object.getPrototypeOf(asyncGenProto));
          } catch {}
        })();
        "#,
    ))?;
    Ok(())
}

