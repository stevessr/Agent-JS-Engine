//! This module implements the global `Intl.DateTimeFormat` object.
//!
//! `Intl.DateTimeFormat` is a built-in object that has properties and methods for date and time i18n.
//!
//! More information:
//!  - [ECMAScript reference][spec]
//!
//! [spec]: https://tc39.es/ecma402/#datetimeformat-objects

use crate::{
    Context, JsArgs, JsData, JsResult, JsString, JsValue,
    builtins::{
        BuiltInBuilder, BuiltInConstructor, BuiltInObject, IntrinsicObject, OrdinaryObject,
        options::{OptionType, get_option},
    },
    context::intrinsics::{Intrinsics, StandardConstructor, StandardConstructors},
    error::JsNativeError,
    js_string,
    object::{JsObject, internal_methods::get_prototype_from_constructor},
    property::Attribute,
    realm::Realm,
    string::StaticJsStrings,
    symbol::JsSymbol,
};

use boa_gc::{Finalize, Trace};
use icu_calendar::preferences::CalendarAlgorithm;
use icu_datetime::preferences::HourCycle;
use icu_locale::{Locale, extensions::unicode::Value};

use super::{
    Service,
    locale::{canonicalize_locale_list, resolve_locale},
    options::IntlOptions,
};

/// JavaScript `Intl.DateTimeFormat` object.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) struct DateTimeFormat {
    initialized: bool,
    locale: JsString,
    calendar: JsString,
    numbering_system: JsString,
    time_zone: JsString,
    weekday: JsString,
    era: JsString,
    year: JsString,
    month: JsString,
    day: JsString,
    day_period: JsString,
    hour: JsString,
    minute: JsString,
    second: JsString,
    fractional_second_digits: JsString,
    time_zone_name: JsString,
    hour_cycle: JsString,
    pattern: JsString,
    bound_format: JsString,
}

impl Service for DateTimeFormat {
    type LangMarker = icu_decimal::provider::DecimalSymbolsV1;

    type LocaleOptions = super::number_format::NumberFormatLocaleOptions;

    fn resolve(
        locale: &mut Locale,
        options: &mut Self::LocaleOptions,
        provider: &crate::context::icu::IntlProvider,
    ) {
        super::number_format::NumberFormat::resolve(locale, options, provider);
    }
}

impl IntrinsicObject for DateTimeFormat {
    fn init(realm: &Realm) {
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_method(
                Self::supported_locales_of,
                js_string!("supportedLocalesOf"),
                1,
            )
            .property(
                JsSymbol::to_string_tag(),
                js_string!("Intl.DateTimeFormat"),
                Attribute::CONFIGURABLE,
            )
            .method(Self::format, js_string!("format"), 1)
            .method(Self::format_to_parts, js_string!("formatToParts"), 1)
            .method(Self::resolved_options, js_string!("resolvedOptions"), 0)
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
}

impl BuiltInObject for DateTimeFormat {
    const NAME: JsString = StaticJsStrings::DATE_TIME_FORMAT;
}

impl DateTimeFormat {
    fn supported_locales_of(
        _: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let locales = args.get_or_undefined(0);
        let options = args.get_or_undefined(1);
        let requested_locales = canonicalize_locale_list(locales, context)?;
        // For now, minimal implementation using canonicalize_locale_list
        Ok(crate::builtins::Array::create_array_from_list(
            requested_locales.into_iter().map(|l| js_string!(l.to_string()).into()),
            context,
        ).into())
    }

    pub(crate) fn format(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let date = args.get_or_undefined(0);
        let object = this.as_object();
        let _dtf = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`format` can only be called on a `DateTimeFormat` object")
            })?;

        // Minimal implementation: call native Date.prototype.toString or similar for now
        // This is where we should use icu_datetime.
        let d = if date.is_undefined() {
            crate::builtins::Date::now(&JsValue::undefined(), &[], context)?
        } else {
            date.clone()
        };
        
        Ok(d.to_string(context)?.into())
    }

    fn format_to_parts(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let date = args.get_or_undefined(0);
        let object = this.as_object();
        let _dtf = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`formatToParts` can only be called on a `DateTimeFormat` object")
            })?;

        let d = if date.is_undefined() {
            crate::builtins::Date::now(&JsValue::undefined(), &[], context)?
        } else {
            date.clone()
        };

        let parts = vec![
            JsObject::with_object_proto(context.intrinsics())
        ];
        parts[0].create_data_property_or_throw(js_string!("type"), js_string!("literal"), context)?;
        parts[0].create_data_property_or_throw(js_string!("value"), d.to_string(context)?, context)?;

        Ok(crate::builtins::Array::create_array_from_list(
            parts.into_iter().map(JsValue::from),
            context,
        ).into())
    }

    fn resolved_options(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let object = this.as_object();
        let dtf = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`resolvedOptions` can only be called on a `DateTimeFormat` object")
            })?;

        let options = JsObject::with_object_proto(context.intrinsics());
        options.create_data_property_or_throw(js_string!("locale"), dtf.locale.clone(), context)?;
        options.create_data_property_or_throw(js_string!("calendar"), dtf.calendar.clone(), context)?;
        options.create_data_property_or_throw(js_string!("numberingSystem"), dtf.numbering_system.clone(), context)?;
        options.create_data_property_or_throw(js_string!("timeZone"), dtf.time_zone.clone(), context)?;
        // TODO: add other options
        Ok(options.into())
    }
}

