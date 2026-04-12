
#[derive(Debug, Clone)]
struct DateTimeFormatOptions {
    locale: String,
    calendar: String,
    numbering_system: String,
    time_zone: String,
    hour_cycle: Option<String>,
    hour12: Option<bool>,
    weekday: Option<String>,
    era: Option<String>,
    year: Option<String>,
    month: Option<String>,
    day: Option<String>,
    day_period: Option<String>,
    hour: Option<String>,
    minute: Option<String>,
    second: Option<String>,
    fractional_second_digits: Option<u8>,
    time_zone_name: Option<String>,
    date_style: Option<String>,
    time_style: Option<String>,
}

impl Finalize for DateTimeFormatOptions {}
unsafe impl Trace for DateTimeFormatOptions {
    boa_engine::gc::empty_trace!();
}

#[derive(Debug, JsData)]
struct DateTimeFormatSlot {
    instance: JsObject,
    options: DateTimeFormatOptions,
    format_fn: std::cell::RefCell<Option<JsObject>>,
}

impl Finalize for DateTimeFormatSlot {}
unsafe impl Trace for DateTimeFormatSlot {
    unsafe fn trace(&self, tracer: &mut boa_engine::gc::Tracer) {
        unsafe { 
            self.instance.trace(tracer);
            if let Some(ref f) = *self.format_fn.borrow() { f.trace(tracer); }
        }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) { self.finalize(); }
}

#[derive(Debug, JsData)]
struct DisplayNamesSlot {
    instance: JsObject,
    type_name: String,
    fallback: String,
}

impl Finalize for DisplayNamesSlot {}
unsafe impl Trace for DisplayNamesSlot {
    unsafe fn trace(&self, tracer: &mut boa_engine::gc::Tracer) {
        unsafe { self.instance.trace(tracer); }
    }
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) { self.finalize(); }
}

const VALID_LOCALE_MATCHERS: &[&str] = &["lookup", "best fit"];
const VALID_FORMAT_MATCHERS: &[&str] = &["basic", "best fit"];
const VALID_CALENDARS: &[&str] = &[
    "buddhist", "chinese", "coptic", "dangi", "ethioaa", "ethiopic",
    "gregory", "hebrew", "indian", "islamic", "islamic-umalqura",
    "islamic-tbla", "islamic-civil", "islamic-rgsa", "iso8601",
    "japanese", "persian", "roc"
];
const VALID_NUMBERING_SYSTEMS: &[&str] = &[
    "arab", "arabext", "bali", "beng", "deva", "fullwide", "gujr",
    "guru", "hanidec", "khmr", "knda", "laoo", "latn", "limb",
    "mlym", "mong", "mymr", "orya", "tamldec", "telu", "thai", "tibt"
];
const VALID_HOUR_CYCLES: &[&str] = &["h11", "h12", "h23", "h24"];
const VALID_WEEKDAYS: &[&str] = &["narrow", "short", "long"];
const VALID_ERAS: &[&str] = &["narrow", "short", "long"];
const VALID_YEARS: &[&str] = &["2-digit", "numeric"];
const VALID_MONTHS: &[&str] = &["2-digit", "numeric", "narrow", "short", "long"];
const VALID_DAYS: &[&str] = &["2-digit", "numeric"];
const VALID_DAY_PERIODS: &[&str] = &["narrow", "short", "long"];
const VALID_HOURS: &[&str] = &["2-digit", "numeric"];
const VALID_MINUTES: &[&str] = &["2-digit", "numeric"];
const VALID_SECONDS: &[&str] = &["2-digit", "numeric"];
const VALID_TIME_ZONE_NAMES: &[&str] = &["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"];
const VALID_DATE_STYLES: &[&str] = &["full", "long", "medium", "short"];
const VALID_TIME_STYLES: &[&str] = &["full", "long", "medium", "short"];

