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
pub(crate) struct DisplayNames {
    locale: Locale,
    style: Style,
    typ: Type,
    fallback: Fallback,
    language_display: LanguageDisplay,
}

impl Service for DisplayNames {
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

impl IntrinsicObject for DisplayNames {
    fn init(realm: &Realm) {
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_method(
                Self::supported_locales_of,
                js_string!("supportedLocalesOf"),
                1,
            )
            .property(
                JsSymbol::to_string_tag(),
                js_string!("Intl.DisplayNames"),
                Attribute::CONFIGURABLE,
            )
            .method(Self::of, js_string!("of"), 1)
            .method(Self::resolved_options, js_string!("resolvedOptions"), 0)
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
}

impl BuiltInObject for DisplayNames {
    const NAME: JsString = StaticJsStrings::DISPLAY_NAMES;
}

impl BuiltInConstructor for DisplayNames {
    const CONSTRUCTOR_ARGUMENTS: usize = 2;
    const PROTOTYPE_STORAGE_SLOTS: usize = 4;
    const CONSTRUCTOR_STORAGE_SLOTS: usize = 1;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::display_names;

    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("cannot call `Intl.DisplayNames` constructor without `new`")
                .into());
        }

        let locales = args.get_or_undefined(0);
        let options = args.get_or_undefined(1);

        let requested_locales = canonicalize_locale_list(locales, context)?;
        let options = get_options_object(options)?;

        let matcher = get_option(&options, js_string!("localeMatcher"), context)?.unwrap_or_default();
        
        let mut intl_options = IntlOptions {
            matcher,
            service_options: super::number_format::NumberFormatLocaleOptions {
                numbering_system: None,
            },
        };

        let locale = resolve_locale::<Self>(
            requested_locales,
            &mut intl_options,
            context.intl_provider(),
        )?;

        let style = get_option(&options, js_string!("style"), context)?.unwrap_or_default();
        let typ = get_option(&options, js_string!("type"), context)?
            .ok_or_else(|| JsNativeError::typ().with_message("type option is required for Intl.DisplayNames"))?;
        let fallback = get_option(&options, js_string!("fallback"), context)?.unwrap_or_default();
        let language_display = get_option(&options, js_string!("languageDisplay"), context)?.unwrap_or_default();

        let prototype = get_prototype_from_constructor(new_target, StandardConstructors::display_names, context)?;
        let display_names = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            Self {
                locale,
                style,
                typ,
                fallback,
                language_display,
            },
        );

        Ok(display_names.into())
    }
}

impl DisplayNames {
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

    fn of(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let code = args.get_or_undefined(0);
        let object = this.as_object();
        let dn = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`of` can only be called on a `DisplayNames` object")
            })?;

        let code_str = code.to_string(context)?.to_std_string_escaped();
        
        // TODO: Proper implementation with ICU4X data
        // For now, very simple fallback or basic data
        let lang = dn.locale.id.language.as_str();
        
        let result = match (lang, dn.typ, code_str.as_str()) {
            ("en", Type::Language, "en") => Some("English"),
            ("en", Type::Language, "zh") => Some("Chinese"),
            ("en", Type::Language, "es") => Some("Spanish"),
            ("en", Type::Region, "US") => Some("United States"),
            ("en", Type::Region, "CN") => Some("China"),
            ("zh", Type::Language, "en") => Some("\u{82F1}\u{8BED}"), // 英语
            ("zh", Type::Language, "zh") => Some("\u{4E2D}\u{6587}"), // 中文
            _ => None,
        };

        if let Some(res) = result {
            return Ok(js_string!(res).into());
        }

        if matches!(dn.fallback, Fallback::Code) {
            Ok(js_string!(code_str).into())
        } else {
            Ok(JsValue::undefined())
        }
    }

    fn resolved_options(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let object = this.as_object();
        let dn = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`resolvedOptions` can only be called on a `DisplayNames` object")
            })?;

        let options = JsObject::with_object_proto(context.intrinsics());
        options.create_data_property_or_throw(js_string!("locale"), js_string!(dn.locale.to_string()), context)?;
        options.create_data_property_or_throw(js_string!("style"), dn.style.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("type"), dn.typ.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("fallback"), dn.fallback.to_js_string(), context)?;
        options.create_data_property_or_throw(js_string!("languageDisplay"), dn.language_display.to_js_string(), context)?;
        
        Ok(options.into())
    }
}
