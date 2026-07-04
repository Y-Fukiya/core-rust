use std::fs;
use std::fs::File;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn load_xpt_dataset_builds_metadata_and_rows() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("STUDYID", 12, "Study Identifier"),
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric("AESEQ", "Sequence Number"),
        ],
        &[
            vec![
                TestXptValue::Text("CDISC-TEST"),
                TestXptValue::Text("AE"),
                TestXptValue::Number(1.0),
            ],
            vec![
                TestXptValue::Text("CDISC-TEST"),
                TestXptValue::Text("AE"),
                TestXptValue::Number(2.0),
            ],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let summary = dataset.summary();

    assert_eq!(dataset.metadata().name, "AE");
    assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
    assert_eq!(dataset.metadata().source_format, DatasetSourceFormat::Xpt);
    assert_eq!(dataset.metadata().variables.len(), 3);
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.columns, vec!["STUDYID", "DOMAIN", "AESEQ"]);
    assert_eq!(
        dataset
            .frame()
            .column("DOMAIN")
            .expect("domain column")
            .get(0)
            .expect("row 1")
            .extract_str(),
        Some("AE")
    );
    assert_eq!(
        dataset
            .frame()
            .column("AESEQ")
            .expect("seq column")
            .get(1)
            .expect("row 2"),
        AnyValue::Int64(2)
    );
}

#[test]
fn load_xpt_dataset_rejects_oversized_file_before_reading() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("huge.xpt");
    let file = File::create(&path).expect("create xpt");
    file.set_len(XPT_MAX_FILE_BYTES as u64 + 1)
        .expect("set sparse len");
    drop(file);

    let error = load_xpt_dataset(&path).expect_err("oversized xpt rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("exceeds maximum supported size"))
    );
}

#[test]
fn load_xpt_dataset_rejects_short_file() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("short.xpt");
    fs::write(&path, vec![b' '; XPT_CARD_LEN - 1]).expect("write short xpt");

    let error = load_xpt_dataset(&path).expect_err("short xpt rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("shorter than one 80-byte record"))
    );
}

#[test]
fn load_xpt_dataset_rejects_excessive_namestr_count_before_allocation() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("too-many-vars.xpt");
    let mut bytes = Vec::new();
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        &format!(
            "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
            10_001
        ),
    );
    fs::write(&path, bytes).expect("write xpt");

    let error = load_xpt_dataset(&path).expect_err("excessive variable count rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("declares too many variables"))
    );
}

#[test]
fn load_xpt_dataset_rejects_truncated_namestr_records() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("truncated-namestr.xpt");
    let mut bytes = Vec::new();
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!000000000000000000000000000001",
    );
    bytes.extend([0_u8; XPT_NAMESTR_LEN - 1]);
    fs::write(&path, bytes).expect("write xpt");

    let error = load_xpt_dataset(&path).expect_err("truncated namestr rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("ended before all NAMESTR records"))
    );
}

#[test]
fn load_xpt_dataset_treats_invalid_utf8_character_values_as_empty() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("invalid-text.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::character(
            "DOMAIN",
            2,
            "Domain Abbreviation",
        )],
        &[vec![TestXptValue::Text("AE")]],
    );
    let mut bytes = fs::read(&path).expect("read xpt");
    let obs_header = find_test_xpt_card(&bytes, "HEADER RECORD*******OBS").expect("obs header");
    let row_start = obs_header + XPT_CARD_LEN;
    bytes[row_start] = 0xff;
    bytes[row_start + 1] = 0xfe;
    fs::write(&path, bytes).expect("write mutated xpt");

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let domain = dataset.frame().column("DOMAIN").expect("domain column");

    assert_eq!(domain.get(0).expect("row 1").extract_str(), Some(""));
}

#[test]
fn load_xpt_dataset_rejects_zero_length_namestr_variable() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("zero-length-var.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::character(
            "DOMAIN",
            2,
            "Domain Abbreviation",
        )],
        &[vec![TestXptValue::Text("AE")]],
    );
    let mut bytes = fs::read(&path).expect("read xpt");
    let namestr_start = find_test_xpt_namestr_start(&bytes).expect("namestr start");
    bytes[namestr_start + 4..namestr_start + 6].copy_from_slice(&0_u16.to_be_bytes());
    fs::write(&path, bytes).expect("write mutated xpt");

    let error = load_xpt_dataset(&path).expect_err("zero-length variable rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("zero length"))
    );
}

#[test]
fn load_xpt_dataset_rejects_numeric_namestr_length_greater_than_eight() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("wide-numeric.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::numeric("AESEQ", "Sequence Number")],
        &[vec![TestXptValue::Number(1.0)]],
    );
    let mut bytes = fs::read(&path).expect("read xpt");
    let namestr_start = find_test_xpt_namestr_start(&bytes).expect("namestr start");
    bytes[namestr_start + 4..namestr_start + 6].copy_from_slice(&9_u16.to_be_bytes());
    fs::write(&path, bytes).expect("write mutated xpt");

    let error = load_xpt_dataset(&path).expect_err("wide numeric variable rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("unsupported length 9"))
    );
}

#[test]
fn load_xpt_dataset_ignores_padding_only_trailing_observations() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("padded.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::character(
            "DOMAIN",
            2,
            "Domain Abbreviation",
        )],
        &[vec![TestXptValue::Text("AE")]],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");

    assert_eq!(dataset.summary().row_count, 1);
}

#[test]
fn load_xpt_dataset_decodes_ibm_float_fraction_and_sign() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("fractional.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::numeric("AEVAL", "Analysis Value")],
        &[
            vec![TestXptValue::Number(0.5)],
            vec![TestXptValue::Number(-2.25)],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let values = dataset.frame().column("AEVAL").expect("value column");

    assert_float_value(values.get(0).expect("row 1"), 0.5);
    assert_float_value(values.get(1).expect("row 2"), -2.25);
}

#[test]
fn load_xpt_dataset_treats_numeric_missing_payload_as_null() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("missing-numeric.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[TestXptVariable::numeric("AEVAL", "Analysis Value")],
        &[vec![TestXptValue::Number(123.0)]],
    );
    let mut bytes = fs::read(&path).expect("read xpt");
    let obs_header = find_test_xpt_card(&bytes, "HEADER RECORD*******OBS").expect("obs header");
    let row_start = obs_header + XPT_CARD_LEN;
    bytes[row_start] = b'_';
    for byte in &mut bytes[row_start + 1..row_start + 8] {
        *byte = 0;
    }
    fs::write(&path, bytes).expect("write mutated xpt");

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let values = dataset.frame().column("AEVAL").expect("value column");

    assert_eq!(values.get(0).expect("row 1"), AnyValue::Null);
}

#[test]
fn load_xpt_dataset_preserves_zero_numeric_values() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric("AESEQ", "Sequence Number"),
        ],
        &[
            vec![TestXptValue::Text("AE"), TestXptValue::Number(0.0)],
            vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let seq = dataset.frame().column("AESEQ").expect("seq column");

    assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(0));
    assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(1));
}

#[test]
fn load_xpt_dataset_decodes_short_numeric_lengths() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric_with_length("AESEQ", 4, "Sequence Number"),
        ],
        &[
            vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
            vec![TestXptValue::Text("AE"), TestXptValue::Number(2.0)],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let seq = dataset.frame().column("AESEQ").expect("seq column");

    assert_eq!(dataset.metadata().variables[1].length, Some(4));
    assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(1));
    assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(2));
}
