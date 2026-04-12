use boa_gc::{Finalize, Trace};
use icu_locale::Locale;

use crate::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsString, JsValue,
    builtins::{
        BuiltInBuilder, BuiltInConstructor, BuiltInObject, IntrinsicObject,
        options::{get_option, get_options_object},
    },
    context::intrinsics::{Intrinsics, StandardConstructor, StandardConstructors},
    js_string,
    object::{JsObject, internal_methods::get_prototype_from_constructor},
    property::Attribute,
    realm::Realm,
    string::StaticJsStrings,
    symbol::JsSymbol,
};

use super::{
    Service,
    locale::{canonicalize_locale_list, filter_locales, resolve_locale},
    options::IntlOptions,
};

mod options;
pub(crate) use options::*;

use crate::builtins::intl::options::get_number_option;

#[derive(Debug, Trace, Finalize, JsData)]
#[boa_gc(unsafe_empty_trace)]
pub(crate) struct DurationFormat {
    locale: Locale,
    numbering_system: Option<icu_locale::extensions::unicode::Value>,
    style: DurationStyle,
    years: UnitStyle,
    years_display: UnitDisplay,
    months: UnitStyle,
    months_display: UnitDisplay,
    weeks: UnitStyle,
    weeks_display: UnitDisplay,
    days: UnitStyle,
    days_display: UnitDisplay,
    hours: UnitStyle,
    hours_display: UnitDisplay,
    minutes: UnitStyle,
    minutes_display: UnitDisplay,
    seconds: UnitStyle,
    seconds_display: UnitDisplay,
    milliseconds: UnitStyle,
    milliseconds_display: UnitDisplay,
    microseconds: UnitStyle,
    microseconds_display: UnitDisplay,
    nanoseconds: UnitStyle,
    nanoseconds_display: UnitDisplay,
    fractional_digits: Option<u8>,
}

