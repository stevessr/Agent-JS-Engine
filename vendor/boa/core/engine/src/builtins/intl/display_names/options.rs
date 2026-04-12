use std::fmt;
use crate::{
    JsString, js_string,
    builtins::options::ParsableOptionType,
};

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum Style {
    #[default]
    Long,
    Short,
    Narrow,
}

impl Style {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Long => js_string!("long"),
            Self::Short => js_string!("short"),
            Self::Narrow => js_string!("narrow"),
        }
    }
}

impl std::str::FromStr for Style {
    type Err = ParseStyleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "long" => Ok(Self::Long),
            "short" => Ok(Self::Short),
            "narrow" => Ok(Self::Narrow),
            _ => Err(ParseStyleError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseStyleError;

impl fmt::Display for ParseStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid Style option")
    }
}

impl ParsableOptionType for Style {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum Type {
    Language,
    Region,
    Script,
    Currency,
    Calendar,
    DateTimeField,
}

impl Type {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Language => js_string!("language"),
            Self::Region => js_string!("region"),
            Self::Script => js_string!("script"),
            Self::Currency => js_string!("currency"),
            Self::Calendar => js_string!("calendar"),
            Self::DateTimeField => js_string!("dateTimeField"),
        }
    }
}

impl std::str::FromStr for Type {
    type Err = ParseTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "language" => Ok(Self::Language),
            "region" => Ok(Self::Region),
            "script" => Ok(Self::Script),
            "currency" => Ok(Self::Currency),
            "calendar" => Ok(Self::Calendar),
            "dateTimeField" => Ok(Self::DateTimeField),
            _ => Err(ParseTypeError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseTypeError;

impl fmt::Display for ParseTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid Type option")
    }
}

impl ParsableOptionType for Type {}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum Fallback {
    #[default]
    Code,
    None,
}

impl Fallback {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Code => js_string!("code"),
            Self::None => js_string!("none"),
        }
    }
}

impl std::str::FromStr for Fallback {
    type Err = ParseFallbackError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "code" => Ok(Self::Code),
            "none" => Ok(Self::None),
            _ => Err(ParseFallbackError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseFallbackError;

impl fmt::Display for ParseFallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid Fallback option")
    }
}

impl ParsableOptionType for Fallback {}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum LanguageDisplay {
    #[default]
    Dialect,
    Standard,
}

impl LanguageDisplay {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Dialect => js_string!("dialect"),
            Self::Standard => js_string!("standard"),
        }
    }
}

impl std::str::FromStr for LanguageDisplay {
    type Err = ParseLanguageDisplayError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dialect" => Ok(Self::Dialect),
            "standard" => Ok(Self::Standard),
            _ => Err(ParseLanguageDisplayError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseLanguageDisplayError;

impl fmt::Display for ParseLanguageDisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid LanguageDisplay option")
    }
}

impl ParsableOptionType for LanguageDisplay {}
