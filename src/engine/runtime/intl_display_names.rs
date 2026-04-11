fn install_intl_display_names_builtin(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl !== 'object' || Intl === null) {
            return;
          }
          if (typeof Intl.DisplayNames === 'function') {
            return;
          }
          if (typeof Intl.getCanonicalLocales !== 'function') {
            return;
          }

          const displayNamesSlots = new WeakMap();
          const VALID_LOCALE_MATCHERS = new Set(['lookup', 'best fit']);
          const VALID_STYLES = new Set(['narrow', 'short', 'long']);
          const VALID_TYPES = new Set(['language', 'region', 'script', 'currency', 'calendar', 'dateTimeField']);
          const VALID_FALLBACKS = new Set(['code', 'none']);
          const VALID_LANGUAGE_DISPLAYS = new Set(['dialect', 'standard']);
          const VALID_DATE_TIME_FIELDS = new Set([
            'era',
            'year',
            'quarter',
            'month',
            'weekOfYear',
            'weekday',
            'day',
            'dayPeriod',
            'hour',
            'minute',
            'second',
            'timeZoneName',
          ]);

          const isObjectLike = (value) =>
            (typeof value === 'object' && value !== null) || typeof value === 'function';

          const defaultLocale = () => {
            const formatter = new Intl.NumberFormat();
            if (typeof formatter.resolvedOptions === 'function') {
              const resolved = formatter.resolvedOptions();
              if (resolved && typeof resolved.locale === 'string' && resolved.locale.length > 0) {
                return resolved.locale;
              }
            }
            return 'en-US';
          };

          function getOption(options, property, allowedValues, fallback) {
            const value = options[property];
            if (value === undefined) {
              return fallback;
            }
            if (typeof value === 'symbol') {
              throw new TypeError('Cannot convert symbol to string');
            }
            const stringValue = String(value);
            if (allowedValues !== undefined && !allowedValues.has(stringValue)) {
              throw new RangeError('Invalid value for option ' + property);
            }
            return stringValue;
          }

          function canonicalizeRequestedLocales(locales) {
            if (locales === undefined) {
              return [];
            }
            return Intl.getCanonicalLocales(locales);
          }

          function canonicalCodeForDisplayNames(type, code) {
            switch (type) {
              case 'language': {
                if (typeof code !== 'string' || code.length === 0) {
                  throw new RangeError('Invalid language code');
                }
                if (
                  !/^[A-Za-z]{2,8}(?:-[A-Za-z]{4})?(?:-(?:[A-Za-z]{2}|[0-9]{3}))?(?:-(?:[A-Za-z0-9]{5,8}|[0-9][A-Za-z0-9]{3}))*$/.test(
                    code
                  )
                ) {
                  throw new RangeError('Invalid language code');
                }
                if (/^root(?:-|$)/i.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                if (/^[A-Za-z]{4}(?:-|$)/.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                if (/-u(?:-|$)/i.test(code)) {
                  throw new RangeError('Invalid language code');
                }
                const segments = code.split('-');
                const normalizedSegments = [segments[0].toLowerCase()];
                let index = 1;

                if (index < segments.length && /^[A-Za-z]{4}$/.test(segments[index])) {
                  normalizedSegments.push(
                    segments[index].charAt(0).toUpperCase() + segments[index].slice(1).toLowerCase()
                  );
                  index += 1;
                }
                if (
                  index < segments.length &&
                  /^(?:[A-Za-z]{2}|[0-9]{3})$/.test(segments[index])
                ) {
                  normalizedSegments.push(segments[index].toUpperCase());
                  index += 1;
                }

                const seenVariants = new Set();
                for (; index < segments.length; index += 1) {
                  const variant = segments[index].toLowerCase();
                  if (seenVariants.has(variant)) {
                    throw new RangeError('Invalid language code');
                  }
                  seenVariants.add(variant);
                  normalizedSegments.push(variant);
                }
                return normalizedSegments.join('-');
              }
              case 'region':
                if (/^(?:[A-Za-z]{2}|[0-9]{3})$/.test(code)) {
                  return code.toUpperCase();
                }
                break;
              case 'script':
                if (/^[A-Za-z]{4}$/.test(code)) {
                  return code.charAt(0).toUpperCase() + code.slice(1).toLowerCase();
                }
                break;
              case 'currency':
                if (/^[A-Za-z]{3}$/.test(code)) {
                  return code.toUpperCase();
                }
                break;
              case 'calendar':
                if (/^[A-Za-z0-9]{3,8}(?:-[A-Za-z0-9]{3,8})*$/.test(code)) {
                  return code.toLowerCase();
                }
                break;
              case 'dateTimeField':
                if (VALID_DATE_TIME_FIELDS.has(code)) {
                  return code;
                }
                break;
            }
            throw new RangeError('Invalid code for Intl.DisplayNames');
          }

          function getIntrinsicDisplayNamesPrototype(newTarget) {
            if (newTarget === undefined || newTarget === DisplayNames) {
              return DisplayNames.prototype;
            }
            const proto = newTarget.prototype;
            if (isObjectLike(proto)) {
              return proto;
            }
            try {
              const otherGlobal = newTarget && newTarget.constructor && newTarget.constructor('return this')();
              const otherIntl = otherGlobal && otherGlobal.Intl;
              const otherDisplayNames = otherIntl && otherIntl.DisplayNames;
              const otherProto = otherDisplayNames && otherDisplayNames.prototype;
              if (isObjectLike(otherProto)) {
                return otherProto;
              }
            } catch {}
            return DisplayNames.prototype;
          }

          function DisplayNames(locales, options) {
            if (new.target === undefined) {
              throw new TypeError('Intl.DisplayNames must be called with new');
            }

            const proto = getIntrinsicDisplayNamesPrototype(new.target);
            const displayNames = Object.create(proto);
            const requestedLocales = canonicalizeRequestedLocales(locales);
            const locale = requestedLocales.length > 0 ? requestedLocales[0] : defaultLocale();

            let normalizedOptions;
            if (options === undefined) {
              normalizedOptions = Object.create(null);
            } else {
              if (!isObjectLike(options)) {
                throw new TypeError('Intl.DisplayNames options must be an object');
              }
              normalizedOptions = options;
            }

            getOption(normalizedOptions, 'localeMatcher', VALID_LOCALE_MATCHERS, 'best fit');
            const style = getOption(normalizedOptions, 'style', VALID_STYLES, 'long');
            const type = getOption(normalizedOptions, 'type', VALID_TYPES, undefined);
            if (type === undefined) {
              throw new TypeError('Intl.DisplayNames type option is required');
            }
            const fallback = getOption(normalizedOptions, 'fallback', VALID_FALLBACKS, 'code');
            const languageDisplay = getOption(
              normalizedOptions,
              'languageDisplay',
              VALID_LANGUAGE_DISPLAYS,
              'dialect'
            );

            displayNamesSlots.set(displayNames, {
              locale,
              style,
              type,
              fallback,
              languageDisplay: type === 'language' ? languageDisplay : undefined,
            });
            return displayNames;
          }

          Object.defineProperty(DisplayNames, 'length', {
            value: 2,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(DisplayNames, 'name', {
            value: 'DisplayNames',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const displayNamesPrototype = Object.create(Object.prototype);

          const ofFn = new Proxy(() => {}, {
            apply(_target, thisArg, args) {
              if (!isObjectLike(thisArg) || !displayNamesSlots.has(thisArg)) {
                throw new TypeError('Intl.DisplayNames.prototype.of called on incompatible receiver');
              }
              const code = args.length > 0 ? String(args[0]) : String(undefined);
              const slot = displayNamesSlots.get(thisArg);
              return canonicalCodeForDisplayNames(slot.type, code);
            },
          });
          Object.defineProperty(ofFn, 'name', {
            value: 'of',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(ofFn, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          const resolvedOptionsFn = new Proxy(() => {}, {
            apply(_target, thisArg) {
              if (!isObjectLike(thisArg) || !displayNamesSlots.has(thisArg)) {
                throw new TypeError(
                  'Intl.DisplayNames.prototype.resolvedOptions called on incompatible receiver'
                );
              }
              const slot = displayNamesSlots.get(thisArg);
              const options = {
                locale: slot.locale,
                style: slot.style,
                type: slot.type,
                fallback: slot.fallback,
              };
              if (slot.languageDisplay !== undefined) {
                options.languageDisplay = slot.languageDisplay;
              }
              return options;
            },
          });
          Object.defineProperty(resolvedOptionsFn, 'name', {
            value: 'resolvedOptions',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(resolvedOptionsFn, 'length', {
            value: 0,
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(displayNamesPrototype, 'constructor', {
            value: DisplayNames,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, 'of', {
            value: ofFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, 'resolvedOptions', {
            value: resolvedOptionsFn,
            writable: true,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(displayNamesPrototype, Symbol.toStringTag, {
            value: 'Intl.DisplayNames',
            writable: false,
            enumerable: false,
            configurable: true,
          });

          Object.defineProperty(DisplayNames, 'prototype', {
            value: displayNamesPrototype,
            writable: false,
            enumerable: false,
            configurable: false,
          });

          Object.defineProperty(Intl, 'DisplayNames', {
            value: DisplayNames,
            writable: true,
            enumerable: false,
            configurable: true,
          });
        })();
        "#,
    ))?;
    Ok(())
}

