use std::sync::LazyLock;

use chrono::{Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use core_rule_model::Operator;
use regex::Regex;

use super::ScalarValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DateValueState {
    Complete,
    Incomplete,
    Invalid,
}

pub(super) fn compare_dates(operator: &Operator, left: &ScalarValue, right: &ScalarValue) -> bool {
    let left_date = left.as_string().and_then(parse_comparable_date);
    let right_date = right.as_string().and_then(parse_comparable_date);
    let (Some(left), Some(right)) = (left_date, right_date) else {
        // CORE treats a nonblank, non-comparable pair as unequal.
        return matches!(operator, Operator::DateNotEqualTo)
            && !(left.is_empty() && right.is_empty());
    };

    let known_fields = left.precision.min(right.precision) as usize + 1;
    let ordering = left.utc_fields()[..known_fields].cmp(&right.utc_fields()[..known_fields]);
    match operator {
        Operator::DateEqualTo => left.precision == right.precision && ordering.is_eq(),
        Operator::DateNotEqualTo => left.precision != right.precision || !ordering.is_eq(),
        Operator::DateLessThan => ordering.is_lt(),
        Operator::DateLessThanOrEqualTo => ordering.is_le(),
        Operator::DateGreaterThan => ordering.is_gt(),
        Operator::DateGreaterThanOrEqualTo => ordering.is_ge(),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DatePrecision {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Microsecond,
}

struct ComparableDate {
    utc: NaiveDateTime,
    precision: DatePrecision,
}

impl ComparableDate {
    fn utc_fields(&self) -> [i32; 7] {
        [
            self.utc.year(),
            self.utc.month() as i32,
            self.utc.day() as i32,
            self.utc.hour() as i32,
            self.utc.minute() as i32,
            self.utc.second() as i32,
            (self.utc.nanosecond() / 1_000) as i32,
        ]
    }
}

fn parse_comparable_date(value: &str) -> Option<ComparableDate> {
    let (date, precision) = match value.len() {
        4 => (
            NaiveDate::from_ymd_opt(i32::from(parse_fixed_digits(value)?), 1, 1)?,
            DatePrecision::Year,
        ),
        7 if value.as_bytes().get(4) == Some(&b'-') => (
            NaiveDate::from_ymd_opt(
                i32::from(parse_fixed_digits(value.get(..4)?)?),
                u32::from(parse_fixed_digits(value.get(5..)?)?),
                1,
            )?,
            DatePrecision::Month,
        ),
        _ => (parse_calendar_date(value)?, DatePrecision::Day),
    };
    if value.len() <= 10 {
        return Some(ComparableDate {
            utc: date.and_time(NaiveTime::MIN),
            precision,
        });
    }

    let ParsedClock::Known {
        time,
        offset,
        precision,
        legacy_syntax: false,
    } = parse_clock(value.get(11..)?)?
    else {
        return None;
    };
    Some(ComparableDate {
        utc: date.and_time(time).checked_sub_offset(offset)?,
        precision,
    })
}

enum ParsedClock {
    Known {
        time: NaiveTime,
        offset: FixedOffset,
        precision: DatePrecision,
        legacy_syntax: bool,
    },
    Incomplete,
}

fn parse_clock(value: &str) -> Option<ParsedClock> {
    static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"\A([0-9]{1,2}|-)(?::([0-9]{2}|-)",
            r"(?::([0-9]{2}|-)(?:\.([0-9]+))?)?)?",
            r"(Z|[+-][0-9]{2}(?::?[0-9]{2})?)?\z",
        ))
        .expect("valid clock pattern")
    });
    let captures = CLOCK.captures(value)?;
    let hour = captures.get(1)?.as_str();
    let minute = captures.get(2).map(|part| part.as_str());
    let second = captures.get(3).map(|part| part.as_str());
    let fraction = captures.get(4).map(|part| part.as_str());
    let offset_text = captures.get(5).map_or("Z", |part| part.as_str());
    let offset = parse_clock_offset(offset_text)?;

    let component = |part: Option<&str>| match part {
        None | Some("-") => Some(0),
        Some(digits) => Some(u32::from(parse_fixed_digits(digits)?)),
    };
    // CORE's datetime parser truncates fractional seconds to microseconds.
    let micros = fraction.map_or(0, |digits| {
        let mut micros = 0;
        let mut place = 100_000;
        for digit in digits.bytes().take(6) {
            micros += u32::from(digit - b'0') * place;
            place /= 10;
        }
        micros
    });
    let time = NaiveTime::from_hms_micro_opt(
        component(Some(hour))?,
        component(minute)?,
        component(second)?,
        micros,
    )?;
    if [Some(hour), minute, second].contains(&Some("-")) {
        return minute.is_some().then_some(ParsedClock::Incomplete);
    }
    if hour.len() == 1 && minute.is_none() {
        return None;
    }

    let precision = match (minute, second, fraction) {
        (_, _, Some(_)) => DatePrecision::Microsecond,
        (_, Some(_), _) => DatePrecision::Second,
        (Some(_), _, _) => DatePrecision::Minute,
        _ => DatePrecision::Hour,
    };
    Some(ParsedClock::Known {
        time,
        offset,
        precision,
        // Retain the classification contract without accepting this syntax in comparisons.
        legacy_syntax: hour.len() == 1 || matches!(offset_text.len(), 3 | 5),
    })
}

