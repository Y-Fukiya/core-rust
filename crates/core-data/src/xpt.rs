use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use indexmap::IndexMap;
use serde_json::Value;

use crate::json_table::records_to_frame;
use crate::{
    canonical_or_original, file_name, file_stem, DataError, DatasetMetadata, DatasetSourceFormat,
    DatasetVariable, LoadedDataset, Result,
};

pub fn load_xpt_dataset(path: impl AsRef<Path>) -> Result<LoadedDataset> {
    let path = path.as_ref();
    let metadata = fs::metadata(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > XPT_MAX_FILE_BYTES as u64 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT file exceeds maximum supported size of {XPT_MAX_FILE_BYTES} bytes"
        )));
    }
    let bytes = fs::read(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed = parse_xpt_v5(&bytes)?;
    let frame = records_to_frame(&parsed.records).map_err(|source| DataError::Polars {
        path: path.to_path_buf(),
        source,
    })?;

    let filename = file_name(path)?;
    let stem = file_stem(path)?.to_ascii_uppercase();
    let name = parsed.dataset_name.unwrap_or_else(|| stem.clone());
    let metadata = DatasetMetadata {
        name: name.clone(),
        domain: Some(name),
        label: parsed.dataset_label,
        filename,
        full_path: canonical_or_original(path),
        source_format: DatasetSourceFormat::Xpt,
        variables: parsed.variables,
    };

    Ok(LoadedDataset::new(metadata, frame))
}

#[derive(Debug, Clone)]
struct ParsedXpt {
    dataset_name: Option<String>,
    dataset_label: Option<String>,
    variables: Vec<DatasetVariable>,
    records: IndexMap<String, Vec<Value>>,
}