fn install_intl_date_time_format_polyfill(context: &mut Context) -> JsResult<()> {
    if context.global_object().has_property(js_string!("__original_DateTimeFormat"), context)? {
        return Ok(());
    }

    let intl: JsObject = context.global_object().get(js_string!("Intl"), context)?.as_object()
        .map(|o| o.clone())
        .ok_or_else(|| JsNativeError::typ().with_message("Intl object not found"))?;
    
    let original_dtf: JsObject = intl.get(js_string!("DateTimeFormat"), context)?.as_object()
        .map(|o| o.clone())
        .ok_or_else(|| JsNativeError::typ().with_message("Intl.DateTimeFormat not found"))?;
    context.global_object().set(js_string!("__original_DateTimeFormat"), original_dtf.clone(), false, context)?;

    let dtf_proto = JsObject::with_object_proto(context.intrinsics());
    let dtf_constructor = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(datetime_format_constructor))
        .name(js_string!("DateTimeFormat"))
        .constructor(true)
        .build();
    
    dtf_constructor.define_property_or_throw(js_string!("prototype"), PropertyDescriptor::builder().value(dtf_proto.clone()).writable(false).enumerable(false).configurable(false).build(), context)?;
    dtf_proto.define_property_or_throw(js_string!("constructor"), PropertyDescriptor::builder().value(dtf_constructor.clone()).writable(true).enumerable(false).configurable(true).build(), context)?;
    
    dtf_constructor.define_property_or_throw(js_string!("supportedLocalesOf"), PropertyDescriptor::builder().value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_supported_locales_of)).name(js_string!("supportedLocalesOf")).length(1).build()).writable(true).enumerable(false).configurable(true).build(), context)?;
    dtf_proto.define_property_or_throw(js_string!("resolvedOptions"), PropertyDescriptor::builder().value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_resolved_options)).name(js_string!("resolvedOptions")).length(0).build()).writable(true).enumerable(false).configurable(true).build(), context)?;
    dtf_proto.define_property_or_throw(js_string!("formatToParts"), PropertyDescriptor::builder().value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_format_to_parts)).name(js_string!("formatToParts")).length(1).build()).writable(true).enumerable(false).configurable(true).build(), context)?;
    
    dtf_proto.define_property_or_throw(
        js_string!("format"),
        PropertyDescriptor::builder()
            .get(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_format_getter))
                .name(js_string!("get format"))
                .build())
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )?;

    // formatRange and formatRangeToParts
    dtf_proto.define_property_or_throw(
        js_string!("formatRange"),
        PropertyDescriptor::builder()
            .value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_format_range))
                .name(js_string!("formatRange"))
                .length(2)
                .build())
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )?;
    dtf_proto.define_property_or_throw(
        js_string!("formatRangeToParts"),
        PropertyDescriptor::builder()
            .value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(dtf_format_range_to_parts))
                .name(js_string!("formatRangeToParts"))
                .length(2)
                .build())
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )?;

    intl.set(js_string!("DateTimeFormat"), dtf_constructor, true, context)?;

    if let Some(original_dn) = intl.get(js_string!("DisplayNames"), context)?.as_object().map(|o| o.clone()) {
        context.global_object().set(js_string!("__original_DisplayNames"), original_dn.clone(), false, context)?;
        let dn_proto = JsObject::with_object_proto(context.intrinsics());
        let dn_constructor = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(display_names_constructor)).name(js_string!("DisplayNames")).constructor(true).build();
        dn_constructor.define_property_or_throw(js_string!("prototype"), PropertyDescriptor::builder().value(dn_proto.clone()).writable(false).enumerable(false).configurable(false).build(), context)?;
        dn_proto.define_property_or_throw(js_string!("constructor"), PropertyDescriptor::builder().value(dn_constructor.clone()).writable(true).enumerable(false).configurable(true).build(), context)?;
        dn_proto.define_property_or_throw(js_string!("of"), PropertyDescriptor::builder().value(FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(display_names_of)).name(js_string!("of")).length(1).build()).writable(true).enumerable(false).configurable(true).build(), context)?;
        intl.set(js_string!("DisplayNames"), dn_constructor, true, context)?;
    }

    // Polyfill Date.prototype.toLocaleString and friends
    let date_proto = context.intrinsics().constructors().date().prototype();
    let to_locale_string_polyfill = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(date_to_locale_string)).name(js_string!("toLocaleString")).length(0).build();
    let to_locale_date_string_polyfill = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(date_to_locale_date_string)).name(js_string!("toLocaleDateString")).length(0).build();
    let to_locale_time_string_polyfill = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(date_to_locale_time_string)).name(js_string!("toLocaleTimeString")).length(0).build();
    
    date_proto.define_property_or_throw(js_string!("toLocaleString"), PropertyDescriptor::builder().value(to_locale_string_polyfill).writable(true).enumerable(false).configurable(true).build(), context)?;
    date_proto.define_property_or_throw(js_string!("toLocaleDateString"), PropertyDescriptor::builder().value(to_locale_date_string_polyfill).writable(true).enumerable(false).configurable(true).build(), context)?;
    date_proto.define_property_or_throw(js_string!("toLocaleTimeString"), PropertyDescriptor::builder().value(to_locale_time_string_polyfill).writable(true).enumerable(false).configurable(true).build(), context)?;

    Ok(())
}