fn parse_clock_offset(value: &str) -> Option<FixedOffset> {
    if value == "Z" {
        return FixedOffset::east_opt(0);
    }
    let hour = i32::from(parse_fixed_digits(value.get(1..3)?)?);
    let minute = match value.get(3..)? {
        "" => 0,
        digits => i32::from(parse_fixed_digits(
            digits.strip_prefix(':').unwrap_or(digits),
        )?),
    };
    if minute >= 60 {
        return None;
    }
    let sign = if value.starts_with('-') { -1 } else { 1 };
    FixedOffset::east_opt(sign * (hour * 60 + minute) * 60)
}

pub(super) fn parse_complete_date(value: &str) -> Option<(u16, u8, u8)> {
    let date = parse_calendar_date(value)?;
    Some((date.year() as u16, date.month() as u8, date.day() as u8))
}

fn parse_calendar_date(value: &str) -> Option<NaiveDate> {
    let date = value.get(..10)?;
    let remainder = value.get(10..).unwrap_or_default();
    if !remainder.is_empty() && !remainder.starts_with('T') {
        return None;
    }

    let bytes = date.as_bytes();
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }

    NaiveDate::from_ymd_opt(
        i32::from(parse_fixed_digits(date.get(..4)?)?),
        u32::from(parse_fixed_digits(date.get(5..7)?)?),
        u32::from(parse_fixed_digits(date.get(8..10)?)?),
    )
}

pub(super) fn classify_date_value(value: &str) -> Option<DateValueState> {
    let value = value.trim();
    if value.is_empty() {
        return Some(DateValueState::Incomplete);
    }
    if parse_calendar_date(value).is_some() {
        return Some(if value.len() == 10 {
            DateValueState::Complete
        } else {
            match value.get(11..).and_then(parse_clock) {
                Some(ParsedClock::Known { precision, .. })
                    if precision >= DatePrecision::Minute =>
                {
                    DateValueState::Complete
                }
                Some(_) => DateValueState::Incomplete,
                None => DateValueState::Invalid,
            }
        });
    }
    if is_incomplete_date(value) {
        return Some(DateValueState::Incomplete);
    }
    Some(DateValueState::Invalid)
}

