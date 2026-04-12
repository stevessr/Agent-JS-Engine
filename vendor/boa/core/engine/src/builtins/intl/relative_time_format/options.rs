use std::fmt;
use crate::{
    JsString, js_string,
    builtins::options::ParsableOptionType,
};

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum RelativeTimeStyle {
    #[default]
    Long,
    Short,
    Narrow,
}

impl RelativeTimeStyle {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Long => js_string!("long"),
            Self::Short => js_string!("short"),
            Self::Narrow => js_string!("narrow"),
        }
    }
}

impl std::str::FromStr for RelativeTimeStyle {
    type Err = ParseRelativeTimeStyleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "long" => Ok(Self::Long),
            "short" => Ok(Self::Short),
            "narrow" => Ok(Self::Narrow),
            _ => Err(ParseRelativeTimeStyleError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseRelativeTimeStyleError;

impl fmt::Display for ParseRelativeTimeStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid RelativeTimeStyle option")
    }
}

impl ParsableOptionType for RelativeTimeStyle {}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum Numeric {
    #[default]
    Always,
    Auto,
}

impl Numeric {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Always => js_string!("always"),
            Self::Auto => js_string!("auto"),
        }
    }
}

impl std::str::FromStr for Numeric {
    type Err = ParseNumericError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "always" => Ok(Self::Always),
            "auto" => Ok(Self::Auto),
            _ => Err(ParseNumericError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseNumericError;

impl fmt::Display for ParseNumericError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid Numeric option")
    }
}

impl ParsableOptionType for Numeric {}
