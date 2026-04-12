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

#[derive(Debug, Trace, Finalize, JsData)]
#[boa_gc(unsafe_empty_trace)]
pub(crate) struct RelativeTimeFormat {
    locale: Locale,
    numbering_system: Option<icu_locale::extensions::unicode::Value>,
    style: RelativeTimeStyle,
    numeric: Numeric,
}

impl Service for RelativeTimeFormat {
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

impl IntrinsicObject for RelativeTimeFormat {
    fn init(realm: &Realm) {
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_method(
                Self::supported_locales_of,
                js_string!("supportedLocalesOf"),
                1,
            )
            .property(
                JsSymbol::to_string_tag(),
                js_string!("Intl.RelativeTimeFormat"),
                Attribute::CONFIGURABLE,
            )
            .method(Self::format, js_string!("format"), 2)
            .method(Self::format_to_parts, js_string!("formatToParts"), 2)
            .method(Self::resolved_options, js_string!("resolvedOptions"), 0)
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
}

impl BuiltInObject for RelativeTimeFormat {
    const NAME: JsString = StaticJsStrings::RELATIVE_TIME_FORMAT;
}

impl BuiltInConstructor for RelativeTimeFormat {
    const CONSTRUCTOR_ARGUMENTS: usize = 0;
    const PROTOTYPE_STORAGE_SLOTS: usize = 4;
    const CONSTRUCTOR_STORAGE_SLOTS: usize = 1;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::relative_time_format;

    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("cannot call `Intl.RelativeTimeFormat` constructor without `new`")
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
        let numeric = get_option(&options, js_string!("numeric"), context)?.unwrap_or_default();

        let prototype = get_prototype_from_constructor(new_target, StandardConstructors::relative_time_format, context)?;
        let relative_time_format = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            Self {
                locale,
                numbering_system: intl_options.service_options.numbering_system,
                style,
                numeric,
            },
        );

        Ok(relative_time_format.into())
    }
}

impl RelativeTimeFormat {
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

    fn format(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let value = args.get_or_undefined(0);
        let unit = args.get_or_undefined(1);

        let object = this.as_object();
        let rtf = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`format` can only be called on a `RelativeTimeFormat` object")
            })?;

        let value_num = value.to_number(context)?;
        if !value_num.is_finite() {
            return Err(JsNativeError::range().with_message("Invalid value").into());
        }

        let unit_str = unit.to_string(context)?.to_std_string_escaped();
        let lang = rtf.locale.id.language.as_str();
        
        let nf_obj = context.intrinsics().constructors().number_format().constructor();
        let nf_instance = crate::builtins::intl::number_format::NumberFormat::constructor(
            &nf_obj.into(),
            &[js_string!(rtf.locale.to_string()).into()],
            context,
        )?.as_object().expect("NF").clone();
        
        let nf = nf_instance.downcast_ref::<crate::builtins::intl::number_format::NumberFormat>()
            .ok_or_else(|| JsNativeError::typ().with_message("Incompatible receiver"))?;

        let mut decimal = fixed_decimal::Decimal::try_from_f64(value_num.abs(), fixed_decimal::DoublePrecision::RoundTrip)
            .map_err(|err| JsNativeError::range().with_message(err.to_string()))?;
        let formatted_value = nf.format(&mut decimal);

        // Minimal manual implementation for RelativeTimeFormat::format
        let is_negative = value_num < 0.0 || (value_num == 0.0 && value_num.is_sign_negative());
        let _is_plural = value_num.abs() != 1.0;

        let res = match (lang, unit_str.as_str()) {
            ("en", "second" | "seconds") => {
                if is_negative { format!("{formatted_value} seconds ago") }
                else { format!("in {formatted_value} seconds") }
            },
            ("en", "minute" | "minutes") => {
                if is_negative { format!("{formatted_value} minutes ago") }
                else { format!("in {formatted_value} minutes") }
            },
            ("en", "hour" | "hours") => {
                if is_negative { format!("{formatted_value} hours ago") }
                else { format!("in {formatted_value} hours") }
            },
            ("en", "day" | "days") => {
                if is_negative { format!("{formatted_value} days ago") }
                else { format!("in {formatted_value} days") }
            },
            ("en", "week" | "weeks") => {
                if is_negative { format!("{formatted_value} weeks ago") }
                else { format!("in {formatted_value} weeks") }
            },
            ("en", "month" | "months") => {
                if is_negative { format!("{formatted_value} months ago") }
                else { format!("in {formatted_value} months") }
            },
            ("en", "year" | "years") => {
                if is_negative { format!("{formatted_value} years ago") }
                else { format!("in {formatted_value} years") }
            },
            _ => {
                if is_negative { format!("{formatted_value} {unit_str} ago") }
                else { format!("in {formatted_value} {unit_str}") }
            }
        };

        Ok(js_string!(res).into())
    }

    fn format_to_parts(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        // TODO
        Ok(crate::builtins::Array::create_array_from_list(std::iter::empty(), _context).into())
    }

    fn resolved_options(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let object = this.as_object();
        let rtf = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`resolvedOptions` can only be called on a `RelativeTimeFormat` object")
            })?;

        let options = JsObject::with_object_proto(context.intrinsics());
        options.create_data_property_or_throw(js_string!("locale"), js_string!(rtf.locale.to_string()), context)?;
        options.create_data_property_or_throw(js_string!("style"), rtf.style.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("numeric"), rtf.numeric.to_js_string(), context)?;
        if let Some(ns) = &rtf.numbering_system {
            options.create_data_property_or_throw(js_string!("numberingSystem"), js_string!(ns.to_string()), context)?;
        }
        Ok(options.into())
    }
}