pub(super) fn is_incomplete_date(value: &str) -> bool {
    if value.len() == 4 {
        return value.chars().all(|character| character.is_ascii_digit());
    }

    if value.len() == 7 {
        let year = value.get(0..4);
        let separator = value.get(4..5);
        let month = value.get(5..7);
        if matches!(separator, Some("-"))
            && year.is_some_and(|year| year.chars().all(|character| character.is_ascii_digit()))
            && month
                .and_then(parse_fixed_digits)
                .is_some_and(|month| (1..=12).contains(&month))
        {
            return true;
        }
    }

    if let Some(month_day) = value.strip_prefix("--") {
        if month_day.len() == 5
            && month_day.as_bytes().get(2) == Some(&b'-')
            && parse_fixed_digits(&month_day[0..2]).is_some_and(|month| (1..=12).contains(&month))
            && parse_fixed_digits(&month_day[3..5]).is_some_and(|day| (1..=31).contains(&day))
        {
            return true;
        }
    }

    if value.len() == 9 && value.as_bytes().get(4..7) == Some(&b"---"[..]) {
        return value[0..4]
            .chars()
            .all(|character| character.is_ascii_digit())
            && parse_fixed_digits(&value[7..9]).is_some_and(|day| (1..=31).contains(&day));
    }

    if value.len() == 10 && value.as_bytes().get(4..8) == Some(&b"----"[..]) {
        return value[0..4]
            .chars()
            .all(|character| character.is_ascii_digit())
            && parse_fixed_digits(&value[8..10]).is_some_and(|day| (1..=31).contains(&day));
    }

    if value
        .strip_prefix("-----T")
        .is_some_and(is_incomplete_iso_time)
    {
        return true;
    }

    false
}

fn is_incomplete_iso_time(value: &str) -> bool {
    value.get(..2).and_then(parse_fixed_digits).is_some() && parse_clock(value).is_some()
}

fn parse_fixed_digits(value: &str) -> Option<u16> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<u16>().ok())
        .flatten()
}

pub(super) fn is_valid_iso_duration(value: &str) -> bool {
    let Some(mut rest) = value.strip_prefix('P') else {
        return false;
    };
    if rest.is_empty() || rest.contains('-') {
        return false;
    }

    if let Some(week) = rest.strip_suffix('W') {
        return is_valid_duration_number(week);
    }

    let mut in_time = false;
    let mut number = String::new();
    let mut saw_component = false;
    let mut last_date_order = 0;
    let mut last_time_order = 0;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if character == 'T' {
            if in_time || !number.is_empty() {
                return false;
            }
            in_time = true;
            continue;
        }
        if character.is_ascii_digit() || character == '.' || character == ',' {
            number.push(character);
            continue;
        }
        if !is_valid_duration_number(&number) {
            return false;
        }

        if in_time {
            let order = match character {
                'H' => 1,
                'M' => 2,
                'S' => 3,
                _ => return false,
            };
            if order <= last_time_order {
                return false;
            }
            last_time_order = order;
        } else {
            let order = match character {
                'Y' => 1,
                'M' => 2,
                'D' => 4,
                _ => return false,
            };
            if order <= last_date_order {
                return false;
            }
            last_date_order = order;
        }

        number.clear();
        saw_component = true;
    }

    saw_component && number.is_empty()
}

