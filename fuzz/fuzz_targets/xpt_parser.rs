#![no_main]

use core_data::load_xpt_dataset;
use libfuzzer_sys::fuzz_target;

const MAX_FUZZ_INPUT_BYTES: usize = 2 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT_BYTES {
        return;
    }

    let Ok(dir) = tempfile::tempdir() else {
        return;
    };
    let path = dir.path().join("input.xpt");
    if std::fs::write(&path, data).is_ok() {
        let _ = load_xpt_dataset(&path);
    }
});