fn date_to_locale_string(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let is_date = if let Some(o) = this.as_object() {
        o.get(js_string!("getTime"), context).ok().map(|v| v.is_callable()).unwrap_or(false)
    } else { false };
    if !is_date {
        return Err(JsNativeError::typ().with_message("Date.prototype.toLocaleString called on non-Date object").into());
    }
    let locales = args.get_or_undefined(0);
    let options = args.get_or_undefined(1);
    let intl = context.global_object().get(js_string!("Intl"), context)?.to_object(context)?;
    let dtf_ctor = intl.get(js_string!("DateTimeFormat"), context)?.to_object(context)?;
    let dtf_instance = dtf_ctor.construct(&[locales.clone(), options.clone()], None, context)?;
    let format_getter = dtf_instance.get(js_string!("format"), context)?;
    let format_fn = format_getter.as_object().ok_or_else(|| JsNativeError::typ().with_message("format is not a function"))?;
    format_fn.call(&dtf_instance.into(), &[this.clone()], context)
}

fn date_to_locale_date_string(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    date_to_locale_string(this, args, context)
}

fn date_to_locale_time_string(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    date_to_locale_string(this, args, context)
}

fn get_option_string(options: &JsObject, key: &str, valid: &[&str], context: &mut Context) -> JsResult<Option<String>> {
    let value = options.get(js_string!(key), context)?;
    if value.is_undefined() { return Ok(None); }
    let s = value.to_string(context)?.to_std_string_escaped();
    if !valid.is_empty() && !valid.contains(&s.as_str()) { return Err(JsNativeError::range().with_message(format!("Invalid value {} for option {}", s, key)).into()); }
    Ok(Some(s))
}

fn extract_unicode_extension(locale: &str, key: &str) -> Option<String> {
    let pattern = format!("-u-(?:[a-z0-9]{{2,8}}-)*{}-([a-z0-9]{{2,8}})", key);
    if let Ok(re) = Regex::new(&pattern) {
        if let Some(caps) = re.captures(locale) {
            return Some(caps.get(1).unwrap().as_str().to_string());
        }
    }
    None
}

fn validate_time_zone(tz: &str) -> JsResult<()> {
    if tz.to_uppercase() == "UTC" { return Ok(()); }
    if tz.starts_with('+') || tz.starts_with('-') {
        let re = Regex::new(r"^[+-]([0-9]{2}):([0-9]{2})(:([0-9]{2}))?$").unwrap();
        if let Some(caps) = re.captures(tz) {
            if caps.get(0).unwrap().as_str().len() == tz.len() {
                let h: i32 = caps.get(1).unwrap().as_str().parse().unwrap_or(99);
                let m: i32 = caps.get(2).unwrap().as_str().parse().unwrap_or(99);
                let s: i32 = caps.get(4).map(|x| x.as_str().parse().unwrap_or(99)).unwrap_or(0);
                if h < 24 && m < 60 && s < 60 {
                    let total_seconds = h * 3600 + m * 60 + s;
                    if total_seconds <= 14 * 3600 {
                        return Ok(());
                    }
                }
            }
        }
        return Err(JsNativeError::range().with_message(format!("Invalid time zone offset: {}", tz)).into());
    }
    if tz.chars().any(|c| !c.is_ascii()) {
        return Err(JsNativeError::range().with_message(format!("Invalid time zone: {}", tz)).into());
    }
    Ok(())
}

fn validate_bcp47_identifier(v: &str) -> bool {
    if v.is_empty() { return false; }
    let segments: Vec<&str> = v.split('-').collect();
    for seg in segments { if seg.len() < 3 || seg.len() > 8 || !seg.chars().all(|c| c.is_ascii_alphanumeric()) { return false; } }
    true
}