#[derive(Debug, Clone)]
struct XptVariable {
    name: String,
    label: Option<String>,
    variable_type: XptVariableType,
    length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum XptVariableType {
    Numeric,
    Character,
}

pub(crate) const XPT_CARD_LEN: usize = 80;
pub(crate) const XPT_NAMESTR_LEN: usize = 140;
pub(crate) const XPT_MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const XPT_MAX_VARIABLES: usize = 10_000;
const XPT_MAX_OBSERVATION_BYTES: usize = 1024 * 1024;
const XPT_MAX_ROWS: usize = 5_000_000;
const XPT_MAX_CELLS: usize = 50_000_000;

fn parse_xpt_v5(bytes: &[u8]) -> Result<ParsedXpt> {
    if bytes.len() > XPT_MAX_FILE_BYTES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT file exceeds maximum supported size of {XPT_MAX_FILE_BYTES} bytes"
        )));
    }
    if bytes.len() < XPT_CARD_LEN {
        return Err(DataError::InvalidDatasetPackage(
            "XPT file is shorter than one 80-byte record".to_owned(),
        ));
    }

    let namestr_header =
        find_xpt_header(bytes, "HEADER RECORD*******NAMESTR").ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT NAMESTR header not found".to_owned())
        })?;
    let variable_count = parse_xpt_header_count(
        &bytes[namestr_header..namestr_header + XPT_CARD_LEN],
    )
    .ok_or_else(|| {
        DataError::InvalidDatasetPackage("XPT NAMESTR header is missing variable count".to_owned())
    })?;
    if variable_count == 0 {
        return Err(DataError::InvalidDatasetPackage(
            "XPT NAMESTR header declares zero variables".to_owned(),
        ));
    }
    if variable_count > XPT_MAX_VARIABLES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT NAMESTR header declares too many variables: {variable_count}"
        )));
    }

    let namestr_start = namestr_header
        .checked_add(XPT_CARD_LEN)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT NAMESTR start overflow".to_owned()))?;
    let namestr_len = variable_count.checked_mul(XPT_NAMESTR_LEN).ok_or_else(|| {
        DataError::InvalidDatasetPackage("XPT NAMESTR length overflow".to_owned())
    })?;
    let namestr_end = namestr_start
        .checked_add(namestr_len)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT NAMESTR end overflow".to_owned()))?;
    if bytes.len() < namestr_end {
        return Err(DataError::InvalidDatasetPackage(
            "XPT file ended before all NAMESTR records were available".to_owned(),
        ));
    }

    let variables = (0..variable_count)
        .map(|index| {
            let offset = namestr_start + index * XPT_NAMESTR_LEN;
            parse_xpt_namestr(&bytes[offset..][..XPT_NAMESTR_LEN])
        })
        .collect::<Result<Vec<_>>>()?;
    let observation_len = variables
        .iter()
        .map(|variable| variable.length)
        .try_fold(0usize, |acc, length| acc.checked_add(length))
        .ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT observation length overflow".to_owned())
        })?;
    if observation_len == 0 {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation length is zero".to_owned(),
        ));
    }
    if observation_len > XPT_MAX_OBSERVATION_BYTES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT observation length exceeds maximum supported size of {XPT_MAX_OBSERVATION_BYTES} bytes"
        )));
    }

    let rounded_namestr_len = round_up_to_card(namestr_len)?;
    let mut data_start = namestr_start
        .checked_add(rounded_namestr_len)
        .ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT observation data start overflow".to_owned())
        })?;
    if bytes
        .get(data_start..data_start.saturating_add(XPT_CARD_LEN))
        .is_some_and(|card| ascii_card(card).starts_with("HEADER RECORD*******OBS"))
    {
        data_start = data_start.checked_add(XPT_CARD_LEN).ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT OBS header end overflow".to_owned())
        })?;
    }
    if data_start > bytes.len() {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation data starts beyond end of file".to_owned(),
        ));
    }

    let observation_data = &bytes[data_start..];
    validate_observation_tail(observation_data, observation_len)?;
    let row_count = observation_row_count(observation_data, observation_len);
    validate_xpt_row_and_cell_limits(row_count, variable_count)?;
    let mut records = variables
        .iter()
        .map(|variable| (variable.name.clone(), Vec::with_capacity(row_count)))
        .collect::<IndexMap<_, _>>();

    for row in observation_chunks(observation_data, observation_len, row_count) {
        let mut offset = 0;
        for variable in &variables {
            let field = &row[offset..offset + variable.length];
            let value = match variable.variable_type {
                XptVariableType::Numeric => decode_xpt_numeric(field),
                XptVariableType::Character => {
                    Value::String(trim_xpt_text(field).unwrap_or_default())
                }
            };
            records
                .get_mut(&variable.name)
                .expect("record column initialized")
                .push(value);
            offset += variable.length;
        }
    }

    Ok(ParsedXpt {
        dataset_name: parse_xpt_dataset_name(bytes),
        dataset_label: None,
        variables: variables
            .into_iter()
            .map(|variable| DatasetVariable {
                name: variable.name,
                label: variable.label,
                variable_type: Some(match variable.variable_type {
                    XptVariableType::Numeric => "Num".to_owned(),
                    XptVariableType::Character => "Char".to_owned(),
                }),
                length: Some(variable.length),
                extra: BTreeMap::new(),
            })
            .collect(),
        records,
    })
}

fn find_xpt_header(bytes: &[u8], header: &str) -> Option<usize> {
    bytes
        .chunks_exact(XPT_CARD_LEN)
        .enumerate()
        .find(|(_index, card)| ascii_card(card).starts_with(header))
        .map(|(index, _card)| index * XPT_CARD_LEN)
}

fn parse_xpt_header_count(card: &[u8]) -> Option<usize> {
    ascii_card(card)
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<usize>().ok())
        .find(|value| *value > 0)
}

fn parse_xpt_namestr(bytes: &[u8]) -> Result<XptVariable> {
    if bytes.len() != XPT_NAMESTR_LEN {
        return Err(DataError::InvalidDatasetPackage(
            "XPT NAMESTR record has invalid length".to_owned(),
        ));
    }

    let ntype = read_xpt_u16(&bytes[0..2]);
    let length = read_xpt_u16(&bytes[4..6]) as usize;
    let name = trim_xpt_text(&bytes[8..16]).unwrap_or_default();
    if name.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "XPT variable has an empty name".to_owned(),
        ));
    }
    if length == 0 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT variable {name} has zero length"
        )));
    }

    let variable_type = match ntype {
        1 => XptVariableType::Numeric,
        2 => XptVariableType::Character,
        other => {
            return Err(DataError::InvalidDatasetPackage(format!(
                "XPT variable {name} has unsupported type {other}"
            )))
        }
    };
    if matches!(variable_type, XptVariableType::Numeric) && length > 8 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT numeric variable {name} has unsupported length {length}"
        )));
    }

    Ok(XptVariable {
        name,
        label: trim_xpt_text(&bytes[16..56]).filter(|label| !label.is_empty()),
        variable_type,
        length,
    })
}

