fn install_intl_date_time_format_polyfill(context: &mut Context) -> JsResult<()> {
    context.eval(Source::from_bytes(
        r#"
        (() => {
          if (typeof Intl !== 'object' || Intl === null) return;
          if (typeof Intl.DateTimeFormat !== 'function') return;

          const DTF = Intl.DateTimeFormat;
          const proto = DTF.prototype;

          // Store original format function if exists
          const originalFormat = proto.format;

          // Internal slot storage
          const dtfSlots = new WeakMap();

          // Valid option values per spec
          const VALID_LOCALE_MATCHERS = ['lookup', 'best fit'];
          const VALID_FORMAT_MATCHERS = ['basic', 'best fit'];
          const VALID_CALENDARS = [
            'buddhist', 'chinese', 'coptic', 'dangi', 'ethioaa', 'ethiopic',
            'gregory', 'hebrew', 'indian', 'islamic', 'islamic-umalqura',
            'islamic-tbla', 'islamic-civil', 'islamic-rgsa', 'iso8601',
            'japanese', 'persian', 'roc', 'islamicc'
          ];
          const VALID_NUMBERING_SYSTEMS = [
            'arab', 'arabext', 'bali', 'beng', 'deva', 'fullwide', 'gujr',
            'guru', 'hanidec', 'khmr', 'knda', 'laoo', 'latn', 'limb',
            'mlym', 'mong', 'mymr', 'orya', 'tamldec', 'telu', 'thai', 'tibt'
          ];
          const VALID_HOUR_CYCLES = ['h11', 'h12', 'h23', 'h24'];
          const VALID_TIME_ZONES = ['UTC'];
          const VALID_WEEKDAYS = ['narrow', 'short', 'long'];
          const VALID_ERAS = ['narrow', 'short', 'long'];
          const VALID_YEARS = ['2-digit', 'numeric'];
          const VALID_MONTHS = ['2-digit', 'numeric', 'narrow', 'short', 'long'];
          const VALID_DAYS = ['2-digit', 'numeric'];
          const VALID_DAY_PERIODS = ['narrow', 'short', 'long'];
          const VALID_HOURS = ['2-digit', 'numeric'];
          const VALID_MINUTES = ['2-digit', 'numeric'];
          const VALID_SECONDS = ['2-digit', 'numeric'];
          const VALID_FRACTIONAL_SECOND_DIGITS = [1, 2, 3];
          const VALID_TIME_ZONE_NAMES = ['short', 'long', 'shortOffset', 'longOffset', 'shortGeneric', 'longGeneric'];
          const VALID_DATE_STYLES = ['full', 'long', 'medium', 'short'];
          const VALID_TIME_STYLES = ['full', 'long', 'medium', 'short'];

          const CALENDAR_ALIASES = {
            'islamicc': 'islamic-civil',
            'ethiopic-amete-alem': 'ethioaa',
          };

          function canonicalizeCalendar(cal) {
            if (typeof cal !== 'string') return undefined;
            const lower = cal.toLowerCase();
            if (CALENDAR_ALIASES[lower]) return CALENDAR_ALIASES[lower];
            // Check if it has invalid uppercase characters (like capital dotted I)
            if (/[\u0130\u0131]/.test(cal)) {
              throw new RangeError('Invalid calendar');
            }
            return lower;
          }

          // Java/legacy non-IANA timezone IDs that must be rejected
          const LEGACY_NON_IANA_TZ = new Set([
            'ACT','AET','AGT','ART','AST','BET','BST','CAT','CNT','CST','CTT',
            'EAT','ECT','IET','IST','JST','MIT','NET','NST','PLT','PNT','PRT',
            'PST','SST','VST',
          ]);

          // Build case-insensitive IANA timezone lookup (lazy, first use)
          let _ianaLookup = null;
          function getIanaLookup() {
            if (_ianaLookup !== null) return _ianaLookup;
            _ianaLookup = new Map();
            // All IANA timezone IDs from test262 hardcoded list (primary + link names)
            const extra = [
              'Africa/Abidjan','Africa/Algiers','Africa/Bissau','Africa/Cairo','Africa/Casablanca',
              'Africa/Ceuta','Africa/El_Aaiun','Africa/Johannesburg','Africa/Juba','Africa/Khartoum',
              'Africa/Lagos','Africa/Maputo','Africa/Monrovia','Africa/Nairobi','Africa/Ndjamena',
              'Africa/Sao_Tome','Africa/Tripoli','Africa/Tunis','Africa/Windhoek',
              'Africa/Accra','Africa/Addis_Ababa','Africa/Asmara','Africa/Asmera','Africa/Bamako',
              'Africa/Bangui','Africa/Banjul','Africa/Blantyre','Africa/Brazzaville','Africa/Bujumbura',
              'Africa/Conakry','Africa/Dakar','Africa/Dar_es_Salaam','Africa/Djibouti','Africa/Douala',
              'Africa/Freetown','Africa/Gaborone','Africa/Harare','Africa/Kampala','Africa/Kigali',
              'Africa/Kinshasa','Africa/Libreville','Africa/Lome','Africa/Luanda','Africa/Lubumbashi',
              'Africa/Lusaka','Africa/Malabo','Africa/Maseru','Africa/Mbabane','Africa/Mogadishu',
              'Africa/Niamey','Africa/Nouakchott','Africa/Ouagadougou','Africa/Porto-Novo','Africa/Timbuktu',
              'America/Adak','America/Anchorage','America/Araguaina',
              'America/Argentina/Buenos_Aires','America/Argentina/Catamarca','America/Argentina/Cordoba',
              'America/Argentina/Jujuy','America/Argentina/La_Rioja','America/Argentina/Mendoza',
              'America/Argentina/Rio_Gallegos','America/Argentina/Salta','America/Argentina/San_Juan',
              'America/Argentina/San_Luis','America/Argentina/Tucuman','America/Argentina/Ushuaia',
              'America/Asuncion','America/Bahia','America/Bahia_Banderas','America/Barbados',
              'America/Belem','America/Belize','America/Boa_Vista','America/Bogota','America/Boise',
              'America/Cambridge_Bay','America/Campo_Grande','America/Cancun','America/Caracas',
              'America/Cayenne','America/Chicago','America/Chihuahua','America/Costa_Rica',
              'America/Cuiaba','America/Danmarkshavn','America/Dawson','America/Dawson_Creek',
              'America/Denver','America/Detroit','America/Edmonton','America/Eirunepe',
              'America/El_Salvador','America/Fort_Nelson','America/Fortaleza','America/Glace_Bay',
              'America/Goose_Bay','America/Grand_Turk','America/Guatemala','America/Guayaquil',
              'America/Guyana','America/Halifax','America/Havana','America/Hermosillo',
              'America/Indiana/Indianapolis','America/Indiana/Knox','America/Indiana/Marengo',
              'America/Indiana/Petersburg','America/Indiana/Tell_City','America/Indiana/Vevay',
              'America/Indiana/Vincennes','America/Indiana/Winamac','America/Inuvik','America/Iqaluit',
              'America/Jamaica','America/Juneau','America/Kentucky/Louisville','America/Kentucky/Monticello',
              'America/La_Paz','America/Lima','America/Los_Angeles','America/Maceio','America/Managua',
              'America/Manaus','America/Martinique','America/Matamoros','America/Mazatlan',
              'America/Menominee','America/Merida','America/Metlakatla','America/Mexico_City',
              'America/Miquelon','America/Moncton','America/Monterrey','America/Montevideo',
              'America/New_York','America/Nome','America/Noronha',
              'America/North_Dakota/Beulah','America/North_Dakota/Center','America/North_Dakota/New_Salem',
              'America/Nuuk','America/Ojinaga','America/Panama','America/Paramaribo','America/Phoenix',
              'America/Port-au-Prince','America/Porto_Velho','America/Puerto_Rico','America/Punta_Arenas',
              'America/Rankin_Inlet','America/Recife','America/Regina','America/Resolute',
              'America/Rio_Branco','America/Santarem','America/Santiago','America/Santo_Domingo',
              'America/Sao_Paulo','America/Scoresbysund','America/Sitka','America/St_Johns',
              'America/Swift_Current','America/Tegucigalpa','America/Thule','America/Tijuana',
              'America/Toronto','America/Vancouver','America/Whitehorse','America/Winnipeg',
              'America/Yakutat','America/Yellowknife',
              'America/Anguilla','America/Antigua','America/Argentina/ComodRivadavia','America/Aruba',
              'America/Atikokan','America/Atka','America/Blanc-Sablon','America/Buenos_Aires',
              'America/Catamarca','America/Cayman','America/Coral_Harbour','America/Cordoba',
              'America/Creston','America/Curacao','America/Dominica','America/Ensenada',
              'America/Fort_Wayne','America/Godthab','America/Grenada','America/Guadeloupe',
              'America/Indianapolis','America/Jujuy','America/Knox_IN','America/Kralendijk',
              'America/Louisville','America/Lower_Princes','America/Marigot','America/Mendoza',
              'America/Montreal','America/Montserrat','America/Nassau','America/Nipigon',
              'America/Pangnirtung','America/Port_of_Spain','America/Porto_Acre','America/Rainy_River',
              'America/Rosario','America/Santa_Isabel','America/Shiprock','America/St_Barthelemy',
              'America/St_Kitts','America/St_Lucia','America/St_Thomas','America/St_Vincent',
              'America/Thunder_Bay','America/Tortola','America/Virgin',
              'Antarctica/Casey','Antarctica/Davis','Antarctica/Macquarie','Antarctica/Mawson',
              'Antarctica/Palmer','Antarctica/Rothera','Antarctica/Troll',
              'Antarctica/DumontDUrville','Antarctica/McMurdo','Antarctica/South_Pole',
              'Antarctica/Syowa','Antarctica/Vostok',
              'Arctic/Longyearbyen',
              'Asia/Almaty','Asia/Amman','Asia/Anadyr','Asia/Aqtau','Asia/Aqtobe','Asia/Ashgabat',
              'Asia/Atyrau','Asia/Baghdad','Asia/Baku','Asia/Bangkok','Asia/Barnaul','Asia/Beirut',
              'Asia/Bishkek','Asia/Chita','Asia/Choibalsan','Asia/Colombo','Asia/Damascus',
              'Asia/Dhaka','Asia/Dili','Asia/Dubai','Asia/Dushanbe','Asia/Famagusta','Asia/Gaza',
              'Asia/Hebron','Asia/Ho_Chi_Minh','Asia/Hong_Kong','Asia/Hovd','Asia/Irkutsk',
              'Asia/Jakarta','Asia/Jayapura','Asia/Jerusalem','Asia/Kabul','Asia/Kamchatka',
              'Asia/Karachi','Asia/Kathmandu','Asia/Khandyga','Asia/Kolkata','Asia/Krasnoyarsk',
              'Asia/Kuching','Asia/Macau','Asia/Magadan','Asia/Makassar','Asia/Manila',
              'Asia/Nicosia','Asia/Novokuznetsk','Asia/Novosibirsk','Asia/Omsk','Asia/Oral',
              'Asia/Pontianak','Asia/Pyongyang','Asia/Qatar','Asia/Qostanay','Asia/Qyzylorda',
              'Asia/Riyadh','Asia/Sakhalin','Asia/Samarkand','Asia/Seoul','Asia/Shanghai',
              'Asia/Singapore','Asia/Srednekolymsk','Asia/Taipei','Asia/Tashkent','Asia/Tbilisi',
              'Asia/Tehran','Asia/Thimphu','Asia/Tokyo','Asia/Tomsk','Asia/Ulaanbaatar',
              'Asia/Urumqi','Asia/Ust-Nera','Asia/Vladivostok','Asia/Yakutsk','Asia/Yangon',
              'Asia/Yekaterinburg','Asia/Yerevan',
              'Asia/Aden','Asia/Ashkhabad','Asia/Bahrain','Asia/Brunei','Asia/Calcutta',
              'Asia/Chongqing','Asia/Chungking','Asia/Dacca','Asia/Harbin','Asia/Istanbul',
              'Asia/Kashgar','Asia/Katmandu','Asia/Kuala_Lumpur','Asia/Kuwait','Asia/Macao',
              'Asia/Muscat','Asia/Phnom_Penh','Asia/Rangoon','Asia/Saigon','Asia/Tel_Aviv',
              'Asia/Thimbu','Asia/Ujung_Pandang','Asia/Ulan_Bator','Asia/Vientiane',
              'Atlantic/Azores','Atlantic/Bermuda','Atlantic/Canary','Atlantic/Cape_Verde',
              'Atlantic/Faroe','Atlantic/Madeira','Atlantic/South_Georgia','Atlantic/Stanley',
              'Atlantic/Faeroe','Atlantic/Jan_Mayen','Atlantic/Reykjavik','Atlantic/St_Helena',
              'Australia/Adelaide','Australia/Brisbane','Australia/Broken_Hill','Australia/Darwin',
              'Australia/Eucla','Australia/Hobart','Australia/Lindeman','Australia/Lord_Howe',
              'Australia/Melbourne','Australia/Perth','Australia/Sydney',
              'Australia/ACT','Australia/Canberra','Australia/Currie','Australia/LHI',
              'Australia/NSW','Australia/North','Australia/Queensland','Australia/South',
              'Australia/Tasmania','Australia/Victoria','Australia/West','Australia/Yancowinna',
              'CET','CST6CDT','EET','EST','EST5EDT','HST','MET','MST','MST7MDT','PST8PDT','WET',
              'Etc/GMT','Etc/GMT+1','Etc/GMT+2','Etc/GMT+3','Etc/GMT+4','Etc/GMT+5','Etc/GMT+6',
              'Etc/GMT+7','Etc/GMT+8','Etc/GMT+9','Etc/GMT+10','Etc/GMT+11','Etc/GMT+12',
              'Etc/GMT-1','Etc/GMT-2','Etc/GMT-3','Etc/GMT-4','Etc/GMT-5','Etc/GMT-6',
              'Etc/GMT-7','Etc/GMT-8','Etc/GMT-9','Etc/GMT-10','Etc/GMT-11','Etc/GMT-12',
              'Etc/GMT-13','Etc/GMT-14','Etc/UTC',
              'Etc/GMT+0','Etc/GMT-0','Etc/GMT0','Etc/Greenwich','Etc/UCT','Etc/Universal','Etc/Zulu',
              'Europe/Andorra','Europe/Astrakhan','Europe/Athens','Europe/Belgrade','Europe/Berlin',
              'Europe/Brussels','Europe/Bucharest','Europe/Budapest','Europe/Chisinau','Europe/Dublin',
              'Europe/Gibraltar','Europe/Helsinki','Europe/Istanbul','Europe/Kaliningrad',
              'Europe/Kirov','Europe/Kyiv','Europe/Lisbon','Europe/London','Europe/Madrid',
              'Europe/Malta','Europe/Minsk','Europe/Moscow','Europe/Paris','Europe/Prague',
              'Europe/Riga','Europe/Rome','Europe/Samara','Europe/Saratov','Europe/Simferopol',
              'Europe/Sofia','Europe/Tallinn','Europe/Tirane','Europe/Ulyanovsk','Europe/Vienna',
              'Europe/Vilnius','Europe/Volgograd','Europe/Warsaw','Europe/Zurich',
              'Europe/Amsterdam','Europe/Belfast','Europe/Bratislava','Europe/Busingen',
              'Europe/Copenhagen','Europe/Guernsey','Europe/Isle_of_Man','Europe/Jersey',
              'Europe/Kiev','Europe/Ljubljana','Europe/Luxembourg','Europe/Mariehamn',
              'Europe/Monaco','Europe/Nicosia','Europe/Oslo','Europe/Podgorica',
              'Europe/San_Marino','Europe/Sarajevo','Europe/Skopje','Europe/Stockholm',
              'Europe/Tiraspol','Europe/Uzhgorod','Europe/Vaduz','Europe/Vatican',
              'Europe/Zagreb','Europe/Zaporozhye',
              'Indian/Chagos','Indian/Maldives','Indian/Mauritius',
              'Indian/Antananarivo','Indian/Christmas','Indian/Cocos','Indian/Comoro',
              'Indian/Kerguelen','Indian/Mahe','Indian/Mayotte','Indian/Reunion',
              'Pacific/Apia','Pacific/Auckland','Pacific/Bougainville','Pacific/Chatham',
              'Pacific/Easter','Pacific/Efate','Pacific/Fakaofo','Pacific/Fiji','Pacific/Galapagos',
              'Pacific/Gambier','Pacific/Guadalcanal','Pacific/Guam','Pacific/Honolulu',
              'Pacific/Kanton','Pacific/Kiritimati','Pacific/Kosrae','Pacific/Kwajalein',
              'Pacific/Marquesas','Pacific/Nauru','Pacific/Niue','Pacific/Norfolk','Pacific/Noumea',
              'Pacific/Pago_Pago','Pacific/Palau','Pacific/Pitcairn','Pacific/Port_Moresby',
              'Pacific/Rarotonga','Pacific/Tahiti','Pacific/Tarawa','Pacific/Tongatapu',
              'Pacific/Chuuk','Pacific/Enderbury','Pacific/Funafuti','Pacific/Johnston',
              'Pacific/Majuro','Pacific/Midway','Pacific/Pohnpei','Pacific/Ponape',
              'Pacific/Saipan','Pacific/Samoa','Pacific/Truk','Pacific/Wake','Pacific/Wallis','Pacific/Yap',
              'Cuba','Egypt','Eire','GB','GB-Eire','GMT','GMT+0','GMT-0','GMT0','Greenwich',
              'Hongkong','Iceland','Iran','Israel','Jamaica','Japan','Kwajalein','Libya',
              'NZ','NZ-CHAT','Navajo','PRC','Poland','Portugal','ROC','ROK','Singapore',
              'Turkey','UCT','UTC','Universal','W-SU','Zulu',
              'Brazil/Acre','Brazil/DeNoronha','Brazil/East','Brazil/West',
              'Canada/Atlantic','Canada/Central','Canada/Eastern','Canada/Mountain',
              'Canada/Newfoundland','Canada/Pacific','Canada/Saskatchewan','Canada/Yukon',
              'Chile/Continental','Chile/EasterIsland',
              'Mexico/BajaNorte','Mexico/BajaSur','Mexico/General',
              'US/Alaska','US/Aleutian','US/Arizona','US/Central','US/East-Indiana',
              'US/Eastern','US/Hawaii','US/Indiana-Starke','US/Michigan','US/Mountain',
              'US/Pacific','US/Samoa',
            ];
            for (const id of extra) _ianaLookup.set(id.toUpperCase(), id);
            try {
              for (const id of Intl.supportedValuesOf('timeZone')) {
                _ianaLookup.set(id.toUpperCase(), id);
              }
            } catch (_e) {}
            return _ianaLookup;
          }

          function canonicalizeTimeZone(tz) {
            if (typeof tz !== 'string') return undefined;
            // Reject empty string
            if (tz.length === 0) throw new RangeError('Invalid time zone: ' + tz);
            // Reject non-ASCII characters
            if (/[^\x00-\x7F]/.test(tz)) throw new RangeError('Invalid time zone: ' + tz);
            // Reject Unicode minus sign (U+2212)
            if (tz.includes('\u2212')) throw new RangeError('Invalid time zone: ' + tz);

            const upper = tz.toUpperCase();

            // Offset timezones
            if (/^[+-]/.test(tz)) {
              const sign = tz[0];
              const rest = tz.slice(1);
              let hours, minutes;
              if (/^\d{2}$/.test(rest)) { hours = parseInt(rest, 10); minutes = 0; }
              else if (/^\d{4}$/.test(rest)) { hours = parseInt(rest.slice(0, 2), 10); minutes = parseInt(rest.slice(2), 10); }
              else if (/^\d{2}:\d{2}$/.test(rest)) { hours = parseInt(rest.slice(0, 2), 10); minutes = parseInt(rest.slice(3), 10); }
              else throw new RangeError('Invalid time zone: ' + tz);
              if (hours > 23 || minutes > 59) throw new RangeError('Invalid time zone: ' + tz);
              const normalizedSign = (sign === '-' && hours === 0 && minutes === 0) ? '+' : sign;
              return normalizedSign + String(hours).padStart(2, '0') + ':' + String(minutes).padStart(2, '0');
            }

            // Reject Java-style legacy non-IANA names
            if (LEGACY_NON_IANA_TZ.has(upper)) throw new RangeError('Invalid time zone: ' + tz);

            // Well-known aliases
            if (upper === 'ETC/GMT') return 'Etc/GMT';
            if (upper === 'ETC/UTC') return 'Etc/UTC';
            if (upper === 'GMT') return 'GMT';
            if (upper === 'UTC') return 'UTC';

            // Case-normalize using IANA lookup
            const lookup = getIanaLookup();
            if (lookup.has(upper)) return lookup.get(upper);

            // For names with a slash: accept as link name (e.g. Asia/Calcutta)
            if (tz.includes('/')) {
              if (/[ ]/.test(tz)) throw new RangeError('Invalid time zone: ' + tz);
              return tz;
            }

            // Single-word names not in lookup are invalid
            throw new RangeError('Invalid time zone: ' + tz);
          }

          // Strip unicode extension keys not valid for DateTimeFormat (ca, nu, hc are valid; tz stripped — CLDR tz values unvalidatable)
          // Also strip keys whose values are not valid for that key.
          function stripInvalidDTFUnicodeExtKeys(locale) {
            if (typeof locale !== 'string') return locale;
            const validKeyValues = {
              ca: VALID_CALENDARS,
              nu: VALID_NUMBERING_SYSTEMS,
              hc: VALID_HOUR_CYCLES,
            };
            return locale.replace(/-u(-[a-z0-9]{2,8})+/gi, (match) => {
              const tokens = match.slice(3).split('-');
              const kept = [];
              let i = 0;
              while (i < tokens.length) {
                const tok = tokens[i].toLowerCase();
                if (tok.length === 2) {
                  const vals = [];
                  let j = i + 1;
                  while (j < tokens.length && tokens[j].length !== 2) { vals.push(tokens[j]); j++; }
                  if (Object.prototype.hasOwnProperty.call(validKeyValues, tok)) {
                    const allowed = validKeyValues[tok];
                    const valStr = vals.join('-').toLowerCase();
                    if (allowed.includes(valStr)) kept.push(tok, ...vals);
                  }
                  i = j;
                } else { i++; }
              }
              return kept.length > 0 ? '-u-' + kept.join('-') : '';
            });
          }

          const _hasOwn = Object.prototype.hasOwnProperty;
          const _getOwnPropDesc = Object.getOwnPropertyDescriptor;

          function getOwnOptionValue(options, property) {
            const desc = _getOwnPropDesc(options, property);
            if (desc === undefined) {
              // Fall back to prototype chain read (spec: Get(options, property))
              return options[property];
            }
            // Accessor descriptor: call the getter (propagates exceptions)
            if (typeof desc.get === 'function') return desc.get.call(options);
            return desc.value;
          }

          function getOption(options, property, type, values, fallback) {
            let value = getOwnOptionValue(options, property);
            if (value === undefined) return fallback;
            if (type === 'boolean') {
              value = Boolean(value);
            } else if (type === 'string') {
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
            let value = getOwnOptionValue(options, property);
            if (value === undefined) return fallback;
            value = Number(value);
            if (!Number.isFinite(value) || value < minimum || value > maximum) {
              throw new RangeError('Invalid ' + property);
            }
            return Math.floor(value);
          }

          // OrdinaryHasInstance implementation that doesn't use Symbol.hasInstance
          function ordinaryHasInstance(C, O) {
            if (typeof C !== 'function') return false;
            if (typeof O !== 'object' || O === null) return false;
            const P = C.prototype;
            if (typeof P !== 'object' || P === null) {
              throw new TypeError('Function has non-object prototype in instanceof check');
            }
            // Walk the prototype chain
            let proto = Object.getPrototypeOf(O);
            while (proto !== null) {
              if (proto === P) return true;
              proto = Object.getPrototypeOf(proto);
            }
            return false;
          }

          function getFunctionRealmGlobal(func) {
            if (typeof func !== 'function') return undefined;
            try {
              const ctor = Object.getPrototypeOf(func)?.constructor;
              if (typeof ctor !== 'function') return undefined;
              return ctor('return this')();
            } catch (_e) {
              return undefined;
            }
          }

          function getIntrinsicDateTimeFormatPrototype(newTarget) {
            const candidate = newTarget?.prototype;
            if (typeof candidate === 'object' && candidate !== null) {
              return candidate;
            }
            const realmGlobal = getFunctionRealmGlobal(newTarget);
            const realmProto = realmGlobal?.Intl?.DateTimeFormat?.prototype;
            if (typeof realmProto === 'object' && realmProto !== null) {
              return realmProto;
            }
            return newProto;
          }

          function applyCalendarFallback(cal) {
            if (cal === 'islamic' || cal === 'islamic-rgsa') {
              return 'islamic-civil';
            }
            return cal;
          }

          // Wrap the constructor to capture options
          const WrappedDTF = function DateTimeFormat(locales, options) {
            const isConstructCall = new.target !== undefined;

            // Use OrdinaryHasInstance instead of instanceof to avoid Symbol.hasInstance lookup
            if (!isConstructCall && !ordinaryHasInstance(WrappedDTF, this)) {
              return new WrappedDTF(locales, options);
            }

            const receiver = isConstructCall
              ? Object.create(getIntrinsicDateTimeFormatPrototype(new.target))
              : this;

            // Convert options to object (ToObject) - primitives like numbers should work
            // null must throw TypeError
            let opts;
            if (options === undefined) {
              opts = Object.create(null);
            } else if (options === null) {
              throw new TypeError('Cannot convert null to object');
            } else {
              opts = Object(options);
            }

            // Validate and canonicalize options - read in spec-defined order
            // Order per spec: localeMatcher, calendar, numberingSystem, hour12, hourCycle, timeZone, 
            //                 weekday, era, year, month, day, dayPeriod, hour, minute, second, fractionalSecondDigits, 
            //                 timeZoneName, formatMatcher, dateStyle, timeStyle
            const localeMatcher = getOption(opts, 'localeMatcher', 'string', VALID_LOCALE_MATCHERS, 'best fit');
            
            // Calendar validation - must be valid Unicode locale identifier type
            // Valid calendars are 3-8 alphanum chars, possibly with subtags separated by hyphens
            let calendar = opts.calendar;
            if (calendar !== undefined) {
              calendar = String(calendar);
              // Calendar must be 3-8 alphanum chars per Unicode locale identifier type
              // With possible subtag of 3-8 alphanum chars separated by hyphen
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(calendar)) {
                throw new RangeError('Invalid calendar');
              }
              calendar = canonicalizeCalendar(calendar);
            }
            
            // numberingSystem validation - must be valid Unicode locale identifier type
            let numberingSystem = opts.numberingSystem;
            if (numberingSystem !== undefined) {
              const ns = String(numberingSystem);
              // numberingSystem must be 3-8 alphanum chars
              if (!/^[a-zA-Z0-9]{3,8}(-[a-zA-Z0-9]{3,8})*$/.test(ns)) {
                throw new RangeError('Invalid numberingSystem');
              }
              numberingSystem = ns;
            }
            
            // hour12 special handling - read once, convert to boolean if defined
            const hour12Raw = opts.hour12;
            const hour12 = hour12Raw !== undefined ? Boolean(hour12Raw) : undefined;
            const hourCycle = getOption(opts, 'hourCycle', 'string', VALID_HOUR_CYCLES, undefined);
            let timeZone = opts.timeZone;
            if (timeZone !== undefined) {
              timeZone = canonicalizeTimeZone(String(timeZone));
            }
            
            const weekday = getOption(opts, 'weekday', 'string', VALID_WEEKDAYS, undefined);
            const era = getOption(opts, 'era', 'string', VALID_ERAS, undefined);
            const year = getOption(opts, 'year', 'string', VALID_YEARS, undefined);
            const month = getOption(opts, 'month', 'string', VALID_MONTHS, undefined);
            const day = getOption(opts, 'day', 'string', VALID_DAYS, undefined);
            // dayPeriod is read before hour per spec
            const dayPeriod = getOption(opts, 'dayPeriod', 'string', VALID_DAY_PERIODS, undefined);
            const hour = getOption(opts, 'hour', 'string', VALID_HOURS, undefined);
            const minute = getOption(opts, 'minute', 'string', VALID_MINUTES, undefined);
            const second = getOption(opts, 'second', 'string', VALID_SECONDS, undefined);
            const fractionalSecondDigits = getNumberOption(opts, 'fractionalSecondDigits', 1, 3, undefined);
            const timeZoneName = getOption(opts, 'timeZoneName', 'string', VALID_TIME_ZONE_NAMES, undefined);
            const formatMatcher = getOption(opts, 'formatMatcher', 'string', VALID_FORMAT_MATCHERS, 'best fit');
            const dateStyle = getOption(opts, 'dateStyle', 'string', VALID_DATE_STYLES, undefined);
            const timeStyle = getOption(opts, 'timeStyle', 'string', VALID_TIME_STYLES, undefined);

            // dateStyle/timeStyle cannot be combined with individual date/time components
            if ((dateStyle !== undefined || timeStyle !== undefined) &&
                (weekday !== undefined || era !== undefined || year !== undefined ||
                 month !== undefined || day !== undefined || dayPeriod !== undefined ||
                 hour !== undefined || minute !== undefined || second !== undefined ||
                 fractionalSecondDigits !== undefined || timeZoneName !== undefined)) {
              throw new TypeError('dateStyle and timeStyle cannot be combined with other date/time options');
            }

            // Per spec: CanonicalizeLocaleList calls ToObject(locales), which throws TypeError for null
            if (locales === null) {
              throw new TypeError('Cannot convert null to object');
            }

            // Create the underlying DTF instance
            let instance;
            try {
              instance = new DTF(locales, options);
            } catch (e) {
              throw e;
            }

            // Determine locale - consistent behavior for undefined and empty array
            let locale;
            const defaultLocale = (() => {
              try {
                return new Intl.NumberFormat().resolvedOptions().locale || 'en-US';
              } catch (e) {
                return 'en-US';
              }
            })();
            
            if (locales === undefined) {
              locale = defaultLocale;
            } else if (typeof locales === 'string') {
              locale = Intl.getCanonicalLocales(locales)[0] || defaultLocale;
            } else if (Array.isArray(locales)) {
              locale = locales.length > 0 ? Intl.getCanonicalLocales(locales)[0] : defaultLocale;
            } else {
              locale = defaultLocale;
            }
            locale = stripInvalidDTFUnicodeExtKeys(locale);

            // Determine if we need to apply default date/time format
            // Per ECMA-402, if no date/time components and no dateStyle/timeStyle specified,
            // default to year: 'numeric', month: 'numeric', day: 'numeric'
            let needsDefault = dateStyle === undefined && timeStyle === undefined &&
              weekday === undefined && era === undefined && year === undefined &&
              month === undefined && day === undefined && dayPeriod === undefined &&
              hour === undefined && minute === undefined && second === undefined &&
              fractionalSecondDigits === undefined && timeZoneName === undefined;
            
            let resolvedYear = year;
            let resolvedMonth = month;
            let resolvedDay = day;
            
            if (needsDefault) {
              resolvedYear = 'numeric';
              resolvedMonth = 'numeric';
              resolvedDay = 'numeric';
            }

            const localeCalendarMatch = typeof locale === 'string'
              ? locale.match(/-u(?:-[a-z0-9]{2,8})*-ca-([a-z0-9-]+)/i)
              : null;
            const resolvedCalendar = applyCalendarFallback(
              calendar || (localeCalendarMatch ? canonicalizeCalendar(localeCalendarMatch[1]) : 'gregory')
            );

            // Detect locale's default numbering system if not explicitly specified
            function detectLocaleNumberingSystem(loc) {
              if (!loc) return 'latn';
              const nuMatch = loc.match(/-u(?:-[a-z0-9]{2,8})*-nu-([a-z0-9]+)/i);
              if (nuMatch) return nuMatch[1].toLowerCase();
              // Known locales with non-Latin default numbering systems
              const lower = loc.toLowerCase();
              if (lower.startsWith('ar')) return 'arab';
              if (lower.startsWith('fa') || lower.startsWith('ps')) return 'arabext';
              if (lower.startsWith('ne') || lower.startsWith('mr')) return 'deva';
              if (lower.startsWith('bn')) return 'beng';
              if (lower.startsWith('gu')) return 'gujr';
              if (lower.startsWith('pa')) return 'guru';
              if (lower.startsWith('km')) return 'khmr';
              if (lower.startsWith('kn')) return 'knda';
              if (lower.startsWith('lo')) return 'laoo';
              if (lower.startsWith('ml')) return 'mlym';
              if (lower.startsWith('my')) return 'mymr';
              if (lower.startsWith('or')) return 'orya';
              if (lower.startsWith('ta')) return 'tamldec';
              if (lower.startsWith('te')) return 'telu';
              if (lower.startsWith('th')) return 'thai';
              if (lower.startsWith('bo')) return 'tibt';
              return 'latn';
            }

            // Store resolved options
            const resolvedOpts = {
              locale: locale,
              calendar: resolvedCalendar,
              numberingSystem: numberingSystem ? String(numberingSystem).toLowerCase() : detectLocaleNumberingSystem(locale),
              timeZone: timeZone,
              hourCycle: hourCycle,
              hour12: hour12,
              weekday: weekday,
              era: era,
              year: resolvedYear,
              month: resolvedMonth,
              day: resolvedDay,
              dayPeriod: dayPeriod,
              hour: hour,
              minute: minute,
              second: second,
              fractionalSecondDigits: fractionalSecondDigits,
              timeZoneName: timeZoneName,
              dateStyle: dateStyle,
              timeStyle: timeStyle,
            };

            // Determine if the format has an hour component:
            // explicit hour option, or timeStyle (which always includes hour)
            const hasHourComponent = hour !== undefined || timeStyle !== undefined;

            // Extract unicode extension values from locale for ca/hc/nu
            function extractLocaleExtKey(loc, key) {
              if (typeof loc !== 'string') return undefined;
              const re = new RegExp('-u(?:-[a-z0-9]{2,8})*-' + key + '-([a-z0-9]+)', 'i');
              const m = loc.match(re);
              return m ? m[1].toLowerCase() : undefined;
            }

            const localeHc = extractLocaleExtKey(locale, 'hc');
            const localeCa = extractLocaleExtKey(locale, 'ca');
            const localeNu = extractLocaleExtKey(locale, 'nu');

            // Locale default hourCycle: 12-hour for en/ja/ko/hi, 24-hour for most others
            function localeDefaultHourCycle(loc) {
              if (!loc) return 'h12';
              const base = loc.toLowerCase().split('-')[0];
              if (base === 'ja') return 'h11';
              if (base === 'en' || base === 'ko' || base === 'hi' || base === 'zh') return 'h12';
              return 'h23';
            }

            if (hasHourComponent) {
              // Resolve hourCycle: options > extension key > locale default
              let resolvedHc = hourCycle !== undefined ? hourCycle
                : localeHc !== undefined ? localeHc
                : localeDefaultHourCycle(locale);

              // hour12 option overrides hourCycle
              if (hour12 !== undefined) {
                resolvedHc = hour12 ? 'h12' : 'h23';
                // Special case: ja uses h11 for 12-hour
                if (hour12 && typeof locale === 'string' && locale.toLowerCase().startsWith('ja')) {
                  resolvedHc = 'h11';
                }
              }

              resolvedOpts.hourCycle = resolvedHc;
              resolvedOpts.hour12 = (resolvedHc === 'h11' || resolvedHc === 'h12');
            } else {
              // No hour component → spec requires hourCycle and hour12 to be undefined
              resolvedOpts.hourCycle = undefined;
              resolvedOpts.hour12 = undefined;
            }

            // Normalize resolved locale: strip extension keys overridden by options
            // Per spec ResolveLocale: if option value differs from extension value,
            // strip the extension key from the locale.
            function stripLocaleExtKey(loc, key) {
              if (typeof loc !== 'string') return loc;
              // Remove -key-value from the -u-... block
              return loc.replace(new RegExp('(-u(?:-[a-z0-9]{2,8})*)(-' + key + '-[a-z0-9]+)', 'i'), '$1');
                // Clean up empty -u- suffix
            }
            function cleanLocale(loc) {
              if (typeof loc !== 'string') return loc;
              // Remove trailing -u with no keys
              return loc.replace(/-u$/i, '').replace(/-u-(?=-u-|$)/gi, '');
            }

            let normalizedLocale = locale;

            // ca: handle invalid calendar option falling back to locale ca
            const calendarOptionValid = calendar !== undefined && VALID_CALENDARS.includes(resolvedCalendar);
            if (calendar !== undefined && !calendarOptionValid) {
              // Invalid calendar option: fall back to locale's ca if valid
              const localeCaCanon = localeCa ? canonicalizeCalendar(localeCa) : undefined;
              if (localeCaCanon && VALID_CALENDARS.includes(localeCaCanon)) {
                resolvedOpts.calendar = localeCaCanon;
                // Keep ca in locale (it's being used)
              } else {
                resolvedOpts.calendar = 'gregory';
                normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'ca'));
              }
            } else if (calendarOptionValid) {
              // Valid calendar option: strip ca from locale if it differs
              if (localeCa && canonicalizeCalendar(localeCa) !== resolvedCalendar) {
                normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'ca'));
              }
            } else if (calendar === undefined && localeCa && canonicalizeCalendar(localeCa) !== resolvedCalendar) {
              normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'ca'));
            }

            // hc: strip if hour12 option set, or if hourCycle option differs from extension
            if (localeHc !== undefined) {
              if (hour12 !== undefined) {
                normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'hc'));
              } else if (hourCycle !== undefined && hourCycle !== localeHc) {
                normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'hc'));
              }
            }

            // nu: handle invalid numberingSystem option falling back to locale nu
            const nsOptionValid = numberingSystem !== undefined && VALID_NUMBERING_SYSTEMS.includes(numberingSystem.toLowerCase());
            if (numberingSystem !== undefined && !nsOptionValid) {
              if (localeNu && VALID_NUMBERING_SYSTEMS.includes(localeNu)) {
                resolvedOpts.numberingSystem = localeNu;
              } else {
                resolvedOpts.numberingSystem = 'latn';
                normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'nu'));
              }
            } else if (nsOptionValid && localeNu && localeNu !== numberingSystem.toLowerCase()) {
              normalizedLocale = cleanLocale(stripLocaleExtKey(normalizedLocale, 'nu'));
            }

            resolvedOpts.locale = normalizedLocale;

            dtfSlots.set(receiver, { instance, resolvedOpts, needsDefault });

            return receiver;
          };

          // Copy static properties
          Object.defineProperty(WrappedDTF, 'length', { value: 0, configurable: true });
          Object.defineProperty(WrappedDTF, 'name', { value: 'DateTimeFormat', configurable: true });

          const supportedLocalesOf = (locales, options) => {
            if (typeof DTF.supportedLocalesOf === 'function') {
              return DTF.supportedLocalesOf(locales, options);
            }
            if (typeof Intl.NumberFormat === 'function' &&
                typeof Intl.NumberFormat.supportedLocalesOf === 'function') {
              return Intl.NumberFormat.supportedLocalesOf(locales, options);
            }

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
            const canonicalized = Intl.getCanonicalLocales(requestedLocales);
            const defaultLocale = (() => {
              try {
                return new Intl.NumberFormat().resolvedOptions().locale || 'en-US';
              } catch (e) {
                return 'en-US';
              }
            })();
            return canonicalized.filter((locale, index, array) =>
              array.indexOf(locale) === index && locale === defaultLocale
            );
          };
          Object.defineProperty(supportedLocalesOf, 'name', {
            value: 'supportedLocalesOf',
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(supportedLocalesOf, 'length', {
            value: 1,
            writable: false,
            enumerable: false,
            configurable: true,
          });
          Object.defineProperty(WrappedDTF, 'supportedLocalesOf', {
            value: supportedLocalesOf,
            writable: true,
            enumerable: false,
            configurable: true,
          });

          // Create new prototype
          const newProto = Object.create(Object.prototype);

          // resolvedOptions method
          Object.defineProperty(newProto, 'resolvedOptions', {
            value: makeNonConstructableAccessor(function resolvedOptions() {
              const slot = dtfSlots.get(this);
              if (!slot) {
                throw new TypeError('Method Intl.DateTimeFormat.prototype.resolvedOptions called on incompatible receiver');
              }
              const opts = slot.resolvedOpts;
              const _dp = (o, k, v) => Object.defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true });
              const result = Object.create(Object.prototype);
              _dp(result, 'locale', opts.locale);
              _dp(result, 'calendar', opts.calendar);
              _dp(result, 'numberingSystem', opts.numberingSystem);
              _dp(result, 'timeZone', opts.timeZone !== undefined ? opts.timeZone : defaultTimeZone());
              if (opts.hourCycle !== undefined) _dp(result, 'hourCycle', opts.hourCycle);
              if (opts.hour12 !== undefined) _dp(result, 'hour12', opts.hour12);
              if (opts.weekday !== undefined) _dp(result, 'weekday', opts.weekday);
              if (opts.era !== undefined) _dp(result, 'era', opts.era);
              if (opts.year !== undefined) _dp(result, 'year', opts.year);
              if (opts.month !== undefined) _dp(result, 'month', opts.month);
              if (opts.day !== undefined) _dp(result, 'day', opts.day);
              if (opts.dayPeriod !== undefined) _dp(result, 'dayPeriod', opts.dayPeriod);
              if (opts.hour !== undefined) _dp(result, 'hour', opts.hour);
              if (opts.minute !== undefined) _dp(result, 'minute', opts.minute);
              if (opts.second !== undefined) _dp(result, 'second', opts.second);
              if (opts.fractionalSecondDigits !== undefined) _dp(result, 'fractionalSecondDigits', opts.fractionalSecondDigits);
              if (opts.timeZoneName !== undefined) _dp(result, 'timeZoneName', opts.timeZoneName);
              if (opts.dateStyle !== undefined) _dp(result, 'dateStyle', opts.dateStyle);
              if (opts.timeStyle !== undefined) _dp(result, 'timeStyle', opts.timeStyle);
              return result;
            }, 'resolvedOptions', 0),
            writable: true,
            enumerable: false,
            configurable: true
          });

          function resolveCalendarId(opts) {
            if (opts.calendar !== undefined) {
              return String(opts.calendar).toLowerCase();
            }
            const locale = typeof opts.locale === 'string' ? opts.locale : '';
            const match = locale.match(/-u(?:-[a-z0-9]{2,8})*-ca-([a-z0-9-]+)/i);
            return match ? match[1].toLowerCase() : 'gregory';
          }

          function defaultTimeZone() {
            try {
              return new DTF().resolvedOptions().timeZone || 'UTC';
            } catch (_e) {
              return 'UTC';
            }
          }

          function getDateTimeFields(d, opts) {
            const calendar = resolveCalendarId(opts);
            if (typeof Temporal === 'object' &&
                Temporal !== null &&
                typeof Temporal.Instant === 'function') {
              try {
                const instant = new Temporal.Instant(BigInt(d.getTime()) * 1000000n);
                const tz = opts.timeZone !== undefined ? opts.timeZone : defaultTimeZone();
                const zoned = instant.toZonedDateTimeISO(tz);
                const weekday = zoned.dayOfWeek % 7;
                let calendarYear = zoned.year;
                let calendarMonth = zoned.month;
                let calendarDay = zoned.day;
                let calendarMonthCode = zoned.monthCode;
                let calendarEraYear = undefined;
                let calendarEra = undefined;
                if (calendar !== 'gregory' && calendar !== 'iso8601') {
                  try {
                    const calendarZoned = zoned.withCalendar(calendar);
                    calendarYear = calendarZoned.year;
                    calendarMonth = calendarZoned.month;
                    calendarDay = calendarZoned.day;
                    calendarMonthCode = calendarZoned.monthCode;
                    if ((calendar === 'chinese' || calendar === 'dangi') && calendarMonthCode) {
                      const m = calendarMonthCode.match(/^M(\d+)/);
                      if (m) {
                        calendarMonth = parseInt(m[1], 10);
                      }
                    }
                    try { calendarEraYear = calendarZoned.eraYear; } catch (_e) {}
                    try { calendarEra = calendarZoned.era; } catch (_e) {}
                  } catch (_calendarError) {
                    // Leave ISO fields in place.
                  }
                } else {
                  try { calendarEraYear = zoned.eraYear; } catch (_e) {}
                  try { calendarEra = zoned.era; } catch (_e) {}
                }
                return {
                  year: zoned.year,
                  month: zoned.month,
                  day: zoned.day,
                  hour: zoned.hour,
                  minute: zoned.minute,
                  second: zoned.second,
                  millisecond: zoned.millisecond,
                  weekday,
                  calendar,
                  calendarYear,
                  calendarMonth,
                  calendarDay,
                  calendarMonthCode,
                  calendarEraYear,
                  calendarEra,
                };
              } catch (_err) {
                // Fall back to local fields below.
              }
            }

            return {
              year: d.getFullYear(),
              month: d.getMonth() + 1,
              day: d.getDate(),
              hour: d.getHours(),
              minute: d.getMinutes(),
              second: d.getSeconds(),
              millisecond: d.getMilliseconds(),
              weekday: d.getDay(),
              calendar,
              calendarYear: d.getFullYear(),
              calendarMonth: d.getMonth() + 1,
              calendarDay: d.getDate(),
              calendarMonthCode: 'M' + String(d.getMonth() + 1).padStart(2, '0'),
              calendarEraYear: undefined,
              calendarEra: undefined,
            };
          }

          function localeUses24Hour(locale) {
            if (typeof locale === 'string') {
              // Check unicode hc extension first
              const hcMatch = locale.match(/-u(?:-[a-z0-9]{2,8})*-hc-([a-z0-9]+)/i);
              if (hcMatch) {
                const hc = hcMatch[1].toLowerCase();
                return hc === 'h23' || hc === 'h24';
              }
            }
            const lower = (locale || 'en-US').toLowerCase();
            return lower.startsWith('zh') || lower.startsWith('ja') ||
              lower.startsWith('ko') || lower.startsWith('de') || lower.startsWith('ru') ||
              lower.startsWith('pl') || lower.startsWith('it') || lower.startsWith('pt') ||
              lower.startsWith('nl') || lower.startsWith('sv') || lower.startsWith('fi') ||
              lower.startsWith('da') || lower.startsWith('nb') || lower.startsWith('cs') ||
              lower.startsWith('hu') || lower.startsWith('ro') || lower.startsWith('sk') ||
              lower.startsWith('uk') || lower.startsWith('hr') || lower.startsWith('bg') ||
              lower.startsWith('el') || lower.startsWith('tr') || lower.startsWith('vi') ||
              lower.startsWith('th') || lower.startsWith('id');
          }

          // Returns the locale-aware day period string for a given hour (0-23).
          // English CLDR data: midnight=0, morning=6-11, noon=12, afternoon=13-17, evening=18-20, night=21-23+0-5
          function getDayPeriodForHour(hour, style) {
            let period;
            if (hour === 0 || (hour >= 21 && hour <= 23)) {
              period = style === 'narrow' ? 'at night' : 'at night';
            } else if (hour >= 1 && hour <= 5) {
              period = 'at night';
            } else if (hour >= 6 && hour <= 11) {
              period = 'in the morning';
            } else if (hour === 12) {
              period = style === 'narrow' ? 'n' : 'noon';
            } else if (hour >= 13 && hour <= 17) {
              period = 'in the afternoon';
            } else {
              period = 'in the evening';
            }
            return period;
          }

          function safeSet(obj, key, value) {
            Object.defineProperty(obj, key, { value, writable: true, enumerable: true, configurable: true });
          }

          function applyDateTimeStyleDefaults(opts) {
            // Use Object.create(null) to avoid triggering tainted Object.prototype setters
            const adjusted = Object.create(null);
            const keys = ['calendar','numberingSystem','timeZone','weekday','era','year','month','day',
              'dayPeriod','hour','minute','second','fractionalSecondDigits','timeZoneName',
              'hourCycle','hour12','dateStyle','timeStyle','overrideYear','locale'];
            for (const k of keys) {
              if (opts[k] !== undefined) safeSet(adjusted, k, opts[k]);
            }
            if (opts.dateStyle !== undefined) {
              if (adjusted.year === undefined) safeSet(adjusted, 'year', opts.dateStyle === 'short' ? '2-digit' : 'numeric');
              if (adjusted.month === undefined) safeSet(adjusted, 'month', opts.dateStyle === 'short' ? 'numeric' : (opts.dateStyle === 'medium' ? 'short' : 'long'));
              if (adjusted.day === undefined) safeSet(adjusted, 'day', 'numeric');
              if (opts.dateStyle === 'full' && adjusted.weekday === undefined) safeSet(adjusted, 'weekday', 'long');
            }
            if (opts.timeStyle !== undefined) {
              if (adjusted.hour === undefined) safeSet(adjusted, 'hour', 'numeric');
              if (adjusted.minute === undefined) safeSet(adjusted, 'minute', 'numeric');
              if (opts.timeStyle !== 'short' && adjusted.second === undefined) safeSet(adjusted, 'second', 'numeric');
              if (opts.timeStyle === 'full' && adjusted.timeZoneName === undefined) safeSet(adjusted, 'timeZoneName', 'long');
              else if (opts.timeStyle === 'long' && adjusted.timeZoneName === undefined) safeSet(adjusted, 'timeZoneName', 'short');
            }
            return adjusted;
          }

          function formatDateWithOptionsToParts(d, opts) {
            const normalized = applyDateTimeStyleDefaults(opts);
            const fields = getDateTimeFields(d, normalized);
            const hasDateComponent = normalized.year !== undefined || normalized.month !== undefined ||
              normalized.day !== undefined || normalized.weekday !== undefined || normalized.era !== undefined;
            const hasTimeComponent = normalized.hour !== undefined || normalized.minute !== undefined ||
              normalized.second !== undefined || normalized.dayPeriod !== undefined ||
              normalized.fractionalSecondDigits !== undefined || normalized.timeZoneName !== undefined;
            const parts = [];

            function pushLiteral(value) {
              if (value) {
                parts.push({ type: 'literal', value });
              }
            }

            function monthName(month, style, calendar) {
              const gregLong = ['January', 'February', 'March', 'April', 'May', 'June',
                'July', 'August', 'September', 'October', 'November', 'December'];
              const gregShort = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
                'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
              const gregNarrow = ['J', 'F', 'M', 'A', 'M', 'J', 'J', 'A', 'S', 'O', 'N', 'D'];
              const hebrewLong = ['Tishri', 'Heshvan', 'Kislev', 'Tevet', 'Shevat', 'Adar', 'Nisan', 'Iyar', 'Sivan', 'Tamuz', 'Av', 'Elul'];
              const islamicLong = ['Muharram', 'Safar', 'Rabiʻ I', 'Rabiʻ II', 'Jumada I', 'Jumada II',
                'Rajab', 'Shaʻban', 'Ramadan', 'Shawwal', 'Dhuʻl-Qiʻdah', 'Dhuʻl-Hijjah'];
              const islamicShort = ['Muh.', 'Saf.', 'Rab. I', 'Rab. II', 'Jum. I', 'Jum. II',
                'Raj.', 'Sha.', 'Ram.', 'Shaw.', 'Dhuʻl-Q.', 'Dhuʻl-H.'];
              const lowerCalendar = String(calendar || 'gregory').toLowerCase();
              const isIslamic = lowerCalendar === 'islamic' || lowerCalendar.startsWith('islamic-');
              const isHebrew = lowerCalendar === 'hebrew';
              const longNames = isIslamic ? islamicLong : (isHebrew ? hebrewLong : gregLong);
              const shortNames = isIslamic ? islamicShort : (isHebrew ? hebrewLong : gregShort);
              const narrowNames = isIslamic ? islamicLong.map((name) => name[0]) : (isHebrew ? hebrewLong.map((name) => name[0]) : gregNarrow);
              if (style === 'long') return longNames[month - 1];
              if (style === 'short') return shortNames[month - 1];
              return narrowNames[month - 1];
            }

            function getEraDisplayName(eraCode, calendar, style) {
              const code = String(eraCode).toLowerCase();
              const cal = String(calendar).toLowerCase();
              const isLong = style === 'long';
              if (cal === 'gregory' || cal === 'iso8601') {
                if (code === 'ce' || code === 'gregory') return isLong ? 'Anno Domini' : 'AD';
                if (code === 'bce' || code === 'gregory-inverse') return isLong ? 'Before Christ' : 'BC';
                return code === 'ad' ? 'AD' : (code === 'bc' ? 'BC' : code.toUpperCase());
              }
              if (cal === 'japanese') {
                const jpEras = {
                  'meiji': isLong ? 'Meiji' : 'Meiji', 'taisho': isLong ? 'Taish\u014D' : 'Taish\u014D',
                  'showa': isLong ? 'Sh\u014Dwa' : 'Sh\u014Dwa', 'heisei': isLong ? 'Heisei' : 'Heisei',
                  'reiwa': isLong ? 'Reiwa' : 'Reiwa',
                  'japanese': isLong ? 'Anno Domini' : 'AD',
                  'japanese-inverse': isLong ? 'Before Christ' : 'BC',
                  'ce': isLong ? 'Anno Domini' : 'AD', 'bce': isLong ? 'Before Christ' : 'BC',
                };
                return jpEras[code] || code.toUpperCase();
              }
              if (cal.startsWith('islamic') || cal === 'islamic-civil') {
                const dy = normalized.overrideYear !== undefined
                  ? normalized.overrideYear
                  : (fields.calendarYear ?? fields.year);
                if (code === 'ah') return 'AH';
                if (code === 'bh' || code.includes('inverse')) return 'BH';
                return dy <= 0 ? 'BH' : 'AH';
              }
              if (cal === 'roc') {
                if (code === 'minguo' || code === 'roc') return isLong ? 'Minguo' : 'Minguo';
                if (code === 'before-roc' || code === 'roc-inverse') return isLong ? 'Before R.O.C.' : 'Before R.O.C.';
                return code.toUpperCase();
              }
              if (cal === 'buddhist') {
                return 'BE';
              }
              if (cal === 'coptic') {
                if (code === 'coptic' || code === 'ad') return isLong ? 'Era of the Martyrs' : 'ERA1';
                if (code === 'coptic-inverse' || code === 'bc') return isLong ? 'Before Era of the Martyrs' : 'ERA0';
                return code.toUpperCase();
              }
              if (cal === 'ethiopic') {
                if (code === 'ethioaa' || code === 'ethiopic-amete-alem') return isLong ? 'Amete Alem' : 'ERA0';
                if (code === 'ethiopic' || code === 'incar' || code === 'mundi') return isLong ? 'Incarnation Era' : 'ERA1';
                return code.toUpperCase();
              }
              if (cal === 'ethioaa') {
                return isLong ? 'Amete Alem' : 'ERA0';
              }
              if (cal === 'persian') {
                return 'AP';
              }
              if (cal === 'indian') {
                return 'Saka';
              }
              if (cal === 'hebrew') {
                return 'AM';
              }
              return code.toUpperCase();
            }

            function weekdayName(weekday, style) {
              const longNames = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
              const shortNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
              const narrowNames = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];
              if (style === 'long') return longNames[weekday];
              if (style === 'short') return shortNames[weekday];
              return narrowNames[weekday];
            }

            if (hasDateComponent || (!hasTimeComponent && normalized.dateStyle === undefined && normalized.timeStyle === undefined)) {
              const dateParts = [];
              if (normalized.weekday !== undefined) {
                dateParts.push({ type: 'weekday', value: weekdayName(fields.weekday, normalized.weekday) });
              }
              if (normalized.month !== undefined) {
                const cal = String(fields.calendar || 'gregory').toLowerCase();
                const isLunisolar = cal === 'chinese' || cal === 'dangi';
                const hebrewMonthNames = {
                  M01: 'Tishri',
                  M02: 'Heshvan',
                  M03: 'Kislev',
                  M04: 'Tevet',
                  M05: 'Shevat',
                  M05L: 'Adar I',
                  M06: 'Adar',
                  M07: 'Nisan',
                  M08: 'Iyar',
                  M09: 'Sivan',
                  M10: 'Tamuz',
                  M11: 'Av',
                  M12: 'Elul',
                };
                let displayMonth = fields.calendarMonth ?? fields.month;
                let value;
                if (cal === 'hebrew' && typeof fields.calendarMonthCode === 'string' &&
                    hebrewMonthNames[fields.calendarMonthCode]) {
                  value = hebrewMonthNames[fields.calendarMonthCode];
                } else if (isLunisolar && typeof fields.calendarMonthCode === 'string' &&
                    /^M\d{2}L?$/.test(fields.calendarMonthCode)) {
                  const isLeapMonth = fields.calendarMonthCode.endsWith('L');
                  const baseMonth = parseInt(
                    isLeapMonth
                      ? fields.calendarMonthCode.slice(1, -1)
                      : fields.calendarMonthCode.slice(1),
                    10
                  );
                  displayMonth = baseMonth;
                  if (normalized.month === '2-digit') {
                    value = isLeapMonth
                      ? (String(baseMonth).padStart(2, '0') + 'bis')
                      : String(baseMonth).padStart(2, '0');
                  } else if (normalized.month === 'numeric') {
                    value = isLeapMonth
                      ? (String(baseMonth) + 'bis')
                      : String(baseMonth);
                  } else {
                    value = isLeapMonth
                      ? ('闰' + String(baseMonth) + '月')
                      : (String(baseMonth) + '月');
                  }
                } else {
                  value = normalized.month === '2-digit'
                    ? String(displayMonth).padStart(2, '0')
                    : normalized.month === 'numeric'
                      ? String(displayMonth)
                      : monthName(displayMonth, normalized.month, fields.calendar);
                }
                dateParts.push({ type: 'month', value });
              }
              if (normalized.day !== undefined) {
                const displayDay = fields.calendarDay ?? fields.day;
                dateParts.push({
                  type: 'day',
                  value: normalized.day === '2-digit'
                    ? String(displayDay).padStart(2, '0')
                    : String(displayDay),
                });
              }
              if (normalized.year !== undefined) {
                const displayYear = normalized.overrideYear !== undefined
                  ? normalized.overrideYear
                  : (fields.calendarYear ?? fields.year);
                // Proleptic Gregorian: no year 0; year 0 CE = 1 BC, year -1 CE = 2 BC, etc.
                const isBC = displayYear <= 0;
                const prolepticYear = isBC ? 1 - displayYear : displayYear;
                const cal = String(normalized.calendar || 'gregory').toLowerCase();
                if ((cal === 'chinese' || cal === 'dangi') && normalized.year === 'numeric') {
                  const stems = '甲乙丙丁戊己庚辛壬癸';
                  const branches = '子丑寅卯辰巳午未申酉戌亥';
                  const relatedYear = fields.calendarYear !== undefined ? fields.calendarYear : displayYear;
                  const y = relatedYear - 4;
                  const ganzhiName = stems[(((y % 10) + 10) % 10)] + branches[(((y % 12) + 12) % 12)];
                  dateParts.push({ type: 'relatedYear', value: String(relatedYear) });
                  dateParts.push({ type: 'yearName', value: ganzhiName });
                  dateParts.push({ type: 'literal', value: '年' });
                } else {
                  const yearVal = fields.calendarEraYear !== undefined && fields.calendarEraYear !== null
                    ? fields.calendarEraYear : prolepticYear;
                  dateParts.push({ type: 'year', value: normalized.year === '2-digit'
                    ? String(yearVal % 100).padStart(2, '0')
                    : String(yearVal) });
                }
              }
              if (normalized.era !== undefined) {
                const cal = String(normalized.calendar || 'gregory').toLowerCase();
                if (cal !== 'chinese' && cal !== 'dangi') {
                  const eraCode = fields.calendarEra;
                  let eraDisplay;
                  if (eraCode !== undefined && eraCode !== null) {
                    eraDisplay = getEraDisplayName(eraCode, cal, normalized.era);
                  } else {
                    const dy = normalized.overrideYear !== undefined
                      ? normalized.overrideYear
                      : (fields.calendarYear ?? fields.year);
                    eraDisplay = dy >= 1 ? 'AD' : 'BC';
                  }
                  dateParts.push({ type: 'era', value: eraDisplay });
                }
              }

              // Determine if month is a named style (long/short/narrow) for locale-aware separators
              const namedMonth = normalized.month === 'long' || normalized.month === 'short' || normalized.month === 'narrow';
              dateParts.forEach((part, index) => {
                if (index > 0) {
                  const prev = dateParts[index - 1].type;
                  const cur = part.type;
                  const cal = String(normalized.calendar || 'gregory').toLowerCase();
                  const isLunisolar = cal === 'chinese' || cal === 'dangi';
                  if (prev === 'relatedYear' && cur === 'yearName') {
                    // No separator between relatedYear and yearName
                  } else if (prev === 'yearName' && cur === 'literal') {
                    // No separator before trailing literal (e.g. '年')
                  } else if (isLunisolar && prev === 'month' && cur === 'day') {
                    // No separator between month and day in lunisolar
                  } else if (isLunisolar && prev === 'day' && (cur === 'relatedYear' || cur === 'year')) {
                    pushLiteral('日');
                  } else if (prev === 'weekday') {
                    pushLiteral(', ');
                  } else if (namedMonth && prev === 'month' && cur === 'day') {
                    pushLiteral(' ');
                  } else if (namedMonth && prev === 'day' && cur === 'year') {
                    pushLiteral(', ');
                  } else if (namedMonth && prev === 'month' && cur === 'year') {
                    pushLiteral(' ');
                  } else if (namedMonth && prev === 'day' && cur === 'relatedYear') {
                    pushLiteral(', ');
                  } else if (namedMonth && prev === 'month' && cur === 'relatedYear') {
                    pushLiteral(' ');
                  } else {
                    pushLiteral('/');
                  }
                }
                parts.push(part);
              });
            }

            if (hasTimeComponent) {
              if (parts.length > 0) {
                pushLiteral(', ');
              }

              const use12Hour = normalized.hour12 !== undefined
                ? normalized.hour12
                : normalized.hourCycle !== undefined
                  ? normalized.hourCycle === 'h11' || normalized.hourCycle === 'h12'
                  : !localeUses24Hour(normalized.locale);

              let displayHour = fields.hour;
              let dayPeriod;
              if (normalized.hour !== undefined) {
                if (normalized.dayPeriod !== undefined) {
                  // Use locale-aware day period instead of AM/PM
                  dayPeriod = getDayPeriodForHour(fields.hour, normalized.dayPeriod);
                  displayHour = fields.hour % 12;
                  if (displayHour === 0) displayHour = 12;
                } else if (use12Hour) {
                  dayPeriod = fields.hour >= 12 ? 'PM' : 'AM';
                  if (normalized.hourCycle === 'h11') {
                    displayHour = fields.hour % 12;
                  } else {
                    displayHour = fields.hour % 12;
                    if (displayHour === 0) displayHour = 12;
                  }
                } else if (normalized.hourCycle === 'h24' && displayHour === 0) {
                  displayHour = 24;
                }
              } else if (normalized.dayPeriod !== undefined) {
                // dayPeriod-only: compute from hour field, no hour output
                dayPeriod = getDayPeriodForHour(fields.hour, normalized.dayPeriod);
              }

              const timeParts = [];
              if (normalized.hour !== undefined) {
                timeParts.push({
                  type: 'hour',
                  value: normalized.hour === '2-digit'
                    ? String(displayHour).padStart(2, '0')
                    : String(displayHour),
                });
              }
              if (normalized.minute !== undefined) {
                timeParts.push({ type: 'minute', value: String(fields.minute).padStart(2, '0') });
              }
              if (normalized.second !== undefined) {
                const secondValue = String(fields.second).padStart(2, '0');
                timeParts.push({ type: 'second', value: secondValue });
              } else if (normalized.fractionalSecondDigits !== undefined) {
                timeParts.push({
                  type: 'fractionalSecond',
                  value: String(fields.millisecond).padStart(3, '0').substring(0, normalized.fractionalSecondDigits),
                });
              }

              timeParts.forEach((part, index) => {
                if (index > 0) {
                  pushLiteral(':');
                }
                parts.push(part);
              });

              // Fractional seconds follow the second part with a '.' literal separator
              if (normalized.second !== undefined && normalized.fractionalSecondDigits !== undefined) {
                const ms = String(fields.millisecond).padStart(3, '0')
                  .substring(0, normalized.fractionalSecondDigits);
                parts.push({ type: 'literal', value: '.' });
                parts.push({ type: 'fractionalSecond', value: ms });
              }

              if (dayPeriod && (use12Hour || normalized.dayPeriod !== undefined)) {
                if (timeParts.length > 0) pushLiteral(' ');
                parts.push({ type: 'dayPeriod', value: dayPeriod });
              }

              if (normalized.timeZoneName !== undefined) {
                const tzStyle = normalized.timeZoneName;
                let zoneName = normalized.timeZone || 'UTC';
                let resolved = false;
                try {
                  if (typeof Temporal === 'object' && Temporal !== null &&
                      typeof Temporal.Instant === 'function') {
                    const instant = new Temporal.Instant(BigInt(d.getTime()) * 1000000n);
                    const offset = instant.toZonedDateTimeISO(zoneName).offset;
                    const hrs = parseInt(offset.slice(1, 3), 10);
                    const mins = parseInt(offset.slice(4, 6), 10);
                    const totalMin = (offset[0] === '-' ? -1 : 1) * (hrs * 60 + mins);
                    if (tzStyle === 'long' || tzStyle === 'longGeneric' || tzStyle === 'longOffset') {
                      if (zoneName === 'UTC' || zoneName === 'Etc/UTC' || totalMin === 0 && (zoneName.startsWith('Etc/') || zoneName === 'UTC' || zoneName === 'GMT')) {
                        zoneName = 'Coordinated Universal Time';
                      } else {
                        const sign = totalMin >= 0 ? '+' : '-';
                        const absH = Math.floor(Math.abs(totalMin) / 60);
                        const absM = Math.abs(totalMin) % 60;
                        zoneName = 'GMT' + sign + String(absH).padStart(2, '0') + ':' + String(absM).padStart(2, '0');
                      }
                    } else {
                      if (zoneName === 'UTC' || zoneName === 'Etc/UTC') {
                        zoneName = 'UTC';
                      } else if (zoneName === 'GMT' || zoneName === 'Etc/GMT') {
                        zoneName = 'GMT';
                      } else {
                        const sign = totalMin >= 0 ? '+' : '-';
                        const absH = Math.floor(Math.abs(totalMin) / 60);
                        const absM = Math.abs(totalMin) % 60;
                        zoneName = 'GMT' + sign + String(absH) + (absM > 0 ? ':' + String(absM).padStart(2, '0') : '');
                      }
                    }
                    resolved = true;
                  }
                } catch (_e) {}
                if (!resolved) {
                  if (tzStyle === 'long') {
                    if (zoneName === 'UTC' || zoneName === 'Etc/UTC') zoneName = 'Coordinated Universal Time';
                    else if (zoneName === 'GMT' || zoneName === 'Etc/GMT') zoneName = 'Greenwich Mean Time';
                    else if (zoneName === 'America/New_York') zoneName = 'Eastern Standard Time';
                    else if (zoneName === 'America/Los_Angeles') zoneName = 'Pacific Standard Time';
                  } else {
                    if (zoneName === 'UTC' || zoneName === 'Etc/UTC') zoneName = 'UTC';
                    else if (zoneName === 'GMT' || zoneName === 'Etc/GMT') zoneName = 'GMT';
                    else if (zoneName === 'America/New_York') zoneName = 'EST';
                    else if (zoneName === 'America/Los_Angeles') zoneName = 'PST';
                    else if (zoneName === 'Europe/Berlin' || zoneName === 'Europe/Vienna') zoneName = 'GMT+1';
                  }
                }
                pushLiteral(' ');
                parts.push({ type: 'timeZoneName', value: zoneName });
              }
            }

            if (parts.length === 0) {
              parts.push({ type: 'literal', value: '' });
            }
            return parts;
          }

          const NUMBERING_SYSTEM_DIGITS = {
            arab: 0x0660, arabext: 0x06F0, beng: 0x09E6, deva: 0x0966,
            gujr: 0x0AE6, guru: 0x0A66, khmr: 0x17E0, knda: 0x0CE6,
            laoo: 0x0ED0, mlym: 0x0D66, mong: 0x1810, mymr: 0x1040,
            orya: 0x0B66, tamldec: 0x0BE6, telu: 0x0C66, thai: 0x0E50, tibt: 0x0F20,
          };
          // Decimal separators for non-Latin numbering systems
          const NUMBERING_SYSTEM_DECIMAL = {
            arab: '\u066B', arabext: '\u066B',
          };

          function applyNumberingSystem(str, ns) {
            if (!ns || ns === 'latn') return str;
            if (ns === 'hanidec') {
              const hanidec = ['〇','一','二','三','四','五','六','七','八','九'];
              return str.replace(/[0-9]/g, (d) => hanidec[Number(d)]);
            }
            const base = NUMBERING_SYSTEM_DIGITS[ns];
            if (base === undefined) return str;
            let result = str.replace(/[0-9]/g, (d) => String.fromCodePoint(base + Number(d)));
            const dec = NUMBERING_SYSTEM_DECIMAL[ns];
            if (dec) result = result.replace(/\./g, dec);
            return result;
          }

          // Helper function to format date according to resolved options
          function formatDateWithOptions(d, opts) {
            const raw = formatDateWithOptionsToParts(d, opts).map((part) => part.value).join('');
            return applyNumberingSystem(raw, opts.numberingSystem);
          }

          function normalizeDateTimeFormatInput(value) {
            if (value === undefined) {
              return new Date(Date.now());
            }

            if (value instanceof Date) {
              return new Date(value.getTime());
            }

            if (typeof value === 'object' && value !== null) {
              const tag = Object.prototype.toString.call(value);
              if (tag === '[object Temporal.Instant]') {
                return new Date(Number(value.epochMilliseconds));
              }
            }

            // Per spec: ToNumber(date), not new Date(date) — string inputs must produce NaN
            return new Date(Number(value));
          }

          function isTemporalInstantValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.Instant]';
          }

          function isTemporalPlainTimeValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainTime]';
          }

          function isTemporalPlainMonthDayValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainMonthDay]';
          }

          function isTemporalPlainDateValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainDate]';
          }

          function isTemporalPlainDateTimeValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainDateTime]';
          }

          function isTemporalPlainYearMonthValue(value) {
            return typeof value === 'object' &&
              value !== null &&
              Object.prototype.toString.call(value) === '[object Temporal.PlainYearMonth]';
          }

          function temporalCalendarId(value) {
            try {
              if (typeof value.calendarId === 'string') {
                return value.calendarId.toLowerCase();
              }
            } catch (_err) {}
            const match = String(value).match(/\[u-ca=([^\]]+)\]/);
            return match ? match[1].toLowerCase() : 'iso8601';
          }

          function temporalDateStringToUTCDate(value) {
            const match = String(value).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})/);
            if (!match) {
              throw new RangeError('Invalid time value');
            }
            return new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
          }

          function copyDefinedDateTimeFormatOptions(opts) {
            const adjusted = Object.create(null);
            const keys = [
              'calendar',
              'numberingSystem',
              'timeZone',
              'weekday',
              'era',
              'year',
              'month',
              'day',
              'dayPeriod',
              'hour',
              'minute',
              'second',
              'fractionalSecondDigits',
              'timeZoneName',
              'hourCycle',
              'hour12',
              'dateStyle',
              'timeStyle',
            ];
            for (const key of keys) {
              if (opts[key] !== undefined) {
                Object.defineProperty(adjusted, key, { value: opts[key], writable: true, enumerable: true, configurable: true });
              }
            }
            return adjusted;
          }

          function temporalInstantFormattingOptions(slot) {
            const opts = copyDefinedDateTimeFormatOptions(slot.resolvedOpts);
            const hasDateStyle = slot.resolvedOpts.dateStyle !== undefined || slot.resolvedOpts.timeStyle !== undefined;
            const hasExplicitCoreFields = slot.resolvedOpts.weekday !== undefined ||
              slot.resolvedOpts.era !== undefined ||
              slot.resolvedOpts.year !== undefined ||
              slot.resolvedOpts.month !== undefined ||
              slot.resolvedOpts.day !== undefined ||
              slot.resolvedOpts.hour !== undefined ||
              slot.resolvedOpts.minute !== undefined ||
              slot.resolvedOpts.second !== undefined;
            const needsInstantDefaults = !hasDateStyle && (
              slot.needsDefault || (!hasExplicitCoreFields && (
                slot.resolvedOpts.fractionalSecondDigits !== undefined ||
                slot.resolvedOpts.timeZoneName !== undefined ||
                slot.resolvedOpts.hour12 !== undefined ||
                slot.resolvedOpts.hourCycle !== undefined
              ))
            );
            if (needsInstantDefaults) {
              const use24Hour = slot.resolvedOpts.hour12 === false ||
                slot.resolvedOpts.hourCycle === 'h23' ||
                slot.resolvedOpts.hourCycle === 'h24' ||
                (slot.resolvedOpts.hour12 === undefined &&
                 slot.resolvedOpts.hourCycle === undefined &&
                 localeUses24Hour(slot.resolvedOpts.locale));
              opts.year ??= 'numeric';
              opts.month ??= 'numeric';
              opts.day ??= 'numeric';
              opts.hour ??= use24Hour ? '2-digit' : 'numeric';
              opts.minute ??= '2-digit';
              opts.second ??= '2-digit';
            }
            return opts;
          }

          function formatTemporalInstant(slot, instant, toParts) {
            const d = new Date(Number(instant.epochMilliseconds));
            if (isNaN(d.getTime())) {
              throw new RangeError('Invalid time value');
            }
            const opts = temporalInstantFormattingOptions(slot);
            opts.locale = slot.resolvedOpts.locale;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainTimeFormattingOptions(slot) {
            // dateStyle-only (no timeStyle) is a no-overlap error
            if (slot.resolvedOpts.dateStyle !== undefined && slot.resolvedOpts.timeStyle === undefined &&
                slot.resolvedOpts.hour === undefined && slot.resolvedOpts.minute === undefined &&
                slot.resolvedOpts.second === undefined && !slot.needsDefault) {
              throw new TypeError('PlainTime cannot be formatted with dateStyle');
            }

            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle;
            delete opts.timeStyle;
            delete opts.weekday;
            delete opts.era;
            delete opts.year;
            delete opts.month;
            delete opts.day;
            delete opts.timeZoneName;

            const hasCoreTimeFields = opts.hour !== undefined ||
              opts.minute !== undefined ||
              opts.second !== undefined;
            if (!hasCoreTimeFields) {
              // Only throw if explicit date-only fields were requested (not needsDefault)
              if (!slot.needsDefault && (slot.resolvedOpts.year !== undefined ||
                  slot.resolvedOpts.month !== undefined ||
                  slot.resolvedOpts.day !== undefined)) {
                throw new TypeError('PlainTime does not overlap with date fields');
              }
              const use24Hour = slot.resolvedOpts.hour12 === false ||
                slot.resolvedOpts.hourCycle === 'h23' ||
                slot.resolvedOpts.hourCycle === 'h24' ||
                (slot.resolvedOpts.hour12 === undefined &&
                 slot.resolvedOpts.hourCycle === undefined &&
                 localeUses24Hour(slot.resolvedOpts.locale));
              opts.hour = use24Hour ? '2-digit' : 'numeric';
              opts.minute = '2-digit';
              opts.second = '2-digit';
            }

            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainTime(slot, plainTime, toParts) {
            const d = new Date(Date.UTC(
              1972,
              0,
              1,
              plainTime.hour,
              plainTime.minute,
              plainTime.second,
              plainTime.millisecond,
            ));
            const opts = temporalPlainTimeFormattingOptions(slot);
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainMonthDayFormattingOptions(slot, plainMonthDay) {
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined) {
              throw new TypeError('PlainMonthDay cannot be formatted with timeStyle');
            }

            const formatterCalendar = resolveCalendarId(slot.resolvedOpts);
            const valueCalendar = temporalCalendarId(plainMonthDay);
            if (formatterCalendar !== valueCalendar) {
              throw new RangeError('calendar mismatch');
            }

            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle;
            delete opts.timeStyle;
            delete opts.weekday;
            delete opts.era;
            delete opts.year;
            delete opts.dayPeriod;
            delete opts.hour;
            delete opts.minute;
            delete opts.second;
            delete opts.fractionalSecondDigits;
            delete opts.timeZoneName;

            const hasDateFields = opts.month !== undefined || opts.day !== undefined;
            if (!hasDateFields) {
              if (slot.resolvedOpts.year !== undefined ||
                  slot.resolvedOpts.hour !== undefined ||
                  slot.resolvedOpts.minute !== undefined ||
                  slot.resolvedOpts.second !== undefined ||
                  slot.resolvedOpts.timeStyle !== undefined) {
                throw new TypeError('PlainMonthDay does not overlap with requested fields');
              }
              opts.month = 'numeric';
              opts.day = 'numeric';
            }

            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            opts.calendar = formatterCalendar;
            return opts;
          }

          function formatTemporalPlainMonthDay(slot, plainMonthDay, toParts) {
            const plainDate = plainMonthDay.toPlainDate({ year: 1972 });
            const d = temporalDateStringToUTCDate(plainDate);
            const opts = temporalPlainMonthDayFormattingOptions(slot, plainMonthDay);
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainDateFormattingOptions(slot, plainDate) {
            // PlainDate has no time data; timeStyle-only is a no-overlap error
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined &&
                slot.resolvedOpts.year === undefined && slot.resolvedOpts.month === undefined &&
                slot.resolvedOpts.day === undefined && slot.resolvedOpts.weekday === undefined &&
                slot.resolvedOpts.era === undefined && !slot.needsDefault) {
              throw new TypeError('PlainDate does not overlap with timeStyle');
            }
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            // Strip time-only fields
            delete opts.hour; delete opts.minute; delete opts.second;
            delete opts.fractionalSecondDigits; delete opts.dayPeriod;
            delete opts.timeZoneName; delete opts.dateStyle; delete opts.timeStyle;
            const hasNonEraDateFields = opts.weekday !== undefined ||
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined;
            const hasAnyDateFields = hasNonEraDateFields || opts.era !== undefined;
            if (!hasAnyDateFields || (!hasNonEraDateFields && opts.era !== undefined)) {
              if (!slot.needsDefault && slot.resolvedOpts.hour !== undefined) {
                throw new TypeError('PlainDate does not overlap with time fields');
              }
              opts.year = 'numeric'; opts.month = 'numeric'; opts.day = 'numeric';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          // Shift a UTC ms timestamp into the valid JS Date range by adding/subtracting
          // multiples of 400 years (146097 days) to preserve weekday and calendar cycle.
          const MS_PER_DAY = 86400000;
          const DAYS_PER_400Y = 146097;
          const MS_PER_400Y = DAYS_PER_400Y * MS_PER_DAY;
          const MAX_DATE_MS = 8640000000000000;

          function temporalDateToInRangeUTC(year, month0, day) {
            // month0 is 0-based
            let y = year, m = month0, d = day;
            // Shift year into range by multiples of 400
            while (true) {
              const ms = Date.UTC(y, m, d);
              if (!isNaN(ms) && ms >= -MAX_DATE_MS && ms <= MAX_DATE_MS) return ms;
              if (y < 0) y += 400; else y -= 400;
            }
          }

          function formatTemporalPlainDate(slot, plainDate, toParts) {
            const match = String(plainDate).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})/);
            if (!match) throw new RangeError('Invalid PlainDate');
            const actualYear = Number(match[1]);
            const ms = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, Number(match[3]));
            const d = new Date(ms);
            const opts = temporalPlainDateFormattingOptions(slot, plainDate);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainDateTimeFormattingOptions(slot) {
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.timeZoneName; delete opts.dateStyle; delete opts.timeStyle;
            const hasNonEraDateFields = opts.weekday !== undefined ||
              opts.year !== undefined || opts.month !== undefined || opts.day !== undefined;
            const hasDateFields = hasNonEraDateFields || opts.era !== undefined;
            const hasTimeFields = opts.hour !== undefined || opts.minute !== undefined ||
              opts.second !== undefined || opts.fractionalSecondDigits !== undefined ||
              opts.dayPeriod !== undefined;
            if (!hasDateFields && !hasTimeFields) {
              opts.year = 'numeric'; opts.month = 'numeric'; opts.day = 'numeric';
              opts.hour = 'numeric'; opts.minute = '2-digit'; opts.second = '2-digit';
            } else if (slot.needsDefault || (!hasNonEraDateFields && opts.era !== undefined && !hasTimeFields)) {
              // needsDefault or era-only: add both date and time defaults for PlainDateTime
              opts.year ??= 'numeric'; opts.month ??= 'numeric'; opts.day ??= 'numeric';
              opts.hour ??= 'numeric'; opts.minute ??= '2-digit'; opts.second ??= '2-digit';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainDateTime(slot, plainDateTime, toParts) {
            const match = String(plainDateTime).match(/^([+-]?\d{4,6})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?/);
            if (!match) throw new RangeError('Invalid PlainDateTime');
            const actualYear = Number(match[1]);
            const fracMs = match[7] ? Math.round(Number(match[7].substring(0, 3).padEnd(3, '0'))) : 0;
            const timeOffset = Number(match[4]) * 3600000 + Number(match[5]) * 60000 + Number(match[6]) * 1000 + fracMs;
            let baseMs = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, Number(match[3]));
            let totalMs = baseMs + timeOffset;
            // If adding time offset pushes out of range, shift base by -400 years
            if (isNaN(totalMs) || totalMs > 8640000000000000 || totalMs < -8640000000000000) {
              baseMs = temporalDateToInRangeUTC(actualYear - 400, Number(match[2]) - 1, Number(match[3]));
              totalMs = baseMs + timeOffset;
            }
            const d = new Date(totalMs);
            const opts = temporalPlainDateTimeFormattingOptions(slot);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          function temporalPlainYearMonthFormattingOptions(slot, plainYearMonth) {
            if (slot.resolvedOpts.timeStyle !== undefined && slot.resolvedOpts.dateStyle === undefined) {
              throw new TypeError('PlainYearMonth cannot be formatted with timeStyle');
            }
            const formatterCalendar = resolveCalendarId(slot.resolvedOpts);
            const valueCalendar = temporalCalendarId(plainYearMonth);
            if (formatterCalendar !== valueCalendar && formatterCalendar !== 'gregory' && formatterCalendar !== 'iso8601') {
              throw new RangeError('calendar mismatch');
            }
            const opts = applyDateTimeStyleDefaults(copyDefinedDateTimeFormatOptions(slot.resolvedOpts));
            delete opts.dateStyle; delete opts.timeStyle; delete opts.weekday;
            delete opts.day; delete opts.dayPeriod; delete opts.hour; delete opts.minute;
            delete opts.second; delete opts.fractionalSecondDigits; delete opts.timeZoneName;
            const hasFields = opts.year !== undefined || opts.month !== undefined;
            if (!hasFields) {
              if (slot.resolvedOpts.day !== undefined || slot.resolvedOpts.hour !== undefined ||
                  slot.resolvedOpts.minute !== undefined || slot.resolvedOpts.second !== undefined) {
                throw new TypeError('PlainYearMonth does not overlap with requested fields');
              }
              opts.year = 'numeric'; opts.month = 'numeric';
            }
            opts.timeZone = 'UTC';
            opts.locale = slot.resolvedOpts.locale;
            return opts;
          }

          function formatTemporalPlainYearMonth(slot, plainYearMonth, toParts) {
            const match = String(plainYearMonth).match(/^([+-]?\d{4,6})-(\d{2})/);
            if (!match) throw new RangeError('Invalid PlainYearMonth');
            const actualYear = Number(match[1]);
            const ms = temporalDateToInRangeUTC(actualYear, Number(match[2]) - 1, 1);
            const d = new Date(ms);
            const opts = temporalPlainYearMonthFormattingOptions(slot, plainYearMonth);
            opts.overrideYear = actualYear;
            return toParts
              ? formatDateWithOptionsToParts(d, opts)
              : formatDateWithOptions(d, opts);
          }

          // Helper to make non-constructable getter/function
          function makeNonConstructableAccessor(impl, name, length) {
            const arrowWrapper = (...args) => impl.apply(undefined, args);
            const handler = {
              apply(target, thisArg, args) {
                return impl.apply(thisArg, args);
              }
            };
            const proxy = new Proxy(arrowWrapper, handler);
            Object.defineProperty(proxy, 'name', { value: name, configurable: true });
            Object.defineProperty(proxy, 'length', { value: length !== undefined ? length : 0, writable: false, enumerable: false, configurable: true });
            return proxy;
          }

          // format getter (returns a bound function)
          // Per spec, the getter must not be a constructor and must not have prototype
          const formatGetterImpl = function() {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method get Intl.DateTimeFormat.prototype.format called on incompatible receiver');
            }
            const boundFormat = (date) => {
              if (isTemporalInstantValue(date)) {
                return formatTemporalInstant(slot, date, false);
              }
              if (isTemporalPlainTimeValue(date)) {
                return formatTemporalPlainTime(slot, date, false);
              }
              if (isTemporalPlainDateTimeValue(date)) {
                return formatTemporalPlainDateTime(slot, date, false);
              }
              if (isTemporalPlainDateValue(date)) {
                return formatTemporalPlainDate(slot, date, false);
              }
              if (isTemporalPlainYearMonthValue(date)) {
                return formatTemporalPlainYearMonth(slot, date, false);
              }
              if (isTemporalPlainMonthDayValue(date)) {
                return formatTemporalPlainMonthDay(slot, date, false);
              }
              const d = normalizeDateTimeFormatInput(date);
              if (isNaN(d.getTime())) {
                throw new RangeError('Invalid time value');
              }
              // Use our custom formatter when dayPeriod, fractionalSecondDigits, non-Latin numbering system,
              // or needsDefault is set (needsDefault: underlying DTF was created without explicit fields,
              // so it may add time components when hour12/hourCycle is set)
              const needsCustomFormat = slot.needsDefault ||
                slot.resolvedOpts.dayPeriod !== undefined ||
                slot.resolvedOpts.fractionalSecondDigits !== undefined ||
                (slot.resolvedOpts.numberingSystem && slot.resolvedOpts.numberingSystem !== 'latn') ||
                (slot.resolvedOpts.era !== undefined &&
                  slot.resolvedOpts.calendar !== 'gregory' &&
                  slot.resolvedOpts.calendar !== 'iso8601') ||
                slot.resolvedOpts.calendar === 'hebrew' ||
                slot.resolvedOpts.calendar === 'chinese' ||
                slot.resolvedOpts.calendar === 'dangi' ||
                slot.resolvedOpts.dateStyle !== undefined ||
                slot.resolvedOpts.timeStyle !== undefined ||
                slot.resolvedOpts.timeZoneName !== undefined;
              if (needsCustomFormat) {
                return formatDateWithOptions(d, slot.resolvedOpts);
              }
              if (slot.instance && typeof slot.instance.format === 'function') {
                return slot.instance.format(d);
              }
              return formatDateWithOptions(d, slot.resolvedOpts);
            };
            Object.defineProperty(boundFormat, 'name', { value: '', configurable: true });
            // Cache the bound format function
            Object.defineProperty(this, 'format', { value: boundFormat, writable: true, configurable: true });
            return boundFormat;
          };
          const formatGetter = makeNonConstructableAccessor(formatGetterImpl, 'get format');
          
          Object.defineProperty(newProto, 'format', {
            get: formatGetter,
            enumerable: false,
            configurable: true
          });

          // formatToParts method
          const formatToPartsImpl = function(date) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatToParts called on incompatible receiver');
            }
            if (isTemporalInstantValue(date)) {
              return formatTemporalInstant(slot, date, true);
            }
            if (isTemporalPlainTimeValue(date)) {
              return formatTemporalPlainTime(slot, date, true);
            }
            if (isTemporalPlainDateTimeValue(date)) {
              return formatTemporalPlainDateTime(slot, date, true);
            }
            if (isTemporalPlainDateValue(date)) {
              return formatTemporalPlainDate(slot, date, true);
            }
            if (isTemporalPlainYearMonthValue(date)) {
              return formatTemporalPlainYearMonth(slot, date, true);
            }
            if (isTemporalPlainMonthDayValue(date)) {
              return formatTemporalPlainMonthDay(slot, date, true);
            }
            const d = normalizeDateTimeFormatInput(date);
            if (isNaN(d.getTime())) {
              throw new RangeError('Invalid time value');
            }
            const needsCustom = slot.needsDefault ||
              slot.resolvedOpts.dayPeriod !== undefined ||
              slot.resolvedOpts.fractionalSecondDigits !== undefined ||
              (slot.resolvedOpts.numberingSystem && slot.resolvedOpts.numberingSystem !== 'latn') ||
              (slot.resolvedOpts.era !== undefined &&
                slot.resolvedOpts.calendar !== 'gregory' &&
                slot.resolvedOpts.calendar !== 'iso8601') ||
              slot.resolvedOpts.calendar === 'hebrew' ||
              slot.resolvedOpts.calendar === 'chinese' ||
              slot.resolvedOpts.calendar === 'dangi' ||
              slot.resolvedOpts.dateStyle !== undefined ||
              slot.resolvedOpts.timeStyle !== undefined ||
              slot.resolvedOpts.timeZoneName !== undefined;
            if (!needsCustom && slot.instance && typeof slot.instance.formatToParts === 'function') {
              return slot.instance.formatToParts(d);
            }
            const rawParts = formatDateWithOptionsToParts(d, slot.resolvedOpts);
            const ns = slot.resolvedOpts.numberingSystem;
            if (ns && ns !== 'latn') {
              return rawParts.map((p) => ({ ...p, value: applyNumberingSystem(p.value, ns) }));
            }
            return rawParts;
          };
          Object.defineProperty(newProto, 'formatToParts', {
            value: makeNonConstructableAccessor(formatToPartsImpl, 'formatToParts', 1),
            writable: true,
            enumerable: false,
            configurable: true
          });

          // Classify a value into a Temporal type name, or null for non-Temporal
          function temporalTypeName(value) {
            if (typeof value !== 'object' || value === null) return null;
            const tag = Object.prototype.toString.call(value);
            const m = tag.match(/^\[object Temporal\.(\w+)\]$/);
            return m ? m[1] : null;
          }

          // Format a single value (Date or Temporal) using this slot
          function formatSingleValue(slot, value) {
            if (isTemporalInstantValue(value)) return formatTemporalInstant(slot, value, false);
            if (isTemporalPlainTimeValue(value)) return formatTemporalPlainTime(slot, value, false);
            if (isTemporalPlainDateTimeValue(value)) return formatTemporalPlainDateTime(slot, value, false);
            if (isTemporalPlainDateValue(value)) return formatTemporalPlainDate(slot, value, false);
            if (isTemporalPlainYearMonthValue(value)) return formatTemporalPlainYearMonth(slot, value, false);
            if (isTemporalPlainMonthDayValue(value)) return formatTemporalPlainMonthDay(slot, value, false);
            const d = normalizeDateTimeFormatInput(value);
            if (isNaN(d.getTime())) throw new RangeError('Invalid time value');
            const needsCustom = slot.needsDefault || slot.resolvedOpts.dayPeriod !== undefined ||
              slot.resolvedOpts.fractionalSecondDigits !== undefined ||
              (slot.resolvedOpts.numberingSystem && slot.resolvedOpts.numberingSystem !== 'latn') ||
              (slot.resolvedOpts.era !== undefined &&
                slot.resolvedOpts.calendar !== 'gregory' &&
                slot.resolvedOpts.calendar !== 'iso8601') ||
              slot.resolvedOpts.calendar === 'hebrew' ||
              slot.resolvedOpts.calendar === 'chinese' ||
              slot.resolvedOpts.calendar === 'dangi';
            if (!needsCustom && slot.instance && typeof slot.instance.format === 'function') {
              return slot.instance.format(d);
            }
            return formatDateWithOptions(d, slot.resolvedOpts);
          }

          // Format a single value to parts
          function formatSingleValueToParts(slot, value) {
            if (isTemporalInstantValue(value)) return formatTemporalInstant(slot, value, true);
            if (isTemporalPlainTimeValue(value)) return formatTemporalPlainTime(slot, value, true);
            if (isTemporalPlainDateTimeValue(value)) return formatTemporalPlainDateTime(slot, value, true);
            if (isTemporalPlainDateValue(value)) return formatTemporalPlainDate(slot, value, true);
            if (isTemporalPlainYearMonthValue(value)) return formatTemporalPlainYearMonth(slot, value, true);
            if (isTemporalPlainMonthDayValue(value)) return formatTemporalPlainMonthDay(slot, value, true);
            const d = normalizeDateTimeFormatInput(value);
            if (isNaN(d.getTime())) throw new RangeError('Invalid time value');
            if (slot.instance && !slot.needsDefault &&
                slot.resolvedOpts.fractionalSecondDigits === undefined &&
                (slot.resolvedOpts.era === undefined ||
                  slot.resolvedOpts.calendar === 'gregory' ||
                  slot.resolvedOpts.calendar === 'iso8601') &&
                slot.resolvedOpts.calendar !== 'hebrew' &&
                slot.resolvedOpts.calendar !== 'chinese' &&
                slot.resolvedOpts.calendar !== 'dangi' &&
                typeof slot.instance.formatToParts === 'function') {
              return slot.instance.formatToParts(d);
            }
            return formatDateWithOptionsToParts(d, slot.resolvedOpts);
          }

          // Core formatRange logic: returns {startStr, endStr, separator}
          // or {collapsed: str} when practically equal
          function formatRangeCore(slot, startDate, endDate) {
            const startType = temporalTypeName(startDate);
            const endType = temporalTypeName(endDate);
            // Mixed Temporal types → TypeError
            if (startType !== endType) {
              throw new TypeError('formatRange: incompatible argument types');
            }
            const startStr = formatSingleValue(slot, startDate);
            const endStr = formatSingleValue(slot, endDate);
            // Practically equal: same formatted output
            if (startStr === endStr) return { collapsed: startStr };
            return { startStr, endStr };
          }

          // Smart range collapsing: given two part arrays, find the longest shared
          // prefix and suffix and mark them as 'shared', leaving only the differing
          // middle parts as startRange/endRange.
          // Returns a flat array of {type, value, source} objects.
          function smartRangeParts(startParts, endParts, separator) {
            const sLen = startParts.length;
            const eLen = endParts.length;
            // Find shared prefix length
            let prefixLen = 0;
            while (
              prefixLen < sLen && prefixLen < eLen &&
              startParts[prefixLen].type === endParts[prefixLen].type &&
              startParts[prefixLen].value === endParts[prefixLen].value
            ) prefixLen++;
            // Find shared suffix length (don't overlap with prefix)
            let suffixLen = 0;
            while (
              suffixLen < sLen - prefixLen && suffixLen < eLen - prefixLen &&
              startParts[sLen - 1 - suffixLen].type === endParts[eLen - 1 - suffixLen].type &&
              startParts[sLen - 1 - suffixLen].value === endParts[eLen - 1 - suffixLen].value
            ) suffixLen++;
            const result = [];
            for (let i = 0; i < prefixLen; i++) result.push({ ...startParts[i], source: 'shared' });
            for (let i = prefixLen; i < sLen - suffixLen; i++) result.push({ ...startParts[i], source: 'startRange' });
            result.push({ type: 'literal', value: separator, source: 'shared' });
            for (let i = prefixLen; i < eLen - suffixLen; i++) result.push({ ...endParts[i], source: 'endRange' });
            for (let i = sLen - suffixLen; i < sLen; i++) result.push({ ...startParts[i], source: 'shared' });
            return result;
          }

          // Check if two Temporal values have the same calendar
          function temporalCalendarMatches(a, b) {
            try {
              const ca = temporalCalendarId(a);
              const cb = temporalCalendarId(b);
              return ca === cb;
            } catch (_e) { return true; }
          }

          // Get the range separator from the underlying instance's formatRangeToParts
          function getRangeSeparator(slot) {
            if (slot.instance && typeof slot.instance.formatRangeToParts === 'function') {
              try {
                const parts = slot.instance.formatRangeToParts(new Date(86400000), new Date(366 * 86400000));
                const sep = parts.find((p) => p.type === 'literal' && p.source === 'shared');
                if (sep) return sep.value;
              } catch (_e) {}
            }
            return ' \u2013 ';
          }

          // Whether to use smart prefix/suffix collapsing for range formatting.
          // Smart collapsing applies when month is a named style (long/short/narrow),
          // matching CLDR range pattern behavior.
          function useSmartCollapsing(slot) {
            const m = slot.resolvedOpts.month;
            if (m === 'long' || m === 'short' || m === 'narrow') return true;
            // dateStyle full/long/medium use named months
            const ds = slot.resolvedOpts.dateStyle;
            return ds === 'full' || ds === 'long' || ds === 'medium';
          }

          // Whether to use the underlying instance for range formatting
          // (only when we don't need custom formatting and month is numeric)
          function canUseInstanceForRange(slot) {
            return slot.instance &&
              !useSmartCollapsing(slot) &&
              slot.resolvedOpts.fractionalSecondDigits === undefined &&
              slot.resolvedOpts.dateStyle === undefined &&
              slot.resolvedOpts.timeStyle === undefined &&
              slot.resolvedOpts.calendar !== 'chinese' &&
              slot.resolvedOpts.calendar !== 'dangi' &&
              slot.resolvedOpts.hour12 === undefined &&
              slot.resolvedOpts.hourCycle === undefined;
          }

          // Core formatRangeToParts logic (shared by formatRange and formatRangeToParts)
          function formatRangeToPartsCore(slot, startDate, endDate) {
            const startType = temporalTypeName(startDate);
            // For plain Date objects with numeric month, use underlying instance
            if (startType === null && canUseInstanceForRange(slot) &&
                typeof slot.instance.formatRangeToParts === 'function') {
              const start = normalizeDateTimeFormatInput(startDate);
              const end = normalizeDateTimeFormatInput(endDate);
              if (!isNaN(start.getTime()) && !isNaN(end.getTime())) {
                return slot.instance.formatRangeToParts(start, end);
              }
            }
            // Custom path: format both sides
            const startParts = formatSingleValueToParts(slot, startDate);
            const endParts = formatSingleValueToParts(slot, endDate);
            const startStr = startParts.map((p) => p.value).join('');
            const endStr = endParts.map((p) => p.value).join('');
            if (startStr === endStr) {
              return startParts.map((p) => ({ ...p, source: 'shared' }));
            }
            const sep = getRangeSeparator(slot);
            // Determine if smart collapsing should be applied
            let applySmartCollapsing = false;
            if (startType !== null) {
              // PlainDateTime and Instant: apply smart collapsing (date portion shared when same)
              // PlainDate, PlainYearMonth, PlainMonthDay, PlainTime: no smart collapsing
              applySmartCollapsing = startType === 'PlainDateTime' || startType === 'Instant';
            } else if (useSmartCollapsing(slot)) {
              // Named month: apply smart collapsing only when years match
              // (CLDR: different year → no collapsing)
              const startD = normalizeDateTimeFormatInput(startDate);
              const endD = normalizeDateTimeFormatInput(endDate);
              applySmartCollapsing = !isNaN(startD.getTime()) && !isNaN(endD.getTime()) &&
                startD.getFullYear() === endD.getFullYear();
            }
            if (applySmartCollapsing) {
              return smartRangeParts(startParts, endParts, sep);
            }
            // Simple range: no smart collapsing
            return [
              ...startParts.map((p) => ({ ...p, source: 'startRange' })),
              { type: 'literal', value: sep, source: 'shared' },
              ...endParts.map((p) => ({ ...p, source: 'endRange' })),
            ];
          }

          // formatRange method
          const formatRangeImpl = function(startDate, endDate) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatRange called on incompatible receiver');
            }
            if (startDate === undefined || endDate === undefined) {
              throw new TypeError('startDate and endDate are required');
            }
            // ToDateTimeFormattable: call valueOf on non-Temporal first, then check types
            const startIsTemp = temporalTypeName(startDate) !== null;
            const endIsTemp = temporalTypeName(endDate) !== null;
            if (!startIsTemp) { const _ = Number(startDate); }
            if (!endIsTemp) { const _ = Number(endDate); }
            if (startIsTemp !== endIsTemp) {
              throw new TypeError('formatRange: incompatible argument types');
            }
            const startType = temporalTypeName(startDate);
            const endType = temporalTypeName(endDate);
            if (startType !== endType) {
              throw new TypeError('formatRange: incompatible argument types');
            }
            if (startType !== null && startType !== 'Instant' && startType !== 'PlainTime') {
              if (!temporalCalendarMatches(startDate, endDate)) {
                throw new RangeError('formatRange: calendar mismatch');
              }
            }
            // Derive from formatRangeToParts for consistency
            return formatRangeToPartsCore(slot, startDate, endDate).map((p) => p.value).join('');
          };
          Object.defineProperty(formatRangeImpl, 'length', { value: 2, writable: false, enumerable: false, configurable: true });
          Object.defineProperty(newProto, 'formatRange', {
            value: makeNonConstructableAccessor(formatRangeImpl, 'formatRange'),
            writable: true,
            enumerable: false,
            configurable: true
          });
          // Ensure the proxy wrapper also has length 2
          Object.defineProperty(newProto.formatRange, 'length', { value: 2, writable: false, enumerable: false, configurable: true });

          // formatRangeToParts method
          const formatRangeToPartsImpl = function(startDate, endDate) {
            const slot = dtfSlots.get(this);
            if (!slot) {
              throw new TypeError('Method Intl.DateTimeFormat.prototype.formatRangeToParts called on incompatible receiver');
            }
            if (startDate === undefined || endDate === undefined) {
              throw new TypeError('startDate and endDate are required');
            }
            const startIsTemp = temporalTypeName(startDate) !== null;
            const endIsTemp = temporalTypeName(endDate) !== null;
            if (!startIsTemp) { const _ = Number(startDate); }
            if (!endIsTemp) { const _ = Number(endDate); }
            if (startIsTemp !== endIsTemp) {
              throw new TypeError('formatRangeToParts: incompatible argument types');
            }
            const startType = temporalTypeName(startDate);
            const endType = temporalTypeName(endDate);
            if (startType !== endType) {
              throw new TypeError('formatRangeToParts: incompatible argument types');
            }
            if (startType !== null && startType !== 'Instant' && startType !== 'PlainTime') {
              if (!temporalCalendarMatches(startDate, endDate)) {
                throw new RangeError('formatRangeToParts: calendar mismatch');
              }
            }
            return formatRangeToPartsCore(slot, startDate, endDate);
          };
          Object.defineProperty(formatRangeToPartsImpl, 'length', { value: 2, writable: false, enumerable: false, configurable: true });
          Object.defineProperty(newProto, 'formatRangeToParts', {
            value: makeNonConstructableAccessor(formatRangeToPartsImpl, 'formatRangeToParts'),
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(newProto.formatRangeToParts, 'length', { value: 2, writable: false, enumerable: false, configurable: true });

          Object.defineProperty(newProto, 'constructor', {
            value: WrappedDTF,
            writable: true,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(newProto, Symbol.toStringTag, {
            value: 'Intl.DateTimeFormat',
            writable: false,
            enumerable: false,
            configurable: true
          });

          Object.defineProperty(WrappedDTF, 'prototype', {
            value: newProto,
            writable: false,
            enumerable: false,
            configurable: false
          });

          // Replace Intl.DateTimeFormat
          Object.defineProperty(Intl, 'DateTimeFormat', {
            value: WrappedDTF,
            writable: true,
            enumerable: false,
            configurable: true
          });
          Object.defineProperty(Intl, '__agentjs_intrinsic_DateTimeFormat__', {
            value: WrappedDTF,
            writable: false,
            enumerable: false,
            configurable: false
          });
        })();
        "#,
    ))?;
    Ok(())
}
