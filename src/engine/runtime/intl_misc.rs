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