fn parse_xpt_dataset_name(bytes: &[u8]) -> Option<String> {
    bytes.chunks_exact(XPT_CARD_LEN).find_map(|card| {
        let card = ascii_card(card);
        let mut parts = card.split_whitespace();
        if parts.next()? == "SAS" {
            let candidate = parts.next()?.trim();
            if !candidate.eq_ignore_ascii_case("SAS") && !candidate.eq_ignore_ascii_case("SASLIB") {
                return Some(candidate.to_ascii_uppercase());
            }
        }
        None
    })
}

fn observation_row_count(data: &[u8], observation_len: usize) -> usize {
    let mut row_count = data.len() / observation_len;
    while row_count > 0 {
        let start = (row_count - 1) * observation_len;
        let row = &data[start..start + observation_len];
        if !row.iter().all(|byte| matches!(*byte, 0 | b' ')) {
            break;
        }
        row_count -= 1;
    }
    row_count
}

fn validate_observation_tail(data: &[u8], observation_len: usize) -> Result<()> {
    let tail_start = data.len() / observation_len * observation_len;
    let tail = &data[tail_start..];
    if tail.iter().any(|byte| !matches!(*byte, 0 | b' ')) {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation data has a non-padding partial observation tail".to_owned(),
        ));
    }
    Ok(())
}

fn validate_xpt_row_and_cell_limits(row_count: usize, variable_count: usize) -> Result<()> {
    if row_count > XPT_MAX_ROWS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT row count exceeds maximum supported count of {XPT_MAX_ROWS}"
        )));
    }
    let cell_count = row_count
        .checked_mul(variable_count)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT cell count overflow".to_owned()))?;
    if cell_count > XPT_MAX_CELLS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT cell count exceeds maximum supported count of {XPT_MAX_CELLS}"
        )));
    }
    Ok(())
}

fn observation_chunks(
    data: &[u8],
    observation_len: usize,
    row_count: usize,
) -> impl Iterator<Item = &[u8]> {
    data.chunks_exact(observation_len).take(row_count)
}

