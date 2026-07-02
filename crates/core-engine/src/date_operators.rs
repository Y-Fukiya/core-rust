use std::cmp::Ordering;

use super::ScalarValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DateValueState {
    Complete,
    Incomplete,
    Invalid,
}

pub(super) fn compare_complete_dates(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    Some(
        parse_orderable_date_for_comparison(left.as_string()?)?
            .cmp(&parse_orderable_date_for_comparison(right.as_string()?)?),
    )
}

fn parse_orderable_date_for_comparison(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    parse_orderable_complete_date(value).or_else(|| parse_orderable_incomplete_date(value))
}

fn parse_orderable_incomplete_date(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    if value.len() == 4 && value.chars().all(|character| character.is_ascii_digit()) {
        return Some((parse_fixed_digits(value)?, 1, 1, 0, 0, 0));
    }

    if value.len() == 7 && value.as_bytes().get(4) == Some(&b'-') {
        let year = parse_fixed_digits(value.get(0..4)?)?;
        let month = parse_fixed_digits(value.get(5..7)?)? as u8;
        if (1..=12).contains(&month) {
            return Some((year, month, 1, 0, 0, 0));
        }
    }

    None
}

fn parse_orderable_complete_date(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    let (year, month, day) = parse_complete_date(value)?;
    let remainder = value.get(10..).unwrap_or_default();
    if remainder.is_empty() {
        return Some((year, month, day, 0, 0, 0));
    }
    let time = remainder
        .strip_prefix('T')
        .or_else(|| remainder.strip_prefix(' '))?;
    let (hour_text, after_hour) = time.split_once(':')?;
    if !(1..=2).contains(&hour_text.len())
        || !hour_text
            .chars()
            .all(|character| character.is_ascii_digit())
        || after_hour.len() < 2
    {
        return None;
    }
    let hour = parse_fixed_digits(hour_text)? as u8;
    let minute = parse_fixed_digits(after_hour.get(0..2)?)? as u8;
    let second = if after_hour.as_bytes().get(2) == Some(&b':') {
        parse_fixed_digits(after_hour.get(3..5)?)? as u8
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((year, month, day, hour, minute, second))
}

pub(super) fn parse_complete_date(value: &str) -> Option<(u16, u8, u8)> {
    let date = value.get(..10)?;
    let remainder = value.get(10..).unwrap_or_default();
    if !remainder.is_empty() && !remainder.starts_with('T') {
        return None;
    }

    let bytes = date.as_bytes();
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }

    let year = parse_fixed_digits(date.get(0..4)?)?;
    let month = parse_fixed_digits(date.get(5..7)?)? as u8;
    let day = parse_fixed_digits(date.get(8..10)?)? as u8;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }

    Some((year, month, day))
}

pub(super) fn classify_date_value(value: &str) -> Option<DateValueState> {
    let value = value.trim();
    if value.is_empty() {
        return Some(DateValueState::Incomplete);
    }
    if parse_orderable_complete_date(value).is_some() {
        return Some(DateValueState::Complete);
    }
    if parse_complete_date(value).is_some()
        && value
            .get(10..)
            .and_then(|remainder| remainder.strip_prefix('T'))
            .is_some_and(is_incomplete_iso_datetime_time)
    {
        return Some(DateValueState::Incomplete);
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
    if value.len() < 2 {
        return false;
    }
    parse_fixed_digits(&value[0..2]).is_some_and(|hour| hour <= 23)
}

fn is_incomplete_iso_datetime_time(value: &str) -> bool {
    if value.len() == 2 {
        return parse_fixed_digits(value).is_some_and(|hour| hour <= 23);
    }

    value.contains('-') && value.contains(':')
}

fn parse_fixed_digits(value: &str) -> Option<u16> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<u16>().ok())
        .flatten()
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
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
