//! This module implements the calendar traits and related components.
//!
//! The goal of the calendar module of `boa_temporal` is to provide
//! Temporal compatible calendar implementations.

use crate::{
    builtins::core::{
        duration::DateDuration, Duration, PlainDate, PlainDateTime, PlainMonthDay, PlainYearMonth,
    },
    error::ErrorMessage,
    iso::{IsoDate, constrain_iso_day, is_valid_iso_day},
    options::{Overflow, Unit},
    parsers::parse_allowed_calendar_formats,
    TemporalError, TemporalResult,
};
use core::str::FromStr;

use icu_calendar::{
    cal::{
        Buddhist, Chinese, Coptic, Dangi, Ethiopian, EthiopianEraStyle, Hebrew, HijriSimulated,
        HijriTabular, HijriUmmAlQura, Indian, Japanese, JapaneseExtended, Persian, Roc,
    },
    AnyCalendar, AnyCalendarKind, Calendar as IcuCalendar, Date as IcuDate,
    DateDuration as IcuDateDuration, DateDurationUnit as IcuUnit, Iso, Ref,
};
use icu_calendar::{
    cal::{HijriTabularEpoch, HijriTabularLeapYears},
    preferences::CalendarAlgorithm,
    types::{MonthCode as IcuMonthCode, YearInfo},
    Gregorian,
};
use icu_locale::extensions::unicode::Value;
use tinystr::{tinystr, TinyAsciiStr};

use super::ZonedDateTime;

mod era;
mod fields;
mod types;

pub use fields::{CalendarFields, YearMonthCalendarFields};
#[cfg(test)]
pub(crate) use types::month_to_month_code;
pub(crate) use types::ResolutionType;
pub use types::{MonthCode, ResolvedCalendarFields};

use era::EraInfo;

/// The core `Calendar` type for `temporal_rs`
///
/// A `Calendar` in `temporal_rs` can be any calendar that is currently
/// supported by [`icu_calendar`].
#[derive(Debug, Clone)]
pub struct Calendar(Ref<'static, AnyCalendar>);

impl Default for Calendar {
    fn default() -> Self {
        Self::ISO
    }
}

impl PartialEq for Calendar {
    fn eq(&self, other: &Self) -> bool {
        self.identifier() == other.identifier()
    }
}

impl Eq for Calendar {}

impl Calendar {
    /// The Buddhist calendar
    pub const BUDDHIST: Self = Self::new(AnyCalendarKind::Buddhist);
    /// The Chinese calendar
    pub const CHINESE: Self = Self::new(AnyCalendarKind::Chinese);
    /// The Coptic calendar
    pub const COPTIC: Self = Self::new(AnyCalendarKind::Coptic);
    /// The Dangi calendar
    pub const DANGI: Self = Self::new(AnyCalendarKind::Dangi);
    /// The Ethiopian calendar
    pub const ETHIOPIAN: Self = Self::new(AnyCalendarKind::Ethiopian);
    /// The Ethiopian Amete Alem calendar
    pub const ETHIOPIAN_AMETE_ALEM: Self = Self::new(AnyCalendarKind::EthiopianAmeteAlem);
    /// The Gregorian calendar
    pub const GREGORIAN: Self = Self::new(AnyCalendarKind::Gregorian);
    /// The Hebrew calendar
    pub const HEBREW: Self = Self::new(AnyCalendarKind::Hebrew);
    /// The Indian calendar
    pub const INDIAN: Self = Self::new(AnyCalendarKind::Indian);
    /// The Hijri Tabular calendar with a Friday epoch
    pub const HIJRI_TABULAR_FRIDAY: Self = Self::new(AnyCalendarKind::HijriTabularTypeIIFriday);
    /// The Hijri Tabular calendar with a Thursday epoch
    pub const HIJRI_TABULAR_THURSDAY: Self = Self::new(AnyCalendarKind::HijriTabularTypeIIThursday);
    /// The Hijri Umm al-Qura calendar
    pub const HIJRI_UMM_AL_QURA: Self = Self::new(AnyCalendarKind::HijriUmmAlQura);
    /// The Hijri simulated calendar
    pub const HIJRI_SIMULATED: Self = Self::new(AnyCalendarKind::HijriSimulatedMecca);
    /// The ISO 8601 calendar
    pub const ISO: Self = Self::new(AnyCalendarKind::Iso);
    /// The Japanese calendar
    pub const JAPANESE: Self = Self::new(AnyCalendarKind::Japanese);
    /// The Persian calendar
    pub const PERSIAN: Self = Self::new(AnyCalendarKind::Persian);
    /// The ROC calendar
    pub const ROC: Self = Self::new(AnyCalendarKind::Roc);

    /// Create a `Calendar` from an ICU [`AnyCalendarKind`].
    #[warn(clippy::wildcard_enum_match_arm)] // Warns if the calendar kind gets out of sync.
    pub const fn new(kind: AnyCalendarKind) -> Self {
        let cal = match kind {
            AnyCalendarKind::Buddhist => &AnyCalendar::Buddhist(Buddhist),
            AnyCalendarKind::Chinese => const { &AnyCalendar::Chinese(Chinese::new()) },
            AnyCalendarKind::Coptic => &AnyCalendar::Coptic(Coptic),
            AnyCalendarKind::Dangi => const { &AnyCalendar::Dangi(Dangi::new()) },
            AnyCalendarKind::Ethiopian => {
                const {
                    &AnyCalendar::Ethiopian(Ethiopian::new_with_era_style(
                        EthiopianEraStyle::AmeteMihret,
                    ))
                }
            }
            AnyCalendarKind::EthiopianAmeteAlem => {
                const {
                    &AnyCalendar::Ethiopian(Ethiopian::new_with_era_style(
                        EthiopianEraStyle::AmeteAlem,
                    ))
                }
            }
            AnyCalendarKind::Gregorian => &AnyCalendar::Gregorian(Gregorian),
            AnyCalendarKind::Hebrew => &AnyCalendar::Hebrew(Hebrew),
            AnyCalendarKind::Indian => &AnyCalendar::Indian(Indian),
            AnyCalendarKind::HijriTabularTypeIIFriday => {
                const {
                    &AnyCalendar::HijriTabular(HijriTabular::new(
                        HijriTabularLeapYears::TypeII,
                        HijriTabularEpoch::Friday,
                    ))
                }
            }
            AnyCalendarKind::HijriSimulatedMecca => {
                const { &AnyCalendar::HijriSimulated(HijriSimulated::new_mecca()) }
            }
            AnyCalendarKind::HijriTabularTypeIIThursday => {
                const {
                    &AnyCalendar::HijriTabular(HijriTabular::new(
                        HijriTabularLeapYears::TypeII,
                        HijriTabularEpoch::Thursday,
                    ))
                }
            }
            AnyCalendarKind::HijriUmmAlQura => {
                const { &AnyCalendar::HijriUmmAlQura(HijriUmmAlQura::new()) }
            }
            AnyCalendarKind::Iso => &AnyCalendar::Iso(Iso),
            AnyCalendarKind::Japanese => const { &AnyCalendar::Japanese(Japanese::new()) },
            AnyCalendarKind::JapaneseExtended => {
                const { &AnyCalendar::JapaneseExtended(JapaneseExtended::new()) }
            }
            AnyCalendarKind::Persian => &AnyCalendar::Persian(Persian),
            AnyCalendarKind::Roc => &AnyCalendar::Roc(Roc),
            _ => {
                debug_assert!(
                    false,
                    "Unreachable: match must handle all variants of `AnyCalendarKind`"
                );
                &AnyCalendar::Iso(Iso)
            }
        };

        Self(Ref(cal))
    }

    /// Returns a `Calendar` from the a slice of UTF-8 encoded bytes.
    pub fn try_from_utf8(bytes: &[u8]) -> TemporalResult<Self> {
        let kind = Self::try_kind_from_utf8(bytes)?;
        Ok(Self::new(kind))
    }

    /// Returns a `Calendar` from the a slice of UTF-8 encoded bytes.
    pub(crate) fn try_kind_from_utf8(bytes: &[u8]) -> TemporalResult<AnyCalendarKind> {
        match bytes.to_ascii_lowercase().as_slice() {
            b"ethioaa" | b"ethiopic-amete-alem" => {
                return Ok(AnyCalendarKind::EthiopianAmeteAlem);
            }
            _ => {}
        }

        // TODO: Determine the best way to handle "julian" here.
        // Not supported by `CalendarAlgorithm`
        let icu_locale_value = Value::try_from_utf8(&bytes.to_ascii_lowercase())
            .map_err(|_| TemporalError::range().with_message("unknown calendar"))?;
        let algorithm = CalendarAlgorithm::try_from(&icu_locale_value)
            .map_err(|_| TemporalError::range().with_message("unknown calendar"))?;
        let calendar_kind = match AnyCalendarKind::try_from(algorithm) {
            Ok(c) => c,
            // Handle `islamic` calendar idenitifier.
            //
            // This should be updated depending on `icu_calendar` support and
            // intl-era-monthcode.
            Err(()) if algorithm == CalendarAlgorithm::Hijri(None) => {
                AnyCalendarKind::HijriTabularTypeIIFriday
            }
            Err(()) => return Err(TemporalError::range().with_message("unknown calendar")),
        };
        Ok(calendar_kind)
    }
}

impl FromStr for Calendar {
    type Err = TemporalError;

    // 13.34 ParseTemporalCalendarString ( string )
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match parse_allowed_calendar_formats(s.as_bytes()) {
            Some([]) => Ok(Calendar::ISO),
            Some(result) => Calendar::try_from_utf8(result),
            None => Calendar::try_from_utf8(s.as_bytes()),
        }
    }
}