fn is_valid_duration_number(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let separator_count = value
        .chars()
        .filter(|character| *character == '.' || *character == ',')
        .count();
    separator_count <= 1
        && !value.starts_with(['.', ','])
        && !value.ends_with(['.', ','])
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.' || character == ',')
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    fn compare(operator: Operator, left: &str, right: &str) -> bool {
        compare_dates(
            &operator,
            &ScalarValue::String(left.to_owned()),
            &ScalarValue::String(right.to_owned()),
        )
    }

    #[test]
    fn date_order_normalizes_both_operands_before_using_common_precision() {
        use Operator::{
            DateGreaterThan, DateGreaterThanOrEqualTo, DateLessThan, DateLessThanOrEqualTo,
        };

        for (left, right, expected) in [
            (
                "2024-01-01T11:00:00+02:00",
                "2024-01-01T10:00:00Z",
                Ordering::Less,
            ),
            (
                "2024-01-01T09:00:00-02:00",
                "2024-01-01T10:00:00Z",
                Ordering::Greater,
            ),
            ("2024-03-31", "2024-03", Ordering::Equal),
            ("2024-04-01", "2024-03", Ordering::Greater),
            ("2024-08-20T07:30:59", "2024-08-20T07:30", Ordering::Equal),
            ("2024-03-01T00:10+01:00", "2024-02-29", Ordering::Equal),
            ("2024-04-01T00:10+01:00", "2024-03", Ordering::Equal),
            ("2023-12-31T23:58-00:30", "2024", Ordering::Equal),
            ("2024-01-01T00:10+01:00", "2024", Ordering::Less),
            ("2024-08-20T07:59:59", "2024-08-20T07", Ordering::Equal),
            (
                "2020-05-20T12:00:00.9",
                "2020-05-20T12:00:00.1",
                Ordering::Greater,
            ),
            (
                "2024-03-01T00:10+01:00",
                "2024-02-29T23:55-00:30",
                Ordering::Less,
            ),
        ] {
            let message = format!("{left} compared with {right}");
            assert_eq!(
                compare(DateLessThan, left, right),
                expected.is_lt(),
                "{message}"
            );
            assert_eq!(
                compare(DateLessThanOrEqualTo, left, right),
                expected.is_le(),
                "{message}"
            );
            assert_eq!(
                compare(DateGreaterThan, left, right),
                expected.is_gt(),
                "{message}"
            );
            assert_eq!(
                compare(DateGreaterThanOrEqualTo, left, right),
                expected.is_ge(),
                "{message}"
            );
            assert_eq!(
                compare(DateLessThan, right, left),
                expected.is_gt(),
                "{message}"
            );
        }
    }

    #[test]
    fn date_equality_requires_the_same_precision_and_normalized_value() {
        for (left, right, equal, unequal) in [
            ("2020-05", "2020-05-01", false, true),
            ("2020", "2020-01", false, true),
            ("2020-05-01T00:00", "2020-05-01T00:00:00", false, true),
            (
                "2020-05-01T02:00:00+02:00",
                "2020-05-01T00:00:00Z",
                true,
                false,
            ),
            ("2020-05-01T00:00:00", "2020-05-01T00:00:00Z", true, false),
            (
                "2020-05-01T00:00:00.000",
                "2020-05-01T00:00:00",
                false,
                true,
            ),
            (
                "2020-05-01T00:00:00.1",
                "2020-05-01T00:00:00.100",
                true,
                false,
            ),
            ("2020-05-01T12:30junk", "2020-05-01T12:30", false, true),
            ("2020-05-01", "2020-05-01T00:00", false, true),
            ("", "", false, false),
            ("2020-05-01", "", false, true),
        ] {
            assert_eq!(
                compare(Operator::DateEqualTo, left, right),
                equal,
                "{left} = {right}"
            );
            assert_eq!(
                compare(Operator::DateNotEqualTo, left, right),
                unequal,
                "{left} != {right}"
            );
        }
    }

    #[test]
    fn malformed_clock_or_offset_is_neither_complete_nor_comparable() {
        for value in [
            "2024-01-06T06:10junk",
            "2024-01-06T06:10:00Zsuffix",
            "2024-01-06T06:10-01:00suffix",
            "2024-01-06T06:10-00:60",
            "2024-01-06T06:10+24:00",
            "2024-01-06T06:10:00.",
            "2024-01-06T-:10junk",
            "-----T06:10junk",
        ] {
            assert_eq!(
                classify_date_value(value),
                Some(DateValueState::Invalid),
                "{value}"
            );
            assert!(
                !compare(Operator::DateLessThanOrEqualTo, value, "2024-01-06T06:10"),
                "{value}"
            );
            assert!(
                !compare(
                    Operator::DateGreaterThanOrEqualTo,
                    value,
                    "2024-01-06T06:10"
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn legacy_classification_and_date_comparison_have_different_grammars() {
        for value in [
            "2024-01-06T6:10",
            "2024-01-06T06:10+00",
            "2024-01-06T06:10+0000",
        ] {
            assert_eq!(classify_date_value(value), Some(DateValueState::Complete));
            assert!(!compare(
                Operator::DateLessThanOrEqualTo,
                value,
                "2024-01-06T06:10"
            ));
            assert!(!compare(
                Operator::DateGreaterThanOrEqualTo,
                "2024-01-06T06:10",
                value
            ));
        }
        for value in [
            "2024-01-06T06",
            "2024-01-06T-:10",
            "2024-01-06T06:-:10",
            "-----T06:10",
        ] {
            assert_eq!(classify_date_value(value), Some(DateValueState::Incomplete));
        }
    }
}
