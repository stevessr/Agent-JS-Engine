use std::fmt;
use crate::{
    JsString,
    builtins::options::ParsableOptionType,
    js_string,
};

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum DurationStyle {
    #[default]
    Short,
    Narrow,
    Long,
    Digital,
}

impl DurationStyle {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Short => js_string!("short"),
            Self::Narrow => js_string!("narrow"),
            Self::Long => js_string!("long"),
            Self::Digital => js_string!("digital"),
        }
    }
}

impl std::str::FromStr for DurationStyle {
    type Err = ParseDurationStyleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "short" => Ok(Self::Short),
            "narrow" => Ok(Self::Narrow),
            "long" => Ok(Self::Long),
            "digital" => Ok(Self::Digital),
            _ => Err(ParseDurationStyleError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseDurationStyleError;

impl fmt::Display for ParseDurationStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid DurationStyle option")
    }
}

impl ParsableOptionType for DurationStyle {}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum UnitStyle {
    #[default]
    Short,
    Narrow,
    Long,
    Numeric,
    TwoDigit,
}

impl UnitStyle {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Short => js_string!("short"),
            Self::Narrow => js_string!("narrow"),
            Self::Long => js_string!("long"),
            Self::Numeric => js_string!("numeric"),
            Self::TwoDigit => js_string!("2-digit"),
        }
    }
}

impl std::str::FromStr for UnitStyle {
    type Err = ParseUnitStyleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "short" => Ok(Self::Short),
            "narrow" => Ok(Self::Narrow),
            "long" => Ok(Self::Long),
            "numeric" => Ok(Self::Numeric),
            "2-digit" => Ok(Self::TwoDigit),
            _ => Err(ParseUnitStyleError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseUnitStyleError;

impl fmt::Display for ParseUnitStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid UnitStyle option")
    }
}

impl ParsableOptionType for UnitStyle {}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub(crate) enum UnitDisplay {
    #[default]
    Always,
    Auto,
}

impl UnitDisplay {
    pub(crate) fn to_js_string(self) -> JsString {
        match self {
            Self::Always => js_string!("always"),
            Self::Auto => js_string!("auto"),
        }
    }
}

impl std::str::FromStr for UnitDisplay {
    type Err = ParseUnitDisplayError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "always" => Ok(Self::Always),
            "auto" => Ok(Self::Auto),
            _ => Err(ParseUnitDisplayError),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParseUnitDisplayError;

impl fmt::Display for ParseUnitDisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("provided string was not a valid UnitDisplay option")
    }
}

impl ParsableOptionType for UnitDisplay {}