impl TryFrom<Unit> for IcuUnit {
    type Error = TemporalError;

    fn try_from(other: Unit) -> TemporalResult<Self> {
        Ok(match other {
            Unit::Day => Self::Days,
            Unit::Week => Self::Weeks,
            Unit::Month => Self::Months,
            Unit::Year => Self::Years,
            _ => {
                return Err(TemporalError::r#type()
                    .with_message("Found time unit when computing CalendarDateUntil."))
            }
        })
    }
}

/// Guard `icu_calendar` date arithmetic from obviously out-of-range intermediate durations.
fn early_constrain_date_duration(duration: &DateDuration) -> TemporalResult<()> {
    // Temporal range is approximately -271821-04-20 to +275760-09-13.
    const TEMPORAL_MAX_ISO_YEAR_DURATION: u64 = (275_760 + 271_821) as u64;
    const YEAR_DURATION: u64 = 2 * TEMPORAL_MAX_ISO_YEAR_DURATION;
    const MONTH_DURATION: u64 = YEAR_DURATION * 13;
    const DAY_DURATION: u64 = YEAR_DURATION * 390;
    const WEEK_DURATION: u64 = DAY_DURATION / 7;

    let err = TemporalError::range().with_enum(ErrorMessage::IntermediateDateTimeOutOfRange);

    if duration.years.unsigned_abs() > YEAR_DURATION {
        return Err(err);
    }
    if duration.months.unsigned_abs() > MONTH_DURATION {
        return Err(err);
    }
    if duration.weeks.unsigned_abs() > WEEK_DURATION {
        return Err(err);
    }
    if duration.days.unsigned_abs() > DAY_DURATION {
        return Err(err);
    }

    Ok(())
}

// ==== Public `CalendarSlot` methods ====

impl Calendar {
    /// Returns whether the current calendar is `ISO`
    #[inline]
    pub fn is_iso(&self) -> bool {
        matches!(self.0 .0, AnyCalendar::Iso(_))
    }

    /// Returns the kind of this calendar
    #[inline]
    pub fn kind(&self) -> AnyCalendarKind {
        self.0 .0.kind()
    }

    /// `CalendarDateFromFields`
    pub fn date_from_fields(
        &self,
        fields: CalendarFields,
        overflow: Overflow,
    ) -> TemporalResult<PlainDate> {
        let resolved_fields =
            ResolvedCalendarFields::try_from_fields(self, &fields, overflow, ResolutionType::Date)?;

        if self.is_iso() {
            // Resolve month and monthCode;
            return PlainDate::new_with_overflow(
                resolved_fields.era_year.arithmetic_year,
                resolved_fields.month_code.to_month_integer(),
                resolved_fields.day,
                self.clone(),
                overflow,
            );
        }

        let build_date = |month_code: MonthCode, day| -> TemporalResult<PlainDate> {
            let (era, year) = resolved_fields
                .era_year
                .era
                .as_ref()
                .map(|e| (Some(e.0), resolved_fields.era_year.year))
                .or_else(|| {
                    self.era_year_for_arithmetic_date(
                        resolved_fields.era_year.arithmetic_year,
                        month_code,
                        day,
                    )
                        .map(|(era, year)| (Some(era), year))
                })
                .unwrap_or((None, resolved_fields.era_year.year));
            let calendar_date = self.0.from_codes(
                era.as_ref().map(TinyAsciiStr::as_str),
                year,
                IcuMonthCode(month_code.0),
                day,
            )?;
            let iso = self.0.to_iso(&calendar_date);
            PlainDate::new_with_overflow(
                Iso.extended_year(&iso),
                Iso.month(&iso).ordinal,
                Iso.day_of_month(&iso).0,
                self.clone(),
                Overflow::Reject,
            )
        };

        if overflow == Overflow::Constrain {
            let mut candidates = [resolved_fields.month_code, resolved_fields.month_code];
            let candidate_count =
                constrain_month_code_candidates(self, resolved_fields.month_code, &mut candidates);
            for month_code in candidates.into_iter().take(candidate_count) {
                let mut day = resolved_fields.day;
                loop {
                    if let Ok(date) = build_date(month_code, day) {
                        return Ok(date);
                    }
                    if day == 1 {
                        break;
                    }
                    day -= 1;
                }
            }
        }

        build_date(resolved_fields.month_code, resolved_fields.day)
    }