impl Service for DurationFormat {
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

impl IntrinsicObject for DurationFormat {
    fn init(realm: &Realm) {
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_method(
                Self::supported_locales_of,
                js_string!("supportedLocalesOf"),
                1,
            )
            .property(
                JsSymbol::to_string_tag(),
                js_string!("Intl.DurationFormat"),
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

impl BuiltInObject for DurationFormat {
    const NAME: JsString = StaticJsStrings::DURATION_FORMAT;
}

impl BuiltInConstructor for DurationFormat {
    const CONSTRUCTOR_ARGUMENTS: usize = 0;
    const PROTOTYPE_STORAGE_SLOTS: usize = 4;
    const CONSTRUCTOR_STORAGE_SLOTS: usize = 1;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::duration_format;

    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("cannot call `Intl.DurationFormat` constructor without `new`")
                .into());
        }

        let locales = args.get_or_undefined(0);
        let options = args.get_or_undefined(1);

        let requested_locales = canonicalize_locale_list(locales, context)?;
        let options = get_options_object(options)?;

        let matcher = get_option(&options, js_string!("localeMatcher"), context)?.unwrap_or_default();
        let numbering_system = get_option::<icu_decimal::preferences::NumberingSystem>(&options, js_string!("numberingSystem"), context)?;

        let mut intl_options = IntlOptions {
            matcher,
            service_options: super::number_format::NumberFormatLocaleOptions {
                numbering_system: numbering_system.map(icu_locale::extensions::unicode::Value::from),
            },
        };

        let locale = resolve_locale::<Self>(
            requested_locales,
            &mut intl_options,
            context.intl_provider(),
        )?;

        let style = get_option(&options, js_string!("style"), context)?.unwrap_or_default();

        let fractional_digits = get_number_option(&options, js_string!("fractionalDigits"), 0, 9, context)?;

        let mut df = Self {
            locale,
            numbering_system: intl_options.service_options.numbering_system,
            style,
            years: UnitStyle::Short,
            years_display: UnitDisplay::Auto,
            months: UnitStyle::Short,
            months_display: UnitDisplay::Auto,
            weeks: UnitStyle::Short,
            weeks_display: UnitDisplay::Auto,
            days: UnitStyle::Short,
            days_display: UnitDisplay::Auto,
            hours: UnitStyle::Short,
            hours_display: UnitDisplay::Auto,
            minutes: UnitStyle::Short,
            minutes_display: UnitDisplay::Auto,
            seconds: UnitStyle::Short,
            seconds_display: UnitDisplay::Auto,
            milliseconds: UnitStyle::Short,
            milliseconds_display: UnitDisplay::Auto,
            microseconds: UnitStyle::Short,
            microseconds_display: UnitDisplay::Auto,
            nanoseconds: UnitStyle::Short,
            nanoseconds_display: UnitDisplay::Auto,
            fractional_digits,
        };

        // Step 6: GetDurationUnitOptions
        df.years = get_option(&options, js_string!("years"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Short } else { UnitStyle::from(style) });
        df.years_display = get_option(&options, js_string!("yearsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Auto } else { UnitDisplay::Always });
        
        df.months = get_option(&options, js_string!("months"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Short } else { UnitStyle::from(style) });
        df.months_display = get_option(&options, js_string!("monthsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Auto } else { UnitDisplay::Always });

        df.weeks = get_option(&options, js_string!("weeks"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Short } else { UnitStyle::from(style) });
        df.weeks_display = get_option(&options, js_string!("weeksDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Auto } else { UnitDisplay::Always });

        df.days = get_option(&options, js_string!("days"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Short } else { UnitStyle::from(style) });
        df.days_display = get_option(&options, js_string!("daysDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Auto } else { UnitDisplay::Always });

        df.hours = get_option(&options, js_string!("hours"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.hours_display = get_option(&options, js_string!("hoursDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        df.minutes = get_option(&options, js_string!("minutes"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.minutes_display = get_option(&options, js_string!("minutesDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        df.seconds = get_option(&options, js_string!("seconds"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.seconds_display = get_option(&options, js_string!("secondsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        df.milliseconds = get_option(&options, js_string!("milliseconds"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.milliseconds_display = get_option(&options, js_string!("millisecondsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        df.microseconds = get_option(&options, js_string!("microseconds"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.microseconds_display = get_option(&options, js_string!("microsecondsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        df.nanoseconds = get_option(&options, js_string!("nanoseconds"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitStyle::Numeric } else { UnitStyle::from(style) });
        df.nanoseconds_display = get_option(&options, js_string!("nanosecondsDisplay"), context)?.unwrap_or(if style == DurationStyle::Digital { UnitDisplay::Always } else { UnitDisplay::Always });

        // Cascading and overrides should be implemented here per spec.
        
        let prototype = get_prototype_from_constructor(new_target, StandardConstructors::duration_format, context)?;
        let duration_format = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            df,
        );

        Ok(duration_format.into())
    }
}

impl From<DurationStyle> for UnitStyle {
    fn from(style: DurationStyle) -> Self {
        match style {
            DurationStyle::Long => Self::Long,
            DurationStyle::Narrow => Self::Narrow,
            _ => Self::Short,
        }
    }
}

impl DurationFormat {
    fn supported_locales_of(
        _: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let locales = args.get_or_undefined(0);
        let options = args.get_or_undefined(1);
        let requested_locales = canonicalize_locale_list(locales, context)?;
        filter_locales::<Self>(requested_locales, options, context).map(JsValue::from)
    }

    pub(crate) fn format(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let duration = args.get_or_undefined(0);
        let object = this.as_object();
        let _df = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`format` can only be called on a `DurationFormat` object")
            })?;

        // Minimal implementation for now
        let duration_str = duration.to_string(context)?.to_std_string_escaped();
        Ok(js_string!(duration_str).into())
    }

    fn format_to_parts(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        // TODO
        Ok(crate::builtins::Array::create_array_from_list(std::iter::empty(), _context).into())
    }

    fn resolved_options(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let object = this.as_object();
        let df = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`resolvedOptions` can only be called on a `DurationFormat` object")
            })?;

        let options = JsObject::with_object_proto(context.intrinsics());
        options.create_data_property_or_throw(js_string!("locale"), js_string!(df.locale.to_string()), context)?;
        options.create_data_property_or_throw(js_string!("style"), df.style.to_js_string(), context)?;
        
        if let Some(ns) = &df.numbering_system {
            options.create_data_property_or_throw(js_string!("numberingSystem"), js_string!(ns.to_string()), context)?;
        }

        options.create_data_property_or_throw(js_string!("years"), df.years.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("yearsDisplay"), df.years_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("months"), df.months.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("monthsDisplay"), df.months_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("weeks"), df.weeks.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("weeksDisplay"), df.weeks_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("days"), df.days.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("daysDisplay"), df.days_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("hours"), df.hours.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("hoursDisplay"), df.hours_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("minutes"), df.minutes.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("minutesDisplay"), df.minutes_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("seconds"), df.seconds.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("secondsDisplay"), df.seconds_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("milliseconds"), df.milliseconds.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("millisecondsDisplay"), df.milliseconds_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("microseconds"), df.microseconds.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("microsecondsDisplay"), df.microseconds_display.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("nanoseconds"), df.nanoseconds.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("nanosecondsDisplay"), df.nanoseconds_display.to_js_string(), context)?;

        if let Some(fd) = df.fractional_digits {
            options.create_data_property_or_throw(js_string!("fractionalDigits"), fd, context)?;
        }

        Ok(options.into())
    }
}