fn decode_xpt_numeric(bytes: &[u8]) -> Value {
    if bytes.split_first().is_some_and(|(first, rest)| {
        matches!(*first, b'.' | b'_' | b'A'..=b'Z') && rest.iter().all(|byte| *byte == 0)
    }) {
        return Value::Null;
    }
    let value = ibm_float_to_f64(bytes);
    if !value.is_finite() {
        return Value::Null;
    }
    if (value.fract().abs() < f64::EPSILON) && value >= i64::MIN as f64 && value <= i64::MAX as f64
    {
        Value::Number(serde_json::Number::from(value as i64))
    } else {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn ibm_float_to_f64(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let sign = if bytes[0] & 0x80 == 0 { 1.0 } else { -1.0 };
    let exponent = (bytes[0] & 0x7f) as i32 - 64;
    let fraction = bytes
        .iter()
        .skip(1)
        .fold(0_u64, |acc, byte| (acc << 8) | u64::from(*byte));
    if fraction == 0 {
        return 0.0;
    }

    let fraction_bits = 8 * (bytes.len().saturating_sub(1) as i32);
    sign * (fraction as f64 / 2_f64.powi(fraction_bits)) * 16_f64.powi(exponent)
}

fn read_xpt_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn trim_xpt_text(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(*byte, 0 | b' '))
        .map(|index| index + 1)
        .unwrap_or(0);
    let start = bytes[..end]
        .iter()
        .position(|byte| !matches!(*byte, 0 | b' '))
        .unwrap_or(end);
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .map(str::to_owned)
}

fn ascii_card(card: &[u8]) -> String {
    String::from_utf8_lossy(card).into_owned()
}

fn round_up_to_card(value: usize) -> Result<usize> {
    value
        .div_ceil(XPT_CARD_LEN)
        .checked_mul(XPT_CARD_LEN)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT card length overflow".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn expect_invalid_dataset_package(error: DataError, expected: &str) {
        assert!(
            matches!(error, DataError::InvalidDatasetPackage(ref message) if message.contains(expected)),
            "expected InvalidDatasetPackage containing {expected:?}, got {error:?}",
        );
    }

    #[test]
    fn parse_xpt_namestr_rejects_non_namestr_lengths() {
        for length in [0, 1, 79, XPT_NAMESTR_LEN - 1, XPT_NAMESTR_LEN + 1, 280] {
            let error = parse_xpt_namestr(&vec![0; length])
                .expect_err("invalid NAMESTR byte length should fail");
            expect_invalid_dataset_package(error, "invalid length");
        }
    }

    #[test]
    fn observation_row_count_trims_padding_rows_and_ignores_partial_tail() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AE");
        data.extend_from_slice(b"  ");
        data.push(b'X');

        assert_eq!(observation_row_count(&data, 2), 1);
    }

    #[test]
    fn validate_observation_tail_allows_padding_only_partial_tail() {
        validate_observation_tail(b"AE  \0 ", 2).expect("padding tail is valid");
    }

    #[test]
    fn validate_observation_tail_rejects_non_padding_partial_tail() {
        let error =
            validate_observation_tail(b"AE  X", 2).expect_err("non-padding tail should fail");

        expect_invalid_dataset_package(error, "partial observation tail");
    }

    #[test]
    fn decode_xpt_numeric_missing_marker_payloads_are_null() {
        for marker in [b'.', b'_', b'A', b'M', b'Z'] {
            let mut payload = [0_u8; 8];
            payload[0] = marker;
            assert_eq!(decode_xpt_numeric(&payload), Value::Null);
        }
    }

    #[test]
    fn ibm_float_to_f64_decodes_sign_exponent_and_fraction() {
        let positive_one = [0x41, 0x10, 0, 0, 0, 0, 0, 0];
        let negative_one = [0xc1, 0x10, 0, 0, 0, 0, 0, 0];
        let positive_half = [0x40, 0x80, 0, 0, 0, 0, 0, 0];

        assert_eq!(decode_xpt_numeric(&positive_one), Value::from(1));
        assert_eq!(decode_xpt_numeric(&negative_one), Value::from(-1));
        assert_eq!(decode_xpt_numeric(&positive_half), Value::from(0.5));
    }

    #[test]
    fn validate_xpt_row_and_cell_limits_rejects_oversized_boundaries() {
        let row_error = validate_xpt_row_and_cell_limits(XPT_MAX_ROWS + 1, 1)
            .expect_err("row count over cap should fail");
        expect_invalid_dataset_package(row_error, "row count exceeds");

        let rows_under_cap = XPT_MAX_CELLS / 11 + 1;
        assert!(rows_under_cap <= XPT_MAX_ROWS);
        let cell_error = validate_xpt_row_and_cell_limits(rows_under_cap, 11)
            .expect_err("cell count over cap should fail");
        expect_invalid_dataset_package(cell_error, "cell count exceeds");
    }

    #[test]
    fn validate_xpt_row_and_cell_limits_rejects_cell_count_overflow() {
        let error = validate_xpt_row_and_cell_limits(2, usize::MAX)
            .expect_err("cell count overflow should fail");
        expect_invalid_dataset_package(error, "cell count overflow");
    }

    proptest! {
        #[test]
        fn namestr_parser_rejects_arbitrary_non_namestr_lengths(length in 0usize..512) {
            prop_assume!(length != XPT_NAMESTR_LEN);
            let error = parse_xpt_namestr(&vec![0; length])
                .expect_err("non-NAMESTR byte length should fail");
            prop_assert!(matches!(
                error,
                DataError::InvalidDatasetPackage(message) if message.contains("invalid length")
            ));
        }

        #[test]
        fn observation_tail_accepts_only_zero_or_space_padding(
            complete_rows in 0usize..8,
            observation_len in 1usize..32,
            tail in proptest::collection::vec(any::<u8>(), 0..32),
        ) {
            let mut data = vec![b'A'; complete_rows * observation_len];
            data.extend_from_slice(&tail);
            let tail_is_partial = tail.len() % observation_len != 0;
            let tail_is_padding = tail.iter().all(|byte| matches!(*byte, 0 | b' '));
            let result = validate_observation_tail(&data, observation_len);

            if tail_is_partial && !tail_is_padding {
                prop_assert!(result.is_err());
            } else {
                prop_assert!(result.is_ok());
            }
        }
    }
}
