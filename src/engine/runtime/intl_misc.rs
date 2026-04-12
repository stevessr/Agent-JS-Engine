fn install_date_locale_methods(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const DateProto = Date.prototype;
          // Capture at install time so tainted Intl.DateTimeFormat doesn't affect us
          const DTF = Intl.DateTimeFormat;
          
          // Store original methods before overwriting
          const originalToLocaleString = DateProto.toLocaleString;
          const originalToLocaleDateString = DateProto.toLocaleDateString;
          const originalToLocaleTimeString = DateProto.toLocaleTimeString;
          
          // Always use polyfill to ensure consistent behavior with Intl.DateTimeFormat
          const toLocaleStringNeedsPolyfill = true;
          const toLocaleDateStringNeedsPolyfill = true;
          const toLocaleTimeStringNeedsPolyfill = true;
          
          // Helper to create a non-constructable function with proper name
          // Uses a Proxy on an arrow function (which has no [[Construct]])
          function makeNonConstructable(impl, name) {
            // Create an arrow function that calls impl with proper this
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            
            const handler = {
              apply(target, thisArg, args) {
                // Call impl with the correct this
                return impl.apply(thisArg, args);
              }
            };
            const proxy = new Proxy(arrowWrapper, handler);
            Object.defineProperty(proxy, 'name', { value: name, configurable: true });
            Object.defineProperty(proxy, 'length', { value: 0, writable: false, enumerable: false, configurable: true });
            return proxy;
          }
          
          // toLocaleString - uses DateTimeFormat with date and time components
          if (toLocaleStringNeedsPolyfill) {
            const toLocaleStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.5 (toLocaleString):
              // - If no options, default to year/month/day/hour/minute/second
              // - If options are given (even just {hour12: false}), use them
              // - toLocaleString does NOT add missing date/time components when some are given
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  year: 'numeric',
                  month: 'numeric', 
                  day: 'numeric',
                  hour: 'numeric',
                  minute: 'numeric',
                  second: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasDate = hasDateOptions(opts);
                const hasTime = hasTimeOptions(opts);
                const hasStyle = opts.dateStyle !== undefined || opts.timeStyle !== undefined;
                
                if (!hasDate && !hasTime && !hasStyle) {
                  // Options given but no date/time/style components (e.g., {hour12: false})
                  // Add both date and time defaults
                  resolvedOptions = Object.assign({}, opts, {
                    year: 'numeric',
                    month: 'numeric',
                    day: 'numeric',
                    hour: 'numeric',
                    minute: 'numeric',
                    second: 'numeric'
                  });
                } else {
                  // If any date/time/style components specified, use options as-is
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleStringFn = makeNonConstructable(toLocaleStringImpl, 'toLocaleString');
            Object.defineProperty(DateProto, 'toLocaleString', {
              value: toLocaleStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
          
          // Helper to check if object has any date components
          function hasDateOptions(opts) {
            return opts && (opts.weekday !== undefined || opts.era !== undefined || 
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined);
          }
          
          // Helper to check if object has any time components
          function hasTimeOptions(opts) {
            return opts && (opts.hour !== undefined || opts.minute !== undefined || 
              opts.second !== undefined || opts.dayPeriod !== undefined);
          }
          
          // toLocaleDateString - uses DateTimeFormat with date components
          // Per ECMA-402: Always includes date components, adds them if missing
          if (toLocaleDateStringNeedsPolyfill) {
            const toLocaleDateStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleDateString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.6:
              // - If no options, default to year/month/day
              // - If no date options (even if other options present), add year/month/day
              // This is "date/date" in the spec - date method requires date components
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  year: 'numeric',
                  month: 'numeric',
                  day: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasDate = hasDateOptions(opts);
                
                if (!hasDate) {
                  // No date options - add date defaults (toLocaleDateString always needs date)
                  resolvedOptions = Object.assign({}, opts, {
                    year: 'numeric',
                    month: 'numeric', 
                    day: 'numeric'
                  });
                } else {
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleDateStringFn = makeNonConstructable(toLocaleDateStringImpl, 'toLocaleDateString');
            Object.defineProperty(DateProto, 'toLocaleDateString', {
              value: toLocaleDateStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
          
          // toLocaleTimeString - uses DateTimeFormat with time components
          // Per ECMA-402: Always includes time components, adds them if missing
          if (toLocaleTimeStringNeedsPolyfill) {
            const toLocaleTimeStringImpl = function(locales, options) {
              if (this === null || this === undefined) {
                throw new TypeError('Date.prototype.toLocaleTimeString called on null or undefined');
              }
              if (!(this instanceof Date)) {
                throw new TypeError('this is not a Date object');
              }
              if (isNaN(this.getTime())) {
                return 'Invalid Date';
              }
              
              // Per ECMA-402 12.5.7:
              // - If no options, default to hour/minute/second
              // - If no time options (even if other options present), add hour/minute/second
              // This is "time/time" in the spec - time method requires time components
              let resolvedOptions;
              if (options === undefined || options === null) {
                resolvedOptions = {
                  hour: 'numeric',
                  minute: 'numeric',
                  second: 'numeric'
                };
              } else {
                const opts = Object(options);
                const hasTime = hasTimeOptions(opts);
                
                if (!hasTime) {
                  // No time options - add time defaults (toLocaleTimeString always needs time)
                  resolvedOptions = Object.assign({}, opts, {
                    hour: 'numeric',
                    minute: 'numeric',
                    second: 'numeric'
                  });
                } else {
                  resolvedOptions = opts;
                }
              }
              const dtf = new DTF(locales, resolvedOptions);
              return dtf.format(this);
            };
            const toLocaleTimeStringFn = makeNonConstructable(toLocaleTimeStringImpl, 'toLocaleTimeString');
            Object.defineProperty(DateProto, 'toLocaleTimeString', {
              value: toLocaleTimeStringFn,
              writable: true,
              enumerable: false,
              configurable: true
            });
          }
        })();
        "#,
    ))?;
    Ok(())
}

fn install_temporal_locale_string_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Temporal !== 'object' || Temporal === null) return;
          if (typeof Intl !== 'object' || Intl === null) return;
          if (typeof Intl.DateTimeFormat !== 'function') return;

          const instantProto = Temporal.Instant && Temporal.Instant.prototype;
          if (instantProto && typeof instantProto.toLocaleString === 'function') {
            const instantToLocaleString = new Proxy(() => {}, {
              apply(_target, thisArg, args) {
                if (Object.prototype.toString.call(thisArg) !== '[object Temporal.Instant]') {
                  throw new TypeError('Temporal.Instant.prototype.toLocaleString called on incompatible receiver');
                }
                const formatter = new Intl.DateTimeFormat(args[0], args[1]);
                return formatter.format(thisArg);
              },
            });
            Object.defineProperty(instantToLocaleString, 'name', {
              value: 'toLocaleString',
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(instantToLocaleString, 'length', {
              value: 0,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(instantProto, 'toLocaleString', {
              value: instantToLocaleString,
              writable: true,
              enumerable: false,
              configurable: true,
            });
          }
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_number_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl !== 'object' || Intl === null) return;
          if (typeof Intl.NumberFormat !== 'function') return;

          const NF = Intl.NumberFormat;
          const proto = NF.prototype;
          const nativeResolvedOptions = proto.resolvedOptions;
          const formatDesc = Object.getOwnPropertyDescriptor(proto, 'format');
          const nativeFormatGetter = formatDesc && formatDesc.get;
          const nativeFormatToParts = proto.formatToParts;
          const digitInfoCache = new Map();
          const nativeBoundFormatCache = new WeakMap();
          const wrappedFormatCache = new WeakMap();

          function makeNonConstructable(impl, name, length) {
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const proxy = new Proxy(arrowWrapper, {
              apply(_target, thisArg, args) {
                return impl.apply(thisArg, args);
              },
            });
            Object.defineProperty(proxy, 'name', {
              value: name,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            Object.defineProperty(proxy, 'length', {
              value: length,
              writable: false,
              enumerable: false,
              configurable: true,
            });
            try { delete proxy.prototype; } catch (_e) {}
            return proxy;
          }

          function unwrapNumberFormat(receiver) {
            try {
              nativeResolvedOptions.call(receiver);
              return receiver;
            } catch (_e) {
              throw new TypeError('Method called on incompatible receiver');
            }
          }

          function getNativeBoundFormat(receiver) {
            let bound = nativeBoundFormatCache.get(receiver);
            if (bound !== undefined) {
              return bound;
            }
            if (typeof nativeFormatGetter !== 'function') {
              throw new TypeError('Intl.NumberFormat.prototype.format is unavailable');
            }
            bound = nativeFormatGetter.call(receiver);
            nativeBoundFormatCache.set(receiver, bound);
            return bound;
          }

          function formatSpecialNumber(receiver, numeric) {
            const opts = nativeResolvedOptions.call(receiver);
            const signDisplay = opts.signDisplay || 'auto';
            const negative = numeric < 0 || Object.is(numeric, -0);
            const isNaNValue = Number.isNaN(numeric);

            let sign = '';
            if (negative) {
              if (signDisplay !== 'never') {
                sign = '-';
              }
            } else if (!isNaNValue) {
              if (signDisplay === 'always') {
                sign = '+';
              } else if (signDisplay === 'exceptZero' && numeric !== 0) {
                sign = '+';
              }
            } else if (signDisplay === 'always') {
              sign = '+';
            }

            if (isNaNValue) {
              return sign + 'NaN';
            }
            return sign + '∞';
          }

          const UNIT_DATA = {
            en: {
              long: { year: ['year', 'years'], month: ['month', 'months'], week: ['week', 'weeks'], day: ['day', 'days'], hour: ['hour', 'hours'], minute: ['minute', 'minutes'], second: ['second', 'seconds'], millisecond: ['millisecond', 'milliseconds'], microsecond: ['microsecond', 'microseconds'], nanosecond: ['nanosecond', 'nanoseconds'] },
              short: { year: ['yr', 'yrs'], month: ['mo', 'mos'], week: ['wk', 'wks'], day: ['day', 'days'], hour: ['hr', 'hrs'], minute: ['min', 'mins'], second: ['sec', 'secs'], millisecond: ['ms', 'ms'], microsecond: ['μs', 'μs'], nanosecond: ['ns', 'ns'] },
              narrow: { year: ['y', 'y'], month: ['m', 'm'], week: ['w', 'w'], day: ['d', 'd'], hour: ['h', 'h'], minute: ['m', 'm'], second: ['s', 's'], millisecond: ['ms', 'ms'], microsecond: ['μs', 'μs'], nanosecond: ['ns', 'ns'] }
            },
            es: {
              long: { year: ['año', 'años'], month: ['mes', 'meses'], week: ['semana', 'semanas'], day: ['día', 'días'], hour: ['hora', 'horas'], minute: ['minuto', 'minutos'], second: ['segundo', 'segundos'], millisecond: ['milisegundo', 'milisegundos'], microsecond: ['microsegundo', 'microsegundos'], nanosecond: ['nanosegundo', 'nanosegundos'] },
              short: { year: ['a', 'a'], month: ['mes', 'mes'], week: ['sem.', 'sem.'], day: ['día', 'días'], hour: ['h', 'h'], minute: ['min', 'min'], second: ['s', 's'], millisecond: ['ms', 'ms'], microsecond: ['μs', 'μs'], nanosecond: ['ns', 'ns'] },
              narrow: { year: ['a', 'a'], month: ['m', 'm'], week: ['s', 's'], day: ['d', 'd'], hour: ['h', 'h'], minute: ['min', 'min'], second: ['s', 's'], millisecond: ['ms', 'ms'], microsecond: ['μs', 'μs'], nanosecond: ['ns', 'ns'] }
            }
          };

          function formatNumberValue(receiver, nativeBound, value) {
            const opts = nativeResolvedOptions.call(receiver);
            let res = (typeof value === 'bigint') ? String(nativeBound(value)) : String(nativeBound(Number(value)));

            if (opts.style === 'unit' && opts.unit) {
               const lang = String(opts.locale).toLowerCase().split('-')[0];
               const data = UNIT_DATA[lang] || UNIT_DATA['en'];
               const style = opts.unitDisplay || 'short';
               const unitData = data[style] || data['short'];
               const labels = unitData[opts.unit];
               if (labels) {
                  // Check if either singular or plural label is already in res
                  if (!res.includes(labels[0]) && !res.includes(labels[1])) {
                    const isPlural = Math.abs(Number(value)) !== 1;
                    const label = isPlural ? labels[1] : labels[0];
                    if (style === 'narrow') return res + label;
                    return res + ' ' + label;
                  }
               }
            }
            return res;
          }

          function getDigitInfo(locale, numberingSystem) {
            const key = locale + '|' + (numberingSystem || '');
            if (digitInfoCache.has(key)) {
              return digitInfoCache.get(key);
            }

            const digitsFormatter = new NF(locale, {
              numberingSystem,
              useGrouping: false,
            });
            const digitsString = String(nativeFormatGetter.call(digitsFormatter)(9876543210));
            const digits = [...digitsString];
            const digitSet = new Set(digits);

            const sampleFormatter = new NF(locale, {
              numberingSystem,
              useGrouping: true,
              minimumFractionDigits: 1,
              maximumFractionDigits: 1,
            });
            const sample = String(nativeFormatGetter.call(sampleFormatter)(1000.1));
            const separators = [...sample].filter((ch) => !digitSet.has(ch));
            const info = {
              digits,
              digitSet,
              group: separators.length > 1 ? separators[0] : undefined,
              decimal: separators.length > 0 ? separators[separators.length - 1] : undefined,
            };
            digitInfoCache.set(key, info);
            return info;
          }

          function splitWhitespaceAffix(text) {
            if (!text) return { leading: '', core: '', trailing: '' };
            const leading = (text.match(/^\s+/) || [''])[0];
            const trailing = (text.match(/\s+$/) || [''])[0];
            return {
              leading,
              core: text.slice(leading.length, text.length - trailing.length),
              trailing,
            };
          }

          function pushAffixParts(parts, text, kind) {
            if (!text) return;
            const { leading, core, trailing } = splitWhitespaceAffix(text);
            if (leading) parts.push({ type: 'literal', value: leading });
            if (core) {
              if (kind === 'percentSign' && core.includes('%')) {
                parts.push({ type: 'percentSign', value: core });
              } else {
                parts.push({ type: kind, value: core });
              }
            }
            if (trailing) parts.push({ type: 'literal', value: trailing });
          }

          function numericPartsFromString(body, info) {
            if (body === 'NaN') {
              return [{ type: 'nan', value: body }];
            }
            if (body === '∞') {
              return [{ type: 'infinity', value: body }];
            }

            const parts = [];
            let integer = '';
            let fraction = '';
            let seenDecimal = false;
            for (const ch of [...body]) {
              if (info.group !== undefined && ch === info.group && !seenDecimal) {
                if (integer) {
                  parts.push({ type: 'integer', value: integer });
                  integer = '';
                }
                parts.push({ type: 'group', value: ch });
              } else if (info.decimal !== undefined && ch === info.decimal) {
                if (integer) {
                  parts.push({ type: 'integer', value: integer });
                  integer = '';
                }
                parts.push({ type: 'decimal', value: ch });
                seenDecimal = true;
              } else if (info.digitSet.has(ch)) {
                if (seenDecimal) {
                  fraction += ch;
                } else {
                  integer += ch;
                }
              } else {
                if (!seenDecimal) {
                  integer += ch;
                } else {
                  fraction += ch;
                }
              }
            }
            if (integer) parts.push({ type: 'integer', value: integer });
            if (fraction) parts.push({ type: 'fraction', value: fraction });
            return parts.length > 0 ? parts : [{ type: 'literal', value: body }];
          }

          function partitionFormattedNumber(receiver, formatted) {
            const opts = nativeResolvedOptions.call(receiver);
            const info = getDigitInfo(opts.locale, opts.numberingSystem);
            const parts = [];
            let rest = formatted;

            if (rest.startsWith('~')) {
              parts.push({ type: 'approximatelySign', value: '~' });
              rest = rest.slice(1);
            }

            if (rest.startsWith('+')) {
              parts.push({ type: 'plusSign', value: '+' });
              rest = rest.slice(1);
            } else if (rest.startsWith('-')) {
              parts.push({ type: 'minusSign', value: '-' });
              rest = rest.slice(1);
            }

            let numberStart = -1;
            for (let i = 0; i < rest.length; i++) {
              const ch = rest[i];
              if (info.digitSet.has(ch) || ch === '∞' || ch === 'N') {
                numberStart = i;
                break;
              }
            }

            if (numberStart === -1) {
              return parts.concat([{ type: 'literal', value: rest }]);
            }

            let numberEnd = rest.length - 1;
            for (let i = rest.length - 1; i >= numberStart; i--) {
              const ch = rest[i];
              if (info.digitSet.has(ch) || ch === '∞' || ch === 'N') {
                numberEnd = i;
                break;
              }
            }

            const prefix = rest.slice(0, numberStart);
            const body = rest.slice(numberStart, numberEnd + 1);
            const suffix = rest.slice(numberEnd + 1);

            if (opts.style === 'currency') {
              pushAffixParts(parts, prefix, 'currency');
            } else if (opts.style === 'unit') {
              pushAffixParts(parts, prefix, 'unit');
            } else if (opts.style === 'percent') {
              pushAffixParts(parts, prefix, 'percentSign');
            } else if (prefix) {
              parts.push({ type: 'literal', value: prefix });
            }

            parts.push(...numericPartsFromString(body, info));

            if (opts.style === 'currency') {
              pushAffixParts(parts, suffix, 'currency');
            } else if (opts.style === 'unit') {
              pushAffixParts(parts, suffix, 'unit');
            } else if (opts.style === 'percent') {
              pushAffixParts(parts, suffix, 'percentSign');
            } else if (suffix) {
              parts.push({ type: 'literal', value: suffix });
            }

            return parts;
          }

          const formatGetter = makeNonConstructable(function format() {
            const receiver = unwrapNumberFormat(this);
            let wrapped = wrappedFormatCache.get(receiver);
            if (wrapped !== undefined) {
              return wrapped;
            }

            const nativeBound = getNativeBoundFormat(receiver);
            wrapped = makeNonConstructable(function(value) {
              return formatNumberValue(receiver, nativeBound, value);
            }, '', 1);
            wrappedFormatCache.set(receiver, wrapped);
            return wrapped;
          }, 'get format', 0);

          Object.defineProperty(proto, 'format', {
            get: formatGetter,
            set: undefined,
            enumerable: false,
            configurable: true,
          });

          const formatToPartsImpl = makeNonConstructable(function formatToParts(value) {
            const receiver = unwrapNumberFormat(this);
            const nativeBound = getNativeBoundFormat(receiver);
            const formatted = formatNumberValue(receiver, nativeBound, value);
            if (typeof nativeFormatToParts === 'function') {
              try {
                return nativeFormatToParts.call(receiver, value);
              } catch (_e) {}
            }
            return partitionFormattedNumber(receiver, formatted).map((part) => ({
              type: part.type,
              value: part.value,
            }));
          }, 'formatToParts', 1);
          Object.defineProperty(proto, 'formatToParts', {
            value: formatToPartsImpl,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          function buildRangeParts(receiver, x, y) {
            const start = Number(x);
            const end = Number(y);
            if (Number.isNaN(start) || Number.isNaN(end)) {
              throw new RangeError('Invalid number range');
            }

            const nativeBound = getNativeBoundFormat(receiver);
            const startFormatted = formatNumberValue(receiver, nativeBound, x);
            const endFormatted = formatNumberValue(receiver, nativeBound, y);
            const startParts = partitionFormattedNumber(receiver, startFormatted);
            const endParts = partitionFormattedNumber(receiver, endFormatted);

            if (startFormatted === endFormatted) {
              return [
                { type: 'approximatelySign', value: '~', source: 'shared' },
                ...startParts.map((part) => ({ ...part, source: 'shared' })),
              ];
            }

            let prefixLen = 0;
            while (
              prefixLen < startParts.length &&
              prefixLen < endParts.length &&
              startParts[prefixLen].type === endParts[prefixLen].type &&
              startParts[prefixLen].value === endParts[prefixLen].value
            ) {
              prefixLen++;
            }

            let suffixLen = 0;
            while (
              suffixLen < startParts.length - prefixLen &&
              suffixLen < endParts.length - prefixLen &&
              startParts[startParts.length - 1 - suffixLen].type === endParts[endParts.length - 1 - suffixLen].type &&
              startParts[startParts.length - 1 - suffixLen].value === endParts[endParts.length - 1 - suffixLen].value
            ) {
              suffixLen++;
            }

            const separator = prefixLen > 0 || suffixLen > 0 ? '–' : ' – ';
            const parts = [];
            for (let i = 0; i < prefixLen; i++) {
              parts.push({ ...startParts[i], source: 'shared' });
            }
            for (let i = prefixLen; i < startParts.length - suffixLen; i++) {
              parts.push({ ...startParts[i], source: 'startRange' });
            }
            parts.push({ type: 'literal', value: separator, source: 'shared' });
            for (let i = prefixLen; i < endParts.length - suffixLen; i++) {
              parts.push({ ...endParts[i], source: 'endRange' });
            }
            for (let i = startParts.length - suffixLen; i < startParts.length; i++) {
              parts.push({ ...startParts[i], source: 'shared' });
            }
            return parts;
          }

          const formatRangeImpl = makeNonConstructable(function formatRange(x, y) {
            const receiver = unwrapNumberFormat(this);
            return buildRangeParts(receiver, x, y).map((part) => part.value).join('');
          }, 'formatRange', 2);
          Object.defineProperty(proto, 'formatRange', {
            value: formatRangeImpl,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          const formatRangeToPartsImpl = makeNonConstructable(function formatRangeToParts(x, y) {
            const receiver = unwrapNumberFormat(this);
            return buildRangeParts(receiver, x, y).map((part) => ({
              type: part.type,
              value: part.value,
              source: part.source,
            }));
          }, 'formatRangeToParts', 2);
          Object.defineProperty(proto, 'formatRangeToParts', {
            value: formatRangeToPartsImpl,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_relative_time_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          // Check if RelativeTimeFormat already exists
          if (typeof Intl.RelativeTimeFormat === 'function') {
            return;
          }
          
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_NUMERIC = ['always', 'auto'];
          const VALID_STYLE = ['long', 'short', 'narrow'];
          const VALID_UNITS = ['year', 'years', 'quarter', 'quarters', 'month', 'months', 
                              'week', 'weeks', 'day', 'days', 'hour', 'hours', 
                              'minute', 'minutes', 'second', 'seconds'];
          
          // Singular unit mapping
          const SINGULAR_UNITS = {
            'years': 'year', 'quarters': 'quarter', 'months': 'month',
            'weeks': 'week', 'days': 'day', 'hours': 'hour',
            'minutes': 'minute', 'seconds': 'second'
          };
          
          // WeakMap to store internal slots
          const rtfSlots = new WeakMap();
          
          function getOption(options, property, type, values, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            if (type === 'string') {
              value = String(value);
            }
            if (values !== undefined && !values.includes(value)) {
              throw new RangeError('Invalid value ' + value + ' for option ' + property);
            }
            return value;
          }
          
          function RelativeTimeFormat(locales, options) {
            if (!(this instanceof RelativeTimeFormat) && new.target === undefined) {
              throw new TypeError('Constructor Intl.RelativeTimeFormat requires "new"');
            }
            
            // Process locales
            let locale;
            if (locales === undefined) {
              locale = new Intl.NumberFormat().resolvedOptions().locale || 'en';
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || 'en';
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? Intl.getCanonicalLocales(locales)[0] : 'en';
            } else {
              locale = 'en';
            }
            
            // Process options
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }
            
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Read numberingSystem
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            const style = getOption(opts, 'style', 'string', VALID_STYLE, 'long');
            const numeric = getOption(opts, 'numeric', 'string', VALID_NUMERIC, 'always');
            
            const resolvedOpts = {
              locale: locale,
              style: style,
              numeric: numeric,
              numberingSystem: numberingSystem || 'latn'
            };
            
            rtfSlots.set(this, resolvedOpts);
          }
          
          RelativeTimeFormat.prototype.resolvedOptions = function resolvedOptions() {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            return {
              locale: slots.locale,
              style: slots.style,
              numeric: slots.numeric,
              numberingSystem: slots.numberingSystem
            };
          };
          
          RelativeTimeFormat.prototype.format = function format(value, unit) {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            
            value = Number(value);
            if (!Number.isFinite(value)) {
              throw new RangeError('Invalid value');
            }
            
            unit = String(unit);
            if (!VALID_UNITS.includes(unit)) {
              throw new RangeError('Invalid unit argument');
            }
            
            // Normalize to singular
            const singularUnit = SINGULAR_UNITS[unit] || unit;
            const absValue = Math.abs(value);
            
            // Simple format implementation
            const style = slots.style;
            const numeric = slots.numeric;
            
            // Handle auto numeric for special cases
            if (numeric === 'auto') {
              if (value === 0) {
                if (singularUnit === 'second') return 'now';
                if (singularUnit === 'minute') return 'this minute';
                if (singularUnit === 'hour') return 'this hour';
                if (singularUnit === 'day') return 'today';
                if (singularUnit === 'week') return 'this week';
                if (singularUnit === 'month') return 'this month';
                if (singularUnit === 'quarter') return 'this quarter';
                if (singularUnit === 'year') return 'this year';
              } else if (value === -1) {
                if (singularUnit === 'second') return '1 second ago';
                if (singularUnit === 'minute') return '1 minute ago';
                if (singularUnit === 'hour') return '1 hour ago';
                if (singularUnit === 'day') return 'yesterday';
                if (singularUnit === 'week') return 'last week';
                if (singularUnit === 'month') return 'last month';
                if (singularUnit === 'quarter') return 'last quarter';
                if (singularUnit === 'year') return 'last year';
              } else if (value === 1) {
                if (singularUnit === 'second') return 'in 1 second';
                if (singularUnit === 'minute') return 'in 1 minute';
                if (singularUnit === 'hour') return 'in 1 hour';
                if (singularUnit === 'day') return 'tomorrow';
                if (singularUnit === 'week') return 'next week';
                if (singularUnit === 'month') return 'next month';
                if (singularUnit === 'quarter') return 'next quarter';
                if (singularUnit === 'year') return 'next year';
              }
            }
            
            // Unit labels based on style
            let unitLabel;
            if (style === 'narrow') {
              const narrowLabels = {
                year: 'yr', month: 'mo', week: 'wk', day: 'd',
                hour: 'hr', minute: 'min', second: 's', quarter: 'qtr'
              };
              unitLabel = narrowLabels[singularUnit] || singularUnit;
            } else if (style === 'short') {
              const shortLabels = {
                year: 'yr.', month: 'mo.', week: 'wk.', day: 'day',
                hour: 'hr.', minute: 'min.', second: 'sec.', quarter: 'qtr.'
              };
              const shortPluralLabels = {
                year: 'yr.', month: 'mo.', week: 'wk.', day: 'days',
                hour: 'hr.', minute: 'min.', second: 'sec.', quarter: 'qtr.'
              };
              unitLabel = absValue === 1 ? (shortLabels[singularUnit] || singularUnit) : (shortPluralLabels[singularUnit] || singularUnit + 's');
            } else {
              // long style
              unitLabel = singularUnit;
              if (absValue !== 1) {
                unitLabel += 's';
              }
            }
            
            // Format number using locale-aware NumberFormat
            const nf = new Intl.NumberFormat(slots.locale, { numberingSystem: slots.numberingSystem });
            const formattedValue = nf.format(absValue);
            
            // Handle negative zero specially: Object.is(value, -0) checks for -0
            // Positive zero is "in 0 X", negative zero is "0 X ago"
            if (value < 0 || Object.is(value, -0)) {
              return formattedValue + ' ' + unitLabel + ' ago';
            } else {
              return 'in ' + formattedValue + ' ' + unitLabel;
            }
          };
          
          RelativeTimeFormat.prototype.formatToParts = function formatToParts(value, unit) {
            const slots = rtfSlots.get(this);
            if (!slots) {
              throw new TypeError('Method called on incompatible receiver');
            }
            
            value = Number(value);
            if (!Number.isFinite(value)) {
              throw new RangeError('Invalid value');
            }
            
            unit = String(unit);
            if (!VALID_UNITS.includes(unit)) {
              throw new RangeError('Invalid unit argument');
            }
            
            const singularUnit = SINGULAR_UNITS[unit] || unit;
            const absValue = Math.abs(value);
            
            // Format number using locale-aware NumberFormat
            const nf = new Intl.NumberFormat(slots.locale, { numberingSystem: slots.numberingSystem });
            const formattedValue = nf.format(absValue);
            
            const parts = [];
            if (value < 0) {
              parts.push({ type: 'integer', value: formattedValue, unit: singularUnit });
              parts.push({ type: 'literal', value: ' ' });
              parts.push({ type: 'literal', value: absValue === 1 ? singularUnit : singularUnit + 's' });
              parts.push({ type: 'literal', value: ' ago' });
            } else {
              parts.push({ type: 'literal', value: 'in ' });
              parts.push({ type: 'integer', value: String(absValue), unit: singularUnit });
              parts.push({ type: 'literal', value: ' ' });
              parts.push({ type: 'literal', value: absValue === 1 ? singularUnit : singularUnit + 's' });
            }
            
            return parts;
          };
          
          RelativeTimeFormat.supportedLocalesOf = function supportedLocalesOf(locales, options) {
            // Process options
            if (options !== undefined) {
              if (options === null) {
                throw new TypeError('Cannot convert null to object');
              }
              const opts = Object(options);
              const matcher = opts.localeMatcher;
              if (matcher !== undefined) {
                const matcherStr = String(matcher);
                if (!VALID_LOCALE_MATCHERS.includes(matcherStr)) {
                  throw new RangeError('Invalid localeMatcher');
                }
              }
            }
            
            if (locales === undefined) return [];
            const requestedLocales = Array.isArray(locales) ? locales : [String(locales)];
            // Validate each locale
            return Intl.getCanonicalLocales(requestedLocales);
          };
          
          // Make supportedLocalesOf non-enumerable and set length to 1
          Object.defineProperty(RelativeTimeFormat, 'supportedLocalesOf', {
            value: RelativeTimeFormat.supportedLocalesOf,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.supportedLocalesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Set up prototype chain
          Object.defineProperty(RelativeTimeFormat, 'prototype', {
            value: RelativeTimeFormat.prototype,
            writable: false,
            enumerable: false,
            configurable: false
          });
          
          // Make prototype methods non-enumerable
          Object.defineProperty(RelativeTimeFormat.prototype, 'format', {
            value: RelativeTimeFormat.prototype.format,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.prototype, 'formatToParts', {
            value: RelativeTimeFormat.prototype.formatToParts,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(RelativeTimeFormat.prototype, 'resolvedOptions', {
            value: RelativeTimeFormat.prototype.resolvedOptions,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat.prototype, 'constructor', {
            value: RelativeTimeFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat.prototype, Symbol.toStringTag, {
            value: 'Intl.RelativeTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Set function length
          Object.defineProperty(RelativeTimeFormat, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(RelativeTimeFormat, 'name', {
            value: 'RelativeTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          // Install on Intl object
          Object.defineProperty(Intl, 'RelativeTimeFormat', {
            value: RelativeTimeFormat,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_duration_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_STYLES = ['long', 'short', 'narrow', 'digital'];
          const VALID_DISPLAYS = ['auto', 'always'];
          const VALID_UNIT_STYLES = ['long', 'short', 'narrow', '2-digit', 'numeric'];
          
          // Unit component names
          const DURATION_UNITS = ['years', 'months', 'weeks', 'days', 'hours', 'minutes', 'seconds', 'milliseconds', 'microseconds', 'nanoseconds'];

          // If native DurationFormat exists, just patch the constructor for validation
          if (typeof Intl.DurationFormat === 'function') {
            const NativeDTF = Intl.DurationFormat;
            const UNIT_CONFIG_NATIVE = {
              years:        ['long','short','narrow'],
              months:       ['long','short','narrow'],
              weeks:        ['long','short','narrow'],
              days:         ['long','short','narrow'],
              hours:        ['long','short','narrow','numeric','2-digit'],
              minutes:      ['long','short','narrow','numeric','2-digit'],
              seconds:      ['long','short','narrow','numeric','2-digit'],
              milliseconds: ['long','short','narrow','numeric'],
              microseconds: ['long','short','narrow','numeric'],
              nanoseconds:  ['long','short','narrow','numeric'],
            };
            function validateDurationOptions(options) {
              if (options === undefined || options === null) return;
              const opts = Object(options);
              const style = opts.style !== undefined ? String(opts.style) : 'short';
              let prevStyle = '';
              for (const unit of DURATION_UNITS) {
                const stylesList = UNIT_CONFIG_NATIVE[unit];
                let unitStyle = opts[unit];
                if (unitStyle !== undefined) {
                  unitStyle = String(unitStyle);
                  if (!stylesList.includes(unitStyle)) throw new RangeError('Invalid ' + unit + ' style: ' + unitStyle);
                  if ((prevStyle === 'numeric' || prevStyle === '2-digit' || prevStyle === 'fractional') &&
                      unitStyle !== 'numeric' && unitStyle !== '2-digit') {
                    throw new RangeError('Invalid style ' + unitStyle + ' for unit ' + unit + ' following ' + prevStyle);
                  }
                } else {
                  if (style === 'digital') {
                    unitStyle = ['hours','minutes','seconds'].includes(unit) ? 'numeric' : style;
                  } else if (prevStyle === 'fractional' || prevStyle === 'numeric' || prevStyle === '2-digit') {
                    unitStyle = 'numeric';
                  } else {
                    unitStyle = style;
                  }
                }
                if ((prevStyle === 'numeric' || prevStyle === '2-digit') &&
                    (unit === 'minutes' || unit === 'seconds')) unitStyle = '2-digit';
                if (['hours','minutes','seconds'].includes(unit) && unitStyle === 'numeric') prevStyle = 'numeric';
                else if (['hours','minutes','seconds'].includes(unit) && unitStyle === '2-digit') prevStyle = '2-digit';
                else if (['milliseconds','microseconds','nanoseconds'].includes(unit) && unitStyle === 'numeric') prevStyle = 'fractional';
              }
            }
            const PatchedDTF = function DurationFormat(locales, options) {
              if (locales === null) throw new TypeError('Cannot convert null to object');
              validateDurationOptions(options);
              if (new.target !== undefined) {
                return Reflect.construct(NativeDTF, [locales, options], new.target);
              }
              return new NativeDTF(locales, options);
            };
            PatchedDTF.prototype = NativeDTF.prototype;
            Object.defineProperty(PatchedDTF.prototype, 'constructor', { value: PatchedDTF, writable: true, configurable: true });
            Object.setPrototypeOf(PatchedDTF, NativeDTF);
            Object.defineProperty(PatchedDTF, 'name', { value: 'DurationFormat', configurable: true });
            Object.defineProperty(PatchedDTF, 'length', { value: 0, configurable: true });
            if (typeof NativeDTF.supportedLocalesOf === 'function') {
              PatchedDTF.supportedLocalesOf = NativeDTF.supportedLocalesOf;
            }
            Intl.DurationFormat = PatchedDTF;
            return;
          }
          
          // WeakMap to store internal slots
          const dfSlots = new WeakMap();

          function makeNonConstructable(impl, name, length) {
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const proxy = new Proxy(arrowWrapper, {
              apply(_target, thisArg, args) {
                return impl.apply(thisArg, args);
              }
            });
            Object.defineProperty(proxy, 'name', {
              value: name,
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
            try { delete proxy.prototype; } catch (_e) {}
            return proxy;
          }

          function hasOwnDurationProperty(obj, key) {
            return Object.prototype.hasOwnProperty.call(obj, key);
          }

          function validateDurationInputRecord(durationObj) {
            let hasSupportedField = false;
            for (const key of Object.keys(durationObj)) {
              if (!DURATION_UNITS.includes(key)) {
                throw new TypeError('Invalid property: ' + key);
              }
              if (durationObj[key] === undefined) {
                throw new TypeError('Property cannot be undefined: ' + key);
              }
              hasSupportedField = true;
            }
            if (!hasSupportedField) {
              throw new TypeError('Duration must contain at least one supported property');
            }
          }

          function formatUnitValue(locale, value, unit, unitDisplay, opts = {}) {
            const nf = new Intl.NumberFormat(locale, { ...opts, style: 'unit', unit: unit.slice(0, -1), unitDisplay: unitDisplay });
            return nf.format(value);
          }

          function getOption(options, property, type, values, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            if (type === 'string') {
              value = String(value);
            } else if (type === 'number') {
              value = Number(value);
              if (!Number.isFinite(value)) {
                throw new RangeError('Invalid ' + property);
              }
            }
            if (values !== undefined && !values.includes(value)) {
              throw new RangeError('Invalid value ' + value + ' for option ' + property);
            }
            return value;
          }
          
          function getNumberOption(options, property, minimum, maximum, fallback) {
            let value = options[property];
            if (value === undefined) return fallback;
            value = Number(value);
            if (!Number.isFinite(value) || value < minimum || value > maximum) {
              throw new RangeError('Invalid ' + property);
            }
            return Math.floor(value);
          }
          
          function DurationFormat(locales, options) {
            if (!(this instanceof DurationFormat) && new.target === undefined) {
              throw new TypeError('Constructor Intl.DurationFormat requires "new"');
            }

            // CanonicalizeLocaleList: null throws TypeError
            if (locales === null) throw new TypeError('Cannot convert null to object');

            // Process locales
            const defaultLocale = (() => {
              try { return new Intl.NumberFormat().resolvedOptions().locale || 'en'; } catch (_e) { return 'en'; }
            })();
            let locale;
            if (locales === undefined) {
              locale = defaultLocale;
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || defaultLocale;
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? (Intl.getCanonicalLocales(locales)[0] || defaultLocale) : defaultLocale;
            } else {
              // ToObject then iterate — for objects with length, use getCanonicalLocales
              const obj = Object(locales);
              const len = obj.length;
              if (len !== undefined && Number(len) > 0) {
                locale = Intl.getCanonicalLocales(locales)[0] || defaultLocale;
              } else if (len !== undefined) {
                locale = defaultLocale;
              } else {
                locale = Intl.getCanonicalLocales(locales)[0] || defaultLocale;
              }
            }
            
            // Process options
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }
            
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Read numberingSystem
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              // Must be 3-8 alphanum chars
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            const style = getOption(opts, 'style', 'string', VALID_STYLES, 'short');

            // GetDurationUnitOptions per spec
            const UNIT_CONFIG = {
              years:        { stylesList: ['long','short','narrow'], digitalBase: undefined },
              months:       { stylesList: ['long','short','narrow'], digitalBase: undefined },
              weeks:        { stylesList: ['long','short','narrow'], digitalBase: undefined },
              days:         { stylesList: ['long','short','narrow'], digitalBase: undefined },
              hours:        { stylesList: ['long','short','narrow','numeric','2-digit'], digitalBase: 'numeric' },
              minutes:      { stylesList: ['long','short','narrow','numeric','2-digit'], digitalBase: '2-digit' },
              seconds:      { stylesList: ['long','short','narrow','numeric','2-digit'], digitalBase: '2-digit' },
              milliseconds: { stylesList: ['long','short','narrow','numeric'], digitalBase: 'numeric' },
              microseconds: { stylesList: ['long','short','narrow','numeric'], digitalBase: 'numeric' },
              nanoseconds:  { stylesList: ['long','short','narrow','numeric'], digitalBase: 'numeric' },
            };

            const unitOptions = {};
            const baseStyles = {};
            const baseDisplays = {};
            
            // Step 6: GetDurationUnitOptions
            for (const unit of DURATION_UNITS) {
              const { stylesList } = UNIT_CONFIG[unit];
              baseStyles[unit] = getOption(opts, unit, 'string', stylesList, undefined);
              baseDisplays[unit] = getOption(opts, unit + 'Display', 'string', VALID_DISPLAYS, undefined);
            }
            
            let prevStyle = '';
            for (let i = 0; i < DURATION_UNITS.length; i++) {
              const unit = DURATION_UNITS[i];
              const { digitalBase } = UNIT_CONFIG[unit];
              let unitStyle = baseStyles[unit];
              let displayDefault = 'always';

              if (unitStyle === undefined) {
                if (style === 'digital') {
                  if (!['hours','minutes','seconds'].includes(unit)) displayDefault = 'auto';
                  unitStyle = digitalBase || 'short';
                } else {
                  if (prevStyle === 'fractional' || prevStyle === 'numeric' || prevStyle === '2-digit') {
                    unitStyle = 'numeric';
                    displayDefault = (unit === 'minutes' || unit === 'seconds') ? 'auto' : 'always';
                  } else {
                    displayDefault = 'auto';
                    unitStyle = style;
                  }
                }
              } else if (prevStyle === 'numeric' || prevStyle === '2-digit' || prevStyle === 'fractional') {
                if (unitStyle !== 'numeric' && unitStyle !== '2-digit') {
                  throw new RangeError('Invalid style ' + unitStyle + ' for unit ' + unit + ' following ' + prevStyle);
                }
              }

              if ((prevStyle === 'numeric' || prevStyle === '2-digit') &&
                  (unit === 'minutes' || unit === 'seconds')) {
                unitStyle = '2-digit';
              }

              if ((unit === 'hours' || unit === 'minutes' || unit === 'seconds') && unitStyle === 'numeric') prevStyle = 'numeric';
              else if ((unit === 'hours' || unit === 'minutes' || unit === 'seconds') && unitStyle === '2-digit') prevStyle = '2-digit';
              else if ((unit === 'milliseconds' || unit === 'microseconds' || unit === 'nanoseconds') && unitStyle === 'numeric') prevStyle = 'fractional';
              else prevStyle = unitStyle;

              const display = baseDisplays[unit] !== undefined ? baseDisplays[unit] : displayDefault;
              unitOptions[unit] = unitStyle;
              unitOptions[unit + 'Display'] = display;
            }
            // fractionalDigits
            const fractionalDigits = getNumberOption(opts, 'fractionalDigits', 0, 9, undefined);
            
            // Store internal slots
            const slots = {
              locale: locale,
              numberingSystem: numberingSystem || 'latn',
              style: style,
              fractionalDigits: fractionalDigits,
              baseStyles: baseStyles,
              ...unitOptions
            };
            dfSlots.set(this, slots);
            
            return this;
          }
          
          // format method
          DurationFormat.prototype.format = makeNonConstructable(function format(duration) {
            const slots = dfSlots.get(this);
            if (!slots) throw new TypeError('Called on incompatible receiver');
            if (duration === undefined) throw new TypeError('Duration is required');
            let durationObj = duration;
            if (typeof duration === 'string') {
              if (typeof Temporal === 'object' && Temporal.Duration) {
                durationObj = Temporal.Duration.from(duration);
              } else {
                throw new RangeError('Invalid duration string');
              }
            }
            if (durationObj === null || typeof durationObj !== 'object') throw new TypeError('Duration must be an object');
            
            validateDurationInputRecord(durationObj);
            
            const components = {};
            for (const unit of DURATION_UNITS) {
              let value = durationObj[unit];
              if (value !== undefined) {
                let num = Number(value);
                if (!Number.isFinite(num)) throw new RangeError('Invalid component: ' + unit);
                components[unit] = Math.trunc(num);
              } else {
                components[unit] = 0;
              }
            }
            
            let hasPositive = false, hasNegative = false;
            for (const unit of DURATION_UNITS) {
              if (components[unit] > 0) hasPositive = true;
              if (components[unit] < 0) hasNegative = true;
            }
            if (hasPositive && hasNegative) throw new RangeError('Mixed signs');
            const isNegative = hasNegative;
            
            // Validation
            if (Math.abs(components.years) >= 4294967296 || Math.abs(components.months) >= 4294967296 || Math.abs(components.weeks) >= 4294967296) {
              throw new RangeError('Out of range');
            }
            const _normNano = BigInt(Math.abs(components.days)) * 86400000000000n +
                              BigInt(Math.abs(components.hours)) * 3600000000000n +
                              BigInt(Math.abs(components.minutes)) * 60000000000n +
                              BigInt(Math.abs(components.seconds)) * 1000000000n +
                              BigInt(Math.abs(components.milliseconds)) * 1000000n +
                              BigInt(Math.abs(components.microseconds)) * 1000n +
                              BigInt(Math.abs(components.nanoseconds));
            if (_normNano >= 9007199254740992000000000n) throw new RangeError('Out of range');
            
            const resultList = [];
            let digitalGroup = [];
            const nfLocale = slots.locale;

            for (let i = 0; i < DURATION_UNITS.length; i++) {
              const unit = DURATION_UNITS[i];
              let value = Math.abs(components[unit]);
              const unitStyle = slots[unit];
              const display = slots[unit + 'Display'];
              const isNumeric = unitStyle === 'numeric' || unitStyle === '2-digit';
              
              if (display === 'always' || value !== 0) {
                // Step 9.g: Fractional absorption
                if (unit === 'seconds' || unit === 'milliseconds' || unit === 'microseconds') {
                  const nextUnit = DURATION_UNITS[i+1];
                  const nextUnitStyle = nextUnit ? slots[nextUnit] : undefined;
                  if (nextUnitStyle === 'numeric' || nextUnitStyle === '2-digit') {
                    let fullVal;
                    const maxFD = slots.fractionalDigits !== undefined ? slots.fractionalDigits : 9;
                    const minFD = slots.fractionalDigits !== undefined ? slots.fractionalDigits : 0;

                    if (unit === 'seconds') {
                      let totalNs = BigInt(Math.abs(components.milliseconds)) * 1000000n +
                                    BigInt(Math.abs(components.microseconds)) * 1000n +
                                    BigInt(Math.abs(components.nanoseconds));
                      let finalS = BigInt(value) + (totalNs / 1000000000n);
                      let remNs = totalNs % 1000000000n;
                      let fracStr = String(remNs).padStart(9, '0').replace(/0+$/, '');
                      fullVal = fracStr.length > 0 ? (String(finalS) + '.' + fracStr) : String(finalS);
                    } else if (unit === 'milliseconds') {
                      let totalNs = BigInt(Math.abs(components.microseconds)) * 1000n + BigInt(Math.abs(components.nanoseconds));
                      let finalMs = BigInt(value) + (totalNs / 1000000n);
                      let remNs = totalNs % 1000000n;
                      let fracStr = String(remNs).padStart(6, '0').replace(/0+$/, '');
                      fullVal = fracStr.length > 0 ? (String(finalMs) + '.' + fracStr) : String(finalMs);
                    } else { // microseconds
                      let ns = BigInt(Math.abs(components.nanoseconds));
                      let finalUs = BigInt(value) + (ns / 1000n);
                      let remNs = ns % 1000n;
                      let fracStr = String(remNs).padStart(3, '0').replace(/0+$/, '');
                      fullVal = fracStr.length > 0 ? (String(finalUs) + '.' + fracStr) : String(finalUs);
                    }
                    
                    if (isNumeric && (unit === 'hours' || unit === 'minutes' || unit === 'seconds')) {
                      // Numeric seconds absorbing milliseconds: part of digital group
                      let fv = String(value); // This is wrong, should use finalS part of fullVal?
                      // Actually fv is the integer part of the unit
                      let integerPart = fullVal.split('.')[0];
                      const needsPadding = unitStyle === '2-digit' || (digitalGroup.length > 0 && unit !== 'hours');
                      if (needsPadding) {
                        integerPart = integerPart.padStart(2, '0');
                      }
                      const finalFullVal = fullVal.includes('.') ? (integerPart + '.' + fullVal.split('.')[1]) : integerPart;

                      const formatted = new Intl.NumberFormat(nfLocale, { 
                        minimumIntegerDigits: needsPadding ? 2 : 1,
                        minimumFractionDigits: minFD, maximumFractionDigits: maxFD, roundingMode: 'trunc',
                        useGrouping: false
                      }).format(finalFullVal);
                      digitalGroup.push(formatted);
                      resultList.push(digitalGroup.join(':'));
                      digitalGroup = [];
                    } else {
                      // Non-numeric unit absorbing smaller units
                      const fmtStyle = isNumeric ? (slots.style === 'narrow' ? 'narrow' : (slots.style === 'long' ? 'long' : 'short')) : unitStyle;
                      resultList.push(formatUnitValue(nfLocale, fullVal, unit, fmtStyle, { useGrouping: false, minimumFractionDigits:minFD, maximumFractionDigits:maxFD, roundingMode:'trunc' }));
                    }
                    break; // All remaining units absorbed
                  }
                }

                if (isNumeric) {
                  if (unit === 'hours' || unit === 'minutes' || unit === 'seconds') {
                    let fv = String(value);
                    if (unitStyle === '2-digit' || (digitalGroup.length > 0 && unit !== 'hours')) fv = fv.padStart(2, '0');
                    const nextUnit = DURATION_UNITS[i+1];
                    const nextUnitStyle = nextUnit ? slots[nextUnit] : undefined;
                    
                    if (nextUnitStyle !== 'numeric' && nextUnitStyle !== '2-digit') {
                      digitalGroup.push(fv);
                      resultList.push(digitalGroup.join(':'));
                      digitalGroup = [];
                    } else {
                      digitalGroup.push(fv);
                    }
                  } else {
                    const fmtStyle = slots.style === 'narrow' ? 'narrow' : (slots.style === 'long' ? 'long' : 'short');
                    resultList.push(formatUnitValue(nfLocale, value, unit, fmtStyle, { useGrouping: false }));
                  }
                } else {
                  if (digitalGroup.length > 0) {
                    resultList.push(digitalGroup.join(':'));
                    digitalGroup = [];
                  }
                  resultList.push(formatUnitValue(nfLocale, value, unit, unitStyle));
                }
              }
            }
            
            const lfStyle = slots.style === 'digital' ? 'short' : slots.style;
            const res = new Intl.ListFormat(nfLocale, {type:'unit', style:lfStyle}).format(resultList);
            return (isNegative ? '-' : '') + res;
          }, 'format', 1);
          
          // formatToParts method
          DurationFormat.prototype.formatToParts = makeNonConstructable(function formatToParts(duration) {
            const slots = dfSlots.get(this);
            if (!slots) throw new TypeError('Called on incompatible receiver');
            return [{ type: 'literal', value: this.format(duration) }];
          }, 'formatToParts', 1);

          // resolvedOptions method
          DurationFormat.prototype.resolvedOptions = makeNonConstructable(function resolvedOptions() {
            const slots = dfSlots.get(this);
            if (!slots) throw new TypeError('Called on incompatible receiver');
            const res = {
              locale: slots.locale,
              numberingSystem: slots.numberingSystem,
              style: slots.style
            };
            if (slots.fractionalDigits !== undefined) {
              res.fractionalDigits = slots.fractionalDigits;
            }
            for (const unit of DURATION_UNITS) {
              res[unit] = slots[unit];
              res[unit + 'Display'] = slots[unit + 'Display'];
            }
            return res;
          }, 'resolvedOptions', 0);

          // static supportedLocalesOf
          DurationFormat.supportedLocalesOf = function supportedLocalesOf(locales, options) {
            if (locales === null) throw new TypeError('Cannot convert null to object');
            return Intl.getCanonicalLocales(locales);
          };

          for (const m of ['format', 'formatToParts', 'resolvedOptions']) {
            const method = DurationFormat.prototype[m];
            Object.defineProperty(DurationFormat.prototype, m, { value: method, writable: true, enumerable: false, configurable: true });
            try { delete method.prototype; } catch(_e) {}
          }
          Object.defineProperty(DurationFormat, 'supportedLocalesOf', { value: DurationFormat.supportedLocalesOf, writable: true, enumerable: false, configurable: true });
          Object.defineProperty(DurationFormat.supportedLocalesOf, 'length', { value: 1, writable: false, enumerable: false, configurable: true });
          try { delete DurationFormat.supportedLocalesOf.prototype; } catch(_e) {}
          
          Object.defineProperty(DurationFormat.prototype, 'constructor', { value: DurationFormat, writable: true, enumerable: false, configurable: true });
          Object.defineProperty(DurationFormat.prototype, Symbol.toStringTag, { value: 'Intl.DurationFormat', writable: false, enumerable: false, configurable: true });
          Object.defineProperty(DurationFormat, 'length', { value: 0, writable: false, enumerable: false, configurable: true });
          Object.defineProperty(DurationFormat, 'name', { value: 'DurationFormat', writable: false, enumerable: false, configurable: true });
          Object.defineProperty(Intl, 'DurationFormat', { value: DurationFormat, writable: true, enumerable: false, configurable: true });
          Object.defineProperty(DurationFormat, 'prototype', { value: DurationFormat.prototype, writable: false, enumerable: false, configurable: false });
        })();
        "#,
    ))?;
    Ok(())
}

fn install_intl_supported_values_of(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl.supportedValuesOf === 'function') {
            return;
          }
          
          // Standard calendars - minimal set per ECMA-402 requirements
          // Note: 'islamic' and 'islamic-rgsa' removed as not all engines support them
          const calendars = [
            'buddhist', 'chinese', 'coptic', 'dangi', 'ethioaa', 'ethiopic',
            'gregory', 'hebrew', 'indian', 'islamic-civil', 'islamic-tbla',
            'islamic-umalqura', 'iso8601', 'japanese', 'persian', 'roc'
          ].sort();
          
          // Standard collations - per ECMA-402, 'standard' and 'search' must NOT be included
          const collations = [
            'compat', 'dict', 'emoji', 'eor', 'phonebk', 'phonetic', 'pinyin',
            'stroke', 'trad', 'unihan', 'zhuyin'
          ].sort();
          
          // ISO 4217 currency codes - including historical and test codes per spec
          const currencies = [
            'AAA', 'ADP', 'AED', 'AFA', 'AFN', 'ALK', 'ALL', 'AMD', 'ANG', 'AOA',
            'AOK', 'AON', 'AOR', 'ARA', 'ARL', 'ARM', 'ARP', 'ARS', 'ATS', 'AUD',
            'AWG', 'AYM', 'AZM', 'AZN', 'BAD', 'BAM', 'BAN', 'BBD', 'BDT', 'BEC',
            'BEF', 'BEL', 'BGL', 'BGM', 'BGN', 'BGO', 'BHD', 'BIF', 'BMD', 'BND',
            'BOB', 'BOL', 'BOP', 'BOV', 'BRB', 'BRC', 'BRE', 'BRL', 'BRN', 'BRR',
            'BRZ', 'BSD', 'BTN', 'BUK', 'BWP', 'BYB', 'BYN', 'BYR', 'BZD', 'CAD',
            'CDF', 'CHE', 'CHF', 'CHW', 'CLE', 'CLF', 'CLP', 'CNH', 'CNX', 'CNY',
            'COP', 'COU', 'CRC', 'CSD', 'CSK', 'CUC', 'CUP', 'CVE', 'CYP', 'CZK',
            'DDM', 'DEM', 'DJF', 'DKK', 'DOP', 'DZD', 'ECS', 'ECV', 'EEK', 'EGP',
            'ERN', 'ESA', 'ESB', 'ESP', 'ETB', 'EUR', 'FIM', 'FJD', 'FKP', 'FRF',
            'GBP', 'GEK', 'GEL', 'GHC', 'GHS', 'GIP', 'GMD', 'GNF', 'GNS', 'GQE',
            'GRD', 'GTQ', 'GWE', 'GWP', 'GYD', 'HKD', 'HNL', 'HRD', 'HRK', 'HTG',
            'HUF', 'IDR', 'IEP', 'ILP', 'ILR', 'ILS', 'INR', 'IQD', 'IRR', 'ISJ',
            'ISK', 'ITL', 'JMD', 'JOD', 'JPY', 'KES', 'KGS', 'KHR', 'KMF', 'KPW',
            'KRH', 'KRO', 'KRW', 'KWD', 'KYD', 'KZT', 'LAK', 'LBP', 'LKR', 'LRD',
            'LSL', 'LTL', 'LTT', 'LUC', 'LUF', 'LUL', 'LVL', 'LVR', 'LYD', 'MAD',
            'MAF', 'MCF', 'MDC', 'MDL', 'MGA', 'MGF', 'MKD', 'MKN', 'MLF', 'MMK',
            'MNT', 'MOP', 'MRO', 'MRU', 'MTL', 'MTP', 'MUR', 'MVP', 'MVR', 'MWK',
            'MXN', 'MXP', 'MXV', 'MYR', 'MZE', 'MZM', 'MZN', 'NAD', 'NGN', 'NIC',
            'NIO', 'NLG', 'NOK', 'NPR', 'NZD', 'OMR', 'PAB', 'PEI', 'PEN', 'PES',
            'PGK', 'PHP', 'PKR', 'PLN', 'PLZ', 'PTE', 'PYG', 'QAR', 'RHD', 'ROL',
            'RON', 'RSD', 'RUB', 'RUR', 'RWF', 'SAR', 'SBD', 'SCR', 'SDD', 'SDG',
            'SDP', 'SEK', 'SGD', 'SHP', 'SIT', 'SKK', 'SLE', 'SLL', 'SOS', 'SRD',
            'SRG', 'SSP', 'STD', 'STN', 'SUR', 'SVC', 'SYP', 'SZL', 'THB', 'TJR',
            'TJS', 'TMM', 'TMT', 'TND', 'TOP', 'TPE', 'TRL', 'TRY', 'TTD', 'TWD',
            'TZS', 'UAH', 'UAK', 'UGS', 'UGX', 'USD', 'USN', 'USS', 'UYI', 'UYP',
            'UYU', 'UYW', 'UZS', 'VEB', 'VED', 'VEF', 'VES', 'VND', 'VNN', 'VUV',
            'WST', 'XAF', 'XAG', 'XAU', 'XBA', 'XBB', 'XBC', 'XBD', 'XCD', 'XDR',
            'XEU', 'XFO', 'XFU', 'XOF', 'XPD', 'XPF', 'XPT', 'XRE', 'XSU', 'XTS',
            'XUA', 'XXX', 'YDD', 'YER', 'YUD', 'YUM', 'YUN', 'YUR', 'ZAL', 'ZAR',
            'ZMK', 'ZMW', 'ZRN', 'ZRZ', 'ZWD', 'ZWL', 'ZWR'
          ].sort();
          
          // Standard numbering systems with simple digit mappings (plus algorithmic)
          const numberingSystems = [
            'adlm', 'ahom', 'arab', 'arabext', 'armn', 'armnlow', 'bali', 'beng',
            'bhks', 'brah', 'cakm', 'cham', 'cyrl', 'deva', 'diak', 'ethi',
            'fullwide', 'gara', 'geor', 'gong', 'gonm', 'grek', 'greklow', 'gujr',
            'guru', 'hanidays', 'hanidec', 'hans', 'hansfin', 'hant', 'hantfin',
            'hebr', 'hmng', 'hmnp', 'java', 'jpan', 'jpanfin', 'jpanyear', 'kali',
            'kawi', 'khmr', 'knda', 'lana', 'lanatham', 'laoo', 'latn', 'lepc',
            'limb', 'mathbold', 'mathdbl', 'mathmono', 'mathsanb', 'mathsans',
            'mlym', 'modi', 'mong', 'mroo', 'mtei', 'mymr', 'mymrshan', 'mymrtlng',
            'nagm', 'newa', 'nkoo', 'olck', 'orya', 'osma', 'outlined', 'rohg',
            'roman', 'romanlow', 'saur', 'segment', 'shrd', 'sind', 'sinh', 'sora',
            'sund', 'sundlatn', 'takr', 'talu', 'taml', 'tamldec', 'telu', 'thai',
            'tibt', 'tirh', 'tnsa', 'vaii', 'wara', 'wcho'
          ].sort();
          
          // Time zones - including Etc/GMT+N zones per spec
          const timeZones = [
            'Africa/Abidjan', 'Africa/Accra', 'Africa/Addis_Ababa', 'Africa/Algiers',
            'Africa/Cairo', 'Africa/Casablanca', 'Africa/Johannesburg', 'Africa/Lagos',
            'Africa/Nairobi', 'Africa/Tunis', 'America/Adak', 'America/Anchorage',
            'America/Argentina/Buenos_Aires', 'America/Bogota', 'America/Caracas',
            'America/Chicago', 'America/Denver', 'America/Halifax', 'America/Lima',
            'America/Los_Angeles', 'America/Mexico_City', 'America/New_York',
            'America/Phoenix', 'America/Santiago', 'America/Sao_Paulo', 'America/St_Johns',
            'America/Toronto', 'America/Vancouver', 'Asia/Almaty', 'Asia/Baghdad',
            'Asia/Baku', 'Asia/Bangkok', 'Asia/Chongqing', 'Asia/Colombo', 'Asia/Dhaka',
            'Asia/Dubai', 'Asia/Ho_Chi_Minh', 'Asia/Hong_Kong', 'Asia/Istanbul',
            'Asia/Jakarta', 'Asia/Jerusalem', 'Asia/Kabul', 'Asia/Karachi',
            'Asia/Kathmandu', 'Asia/Kolkata', 'Asia/Kuala_Lumpur', 'Asia/Kuwait',
            'Asia/Manila', 'Asia/Rangoon', 'Asia/Riyadh', 'Asia/Seoul', 'Asia/Shanghai',
            'Asia/Singapore', 'Asia/Taipei', 'Asia/Tehran', 'Asia/Tokyo', 'Asia/Vladivostok',
            'Atlantic/Azores', 'Atlantic/Canary', 'Atlantic/Reykjavik',
            'Australia/Adelaide', 'Australia/Brisbane', 'Australia/Darwin',
            'Australia/Hobart', 'Australia/Melbourne', 'Australia/Perth', 'Australia/Sydney',
            'Etc/GMT', 'Etc/GMT+0', 'Etc/GMT+1', 'Etc/GMT+10', 'Etc/GMT+11', 'Etc/GMT+12',
            'Etc/GMT+2', 'Etc/GMT+3', 'Etc/GMT+4', 'Etc/GMT+5', 'Etc/GMT+6', 'Etc/GMT+7',
            'Etc/GMT+8', 'Etc/GMT+9', 'Etc/GMT-0', 'Etc/GMT-1', 'Etc/GMT-10', 'Etc/GMT-11',
            'Etc/GMT-12', 'Etc/GMT-13', 'Etc/GMT-14', 'Etc/GMT-2', 'Etc/GMT-3', 'Etc/GMT-4',
            'Etc/GMT-5', 'Etc/GMT-6', 'Etc/GMT-7', 'Etc/GMT-8', 'Etc/GMT-9', 'Etc/GMT0',
            'Etc/UTC', 'Europe/Amsterdam', 'Europe/Athens', 'Europe/Belgrade',
            'Europe/Berlin', 'Europe/Brussels', 'Europe/Bucharest', 'Europe/Budapest',
            'Europe/Copenhagen', 'Europe/Dublin', 'Europe/Helsinki', 'Europe/Kiev',
            'Europe/Lisbon', 'Europe/London', 'Europe/Madrid', 'Europe/Moscow',
            'Europe/Oslo', 'Europe/Paris', 'Europe/Prague', 'Europe/Rome', 'Europe/Sofia',
            'Europe/Stockholm', 'Europe/Vienna', 'Europe/Warsaw', 'Europe/Zurich',
            'Pacific/Auckland', 'Pacific/Fiji', 'Pacific/Guam', 'Pacific/Honolulu',
            'Pacific/Kiritimati', 'Pacific/Midway', 'Pacific/Noumea', 'Pacific/Pago_Pago',
            'Pacific/Tahiti', 'UTC'
          ].sort();
          
          // Standard units for NumberFormat
          const units = [
            'acre', 'bit', 'byte', 'celsius', 'centimeter', 'day', 'degree',
            'fahrenheit', 'fluid-ounce', 'foot', 'gallon', 'gigabit', 'gigabyte',
            'gram', 'hectare', 'hour', 'inch', 'kilobit', 'kilobyte', 'kilogram',
            'kilometer', 'liter', 'megabit', 'megabyte', 'meter', 'microsecond',
            'mile', 'mile-scandinavian', 'milliliter', 'millimeter', 'millisecond',
            'minute', 'month', 'nanosecond', 'ounce', 'percent', 'petabyte', 'pound',
            'second', 'stone', 'terabit', 'terabyte', 'week', 'yard', 'year'
          ].sort();
          
          function supportedValuesOf(key) {
            // Throw TypeError for Symbol
            if (typeof key === 'symbol') {
              throw new TypeError('Cannot convert a Symbol value to a string');
            }
            const keyStr = String(key);
            switch (keyStr) {
              case 'calendar':
                return calendars.slice();
              case 'collation':
                return collations.slice();
              case 'currency':
                return currencies.slice();
              case 'numberingSystem':
                return numberingSystems.slice();
              case 'timeZone':
                return timeZones.slice();
              case 'unit':
                return units.slice();
              default:
                throw new RangeError('Invalid key: ' + keyStr);
            }
          }
          
          Object.defineProperty(supportedValuesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(supportedValuesOf, 'name', {
            value: 'supportedValuesOf',
            writable: false,
            enumerable: false,
            configurable: true
          });
          
          Object.defineProperty(Intl, 'supportedValuesOf', {
            value: supportedValuesOf,
            writable: true,
            enumerable: false,
            configurable: true
          });
        })();
        "#,
    ))?;
    Ok(())
}
