//! Boa's implementation of ECMAScript's global `Intl` object.
//!
//! The `Intl` namespace object contains several constructors as well as functionality common to the
//! internationalization constructors and other language sensitive functions. Collectively, they
//! comprise the ECMAScript Internationalization API, which provides language sensitive string
//! comparison, number formatting, date and time formatting, and more.
//!
//! More information:
//!  - [ECMAScript reference][spec]
//!  - [MDN documentation][mdn]
//!
//!
//! [spec]: https://tc39.es/ecma402/#intl-object
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl

use crate::{
    Context, JsArgs, JsData, JsResult, JsString, JsValue,
    builtins::{Array, BuiltInBuilder, BuiltInObject, IntrinsicObject},
    context::{icu::IntlProvider, intrinsics::Intrinsics},
    js_string,
    object::JsObject,
    property::Attribute,
    realm::Realm,
    string::StaticJsStrings,
    symbol::JsSymbol,
};

use boa_gc::{Finalize, Trace};
use icu_provider::{DataMarker, DataMarkerAttributes};
use static_assertions::const_assert;

pub(crate) mod collator;
pub(crate) mod date_time_format;
pub(crate) mod display_names;
pub(crate) mod duration_format;
pub(crate) mod list_format;
pub(crate) mod locale;
pub(crate) mod number_format;
pub(crate) mod plural_rules;
pub(crate) mod relative_time_format;
pub(crate) mod segmenter;

pub(crate) use self::{
    collator::Collator, date_time_format::DateTimeFormat, display_names::DisplayNames,
    duration_format::DurationFormat, list_format::ListFormat, locale::Locale,
    number_format::NumberFormat, plural_rules::PluralRules,
    relative_time_format::RelativeTimeFormat, segmenter::Segmenter,
};

mod options;

// No singletons are allowed as lang markers.
// Hopefully, we'll be able to migrate this to the definition of `Service` in the future
// (https://github.com/rust-lang/rust/issues/76560)
const_assert! {!<Collator as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<DisplayNames as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<ListFormat as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<NumberFormat as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<RelativeTimeFormat as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<DurationFormat as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<PluralRules as Service>::LangMarker::INFO.is_singleton}
const_assert! {!<Segmenter as Service>::LangMarker::INFO.is_singleton}

/// JavaScript `Intl` object.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
#[boa_gc(unsafe_empty_trace)]
pub struct Intl {
    fallback_symbol: JsSymbol,
}

impl Intl {
    /// Gets this realm's `Intl` object's `[[FallbackSymbol]]` slot.
    #[must_use]
    pub fn fallback_symbol(&self) -> JsSymbol {
        self.fallback_symbol.clone()
    }

    pub(crate) fn new() -> Option<Self> {
        let fallback_symbol = JsSymbol::new(Some(js_string!("IntlLegacyConstructedSymbol")))?;
        Some(Self { fallback_symbol })
    }
}

impl IntrinsicObject for Intl {
    fn init(realm: &Realm) {
        BuiltInBuilder::with_intrinsic::<Self>(realm)
            .static_property(
                JsSymbol::to_string_tag(),
                Self::NAME,
                Attribute::CONFIGURABLE,
            )
            .static_property(
                Collator::NAME,
                realm.intrinsics().constructors().collator().constructor(),
                Collator::ATTRIBUTE,
            )
            .static_property(
                ListFormat::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .list_format()
                    .constructor(),
                ListFormat::ATTRIBUTE,
            )
            .static_property(
                Locale::NAME,
                realm.intrinsics().constructors().locale().constructor(),
                Locale::ATTRIBUTE,
            )
            .static_property(
                Segmenter::NAME,
                realm.intrinsics().constructors().segmenter().constructor(),
                Segmenter::ATTRIBUTE,
            )
            .static_property(
                PluralRules::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .plural_rules()
                    .constructor(),
                PluralRules::ATTRIBUTE,
            )
            .static_property(
                DisplayNames::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .display_names()
                    .constructor(),
                DisplayNames::ATTRIBUTE,
            )
            .static_property(
                RelativeTimeFormat::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .relative_time_format()
                    .constructor(),
                RelativeTimeFormat::ATTRIBUTE,
            )
            .static_property(
                DurationFormat::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .duration_format()
                    .constructor(),
                DurationFormat::ATTRIBUTE,
            )
            .static_property(
                DateTimeFormat::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .date_time_format()
                    .constructor(),
                DateTimeFormat::ATTRIBUTE,
            )
            .static_property(
                NumberFormat::NAME,
                realm
                    .intrinsics()
                    .constructors()
                    .number_format()
                    .constructor(),
                NumberFormat::ATTRIBUTE,
            )
            .static_method(
                Self::get_canonical_locales,
                js_string!("getCanonicalLocales"),
                1,
            )
            .static_method(
                Self::supported_values_of,
                js_string!("supportedValuesOf"),
                1,
            )
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        intrinsics.objects().intl().upcast()
    }
}