    /// `CalendarPlainMonthDayFromFields`
    pub fn month_day_from_fields(
        &self,
        mut fields: CalendarFields,
        overflow: Overflow,
    ) -> TemporalResult<PlainMonthDay> {
        // You are allowed to specify year information, however
        // it is *only* used for resolving the given month/day data.
        //
        // For example, constructing a PlainMonthDay for {year: 2025, month: 2, day: 29}
        // with overflow: constrain will produce 02-28 since it will constrain
        // the date to 2025-02-28 first, and only *then* will it construct an MD.
        //
        // This is specced partially in https://tc39.es/proposal-temporal/#sec-temporal-calendarmonthdaytoisoreferencedate
        // notice that RegulateISODate is called with the passed-in year, but the reference year is used regardless
        // of the passed in year in the final result.
        //
        // There may be more efficient ways to do this, but this works pretty well and doesn't require
        // calendrical knowledge.
        let had_explicit_year =
            fields.year.is_some() || (fields.era.is_some() && fields.era_year.is_some());
        if had_explicit_year {
            if self.is_iso() {
                let year = fields.year.ok_or_else(|| {
                    TemporalError::r#type()
                        .with_message("Required fields missing to determine an era and year.")
                })?;
                let month_code = types::resolve_iso_month(self, &fields, overflow)?;
                let day = fields.day.ok_or_else(|| {
                    TemporalError::r#type().with_message("MonthDay must specify day")
                })?;
                let day = if overflow == Overflow::Constrain {
                    constrain_iso_day(year, month_code.to_month_integer(), day)
                } else {
                    if !is_valid_iso_day(year, month_code.to_month_integer(), day) {
                        return Err(
                            TemporalError::range()
                                .with_message("day value is not in a valid range."),
                        );
                    }
                    day
                };

                return PlainMonthDay::new_with_overflow(
                    month_code.to_month_integer(),
                    day,
                    self.clone(),
                    Overflow::Reject,
                    None,
                );
            }
            let date = self.date_from_fields(fields, overflow)?;
            fields = CalendarFields::from_date(&date);
        }

        if !had_explicit_year
            && matches!(
            self.kind(),
            AnyCalendarKind::Chinese | AnyCalendarKind::Dangi
        ) {
            let month_code = MonthCode::try_from_fields(self, &fields, None, overflow)?;
            let day = fields
                .day
                .ok_or(TemporalError::r#type().with_message("MonthDay must specify day"))?;
            let (month_code, reference_year, day) =
                chinese_or_dangi_month_day_reference(self.kind(), month_code, day, overflow)?;

            let calendar_date = self
                .0
                .from_codes(None, reference_year, IcuMonthCode(month_code.as_tinystr()), day)?;
            let iso = self.0.to_iso(&calendar_date);
            return PlainMonthDay::new_with_overflow(
                Iso.month(&iso).ordinal,
                Iso.day_of_month(&iso).0,
                self.clone(),
                Overflow::Reject,
                Some(Iso.extended_year(&iso)),
            );
        }

        let resolved_fields = ResolvedCalendarFields::try_from_fields(
            self,
            &fields,
            overflow,
            ResolutionType::MonthDay,
        )?;
        if self.is_iso() {
            return PlainMonthDay::new_with_overflow(
                resolved_fields.month_code.to_month_integer(),
                resolved_fields.day,
                self.clone(),
                overflow,
                None,
            );
        }

        let build_month_day = |month_code: MonthCode, day: u8| -> TemporalResult<PlainMonthDay> {
            let (era, year) = resolved_fields
                .era_year
                .era
                .as_ref()
                .map(|e| (Some(e.0), resolved_fields.era_year.year))
                .or_else(|| {
                    self.era_year_for_arithmetic_date(
                        resolved_fields.era_year.arithmetic_year,
                        month_code,
                        day,
                    )
                        .map(|(era, year)| (Some(era), year))
                })
                .unwrap_or((None, resolved_fields.era_year.year));
            let calendar_date = self.0.from_codes(
                era.as_ref().map(TinyAsciiStr::as_str),
                year,
                IcuMonthCode(month_code.as_tinystr()),
                day,
            )?;
            let iso = self.0.to_iso(&calendar_date);
            PlainMonthDay::new_with_overflow(
                Iso.month(&iso).ordinal,
                Iso.day_of_month(&iso).0,
                self.clone(),
                Overflow::Reject,
                Some(Iso.extended_year(&iso)),
            )
        };

        if overflow == Overflow::Constrain {
            let mut candidates = [resolved_fields.month_code, resolved_fields.month_code];
            let candidate_count =
                constrain_month_code_candidates(self, resolved_fields.month_code, &mut candidates);
            for month_code in candidates.into_iter().take(candidate_count) {
                let mut day = resolved_fields.day;
                loop {
                    if let Ok(month_day) = build_month_day(month_code, day) {
                        return Ok(month_day);
                    }

                    if day == 1 {
                        break;
                    }
                    day -= 1;
                }
            }
        }

        build_month_day(resolved_fields.month_code, resolved_fields.day)
    }

    /// `CalendarPlainYearMonthFromFields`
    pub fn year_month_from_fields(
        &self,
        fields: YearMonthCalendarFields,
        overflow: Overflow,
    ) -> TemporalResult<PlainYearMonth> {
        // TODO: add a from_partial_year_month method on ResolvedCalendarFields
        let resolved_fields = ResolvedCalendarFields::try_from_fields(
            self,
            &CalendarFields::from(fields),
            overflow,
            ResolutionType::YearMonth,
        )?;
        if self.is_iso() {
            return PlainYearMonth::new_with_overflow(
                resolved_fields.era_year.arithmetic_year,
                resolved_fields.month_code.to_month_integer(),
                Some(resolved_fields.day),
                self.clone(),
                overflow,
            );
        }

        // NOTE: This might preemptively throw as `ICU4X` does not support regulating.
        let (era, year) = resolved_fields
            .era_year
            .era
            .as_ref()
            .map(|e| (Some(e.0), resolved_fields.era_year.year))
            .or_else(|| {
                self.era_year_for_arithmetic_date(
                    resolved_fields.era_year.arithmetic_year,
                    resolved_fields.month_code,
                    resolved_fields.day,
                )
                    .map(|(era, year)| (Some(era), year))
            })
            .unwrap_or((None, resolved_fields.era_year.year));
        let calendar_date =
            self.0.from_codes(
                era.as_ref().map(TinyAsciiStr::as_str),
                year,
                IcuMonthCode(resolved_fields.month_code.0),
                resolved_fields.day,
            )?;
        let iso = self.0.to_iso(&calendar_date);
        PlainYearMonth::new_with_overflow(
            Iso.year_info(&iso).year,
            Iso.month(&iso).ordinal,
            Some(Iso.day_of_month(&iso).0),
            self.clone(),
            overflow,
        )
    }

    /// `CalendarDateAdd`
    pub fn date_add(
        &self,
        date: &IsoDate,
        duration: &DateDuration,
        overflow: Overflow,
    ) -> TemporalResult<PlainDate> {
        // 1. If calendar is "iso8601", then
        if self.is_iso() {
            let result = date.add_date_duration(duration, overflow)?;
            // 11. Return ? CreateTemporalDate(result.[[Year]], result.[[Month]], result.[[Day]], "iso8601").
            return PlainDate::try_new(result.year, result.month, result.day, self.clone());
        }

        early_constrain_date_duration(duration)?;

        #[derive(Clone, Copy)]
        struct CalendarDateRecord {
            iso: IsoDate,
            year: i32,
            month: u8,
            month_code: MonthCode,
            day: u8,
        }

        impl CalendarDateRecord {
            fn from_iso(calendar: &Calendar, iso: IsoDate) -> Self {
                Self {
                    iso,
                    year: calendar.year(&iso),
                    month: calendar.month(&iso),
                    month_code: calendar.month_code(&iso),
                    day: calendar.day(&iso),
                }
            }
        }

        let regulate_day = |record: &CalendarDateRecord, day: u8| -> TemporalResult<CalendarDateRecord> {
            let fields = CalendarFields::new()
                .with_year(record.year)
                .with_month_code(record.month_code)
                .with_day(day);
            let regulated = self.date_from_fields(fields, Overflow::Constrain)?;
            Ok(CalendarDateRecord::from_iso(self, regulated.iso))
        };
        let adjust_calendar_date =
            |year: i32,
             month_code: MonthCode,
             day: u8,
             overflow: Overflow|
             -> TemporalResult<CalendarDateRecord> {
                let fields = CalendarFields::new()
                    .with_year(year)
                    .with_month_code(month_code)
                    .with_day(day);
                let adjusted = self.date_from_fields(fields, overflow)?;
                Ok(CalendarDateRecord::from_iso(self, adjusted.iso))
            };
        let add_days_calendar =
            |record: &CalendarDateRecord, days: i64| -> TemporalResult<CalendarDateRecord> {
                let added = IsoDate::try_balance(
                    record.iso.year,
                    i32::from(record.iso.month),
                    i64::from(record.iso.day) + days,
                )?;
                Ok(CalendarDateRecord::from_iso(self, added))
            };
        let days_in_previous_month = |record: &CalendarDateRecord| -> TemporalResult<u8> {
            let first_of_month = regulate_day(record, 1)?;
            let previous = add_days_calendar(&first_of_month, -1)?;
            Ok(previous.day)
        };
        let add_months_calendar =
            |record: &CalendarDateRecord, months: i64, overflow: Overflow| -> TemporalResult<CalendarDateRecord> {
                let original_day = record.day;
                let mut calendar_date = *record;

                for _ in 0..months.unsigned_abs() {
                    let month = calendar_date.month;
                    let old_calendar_date = calendar_date;
                    let days = if months < 0 {
                        -i64::from(original_day.max(days_in_previous_month(&calendar_date)?))
                    } else {
                        i64::from(self.days_in_month(&calendar_date.iso))
                    };

                    calendar_date = add_days_calendar(&calendar_date, days)?;

                    if months > 0 {
                        let months_in_old_year = self.months_in_year(&old_calendar_date.iso);
                        while calendar_date.month - 1 != month % months_in_old_year as u8 {
                            calendar_date = add_days_calendar(&calendar_date, -1)?;
                        }
                    }

                    if calendar_date.day != original_day {
                        calendar_date = regulate_day(&calendar_date, original_day)?;
                    }
                }

                if overflow == Overflow::Reject && calendar_date.day != original_day {
                    return Err(TemporalError::range()
                        .with_message("Day does not exist in resulting calendar month."));
                }

                Ok(calendar_date)
            };

        let record = CalendarDateRecord::from_iso(self, *date);
        let mut year_month_anchor = regulate_day(&record, 1)?;
        if duration.years != 0 {
            let years = i32::try_from(duration.years)
                .map_err(|_| TemporalError::range().with_enum(ErrorMessage::IntermediateDateTimeOutOfRange))?;
            let year = record
                .year
                .checked_add(years)
                .ok_or_else(|| TemporalError::range().with_enum(ErrorMessage::IntermediateDateTimeOutOfRange))?;
            year_month_anchor = adjust_calendar_date(year, record.month_code, 1, overflow)?;
        }
        if duration.months != 0 {
            year_month_anchor = add_months_calendar(&year_month_anchor, duration.months, Overflow::Constrain)?;
        }
        let year_month_added = if duration.years != 0 || duration.months != 0 {
            adjust_calendar_date(
                year_month_anchor.year,
                year_month_anchor.month_code,
                record.day,
                overflow,
            )?
        } else {
            record
        };
        let added_days = add_days_calendar(&year_month_added, duration.days + 7 * duration.weeks)?;

        PlainDate::new_with_overflow(
            added_days.iso.year,
            added_days.iso.month,
            added_days.iso.day,
            self.clone(),
            overflow,
        )
    }

    /// `CalendarDateUntil`
    pub fn date_until(
        &self,
        one: &IsoDate,
        two: &IsoDate,
        largest_unit: Unit,
    ) -> TemporalResult<Duration> {
        if self.is_iso() {
            let date_duration = one.diff_iso_date(two, largest_unit)?;
            return Ok(Duration::from(date_duration));
        }

        #[derive(Clone, Copy)]
        struct CalendarDateRecord {
            iso: IsoDate,
            year: i32,
            month: u8,
            month_code: MonthCode,
            day: u8,
        }

        impl CalendarDateRecord {
            fn from_iso(calendar: &Calendar, iso: IsoDate) -> Self {
                Self {
                    iso,
                    year: calendar.year(&iso),
                    month: calendar.month(&iso),
                    month_code: calendar.month_code(&iso),
                    day: calendar.day(&iso),
                }
            }
        }

        let compare = |a: &CalendarDateRecord, b: &CalendarDateRecord| {
            match a
                .year
                .cmp(&b.year)
                .then_with(|| a.month.cmp(&b.month))
                .then_with(|| a.day.cmp(&b.day))
            {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        };
        let calendar_days_until =
            |a: &CalendarDateRecord, b: &CalendarDateRecord| b.iso.to_epoch_days() - a.iso.to_epoch_days();
        let regulate_day = |record: &CalendarDateRecord, day: u8| -> TemporalResult<CalendarDateRecord> {
            let fields = CalendarFields::new()
                .with_year(record.year)
                .with_month_code(record.month_code)
                .with_day(day);
            let regulated = self.date_from_fields(fields, Overflow::Constrain)?;
            Ok(CalendarDateRecord::from_iso(self, regulated.iso))
        };
        let adjust_calendar_date = |year: i32,
                                    month: u8,
                                    month_code: MonthCode,
                                    day: u8|
         -> TemporalResult<CalendarDateRecord> {
            let fields = CalendarFields::new()
                .with_year(year)
                .with_month_code(month_code)
                .with_day(day);
            let adjusted = self.date_from_fields(fields, Overflow::Constrain)?;
            Ok(CalendarDateRecord::from_iso(self, adjusted.iso))
        };
        let add_days_calendar =
            |record: &CalendarDateRecord, days: i64| -> TemporalResult<CalendarDateRecord> {
                let added = IsoDate::try_balance(
                    record.iso.year,
                    i32::from(record.iso.month),
                    i64::from(record.iso.day) + days,
                )?;
                Ok(CalendarDateRecord::from_iso(self, added))
            };
        let days_in_previous_month = |record: &CalendarDateRecord| -> TemporalResult<u8> {
            let first_of_month = regulate_day(record, 1)?;
            let previous = add_days_calendar(&first_of_month, -1)?;
            Ok(previous.day)
        };
        let add_months_calendar =
            |record: &CalendarDateRecord, months: i64, overflow: Overflow| -> TemporalResult<CalendarDateRecord> {
                let original_day = record.day;
                let mut calendar_date = *record;

                for _ in 0..months.unsigned_abs() {
                    let month = calendar_date.month;
                    let old_calendar_date = calendar_date;
                    let days = if months < 0 {
                        -i64::from(original_day.max(days_in_previous_month(&calendar_date)?))
                    } else {
                        i64::from(self.days_in_month(&calendar_date.iso))
                    };

                    calendar_date = add_days_calendar(&calendar_date, days)?;

                    if months > 0 {
                        let months_in_old_year = self.months_in_year(&old_calendar_date.iso);
                        while calendar_date.month - 1 != month % months_in_old_year as u8 {
                            calendar_date = add_days_calendar(&calendar_date, -1)?;
                        }
                    }

                    if calendar_date.day != original_day {
                        calendar_date = regulate_day(&calendar_date, original_day)?;
                    }
                }

                if overflow == Overflow::Reject && calendar_date.day != original_day {
                    return Err(TemporalError::range()
                        .with_message("Day does not exist in resulting calendar month."));
                }

                Ok(calendar_date)
            };

        let calendar_one = CalendarDateRecord::from_iso(self, *one);
        let calendar_two = CalendarDateRecord::from_iso(self, *two);

        let result = match largest_unit {
            Unit::Day => DateDuration::new(0, 0, 0, calendar_days_until(&calendar_one, &calendar_two))?,
            Unit::Week => {
                let total_days = calendar_days_until(&calendar_one, &calendar_two);
                let days = total_days % 7;
                let weeks = (total_days - days) / 7;
                DateDuration::new(0, 0, weeks, days)?
            }
            Unit::Month | Unit::Year => {
                let sign = compare(&calendar_two, &calendar_one);
                if sign == 0 {
                    DateDuration::default()
                } else {
                    let mut years = 0i64;
                    let mut months = 0i64;
                    let diff_years = i64::from(calendar_two.year - calendar_one.year);
                    let diff_days = i32::from(calendar_two.day) - i32::from(calendar_one.day);

                    if largest_unit == Unit::Year && diff_years != 0 {
                        let diff_in_year_sign = match calendar_two.month_code.0.cmp(&calendar_one.month_code.0) {
                            core::cmp::Ordering::Greater => 1,
                            core::cmp::Ordering::Less => -1,
                            core::cmp::Ordering::Equal => diff_days.signum(),
                        };
                        let is_one_further_in_year = diff_in_year_sign * sign < 0;
                        years = if is_one_further_in_year {
                            diff_years - i64::from(sign)
                        } else {
                            diff_years
                        };

                        if years != 0 {
                            let years_added = adjust_calendar_date(
                                calendar_one.year + years as i32,
                                calendar_one.month,
                                calendar_one.month_code,
                                calendar_one.day,
                            )?;
                            let candidate_cmp = compare(&calendar_two, &years_added) * sign;
                            let intercalary_last_day_match = candidate_cmp == 0
                                && years_added.day != calendar_one.day
                                && matches!(
                                    self.kind(),
                                    AnyCalendarKind::Coptic
                                        | AnyCalendarKind::Ethiopian
                                        | AnyCalendarKind::EthiopianAmeteAlem
                                )
                                && calendar_one.month_code.to_month_integer() == 13
                                && calendar_two.month_code.to_month_integer() == 13
                                && u16::from(calendar_one.day) == self.days_in_month(&calendar_one.iso)
                                && u16::from(calendar_two.day) == self.days_in_month(&calendar_two.iso);
                            if candidate_cmp < 0
                                || (candidate_cmp == 0
                                    && years_added.day != calendar_one.day
                                    && !intercalary_last_day_match)
                            {
                                years -= i64::from(sign);
                            }
                        }
                    }

                    let years_added = if years != 0 {
                        adjust_calendar_date(
                            calendar_one.year + years as i32,
                            calendar_one.month,
                            calendar_one.month_code,
                            calendar_one.day,
                        )?
                    } else {
                        calendar_one
                    };

                    let mut current = years_added;
                    let mut previous = years_added;
                    let mut last_month_was_constrained = false;
                    loop {
                        let mut next =
                            add_months_calendar(&current, i64::from(sign), Overflow::Constrain)?;
                        let mut constrained = false;
                        if next.day != calendar_one.day {
                            next = regulate_day(&next, calendar_one.day)?;
                            constrained = next.day != calendar_one.day;
                        }
                        if compare(&calendar_two, &next) * sign < 0 {
                            break;
                        }
                        months += i64::from(sign);
                        previous = current;
                        current = next;
                        last_month_was_constrained = constrained;
                    }

                    let mut remaining_days = calendar_days_until(&current, &calendar_two);
                    let intercalary_last_day_month_match = remaining_days == 0
                        && last_month_was_constrained
                        && largest_unit == Unit::Month
                        && sign < 0
                        && matches!(
                            self.kind(),
                            AnyCalendarKind::Coptic
                                | AnyCalendarKind::Ethiopian
                                | AnyCalendarKind::EthiopianAmeteAlem
                        )
                        && calendar_one.month_code.to_month_integer() == 13
                        && calendar_two.month_code.to_month_integer() == 13
                        && u16::from(calendar_one.day) == self.days_in_month(&calendar_one.iso)
                        && u16::from(calendar_two.day) == self.days_in_month(&calendar_two.iso);
                    if months != 0
                        && remaining_days == 0
                        && last_month_was_constrained
                        && !intercalary_last_day_month_match
                    {
                        months -= i64::from(sign);
                        current = previous;
                        remaining_days = calendar_days_until(&current, &calendar_two);
                    }
                    DateDuration::new(years, months, 0, remaining_days)?
                }
            }
            _ => {
                let calendar_date1 = IcuDate::new_from_iso(one.to_icu4x(), self.0.clone());
                let calendar_date2 = IcuDate::new_from_iso(two.to_icu4x(), self.0.clone());
                let diff =
                    calendar_date2.until(&calendar_date1, largest_unit.try_into()?, IcuUnit::Days);
                DateDuration::new(
                    i64::from(diff.years),
                    i64::from(diff.months),
                    i64::from(diff.weeks),
                    i64::from(diff.days),
                )?
            }
        };

        Ok(Duration::from(result))
    }

    /// `CalendarEra`
    pub fn era(&self, iso_date: &IsoDate) -> Option<TinyAsciiStr<16>> {
        if self.is_iso() {
            return None;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0
            .year_info(&calendar_date)
            .era()
            .map(|era_info| era_info.era)
    }

    /// `CalendarEraYear`
    pub fn era_year(&self, iso_date: &IsoDate) -> Option<i32> {
        if self.is_iso() {
            return None;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0
            .year_info(&calendar_date)
            .era()
            .map(|era_info| era_info.year)
    }

    /// `CalendarArithmeticYear`
    pub fn year(&self, iso_date: &IsoDate) -> i32 {
        if self.is_iso() {
            return iso_date.year;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        match (self.kind(), self.0.year_info(&calendar_date)) {
            (AnyCalendarKind::Chinese | AnyCalendarKind::Dangi, YearInfo::Cyclic(cyclic)) => {
                cyclic.related_iso
            }
            _ => self.0.extended_year(&calendar_date),
        }
    }

    /// `CalendarMonth`
    pub fn month(&self, iso_date: &IsoDate) -> u8 {
        if self.is_iso() {
            return iso_date.month;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.month(&calendar_date).ordinal
    }

    /// `CalendarMonthCode`
    pub fn month_code(&self, iso_date: &IsoDate) -> MonthCode {
        if self.is_iso() {
            let mc = iso_date.to_icu4x().month().standard_code.0;
            return MonthCode(mc);
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        MonthCode(self.0.month(&calendar_date).standard_code.0)
    }

    /// `CalendarDay`
    pub fn day(&self, iso_date: &IsoDate) -> u8 {
        if self.is_iso() {
            return iso_date.day;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.day_of_month(&calendar_date).0
    }

    /// `CalendarDayOfWeek`
    pub fn day_of_week(&self, iso_date: &IsoDate) -> u16 {
        iso_date.to_icu4x().day_of_week() as u16
    }

    /// `CalendarDayOfYear`
    pub fn day_of_year(&self, iso_date: &IsoDate) -> u16 {
        if self.is_iso() {
            return iso_date.to_icu4x().day_of_year().0;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.day_of_year(&calendar_date).0
    }

    /// `CalendarWeekOfYear`
    pub fn week_of_year(&self, iso_date: &IsoDate) -> Option<u8> {
        if self.is_iso() {
            return Some(iso_date.to_icu4x().week_of_year().week_number);
        }
        // TODO: Research in ICU4X and determine best approach.
        None
    }

    /// `CalendarYearOfWeek`
    pub fn year_of_week(&self, iso_date: &IsoDate) -> Option<i32> {
        if self.is_iso() {
            return Some(iso_date.to_icu4x().week_of_year().iso_year);
        }
        // TODO: Research in ICU4X and determine best approach.
        None
    }

    /// `CalendarDaysInWeek`
    pub fn days_in_week(&self, _iso_date: &IsoDate) -> u16 {
        7
    }

    /// `CalendarDaysInMonth`
    pub fn days_in_month(&self, iso_date: &IsoDate) -> u16 {
        if self.is_iso() {
            return iso_date.to_icu4x().days_in_month() as u16;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.days_in_month(&calendar_date) as u16
    }

    /// `CalendarDaysInYear`
    pub fn days_in_year(&self, iso_date: &IsoDate) -> u16 {
        if self.is_iso() {
            return iso_date.to_icu4x().days_in_year();
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.days_in_year(&calendar_date)
    }

    /// `CalendarMonthsInYear`
    pub fn months_in_year(&self, iso_date: &IsoDate) -> u16 {
        if self.is_iso() {
            return 12;
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.months_in_year(&calendar_date) as u16
    }

    /// `CalendarInLeapYear`
    pub fn in_leap_year(&self, iso_date: &IsoDate) -> bool {
        if self.is_iso() {
            return iso_date.to_icu4x().is_in_leap_year();
        }
        let calendar_date = self.0.from_iso(*iso_date.to_icu4x().inner());
        self.0.is_in_leap_year(&calendar_date)
    }

    /// Returns the identifier of this calendar slot.
    pub fn identifier(&self) -> &'static str {
        match self.kind() {
            AnyCalendarKind::Buddhist => "buddhist",
            AnyCalendarKind::Chinese => "chinese",
            AnyCalendarKind::Coptic => "coptic",
            AnyCalendarKind::Dangi => "dangi",
            AnyCalendarKind::Ethiopian => "ethiopic",
            AnyCalendarKind::EthiopianAmeteAlem => "ethioaa",
            AnyCalendarKind::Gregorian => "gregory",
            AnyCalendarKind::Hebrew => "hebrew",
            AnyCalendarKind::Indian => "indian",
            AnyCalendarKind::HijriSimulatedMecca => "islamic",
            AnyCalendarKind::HijriTabularTypeIIFriday => "islamic-civil",
            AnyCalendarKind::HijriTabularTypeIIThursday => "islamic-tbla",
            AnyCalendarKind::HijriUmmAlQura => "islamic-umalqura",
            AnyCalendarKind::Iso => "iso8601",
            AnyCalendarKind::Japanese | AnyCalendarKind::JapaneseExtended => "japanese",
            AnyCalendarKind::Persian => "persian",
            AnyCalendarKind::Roc => "roc",
            _ => match self.0.calendar_algorithm() {
                Some(c) => c.as_str(),
                None if self.is_iso() => "iso8601",
                None => "julian",
            },
        }
    }
}

fn chinese_or_dangi_regular_day30_year(kind: AnyCalendarKind, month: u8) -> i32 {
    match (kind, month) {
        (AnyCalendarKind::Dangi, 3) => 1968,
        (_, 1) => 1970,
        (_, 2) => 1972,
        (_, 3) => 1966,
        (_, 4) => 1970,
        (_, 5) => 1972,
        (_, 6) => 1971,
        (_, 7) => 1972,
        (_, 8) => 1971,
        (_, 9) => 1972,
        (_, 10) => 1972,
        (_, 11) => 1970,
        (_, 12) => 1972,
        _ => 1972,
    }
}

fn chinese_or_dangi_leap_day_1_to_29_year(month: u8, day: u8) -> Option<i32> {
    match month {
        1 => None,
        2 => Some(1947),
        3 => Some(1966),
        4 => Some(1963),
        5 => Some(1971),
        6 => Some(1960),
        7 => Some(1968),
        8 => Some(1957),
        9 => Some(2014),
        10 => Some(1984),
        11 => Some(if day == 29 { 2034 } else { 2033 }),
        12 => None,
        _ => None,
    }
}

fn chinese_or_dangi_leap_day30_year(month: u8) -> Option<i32> {
    match month {
        3 => Some(1955),
        4 => Some(1944),
        5 => Some(1952),
        6 => Some(1941),
        7 => Some(1938),
        _ => None,
    }
}

fn chinese_or_dangi_month_day_reference(
    kind: AnyCalendarKind,
    month_code: MonthCode,
    mut day: u8,
    overflow: Overflow,
) -> TemporalResult<(MonthCode, i32, u8)> {
    if day > 30 {
        if overflow == Overflow::Reject {
            return Err(TemporalError::range().with_message("Day out of range."));
        }
        day = 30;
    }

    if !month_code.is_leap_month() {
        let reference_year = if day <= 29 {
            1972
        } else {
            chinese_or_dangi_regular_day30_year(kind, month_code.to_month_integer())
        };
        return Ok((month_code, reference_year, day));
    }

    let month = month_code.to_month_integer();
    if day <= 29 {
        if let Some(reference_year) = chinese_or_dangi_leap_day_1_to_29_year(month, day) {
            return Ok((month_code, reference_year, day));
        }
        if overflow == Overflow::Reject {
            return Err(TemporalError::range().with_message("Day out of range."));
        }
        return Ok((month_code.without_leap(), 1972, day));
    }

    if let Some(reference_year) = chinese_or_dangi_leap_day30_year(month) {
        return Ok((month_code, reference_year, day));
    }
    if overflow == Overflow::Reject {
        return Err(TemporalError::range().with_message("Day out of range."));
    }

    Ok((
        month_code.without_leap(),
        chinese_or_dangi_regular_day30_year(kind, month),
        30,
    ))
}

fn constrain_month_code_candidates(
    calendar: &Calendar,
    month_code: MonthCode,
    candidates: &mut [MonthCode; 2],
) -> usize {
    candidates[0] = month_code;
    let fallback = match calendar.kind() {
        AnyCalendarKind::Chinese | AnyCalendarKind::Dangi if month_code.is_leap_month() => {
            Some(month_code.without_leap())
        }
        AnyCalendarKind::Hebrew if month_code.as_tinystr() == tinystr!(4, "M05L") => {
            Some(MonthCode(tinystr!(4, "M06")))
        }
        _ => None,
    };

    if let Some(fallback) = fallback {
        if fallback != month_code {
            candidates[1] = fallback;
            return 2;
        }
    }

    1
}

impl Calendar {
    pub(crate) fn get_era_info(&self, era_alias: &TinyAsciiStr<19>) -> Option<EraInfo> {
        match self.0 .0.kind() {
            AnyCalendarKind::Buddhist if *era_alias == tinystr!(19, "be") => {
                Some(era::BUDDHIST_ERA)
            }
            AnyCalendarKind::Coptic if *era_alias == tinystr!(19, "am") => Some(era::COPTIC_ERA),
            AnyCalendarKind::Ethiopian if era::ETHIOPIC_ERA_IDENTIFIERS.contains(era_alias) => {
                Some(era::ETHIOPIC_ERA)
            }
            AnyCalendarKind::Ethiopian
                if era::ETHIOPIC_ETHOPICAA_ERA_IDENTIFIERS.contains(era_alias) =>
            {
                Some(era::ETHIOPIC_ETHIOAA_ERA)
            }
            AnyCalendarKind::EthiopianAmeteAlem
                if era::ETHIOAA_ERA_IDENTIFIERS.contains(era_alias) =>
            {
                Some(era::ETHIOAA_ERA)
            }
            AnyCalendarKind::Gregorian if era::GREGORY_ERA_IDENTIFIERS.contains(era_alias) => {
                Some(era::GREGORY_ERA)
            }
            AnyCalendarKind::Gregorian
                if era::GREGORY_INVERSE_ERA_IDENTIFIERS.contains(era_alias) =>
            {
                Some(era::GREGORY_INVERSE_ERA)
            }
            AnyCalendarKind::Hebrew if *era_alias == tinystr!(19, "am") => Some(era::HEBREW_ERA),
            AnyCalendarKind::Indian if *era_alias == tinystr!(19, "shaka") => Some(era::INDIAN_ERA),
            AnyCalendarKind::HijriTabularTypeIIFriday
            | AnyCalendarKind::HijriSimulatedMecca
            | AnyCalendarKind::HijriTabularTypeIIThursday
            | AnyCalendarKind::HijriUmmAlQura
                if *era_alias == tinystr!(19, "ah") =>
            {
                Some(era::ISLAMIC_ERA)
            }
            AnyCalendarKind::HijriTabularTypeIIFriday
            | AnyCalendarKind::HijriSimulatedMecca
            | AnyCalendarKind::HijriTabularTypeIIThursday
            | AnyCalendarKind::HijriUmmAlQura
                if *era_alias == tinystr!(19, "bh") =>
            {
                Some(era::ISLAMIC_INVERSE_ERA)
            }
            AnyCalendarKind::Japanese if *era_alias == tinystr!(19, "heisei") => {
                Some(era::HEISEI_ERA)
            }
            AnyCalendarKind::Japanese if era::JAPANESE_ERA_IDENTIFIERS.contains(era_alias) => {
                Some(era::JAPANESE_ERA)
            }
            AnyCalendarKind::Japanese
                if era::JAPANESE_INVERSE_ERA_IDENTIFIERS.contains(era_alias) =>
            {
                Some(era::JAPANESE_INVERSE_ERA)
            }
            AnyCalendarKind::Japanese if *era_alias == tinystr!(19, "meiji") => {
                Some(era::MEIJI_ERA)
            }
            AnyCalendarKind::Japanese if *era_alias == tinystr!(19, "reiwa") => {
                Some(era::REIWA_ERA)
            }
            AnyCalendarKind::Japanese if *era_alias == tinystr!(19, "showa") => {
                Some(era::SHOWA_ERA)
            }
            AnyCalendarKind::Japanese if *era_alias == tinystr!(19, "taisho") => {
                Some(era::TAISHO_ERA)
            }
            AnyCalendarKind::Persian if *era_alias == tinystr!(19, "ap") => Some(era::PERSIAN_ERA),
            AnyCalendarKind::Roc if *era_alias == tinystr!(19, "roc") => Some(era::ROC_ERA),
            AnyCalendarKind::Roc if *era_alias == tinystr!(19, "broc") => {
                Some(era::ROC_INVERSE_ERA)
            }
            _ => None,
        }
    }

    pub(crate) fn get_calendar_default_era(&self) -> Option<EraInfo> {
        match self.0 .0.kind() {
            AnyCalendarKind::Buddhist => Some(era::BUDDHIST_ERA),
            AnyCalendarKind::Chinese => None,
            AnyCalendarKind::Coptic => Some(era::COPTIC_ERA),
            AnyCalendarKind::Dangi => None,
            AnyCalendarKind::Ethiopian => Some(era::ETHIOPIC_ERA),
            AnyCalendarKind::EthiopianAmeteAlem => Some(era::ETHIOAA_ERA),
            AnyCalendarKind::Gregorian => Some(era::GREGORY_ERA),
            AnyCalendarKind::Hebrew => Some(era::HEBREW_ERA),
            AnyCalendarKind::Indian => Some(era::INDIAN_ERA),
            AnyCalendarKind::HijriSimulatedMecca => Some(era::ISLAMIC_ERA),
            AnyCalendarKind::HijriTabularTypeIIFriday => Some(era::ISLAMIC_ERA),
            AnyCalendarKind::HijriTabularTypeIIThursday => Some(era::ISLAMIC_ERA),
            AnyCalendarKind::HijriUmmAlQura => Some(era::ISLAMIC_ERA),
            AnyCalendarKind::Iso => None,
            AnyCalendarKind::Japanese => Some(era::JAPANESE_ERA),
            AnyCalendarKind::Persian => Some(era::PERSIAN_ERA),
            AnyCalendarKind::Roc => Some(era::ROC_ERA),
            _ => None,
        }
    }

    pub(crate) fn era_year_for_arithmetic_year(
        &self,
        arithmetic_year: i32,
    ) -> Option<(TinyAsciiStr<16>, i32)> {
        let eras: &[era::EraInfo] = match self.0 .0.kind() {
            AnyCalendarKind::Buddhist => &[era::BUDDHIST_ERA],
            AnyCalendarKind::Coptic => &[era::COPTIC_ERA],
            AnyCalendarKind::Ethiopian => &[era::ETHIOPIC_ERA, era::ETHIOPIC_ETHIOAA_ERA],
            AnyCalendarKind::EthiopianAmeteAlem => &[era::ETHIOAA_ERA],
            AnyCalendarKind::Gregorian => &[era::GREGORY_ERA, era::GREGORY_INVERSE_ERA],
            AnyCalendarKind::Hebrew => &[era::HEBREW_ERA],
            AnyCalendarKind::Indian => &[era::INDIAN_ERA],
            AnyCalendarKind::HijriSimulatedMecca
            | AnyCalendarKind::HijriTabularTypeIIFriday
            | AnyCalendarKind::HijriTabularTypeIIThursday
            | AnyCalendarKind::HijriUmmAlQura => &[era::ISLAMIC_ERA, era::ISLAMIC_INVERSE_ERA],
            AnyCalendarKind::Japanese => &[
                era::JAPANESE_ERA,
                era::JAPANESE_INVERSE_ERA,
                era::MEIJI_ERA,
                era::TAISHO_ERA,
                era::SHOWA_ERA,
                era::HEISEI_ERA,
                era::REIWA_ERA,
            ],
            AnyCalendarKind::Persian => &[era::PERSIAN_ERA],
            AnyCalendarKind::Roc => &[era::ROC_ERA, era::ROC_INVERSE_ERA],
            _ => &[],
        };

        eras.iter().find_map(|era| {
            era.era_year_for_arithmetic_year(arithmetic_year)
                .map(|year| (era.name, year))
        })
    }

    pub(crate) fn era_year_for_arithmetic_date(
        &self,
        arithmetic_year: i32,
        month_code: MonthCode,
        day: u8,
    ) -> Option<(TinyAsciiStr<16>, i32)> {
        if self.kind() != AnyCalendarKind::Japanese {
            return self.era_year_for_arithmetic_year(arithmetic_year);
        }

        let month = month_code.to_month_integer();
        let is_on_or_after = |start_year: i32, start_month: u8, start_day: u8| {
            (arithmetic_year, month, day) >= (start_year, start_month, start_day)
        };

        if arithmetic_year >= 1 && !is_on_or_after(1873, 1, 1) {
            return Some((tinystr!(16, "ce"), arithmetic_year));
        }
        if is_on_or_after(2019, 5, 1) {
            return Some((tinystr!(16, "reiwa"), arithmetic_year - 2019 + 1));
        }
        if is_on_or_after(1989, 1, 8) {
            return Some((tinystr!(16, "heisei"), arithmetic_year - 1989 + 1));
        }
        if is_on_or_after(1926, 12, 25) {
            return Some((tinystr!(16, "showa"), arithmetic_year - 1926 + 1));
        }
        if is_on_or_after(1912, 7, 30) {
            return Some((tinystr!(16, "taisho"), arithmetic_year - 1912 + 1));
        }
        if is_on_or_after(1873, 1, 1) {
            return Some((tinystr!(16, "meiji"), arithmetic_year - 1868 + 1));
        }

        self.era_year_for_arithmetic_year(arithmetic_year)
    }

    pub(crate) fn calendar_has_eras(kind: AnyCalendarKind) -> bool {
        match kind {
            AnyCalendarKind::Buddhist
            | AnyCalendarKind::Coptic
            | AnyCalendarKind::Ethiopian
            | AnyCalendarKind::EthiopianAmeteAlem
            | AnyCalendarKind::Gregorian
            | AnyCalendarKind::Hebrew
            | AnyCalendarKind::Indian
            | AnyCalendarKind::HijriSimulatedMecca
            | AnyCalendarKind::HijriTabularTypeIIFriday
            | AnyCalendarKind::HijriTabularTypeIIThursday
            | AnyCalendarKind::HijriUmmAlQura
            | AnyCalendarKind::Japanese
            | AnyCalendarKind::Persian
            | AnyCalendarKind::Roc => true,
            AnyCalendarKind::Chinese | AnyCalendarKind::Dangi | AnyCalendarKind::Iso => false,
            _ => false,
        }
    }
}

impl From<PlainDate> for Calendar {
    fn from(value: PlainDate) -> Self {
        value.calendar().clone()
    }
}

impl From<PlainDateTime> for Calendar {
    fn from(value: PlainDateTime) -> Self {
        value.calendar().clone()
    }
}

impl From<ZonedDateTime> for Calendar {
    fn from(value: ZonedDateTime) -> Self {
        value.calendar().clone()
    }
}

impl From<PlainMonthDay> for Calendar {
    fn from(value: PlainMonthDay) -> Self {
        value.calendar().clone()
    }
}

impl From<PlainYearMonth> for Calendar {
    fn from(value: PlainYearMonth) -> Self {
        value.calendar().clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::{iso::IsoDate, options::Unit};
    use core::str::FromStr;

    use super::Calendar;

    #[test]
    fn calendar_from_str_is_case_insensitive() {
        let cal_str = "iSo8601";
        let calendar = Calendar::try_from_utf8(cal_str.as_bytes()).unwrap();
        assert_eq!(calendar, Calendar::default());

        let cal_str = "iSO8601";
        let calendar = Calendar::try_from_utf8(cal_str.as_bytes()).unwrap();
        assert_eq!(calendar, Calendar::default());
    }

    #[test]
    fn calendar_invalid_ascii_value() {
        let cal_str = "İSO8601";
        let _err = Calendar::from_str(cal_str).unwrap_err();

        let cal_str = "\u{0130}SO8601";
        let _err = Calendar::from_str(cal_str).unwrap_err();

        // Verify that an empty calendar is an error.
        let cal_str = "2025-02-07T01:24:00-06:00[u-ca=]";
        let _err = Calendar::from_str(cal_str).unwrap_err();
    }

    #[test]
    fn date_until_largest_year() {
        // tests format: (Date one, PlainDate two, Duration result)
        let tests = [
            ((2021, 7, 16), (2021, 7, 16), (0, 0, 0, 0, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2021, 7, 17), (0, 0, 0, 1, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2021, 7, 23), (0, 0, 0, 7, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2021, 8, 16), (0, 1, 0, 0, 0, 0, 0, 0, 0, 0)),
            (
                (2020, 12, 16),
                (2021, 1, 16),
                (0, 1, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            ((2021, 1, 5), (2021, 2, 5), (0, 1, 0, 0, 0, 0, 0, 0, 0, 0)),
            ((2021, 1, 7), (2021, 3, 7), (0, 2, 0, 0, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2021, 8, 17), (0, 1, 0, 1, 0, 0, 0, 0, 0, 0)),
            (
                (2021, 7, 16),
                (2021, 8, 13),
                (0, 0, 0, 28, 0, 0, 0, 0, 0, 0),
            ),
            ((2021, 7, 16), (2021, 9, 16), (0, 2, 0, 0, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2022, 7, 16), (1, 0, 0, 0, 0, 0, 0, 0, 0, 0)),
            (
                (2021, 7, 16),
                (2031, 7, 16),
                (10, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            ((2021, 7, 16), (2022, 7, 19), (1, 0, 0, 3, 0, 0, 0, 0, 0, 0)),
            ((2021, 7, 16), (2022, 9, 19), (1, 2, 0, 3, 0, 0, 0, 0, 0, 0)),
            (
                (2021, 7, 16),
                (2031, 12, 16),
                (10, 5, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1997, 12, 16),
                (2021, 7, 16),
                (23, 7, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1997, 7, 16),
                (2021, 7, 16),
                (24, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1997, 7, 16),
                (2021, 7, 15),
                (23, 11, 0, 29, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1997, 6, 16),
                (2021, 6, 15),
                (23, 11, 0, 30, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1960, 2, 16),
                (2020, 3, 16),
                (60, 1, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1960, 2, 16),
                (2021, 3, 15),
                (61, 0, 0, 27, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1960, 2, 16),
                (2020, 3, 15),
                (60, 0, 0, 28, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 3, 30),
                (2021, 7, 16),
                (0, 3, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2020, 3, 30),
                (2021, 7, 16),
                (1, 3, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1960, 3, 30),
                (2021, 7, 16),
                (61, 3, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2019, 12, 30),
                (2021, 7, 16),
                (1, 6, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2020, 12, 30),
                (2021, 7, 16),
                (0, 6, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1997, 12, 30),
                (2021, 7, 16),
                (23, 6, 0, 16, 0, 0, 0, 0, 0, 0),
            ),
            (
                (1, 12, 25),
                (2021, 7, 16),
                (2019, 6, 0, 21, 0, 0, 0, 0, 0, 0),
            ),
            ((2019, 12, 30), (2021, 3, 5), (1, 2, 0, 5, 0, 0, 0, 0, 0, 0)),
            (
                (2021, 7, 17),
                (2021, 7, 16),
                (0, 0, 0, -1, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 23),
                (2021, 7, 16),
                (0, 0, 0, -7, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 8, 16),
                (2021, 7, 16),
                (0, -1, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 1, 16),
                (2020, 12, 16),
                (0, -1, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            ((2021, 2, 5), (2021, 1, 5), (0, -1, 0, 0, 0, 0, 0, 0, 0, 0)),
            ((2021, 3, 7), (2021, 1, 7), (0, -2, 0, 0, 0, 0, 0, 0, 0, 0)),
            (
                (2021, 8, 17),
                (2021, 7, 16),
                (0, -1, 0, -1, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 8, 13),
                (2021, 7, 16),
                (0, 0, 0, -28, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 9, 16),
                (2021, 7, 16),
                (0, -2, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2022, 7, 16),
                (2021, 7, 16),
                (-1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2031, 7, 16),
                (2021, 7, 16),
                (-10, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2022, 7, 19),
                (2021, 7, 16),
                (-1, 0, 0, -3, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2022, 9, 19),
                (2021, 7, 16),
                (-1, -2, 0, -3, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2031, 12, 16),
                (2021, 7, 16),
                (-10, -5, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (1997, 12, 16),
                (-23, -7, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (1997, 7, 16),
                (-24, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 15),
                (1997, 7, 16),
                (-23, -11, 0, -30, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 6, 15),
                (1997, 6, 16),
                (-23, -11, 0, -29, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2020, 3, 16),
                (1960, 2, 16),
                (-60, -1, 0, 0, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 3, 15),
                (1960, 2, 16),
                (-61, 0, 0, -28, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2020, 3, 15),
                (1960, 2, 16),
                (-60, 0, 0, -28, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (2021, 3, 30),
                (0, -3, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (2020, 3, 30),
                (-1, -3, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (1960, 3, 30),
                (-61, -3, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (2019, 12, 30),
                (-1, -6, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (2020, 12, 30),
                (0, -6, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (1997, 12, 30),
                (-23, -6, 0, -17, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 7, 16),
                (1, 12, 25),
                (-2019, -6, 0, -22, 0, 0, 0, 0, 0, 0),
            ),
            (
                (2021, 3, 5),
                (2019, 12, 30),
                (-1, -2, 0, -6, 0, 0, 0, 0, 0, 0),
            ),
        ];

        let calendar = Calendar::default();

        for test in tests {
            let first = IsoDate::new_unchecked(test.0 .0, test.0 .1, test.0 .2);
            let second = IsoDate::new_unchecked(test.1 .0, test.1 .1, test.1 .2);
            let result = calendar.date_until(&first, &second, Unit::Year).unwrap();
            assert_eq!(
                result.years() as i32,
                test.2 .0,
                "year failed for test \"{test:?}\""
            );
            assert_eq!(
                result.months() as i32,
                test.2 .1,
                "months failed for test \"{test:?}\""
            );
            assert_eq!(
                result.weeks() as i32,
                test.2 .2,
                "weeks failed for test \"{test:?}\""
            );
            assert_eq!(
                result.days(),
                test.2 .3,
                "days failed for test \"{test:?}\""
            );
        }
    }
}