impl BuiltInConstructor for DateTimeFormat {
    const CONSTRUCTOR_ARGUMENTS: usize = 0;
    const PROTOTYPE_STORAGE_SLOTS: usize = 0;
    const CONSTRUCTOR_STORAGE_SLOTS: usize = 0;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::date_time_format;
    /// The `Intl.DateTimeFormat` constructor is the `%DateTimeFormat%` intrinsic object and a standard built-in property of the `Intl` object.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma402/#datetimeformat-objects
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/DateTimeFormat
    fn constructor(
        new_target: &JsValue,
        _args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, let newTarget be the active function object, else let newTarget be NewTarget.
        let new_target = &if new_target.is_undefined() {
            context
                .active_function_object()
                .unwrap_or_else(|| {
                    context
                        .intrinsics()
                        .constructors()
                        .date_time_format()
                        .constructor()
                })
                .into()
        } else {
            new_target.clone()
        };

        let locales = _args.get_or_undefined(0);
        let options = _args.get_or_undefined(1);

        let requested_locales = canonicalize_locale_list(locales, context)?;
        let _options = to_date_time_options(options, &DateTimeReqs::AnyAll, &DateTimeReqs::AnyAll, context)?;

        let matcher = get_option(&_options, js_string!("localeMatcher"), context)?.unwrap_or_default();
        let numbering_system = get_option::<icu_decimal::preferences::NumberingSystem>(&_options, js_string!("numberingSystem"), context)?;

        let mut intl_options = IntlOptions {
            matcher,
            service_options: super::number_format::NumberFormatLocaleOptions {
                numbering_system: numbering_system.map(Value::from),
            },
        };

        let locale = resolve_locale::<Self>(
            requested_locales,
            &mut intl_options,
            context.intl_provider(),
        )?;

        let calendar = get_option::<CalendarAlgorithm>(&_options, js_string!("calendar"), context)?;
        let time_zone = get_option::<JsString>(&_options, js_string!("timeZone"), context)?;

        let prototype = get_prototype_from_constructor(
            new_target,
            StandardConstructors::date_time_format,
            context,
        )?;
        
        let dtf = Self {
            initialized: true,
            locale: js_string!(locale.to_string()),
            calendar: calendar.map(|c| js_string!(Value::from(c).to_string())).unwrap_or(js_string!("gregory")),
            numbering_system: intl_options.service_options.numbering_system.map(|ns| js_string!(ns.to_string())).unwrap_or(js_string!("latn")),
            time_zone: time_zone.unwrap_or(js_string!("UTC")),
            weekday: get_option::<JsString>(&_options, js_string!("weekday"), context)?.unwrap_or_default(),
            era: get_option::<JsString>(&_options, js_string!("era"), context)?.unwrap_or_default(),
            year: get_option::<JsString>(&_options, js_string!("year"), context)?.unwrap_or_default(),
            month: get_option::<JsString>(&_options, js_string!("month"), context)?.unwrap_or_default(),
            day: get_option::<JsString>(&_options, js_string!("day"), context)?.unwrap_or_default(),
            day_period: get_option::<JsString>(&_options, js_string!("dayPeriod"), context)?.unwrap_or_default(),
            hour: get_option::<JsString>(&_options, js_string!("hour"), context)?.unwrap_or_default(),
            minute: get_option::<JsString>(&_options, js_string!("minute"), context)?.unwrap_or_default(),
            second: get_option::<JsString>(&_options, js_string!("second"), context)?.unwrap_or_default(),
            fractional_second_digits: get_option::<JsString>(&_options, js_string!("fractionalSecondDigits"), context)?.unwrap_or_default(),
            time_zone_name: get_option::<JsString>(&_options, js_string!("timeZoneName"), context)?.unwrap_or_default(),
            hour_cycle: get_option::<JsString>(&_options, js_string!("hourCycle"), context)?.unwrap_or_default(),
            pattern: js_string!(),
            bound_format: js_string!(),
        };

        let date_time_format = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            dtf,
        );

        Ok(date_time_format.into())
    }
}

/// Represents the `required` and `defaults` arguments in the abstract operation
/// `toDateTimeOptions`.
///
/// Since `required` and `defaults` differ only in the `any` and `all` variants,
/// we combine both in a single variant `AnyAll`.
#[allow(unused)]
#[derive(Debug, PartialEq)]
pub(crate) enum DateTimeReqs {
    Date,
    Time,
    AnyAll,
}

