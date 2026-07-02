use super::{row_key, take_dataset_rows, DataError, LoadedDataset, Result};

pub fn sort_dataset_by_columns(
    dataset: &LoadedDataset,
    keys: &[String],
    descending: bool,
) -> Result<LoadedDataset> {
    if keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "sort operation requires at least one key".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame().column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "sort key not found: {key}"
            )));
        }
    }

    let mut keyed_rows = (0..dataset.frame().height())
        .map(|row| row_key(dataset.frame(), keys, row).map(|key| (key, row as u32)))
        .collect::<Result<Vec<_>>>()?;
    keyed_rows.sort_by(|left, right| {
        let key_order = if descending {
            right.0.cmp(&left.0)
        } else {
            left.0.cmp(&right.0)
        };
        key_order.then_with(|| left.1.cmp(&right.1))
    });
    let indices = keyed_rows
        .into_iter()
        .map(|(_key, row)| row)
        .collect::<Vec<_>>();
    take_dataset_rows(dataset, &indices)
}