impl BuiltInObject for Intl {
    const NAME: JsString = StaticJsStrings::INTL;
}

impl Intl {
    /// `Intl.getCanonicalLocales ( locales )`
    ///
    /// Returns an array containing the canonical locale names.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN docs][mdn]
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-intl.getcanonicallocales
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/getCanonicalLocales
    pub(crate) fn get_canonical_locales(
        _: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let locales = args.get_or_undefined(0);

        // 1. Let ll be ? CanonicalizeLocaleList(locales).
        let ll = locale::canonicalize_locale_list(locales, context)?;

        // 2. Return CreateArrayFromList(ll).
        Ok(JsValue::new(Array::create_array_from_list(
            ll.into_iter().map(|loc| js_string!(loc.to_string()).into()),
            context,
        )))
    }

    /// `Intl.supportedValuesOf ( key )`
    ///
    /// The `Intl.supportedValuesOf` function returns a sorted array containing the supported values
    /// for the given key.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-intl.supportedvaluesof
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/supportedValuesOf
    pub(crate) fn supported_values_of(
        _: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let key = args.get_or_undefined(0);
        if key.is_symbol() {
            return Err(crate::error::JsNativeError::typ()
                .with_message("Intl.supportedValuesOf: key cannot be a symbol")
                .into());
        }
        let key_str = key.to_string(context)?.to_std_string_escaped();

        let values: Vec<&str> = match key_str.as_str() {
            "calendar" => vec![
                "buddhist",
                "chinese",
                "coptic",
                "dangi",
                "ethioaa",
                "ethiopic",
                "gregory",
                "hebrew",
                "indian",
                "islamic-civil",
                "islamic-tbla",
                "islamic-umalqura",
                "iso8601",
                "japanese",
                "persian",
                "roc",
            ],
            "collation" => vec![
                "big5han", "compat", "dict", "direct", "ducet", "emoji", "eor", "gb2312",
                "phonebk", "phonetic", "pinyin", "reformed", "searchjl", "stroke", "trad",
                "unihan", "zhuyin",
            ],
            "currency" => vec![
                "ADP", "AED", "AFA", "AFN", "ALK", "ALL", "AMD", "ANG", "AOA", "AOK", "AON", "AOR",
                "ARA", "ARL", "ARM", "ARP", "ARS", "ATS", "AUD", "AWG", "AZM", "AZN", "BAD", "BAM",
                "BAN", "BBD", "BDT", "BEC", "BEF", "BEL", "BGL", "BGM", "BGN", "BGO", "BHD", "BIF",
                "BMD", "BND", "BOB", "BOL", "BOP", "BOV", "BRB", "BRC", "BRE", "BRL", "BRN", "BRR",
                "BRZ", "BSD", "BTN", "BUK", "BWP", "BYB", "BYN", "BYR", "BZD", "CAD", "CDF", "CHC",
                "CHE", "CHF", "CHW", "CLE", "CLF", "CLP", "CNH", "CNX", "CNY", "COP", "COU", "CRC",
                "CSD", "CSK", "CUC", "CUP", "CVE", "CYP", "CZK", "DDM", "DEM", "DJF", "DKK", "DOP",
                "DZD", "ECS", "ECV", "EEK", "EGP", "ERN", "ESA", "ESB", "ESP", "ETB", "EUR", "FIM",
                "FJD", "FKP", "FRF", "GBP", "GEK", "GEL", "GHC", "GHS", "GIP", "GMD", "GNF", "GNS",
                "GQE", "GRD", "GTQ", "GWE", "GWP", "GYD", "HKD", "HNL", "HRD", "HRK", "HTG", "HUF",
                "IDR", "IEP", "ILP", "ILR", "ILS", "INR", "IQD", "IRR", "ISJ", "ISK", "ITL", "JMD",
                "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRH", "KRO", "KRW", "KWD", "KYD",
                "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LTL", "LTT", "LUC", "LUF", "LUL", "LVL",
                "LVR", "LYD", "MAD", "MAF", "MCF", "MDC", "MDL", "MGA", "MGF", "MKD", "MKN", "MLF",
                "MMK", "MNT", "MOP", "MRO", "MRU", "MTL", "MTP", "MUR", "MVP", "MVR", "MWK", "MXN",
                "MXP", "MXV", "MYR", "MZE", "MZM", "MZN", "NAD", "NGN", "NIC", "NIO", "NLG", "NOK",
                "NPR", "NZD", "OMR", "PAB", "PEI", "PEN", "PES", "PGK", "PHP", "PKR", "PLN", "PLZ",
                "PTE", "PYG", "QAR", "RHD", "ROL", "RON", "RSD", "RUB", "RUR", "RWF", "SAR", "SBD",
                "SCR", "SDD", "SDG", "SDP", "SEK", "SGD", "SHP", "SIT", "SKK", "SLE", "SLL", "SOS",
                "SRD", "SRG", "SSP", "STD", "STN", "SUR", "SVC", "SYP", "SZL", "THB", "TJR", "TJS",
                "TMM", "TMT", "TND", "TOP", "TPE", "TRL", "TRY", "TTD", "TWD", "TZS", "UAH", "UAK",
                "UGS", "UGX", "USD", "USN", "USS", "UYI", "UYP", "UYU", "UYW", "UZS", "VEB", "VED",
                "VEF", "VES", "VND", "VNN", "VUV", "WST", "XAF", "XAG", "XAU", "XBA", "XBB", "XBC",
                "XBD", "XCD", "XDR", "XEU", "XFO", "XFU", "XOF", "XPD", "XPF", "XPT", "XRE", "XSU",
                "XTS", "XUA", "XXX", "YDD", "YER", "YUD", "YUM", "YUN", "YUR", "ZAL", "ZAR", "ZMK",
                "ZMW", "ZRN", "ZRZ", "ZWD", "ZWL", "ZWR",
            ],
            "numberingSystem" => vec![
                "adlm", "ahom", "arab", "arabext", "armn", "armnlow", "bali", "beng", "bhks",
                "brah", "cakm", "cham", "cyrl", "deva", "diak", "ethi", "fullwide", "gara", "geor",
                "gong", "gonm", "grek", "greklow", "gujr", "guru", "hanidays", "hanidec", "hans",
                "hansfin", "hant", "hantfin", "hebr", "hmng", "hmnp", "java", "jpan", "jpanfin",
                "jpanyear", "kali", "kawi", "khmr", "knda", "lana", "lanatham", "laoo", "latn",
                "lepc", "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym",
                "modi", "mong", "mroo", "mtei", "mymr", "mymrshan", "mymrtlng", "nagm", "newa",
                "nkoo", "olck", "orya", "osma", "outlined", "rohg", "roman", "romanlow", "saur",
                "segment", "shrd", "sind", "sinh", "sora", "sund", "sundlatn", "takr", "talu",
                "taml", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa", "vaii", "wara", "wcho",
            ],
            "timeZone" => vec!["UTC"],
            "unit" => vec![
                "acre",
                "bit",
                "byte",
                "celsius",
                "centimeter",
                "day",
                "degree",
                "fahrenheit",
                "fluid-ounce",
                "foot",
                "gallon",
                "gigabit",
                "gigabyte",
                "gram",
                "hectare",
                "hour",
                "inch",
                "kilobit",
                "kilobyte",
                "kilogram",
                "kilometer",
                "liter",
                "megabit",
                "megabyte",
                "meter",
                "microsecond",
                "mile",
                "mile-scandinavian",
                "milliliter",
                "millimeter",
                "millisecond",
                "minute",
                "month",
                "nanosecond",
                "ounce",
                "percent",
                "petabyte",
                "pound",
                "second",
                "stone",
                "terabit",
                "terabyte",
                "week",
                "yard",
                "year",
            ],
            _ => {
                return Err(crate::error::JsNativeError::range()
                    .with_message(format!("Intl.supportedValuesOf: invalid key '{key_str}'"))
                    .into());
            }
        };

        let mut values: Vec<JsValue> = values.into_iter().map(|v| js_string!(v).into()).collect();
        values.sort_by(|a, b| {
            let a = a.to_string(context).unwrap();
            let b = b.to_string(context).unwrap();
            a.as_str().cmp(&b.as_str())
        });

        Ok(Array::create_array_from_list(values, context).into())
    }
}

/// A service component that is part of the `Intl` API.
///
/// This needs to be implemented for every `Intl` service in order to use the functions
/// defined in `locale::utils`, such as locale resolution and selection.
trait Service {
    /// The data marker used by [`resolve_locale`][locale::resolve_locale] to decide
    /// which locales are supported by this service.
    type LangMarker: DataMarker;

    /// The attributes used to resolve the locale.
    const ATTRIBUTES: &'static DataMarkerAttributes = DataMarkerAttributes::empty();

    /// The set of options used in the [`Service::resolve`] method to resolve the provided
    /// locale.
    type LocaleOptions;

    /// Resolves the final value of `locale` from a set of `options`.
    ///
    /// The provided `options` will also be modified with the final values, in case there were
    /// changes in the resolution algorithm.
    ///
    /// # Note
    ///
    /// - A correct implementation must ensure `locale` and `options` are both written with the
    ///   new final values.
    /// - If the implementor service doesn't contain any `[[RelevantExtensionKeys]]`, this can be
    ///   skipped.
    fn resolve(
        _locale: &mut icu_locale::Locale,
        _options: &mut Self::LocaleOptions,
        _provider: &IntlProvider,
    ) {
    }
}