/// The abstract operation `toDateTimeOptions` is called with arguments `options`, `required` and
/// `defaults`.
///
/// More information:
///  - [ECMAScript reference][spec]
///
/// [spec]: https://tc39.es/ecma402/#sec-todatetimeoptions
#[allow(unused)]
pub(crate) fn to_date_time_options(
    options: &JsValue,
    required: &DateTimeReqs,
    defaults: &DateTimeReqs,
    context: &mut Context,
) -> JsResult<JsObject> {
    // 1. If options is undefined, let options be null;
    // otherwise let options be ? ToObject(options).
    // 2. Let options be ! OrdinaryObjectCreate(options).
    let options = if options.is_undefined() {
        None
    } else {
        Some(options.to_object(context)?)
    };
    let options = JsObject::from_proto_and_data_with_shared_shape(
        context.root_shape(),
        options,
        OrdinaryObject,
    );

    // 3. Let needDefaults be true.
    let mut need_defaults = true;

    // 4. If required is "date" or "any", then
    if [DateTimeReqs::Date, DateTimeReqs::AnyAll].contains(required) {
        // a. For each property name prop of « "weekday", "year", "month", "day" », do
        for property in [
            js_string!("weekday"),
            js_string!("year"),
            js_string!("month"),
            js_string!("day"),
        ] {
            // i. Let value be ? Get(options, prop).
            let value = options.get(property, context)?;

            // ii. If value is not undefined, let needDefaults be false.
            if !value.is_undefined() {
                need_defaults = false;
            }
        }
    }

    // 5. If required is "time" or "any", then
    if [DateTimeReqs::Time, DateTimeReqs::AnyAll].contains(required) {
        // a. For each property name prop of « "dayPeriod", "hour", "minute", "second",
        // "fractionalSecondDigits" », do
        for property in [
            js_string!("dayPeriod"),
            js_string!("hour"),
            js_string!("minute"),
            js_string!("second"),
            js_string!("fractionalSecondDigits"),
        ] {
            // i. Let value be ? Get(options, prop).
            let value = options.get(property, context)?;

            // ii. If value is not undefined, let needDefaults be false.
            if !value.is_undefined() {
                need_defaults = false;
            }
        }
    }

    // 6. Let dateStyle be ? Get(options, "dateStyle").
    let date_style = options.get(js_string!("dateStyle"), context)?;

    // 7. Let timeStyle be ? Get(options, "timeStyle").
    let time_style = options.get(js_string!("timeStyle"), context)?;

    // 8. If dateStyle is not undefined or timeStyle is not undefined, let needDefaults be false.
    if !date_style.is_undefined() || !time_style.is_undefined() {
        need_defaults = false;
    }

    // 9. If required is "date" and timeStyle is not undefined, then
    if required == &DateTimeReqs::Date && !time_style.is_undefined() {
        // a. Throw a TypeError exception.
        return Err(JsNativeError::typ()
            .with_message("'date' is required, but timeStyle was defined")
            .into());
    }

    // 10. If required is "time" and dateStyle is not undefined, then
    if required == &DateTimeReqs::Time && !date_style.is_undefined() {
        // a. Throw a TypeError exception.
        return Err(JsNativeError::typ()
            .with_message("'time' is required, but dateStyle was defined")
            .into());
    }

    // 11. If needDefaults is true and defaults is either "date" or "all", then
    if need_defaults && [DateTimeReqs::Date, DateTimeReqs::AnyAll].contains(defaults) {
        // a. For each property name prop of « "year", "month", "day" », do
        for property in [js_string!("year"), js_string!("month"), js_string!("day")] {
            // i. Perform ? CreateDataPropertyOrThrow(options, prop, "numeric").
            options.create_data_property_or_throw(property, js_string!("numeric"), context)?;
        }
    }

    // 12. If needDefaults is true and defaults is either "time" or "all", then
    if need_defaults && [DateTimeReqs::Time, DateTimeReqs::AnyAll].contains(defaults) {
        // a. For each property name prop of « "hour", "minute", "second" », do
        for property in [
            js_string!("hour"),
            js_string!("minute"),
            js_string!("second"),
        ] {
            // i. Perform ? CreateDataPropertyOrThrow(options, prop, "numeric").
            options.create_data_property_or_throw(property, js_string!("numeric"), context)?;
        }
    }

    // 13. Return options.
    Ok(options)
}

impl OptionType for CalendarAlgorithm {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        let s = value.to_string(context)?.to_std_string_escaped();
        Value::try_from_str(&s)
            .ok()
            .and_then(|v| CalendarAlgorithm::try_from(&v).ok())
            .ok_or_else(|| {
                JsNativeError::range()
                    .with_message(format!("provided calendar `{s}` is invalid"))
                    .into()
            })
    }
}

// TODO: track https://github.com/unicode-org/icu4x/issues/6597 and
// https://github.com/tc39/ecma402/issues/1002 for resolution on
// `HourCycle::H24`.
impl OptionType for HourCycle {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_string(context)?.to_std_string_escaped().as_str() {
            "h11" => Ok(HourCycle::H11),
            "h12" => Ok(HourCycle::H12),
            "h23" => Ok(HourCycle::H23),
            _ => Err(JsNativeError::range()
                .with_message("provided hour cycle was not `h11`, `h12` or `h23`")
                .into()),
        }
    }
}
