use std::fs;

pub(super) fn write_test_xpt_char_dataset(
    path: &std::path::Path,
    dataset_name: &str,
    columns: &[&str],
    rows: &[Vec<&str>],
) {
    const CARD_LEN: usize = 80;
    const NAMESTR_LEN: usize = 140;

    let mut bytes = Vec::new();
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        "SAS     SAS     SASLIB  9.4     X64_10PRO                       18JUN26:00:00:00",
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******MEMBER  HEADER RECORD!!!!!!!000000000000000001600000000140",
    );
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******DSCRPTR HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        &format!(
            "SAS     {:<8}SASDATA 9.4     X64_10PRO                       18JUN26:00:00:00",
            dataset_name
        ),
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        &format!(
            "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
            columns.len()
        ),
    );

    let lengths = columns
        .iter()
        .map(|column| match *column {
            "DOMAIN" => 2,
            "AESEQ" | "CMSEQ" | "SEQ" => 8,
            _ => 12,
        })
        .collect::<Vec<_>>();
    let mut offset = 0_u32;
    let mut namestrs = Vec::new();
    for (index, (column, length)) in columns.iter().zip(&lengths).enumerate() {
        let mut namestr = vec![0_u8; NAMESTR_LEN];
        namestr[0..2].copy_from_slice(&2_u16.to_be_bytes());
        namestr[4..6].copy_from_slice(&(*length as u16).to_be_bytes());
        namestr[6..8].copy_from_slice(&((index + 1) as u16).to_be_bytes());
        write_padded(&mut namestr[8..16], column);
        write_padded(&mut namestr[16..56], column);
        namestr[84..88].copy_from_slice(&offset.to_be_bytes());
        offset += *length as u32;
        namestrs.extend(namestr);
    }
    pad_to_xpt_card(&mut namestrs);
    bytes.extend(namestrs);

    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******OBS     HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    for row in rows {
        assert_eq!(row.len(), columns.len());
        for (value, length) in row.iter().zip(&lengths) {
            let start = bytes.len();
            bytes.resize(start + *length, b' ');
            write_padded(&mut bytes[start..start + *length], value);
        }
    }
    pad_to_xpt_card(&mut bytes);

    fs::write(path, bytes).expect("write xpt");

    fn push_xpt_card(bytes: &mut Vec<u8>, value: &str) {
        let start = bytes.len();
        bytes.resize(start + CARD_LEN, b' ');
        write_padded(&mut bytes[start..start + CARD_LEN], value);
    }

    fn write_padded(target: &mut [u8], value: &str) {
        let bytes = value.as_bytes();
        let len = bytes.len().min(target.len());
        target[..len].copy_from_slice(&bytes[..len]);
    }

    fn pad_to_xpt_card(bytes: &mut Vec<u8>) {
        let remainder = bytes.len() % CARD_LEN;
        if remainder != 0 {
            bytes.resize(bytes.len() + CARD_LEN - remainder, b' ');
        }
    }
}

pub(super) fn write_raw_rule(
    dir: &std::path::Path,
    id: &str,
    rule_type: &str,
    extra_rule_field: &str,
    operator: &str,
) {
    fs::write(
        dir.join(format!("{id}.json")),
        format!(
            r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{}}, "Classes": {{}} }},
  "Sensitivity": "Record",
  {rule_type},
  {extra_rule_field}
  "Check": {{
    "name": "DOMAIN",
    {operator},
    "value": "AE"
  }},
  "Outcome": {{ "Message": "DOMAIN must be AE" }}
}}"#
        ),
    )
    .expect("write raw rule");
}