fn get_prototype_from_constructor(new_target: &BoaValue, default_proto: JsObject, context: &mut Context) -> JsResult<JsObject> {
    if let Some(o) = new_target.as_object() {
        let p = o.get(js_string!("prototype"), context)?;
        if let Some(proto_obj) = p.as_object() {
            Ok(proto_obj.clone())
        } else {
            Ok(default_proto)
        }
    } else {
        Ok(default_proto)
    }
}

fn datetime_format_constructor(new_target: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let original_dtf: JsObject = context.global_object().get(js_string!("__original_DateTimeFormat"), context)?.as_object().unwrap().clone();
    
    if new_target.is_undefined() {
        return Ok(original_dtf.construct(args, None, context)?.into());
    }

    let proto = get_prototype_from_constructor(new_target, context.intrinsics().constructors().date_time_format().prototype(), context)?;

    let locales = args.get_or_undefined(0);
    let options_val = args.get_or_undefined(1);
    let options = if options_val.is_undefined() { JsObject::with_object_proto(context.intrinsics()) } else if options_val.is_null() { return Err(JsNativeError::typ().with_message("Cannot convert null to object").into()); } else { options_val.to_object(context)? };

    // Strictly follow property access order from ECMA-402
    let _locale_matcher = get_option_string(&options, "localeMatcher", VALID_LOCALE_MATCHERS, context)?;
    let mut calendar_opt = get_option_string(&options, "calendar", &[], context)?.map(|s| s.to_lowercase());
    if let Some(ref cal) = calendar_opt {
        if cal == "islamicc" { calendar_opt = Some("islamic-civil".to_string()); }
        if !validate_bcp47_identifier(calendar_opt.as_ref().unwrap()) { return Err(JsNativeError::range().with_message("Invalid calendar").into()); }
    }
    let numbering_system_opt = get_option_string(&options, "numberingSystem", &[], context)?.map(|s| s.to_lowercase());
    if let Some(ref ns) = numbering_system_opt {
        if !validate_bcp47_identifier(ns) { return Err(JsNativeError::range().with_message("Invalid numberingSystem").into()); }
    }
    
    let hour12_v = options.get(js_string!("hour12"), context)?;
    let hour12 = if hour12_v.is_undefined() { None } else { Some(hour12_v.to_boolean()) };
    let hour_cycle = get_option_string(&options, "hourCycle", VALID_HOUR_CYCLES, context)?;
    let time_zone = get_option_string(&options, "timeZone", &[], context)?;
    if let Some(ref tz) = time_zone { validate_time_zone(tz)?; }

    let weekday = get_option_string(&options, "weekday", VALID_WEEKDAYS, context)?;
    let era = get_option_string(&options, "era", VALID_ERAS, context)?;
    let year = get_option_string(&options, "year", VALID_YEARS, context)?;
    let month = get_option_string(&options, "month", VALID_MONTHS, context)?;
    let day = get_option_string(&options, "day", VALID_DAYS, context)?;
    let day_period = get_option_string(&options, "dayPeriod", VALID_DAY_PERIODS, context)?;
    let hour = get_option_string(&options, "hour", VALID_HOURS, context)?;
    let minute = get_option_string(&options, "minute", VALID_MINUTES, context)?;
    let second = get_option_string(&options, "second", VALID_SECONDS, context)?;
    let fractional_second_digits = { let val = options.get(js_string!("fractionalSecondDigits"), context)?; if val.is_undefined() { None } else { let n = val.to_number(context)?; if !n.is_finite() || n < 1.0 || n > 3.0 { return Err(JsNativeError::range().with_message("Invalid fractionalSecondDigits").into()); } Some(n as u8) } };
    let time_zone_name = get_option_string(&options, "timeZoneName", VALID_TIME_ZONE_NAMES, context)?;
    let _format_matcher = get_option_string(&options, "formatMatcher", VALID_FORMAT_MATCHERS, context)?;
    let date_style = get_option_string(&options, "dateStyle", VALID_DATE_STYLES, context)?;
    let time_style = get_option_string(&options, "timeStyle", VALID_TIME_STYLES, context)?;

    if (date_style.is_some() || time_style.is_some()) && (weekday.is_some() || era.is_some() || year.is_some() || month.is_some() || day.is_some() || day_period.is_some() || hour.is_some() || minute.is_some() || second.is_some() || fractional_second_digits.is_some() || time_zone_name.is_some()) {
        return Err(JsNativeError::typ().with_message("dateStyle and timeStyle cannot be combined with other date/time options").into());
    }

    let needs_default = date_style.is_none() && time_style.is_none() && weekday.is_none() && year.is_none() && month.is_none() && day.is_none() && hour.is_none() && minute.is_none() && second.is_none() && fractional_second_digits.is_none() && day_period.is_none() && time_zone_name.is_none();
    
    let mut filtered_options = ObjectInitializer::new(context);
    if needs_default {
        filtered_options.property(js_string!("year"), js_string!("numeric"), Attribute::all());
        filtered_options.property(js_string!("month"), js_string!("numeric"), Attribute::all());
        filtered_options.property(js_string!("day"), js_string!("numeric"), Attribute::all());
    }
    if let Some(ref v) = calendar_opt { if VALID_CALENDARS.contains(&v.as_str()) { filtered_options.property(js_string!("calendar"), js_string!(v.as_str()), Attribute::all()); } }
    if let Some(ref v) = numbering_system_opt { if VALID_NUMBERING_SYSTEMS.contains(&v.as_str()) { filtered_options.property(js_string!("numberingSystem"), js_string!(v.as_str()), Attribute::all()); } }
    if let Some(v) = hour12 { filtered_options.property(js_string!("hour12"), v, Attribute::all()); }
    if let Some(ref v) = hour_cycle { filtered_options.property(js_string!("hourCycle"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = time_zone { filtered_options.property(js_string!("timeZone"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = weekday { filtered_options.property(js_string!("weekday"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = era { filtered_options.property(js_string!("era"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = year { filtered_options.property(js_string!("year"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = month { filtered_options.property(js_string!("month"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = day { filtered_options.property(js_string!("day"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = day_period { filtered_options.property(js_string!("dayPeriod"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = hour { filtered_options.property(js_string!("hour"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = minute { filtered_options.property(js_string!("minute"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = second { filtered_options.property(js_string!("second"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = fractional_second_digits { filtered_options.property(js_string!("fractionalSecondDigits"), v, Attribute::all()); }
    if let Some(ref v) = time_zone_name { filtered_options.property(js_string!("timeZoneName"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = date_style { filtered_options.property(js_string!("dateStyle"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(ref v) = time_style { filtered_options.property(js_string!("timeStyle"), js_string!(v.as_str()), Attribute::all()); }

    let dtf_instance_val: BoaValue = match original_dtf.construct(&[locales.clone(), filtered_options.build().into()], None, context) {
        Ok(v) => v.into(),
        Err(e) => {
            if e.to_string().contains("timeZone") { return Err(JsNativeError::range().with_message("Invalid time zone").into()); }
            return Err(e);
        }
    };
    let dtf_instance: JsObject = dtf_instance_val.to_object(context)?;
    
    let resolved_val = match dtf_instance.get(js_string!("resolvedOptions"), context) {
        Ok(v) => if let Some(o) = v.as_object() { 
            match o.call(&dtf_instance.clone().into(), &[], context) {
                Ok(rv) => rv,
                Err(_) => BoaValue::undefined(),
            }
        } else { BoaValue::undefined() },
        Err(_) => BoaValue::undefined(),
    };
    let resolved = resolved_val.to_object(context)?;
    
    fn get_resolved_string(obj: &JsObject, key: &str, context: &mut Context) -> String {
        obj.get(js_string!(key), context).unwrap_or(BoaValue::undefined()).to_string(context).unwrap_or(js_string!("")).to_std_string_escaped()
    }
    fn get_resolved_opt_string(obj: &JsObject, key: &str, context: &mut Context) -> Option<String> {
        let val = obj.get(js_string!(key), context).unwrap_or(BoaValue::undefined());
        if val.is_undefined() { None } else { Some(val.to_string(context).unwrap_or(js_string!("")).to_std_string_escaped()) }
    }

    let resolved_locale = get_resolved_string(&resolved, "locale", context);
    let resolved_calendar = get_resolved_string(&resolved, "calendar", context);
    let resolved_numbering_system = get_resolved_string(&resolved, "numberingSystem", context);
    let resolved_time_zone = get_resolved_string(&resolved, "timeZone", context);

    let loc_input = locales.to_string(context).unwrap_or_else(|_| js_string!("")).to_std_string_escaped();
    let mut final_locale = resolved_locale;
    let mut final_calendar = calendar_opt.clone().unwrap_or_else(|| resolved_calendar.clone());
    if !VALID_CALENDARS.contains(&final_calendar.as_str()) { final_calendar = resolved_calendar.clone(); }
    let mut final_numbering_system = numbering_system_opt.clone().unwrap_or_else(|| resolved_numbering_system.clone());
    if !VALID_NUMBERING_SYSTEMS.contains(&final_numbering_system.as_str()) { final_numbering_system = resolved_numbering_system.clone(); }

    if loc_input.contains("-u-") {
        if let Some(ext_ca) = extract_unicode_extension(&loc_input, "ca") {
            let mut restore = false;
            if VALID_CALENDARS.contains(&ext_ca.as_str()) {
                if final_calendar == ext_ca { restore = true; }
                if !restore && calendar_opt.is_none() { restore = true; } 
            }
            if restore {
                final_calendar = ext_ca.clone();
                if !final_locale.contains("-u-ca-") {
                    if final_locale.contains("-u-") { final_locale = final_locale.replace("-u-", &format!("-u-ca-{}-", ext_ca)); }
                    else { final_locale = format!("{}-u-ca-{}", final_locale, ext_ca); }
                }
            } else if final_locale.contains("-u-ca-") {
                let re = Regex::new(r"-u-ca-[a-z0-9]+").unwrap();
                final_locale = re.replace(&final_locale, "").to_string().replace("-u-u-", "-u-");
                if final_locale.ends_with("-u") { final_locale.truncate(final_locale.len() - 2); }
            }
        }
        if let Some(ext_nu) = extract_unicode_extension(&loc_input, "nu") {
            let mut restore = false;
            if VALID_NUMBERING_SYSTEMS.contains(&ext_nu.as_str()) {
                if final_numbering_system == ext_nu { restore = true; }
                if !restore && numbering_system_opt.is_none() { restore = true; }
            }
            if restore {
                final_numbering_system = ext_nu.clone();
                if !final_locale.contains("-u-nu-") {
                    if final_locale.contains("-u-") { final_locale = final_locale.replace("-u-", &format!("-u-nu-{}-", ext_nu)); }
                    else { final_locale = format!("{}-u-nu-{}", final_locale, ext_nu); }
                }
            } else if final_locale.contains("-u-nu-") {
                let re = Regex::new(r"-u-nu-[a-z0-9]+").unwrap();
                final_locale = re.replace(&final_locale, "").to_string().replace("-u-u-", "-u-");
                if final_locale.ends_with("-u") { final_locale.truncate(final_locale.len() - 2); }
            }
        }
    }

    let resolved_hour12 = {
        let val = resolved.get(js_string!("hour12"), context).unwrap_or(BoaValue::undefined());
        if val.is_undefined() { hour12 } else { Some(val.to_boolean()) }
    };
    let resolved_fsd = {
        let val = resolved.get(js_string!("fractionalSecondDigits"), context).unwrap_or(BoaValue::undefined());
        if val.is_undefined() { fractional_second_digits } else { Some(val.to_number(context).unwrap_or(0.0) as u8) }
    };

    let slot_options = DateTimeFormatOptions {
        locale: final_locale,
        calendar: final_calendar,
        numbering_system: final_numbering_system,
        time_zone: resolved_time_zone,
        hour_cycle: get_resolved_opt_string(&resolved, "hourCycle", context).or(hour_cycle),
        hour12: resolved_hour12,
        weekday: get_resolved_opt_string(&resolved, "weekday", context).or(weekday),
        era: get_resolved_opt_string(&resolved, "era", context).or(era),
        year: get_resolved_opt_string(&resolved, "year", context).or(year).or(if needs_default { Some("numeric".to_string()) } else { None }),
        month: get_resolved_opt_string(&resolved, "month", context).or(month).or(if needs_default { Some("numeric".to_string()) } else { None }),
        day: get_resolved_opt_string(&resolved, "day", context).or(day).or(if needs_default { Some("numeric".to_string()) } else { None }),
        day_period: get_resolved_opt_string(&resolved, "dayPeriod", context).or(day_period),
        hour: get_resolved_opt_string(&resolved, "hour", context).or(hour),
        minute: get_resolved_opt_string(&resolved, "minute", context).or(minute),
        second: get_resolved_opt_string(&resolved, "second", context).or(second),
        fractional_second_digits: resolved_fsd,
        time_zone_name: get_resolved_opt_string(&resolved, "timeZoneName", context).or(time_zone_name),
        date_style: get_resolved_opt_string(&resolved, "dateStyle", context).or(date_style),
        time_style: get_resolved_opt_string(&resolved, "timeStyle", context).or(time_style),
    };

    let obj = JsObject::from_proto_and_data(proto, DateTimeFormatSlot { instance: dtf_instance, options: slot_options, format_fn: std::cell::RefCell::new(None) });
    Ok(obj.into())
}

fn dtf_supported_locales_of(_: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let original_dtf = context.global_object().get(js_string!("__original_DateTimeFormat"), context)?.as_object().unwrap().clone();
    let supported_locales_of = original_dtf.get(js_string!("supportedLocalesOf"), context)?.to_object(context)?;
    supported_locales_of.call(&original_dtf.into(), args, context)
}

fn dtf_resolved_options(this: &BoaValue, _: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Method called on incompatible receiver"))?;
    let mut res = ObjectInitializer::new(context);
    res.property(js_string!("locale"), js_string!(slot.options.locale.as_str()), Attribute::all());
    res.property(js_string!("calendar"), js_string!(slot.options.calendar.as_str()), Attribute::all());
    res.property(js_string!("numberingSystem"), js_string!(slot.options.numbering_system.as_str()), Attribute::all());
    res.property(js_string!("timeZone"), js_string!(slot.options.time_zone.as_str()), Attribute::all());
    if let Some(v) = &slot.options.hour_cycle { res.property(js_string!("hourCycle"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = slot.options.hour12 { res.property(js_string!("hour12"), v, Attribute::all()); }
    if let Some(v) = &slot.options.weekday { res.property(js_string!("weekday"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.era { res.property(js_string!("era"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.year { res.property(js_string!("year"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.month { res.property(js_string!("month"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.day { res.property(js_string!("day"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.hour { res.property(js_string!("hour"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.minute { res.property(js_string!("minute"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.second { res.property(js_string!("second"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = slot.options.fractional_second_digits { res.property(js_string!("fractionalSecondDigits"), v, Attribute::all()); }
    if let Some(v) = &slot.options.day_period { res.property(js_string!("dayPeriod"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.time_zone_name { res.property(js_string!("timeZoneName"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.date_style { res.property(js_string!("dateStyle"), js_string!(v.as_str()), Attribute::all()); }
    if let Some(v) = &slot.options.time_style { res.property(js_string!("timeStyle"), js_string!(v.as_str()), Attribute::all()); }
    Ok(res.build().into())
}

fn dtf_format_getter(this: &BoaValue, _: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Method called on incompatible receiver"))?;
    
    if let Some(ref f) = *slot.format_fn.borrow() {
        return Ok(f.clone().into());
    }
    
    let format_fn = NativeFunction::from_fn_ptr(dtf_format_function);
    let format_obj = FunctionObjectBuilder::new(context.realm(), format_fn).name(js_string!("")).length(1).build();
    let bind = format_obj.get(js_string!("bind"), context)?.to_object(context)?;
    let bound_fn = bind.call(&format_obj.into(), &[this.clone()], context)?.to_object(context)?;
    
    bound_fn.define_property_or_throw(js_string!("name"), PropertyDescriptor::builder().value(js_string!("")).writable(false).enumerable(false).configurable(true).build(), context)?;
    bound_fn.define_property_or_throw(js_string!("length"), PropertyDescriptor::builder().value(1).writable(false).enumerable(false).configurable(true).build(), context)?;
    
    *slot.format_fn.borrow_mut() = Some(bound_fn.clone());
    Ok(bound_fn.into())
}

fn dtf_format_function(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Incompatible receiver"))?;
    let date = args.get_or_undefined(0);
    let is_zdt = date.as_object().map(|o| { let tag = BoaValue::from(o.clone()).to_string(context).unwrap_or_else(|_| js_string!("")).to_std_string_escaped(); tag == "[object Temporal.ZonedDateTime]" }).unwrap_or(false);
    if is_zdt { return Err(JsNativeError::typ().with_message("Intl.DateTimeFormat.prototype.format() does not support Temporal.ZonedDateTime").into()); }
    
    let format_fn = slot.instance.get(js_string!("format"), context)?.to_object(context)?;
    format_fn.call(&slot.instance.clone().into(), args, context)
}

fn dtf_format_to_parts(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Incompatible receiver"))?;
    let date = args.get_or_undefined(0);
    let is_zdt = date.as_object().map(|o| { let tag = BoaValue::from(o.clone()).to_string(context).unwrap_or_else(|_| js_string!("")).to_std_string_escaped(); tag == "[object Temporal.ZonedDateTime]" }).unwrap_or(false);
    if is_zdt { return Err(JsNativeError::typ().with_message("Intl.DateTimeFormat.prototype.formatToParts() does not support Temporal.ZonedDateTime").into()); }
    let format_to_parts = slot.instance.get(js_string!("formatToParts"), context)?.to_object(context)?;
    format_to_parts.call(&slot.instance.clone().into(), args, context)
}

fn dtf_format_range(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Incompatible receiver"))?;
    let format_range = slot.instance.get(js_string!("formatRange"), context)?.to_object(context)?;
    format_range.call(&slot.instance.clone().into(), args, context)
}

fn dtf_format_range_to_parts(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DateTimeFormatSlot>().ok_or_else(|| JsNativeError::typ().with_message("Incompatible receiver"))?;
    let format_range_to_parts = slot.instance.get(js_string!("formatRangeToParts"), context)?.to_object(context)?;
    format_range_to_parts.call(&slot.instance.clone().into(), &[args.get_or_undefined(0).clone(), args.get_or_undefined(1).clone()], context)
}

fn display_names_constructor(new_target: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    if new_target.is_undefined() { return Err(JsNativeError::typ().with_message("Constructor Intl.DisplayNames requires \"new\"").into()); }
    let proto = get_prototype_from_constructor(new_target, context.intrinsics().constructors().display_names().prototype(), context)?;
    let locales = args.get_or_undefined(0);
    let options_val = args.get_or_undefined(1);
    if options_val.is_undefined() { return Err(JsNativeError::typ().with_message("options argument is required for Intl.DisplayNames").into()); }
    let options = options_val.to_object(context)?;
    let type_name = get_option_string(&options, "type", &["language", "region", "script", "currency", "calendar", "dateTimeField"], context)?.ok_or_else(|| JsNativeError::typ().with_message("type option is required for Intl.DisplayNames"))?;
    let fallback = get_option_string(&options, "fallback", &["code", "none"], context)?.unwrap_or_else(|| "code".to_string());
    let original_dn: JsObject = context.global_object().get(js_string!("__original_DisplayNames"), context)?.as_object().unwrap().clone();
    let dn_instance_val: BoaValue = original_dn.construct(&[locales.clone(), options_val.clone()], None, context)?.into();
    let dn_instance: JsObject = dn_instance_val.to_object(context)?;
    let obj = JsObject::from_proto_and_data(proto, DisplayNamesSlot { instance: dn_instance, type_name, fallback });
    Ok(obj.into())
}

fn display_names_of(this: &BoaValue, args: &[BoaValue], context: &mut Context) -> JsResult<BoaValue> {
    let obj = this.to_object(context)?;
    let slot = obj.downcast_ref::<DisplayNamesSlot>().ok_or_else(|| JsNativeError::typ().with_message("Method called on incompatible receiver"))?;
    let code_val = args.get_or_undefined(0);
    let code = code_val.to_string(context)?.to_std_string_escaped();
    if slot.type_name == "calendar" {
        let segments: Vec<&str> = code.split('-').collect();
        let mut valid = !segments.is_empty();
        for seg in segments { if seg.len() < 3 || seg.len() > 8 || !seg.chars().all(|c| c.is_ascii_alphanumeric()) { valid = false; break; } }
        if !valid { return Err(JsNativeError::range().with_message("Invalid calendar code").into()); }
    }
    let of_fn: JsObject = slot.instance.get(js_string!("of"), context)?.to_object(context)?;
    of_fn.call(&slot.instance.clone().into(), args, context)
}
