use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::named_usdm_object_name;
use crate::usdm_values::{
    format_code, format_quantity_single, format_quantity_single_with_missing_unit,
    format_semicolon_list, format_string_list, json_string, string_array, value_exists,
    value_string,
};

pub(crate) fn collect_usdm_administrable_product_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let administered_product_ids = version
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|intervention| {
                intervention
                    .get("administrations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|administration| {
                administration
                    .get("administrableProductId")
                    .and_then(value_string)
            })
            .collect::<HashSet<_>>();
        let medical_devices = version
            .get("medicalDevices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (product_index, product) in version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            rows.push(usdm_administrable_product_row(
                product,
                &administered_product_ids,
                &medical_devices,
                &format!("/study/versions/{version_index}/administrableProducts/{product_index}"),
            ));
        }
    }
}

pub(crate) fn collect_usdm_administration_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let administrable_products = version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let medical_devices = version
            .get("medicalDevices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (intervention_index, intervention) in version
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (administration_index, administration) in intervention
                .get("administrations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                rows.push(usdm_administration_row(
                    administration,
                    intervention,
                    &administrable_products,
                    &medical_devices,
                    &format!(
                        "/study/versions/{version_index}/studyInterventions/{intervention_index}/administrations/{administration_index}"
                    ),
                ));
            }
        }
    }
}

pub(crate) fn collect_usdm_strength_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (product_index, product) in version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (ingredient_index, ingredient) in product
                .get("ingredients")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(substance) = ingredient.get("substance") {
                    collect_usdm_strength_rows_at(
                        substance,
                        product,
                        ingredient,
                        substance,
                        &format!(
                            "/study/versions/{version_index}/administrableProducts/{product_index}/ingredients/{ingredient_index}/substance"
                        ),
                        rows,
                    );
                }
            }
        }
    }
}

pub(crate) fn collect_usdm_amendment_reason_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (amendment_index, amendment) in version
            .get("amendments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let primary_code = amendment
                .get("primaryReason")
                .and_then(|reason| reason.get("code"))
                .and_then(|code| code.get("code"))
                .and_then(value_string);
            if let Some(reason) = amendment.get("primaryReason") {
                rows.push(usdm_amendment_reason_row(
                    reason,
                    &format!(
                        "/study/versions/{version_index}/amendments/{amendment_index}/primaryReason"
                    ),
                    amendment,
                    None,
                ));
            }
            for (reason_index, reason) in amendment
                .get("secondaryReasons")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                rows.push(usdm_amendment_reason_row(
                    reason,
                    &format!(
                        "/study/versions/{version_index}/amendments/{amendment_index}/secondaryReasons/{reason_index}"
                    ),
                    amendment,
                    primary_code.as_deref(),
                ));
            }
        }
    }
}

pub(crate) fn collect_usdm_product_organization_role_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let valid_ids = version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                version
                    .get("medicalDevices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .filter_map(|value| value.get("id").and_then(value_string))
            .collect::<HashSet<_>>();
        for (role_index, role) in version
            .get("productOrganizationRoles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            rows.push(usdm_product_organization_role_row(
                role,
                &format!("/study/versions/{version_index}/productOrganizationRoles/{role_index}"),
                &valid_ids,
            ));
        }
    }
}

fn collect_usdm_strength_rows_at(
    value: &Value,
    product: &Value,
    ingredient: &Value,
    substance: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if let Some(strengths) = object.get("strengths").and_then(Value::as_array) {
                for (strength_index, strength) in strengths.iter().enumerate() {
                    rows.push(usdm_strength_row(
                        strength,
                        product,
                        ingredient,
                        substance,
                        &format!("{path}/strengths/{strength_index}"),
                    ));
                }
            }
            for (key, child) in object {
                if key != "strengths" {
                    collect_usdm_strength_rows_at(
                        child,
                        product,
                        ingredient,
                        substance,
                        &format!("{path}/{key}"),
                        rows,
                    );
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_strength_rows_at(
                    child,
                    product,
                    ingredient,
                    substance,
                    &format!("{path}/{index}"),
                    rows,
                );
            }
        }
        _ => {}
    }
}

fn usdm_administrable_product_row(
    product: &Value,
    administered_product_ids: &HashSet<String>,
    medical_devices: &[Value],
    path: &str,
) -> BTreeMap<String, Value> {
    let product_id = product.get("id").and_then(value_string).unwrap_or_default();
    let embedded_devices = medical_devices
        .iter()
        .filter(|device| {
            device
                .get("embeddedProductId")
                .and_then(value_string)
                .as_deref()
                == Some(&product_id)
        })
        .collect::<Vec<_>>();
    let has_sourcing = product
        .get("sourcing")
        .is_some_and(|sourcing| !sourcing.is_null());
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(product.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(product.get("id")));
    row.insert("name".to_owned(), json_string(product.get("name")));
    row.insert(
        "sourcing".to_owned(),
        product
            .get("sourcing")
            .map(|sourcing| Value::String(format_code(Some(sourcing))))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "MedicalDevice.id".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("id").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "MedicalDevice.name".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("name").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "MedicalDevice.embeddedProductId".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("embeddedProductId").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "administrable_product_embedded_only_sourcing".to_owned(),
        Value::Bool(
            has_sourcing
                && !embedded_devices.is_empty()
                && !administered_product_ids.contains(&product_id),
        ),
    );
    row
}

fn usdm_administration_row(
    administration: &Value,
    intervention: &Value,
    administrable_products: &[Value],
    medical_devices: &[Value],
    path: &str,
) -> BTreeMap<String, Value> {
    let dose = administration.get("dose");
    let route = administration.get("route");
    let frequency = administration.get("frequency");
    let dose_id_exists = value_exists(dose.and_then(|dose| dose.get("id")));
    let route_id_exists = value_exists(route.and_then(|route| route.get("id")));
    let frequency_id_exists = value_exists(frequency.and_then(|frequency| frequency.get("id")));
    let administrable_product_id = administration
        .get("administrableProductId")
        .and_then(value_string);
    let medical_device_id = administration.get("medicalDeviceId").and_then(value_string);
    let medical_device = medical_device_id.as_deref().and_then(|id| {
        medical_devices
            .iter()
            .find(|device| device.get("id").and_then(value_string).as_deref() == Some(id))
    });
    let embedded_product_id = medical_device
        .and_then(|device| device.get("embeddedProductId"))
        .and_then(value_string);
    let has_product = administrable_product_id
        .as_deref()
        .is_some_and(|id| !id.is_empty())
        || embedded_product_id
            .as_deref()
            .is_some_and(|id| !id.is_empty());
    let product_name = administrable_product_id
        .as_deref()
        .or(embedded_product_id.as_deref())
        .and_then(|id| named_usdm_object_name(administrable_products, id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "StudyIntervention.id".to_owned(),
        json_string(intervention.get("id")),
    );
    row.insert(
        "StudyIntervention.name".to_owned(),
        json_string(intervention.get("name")),
    );
    row.insert("name".to_owned(), json_string(administration.get("name")));
    row.insert(
        "dose.id".to_owned(),
        json_string(dose.and_then(|dose| dose.get("id"))),
    );
    row.insert(
        "dose(value)".to_owned(),
        dose.map(usdm_format_quantity_or_missing)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "dose(value/range)".to_owned(),
        dose.map(usdm_format_quantity_or_missing)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "route.id".to_owned(),
        json_string(route.and_then(|route| route.get("id"))),
    );
    row.insert(
        "route".to_owned(),
        route
            .and_then(|route| route.get("standardCode"))
            .map(|code| Value::String(format_code(Some(code))))
            .unwrap_or_else(|| Value::String("()".to_owned())),
    );
    row.insert(
        "frequency.id".to_owned(),
        json_string(frequency.and_then(|frequency| frequency.get("id"))),
    );
    row.insert(
        "frequency".to_owned(),
        frequency
            .and_then(|frequency| frequency.get("standardCode"))
            .map(|code| Value::String(format_code(Some(code))))
            .unwrap_or_else(|| Value::String("()".to_owned())),
    );
    row.insert(
        "administrableProductId".to_owned(),
        json_string(administration.get("administrableProductId")),
    );
    row.insert(
        "medicalDeviceId".to_owned(),
        json_string(administration.get("medicalDeviceId")),
    );
    row.insert(
        "MedicalDevice.name".to_owned(),
        medical_device
            .and_then(|device| device.get("name"))
            .and_then(value_string)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "MedicalDevice.embeddedProductId".to_owned(),
        embedded_product_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "AdministrableProduct.name".to_owned(),
        product_name.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "administration_dose_route_xor".to_owned(),
        Value::Bool(dose_id_exists != route_id_exists),
    );
    row.insert(
        "administration_dose_without_frequency".to_owned(),
        Value::Bool(dose_id_exists && !frequency_id_exists),
    );
    row.insert(
        "administration_dose_product_xor".to_owned(),
        Value::Bool(dose_id_exists != has_product),
    );
    row.insert(
        "administration_duplicate_embedded_product".to_owned(),
        Value::Bool(
            medical_device_id
                .as_deref()
                .is_some_and(|id| !id.is_empty())
                && administrable_product_id.is_some()
                && administrable_product_id == embedded_product_id,
        ),
    );
    row
}

fn usdm_strength_row(
    strength: &Value,
    product: &Value,
    ingredient: &Value,
    substance: &Value,
    path: &str,
) -> BTreeMap<String, Value> {
    let numerator = strength.get("numerator");
    let numerator_value = numerator.and_then(|value| value.get("value"));
    let numerator_min = numerator.and_then(|value| value.get("minValue"));
    let numerator_max = numerator.and_then(|value| value.get("maxValue"));
    let denominator = strength.get("denominator");
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "AdministrableProduct.id".to_owned(),
        json_string(product.get("id")),
    );
    row.insert(
        "AdministrableProduct.name".to_owned(),
        json_string(product.get("name")),
    );
    row.insert(
        "Ingredient.id".to_owned(),
        json_string(ingredient.get("id")),
    );
    row.insert("Substance.id".to_owned(), json_string(substance.get("id")));
    row.insert(
        "Substance.name".to_owned(),
        json_string(substance.get("name")),
    );
    row.insert("name".to_owned(), json_string(strength.get("name")));
    row.insert(
        "numerator.value".to_owned(),
        numerator_value.cloned().unwrap_or(Value::Null),
    );
    row.insert(
        "numerator.minValue".to_owned(),
        numerator_min
            .map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "numerator.maxValue".to_owned(),
        numerator_max
            .map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "denominator.id".to_owned(),
        json_string(denominator.and_then(|denominator| denominator.get("id"))),
    );
    row.insert(
        "denominator.value".to_owned(),
        denominator
            .and_then(|denominator| denominator.get("value"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    row.insert(
        "strength_numerator_value_missing_unit".to_owned(),
        Value::Bool(
            value_exists(numerator_value) && !value_exists(numerator.and_then(|n| n.get("unit"))),
        ),
    );
    row.insert(
        "strength_numerator_range_missing_unit".to_owned(),
        Value::Bool(
            (value_exists(numerator_min.and_then(|v| v.get("value")))
                && !value_exists(numerator_min.and_then(|v| v.get("unit"))))
                || (value_exists(numerator_max.and_then(|v| v.get("value")))
                    && !value_exists(numerator_max.and_then(|v| v.get("unit")))),
        ),
    );
    row.insert(
        "strength_denominator_missing_unit".to_owned(),
        Value::Bool(
            value_exists(denominator.and_then(|denominator| denominator.get("id")))
                && !value_exists(denominator.and_then(|denominator| denominator.get("unit"))),
        ),
    );
    row
}

fn usdm_amendment_reason_row(
    reason: &Value,
    path: &str,
    amendment: &Value,
    primary_code: Option<&str>,
) -> BTreeMap<String, Value> {
    let reason_code = reason
        .get("code")
        .and_then(|code| code.get("code"))
        .and_then(value_string);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "StudyAmendment.id".to_owned(),
        json_string(amendment.get("id")),
    );
    row.insert(
        "StudyAmendment.name".to_owned(),
        json_string(amendment.get("name")),
    );
    row.insert(
        "code".to_owned(),
        Value::String(format_code(reason.get("code"))),
    );
    row.insert(
        "primaryReason.code".to_owned(),
        primary_code
            .map(|code| Value::String(code.to_owned()))
            .unwrap_or_else(|| Value::String(format_code(reason.get("code")))),
    );
    row.insert(
        "primary_reason_not_applicable".to_owned(),
        Value::Bool(reason_code.as_deref() == Some("C48660")),
    );
    row.insert(
        "secondary_reason_matches_primary".to_owned(),
        Value::Bool(
            primary_code.is_some()
                && reason_code
                    .as_deref()
                    .is_some_and(|code| Some(code) == primary_code),
        ),
    );
    row
}

fn usdm_product_organization_role_row(
    role: &Value,
    path: &str,
    valid_ids: &HashSet<String>,
) -> BTreeMap<String, Value> {
    let applies_to_ids = string_array(role.get("appliesToIds"));
    let valid_reference =
        !applies_to_ids.is_empty() && applies_to_ids.iter().any(|id| valid_ids.contains(id));
    let invalid_reference = applies_to_ids.iter().any(|id| !valid_ids.contains(id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(role.get("name")));
    row.insert(
        "appliesToIds".to_owned(),
        if applies_to_ids.is_empty() {
            Value::Null
        } else {
            Value::String(format_string_list(&applies_to_ids))
        },
    );
    row.insert(
        "product_role_missing_valid_target".to_owned(),
        Value::Bool(!valid_reference),
    );
    row.insert(
        "product_role_invalid_target".to_owned(),
        Value::Bool(invalid_reference),
    );
    row
}

fn usdm_format_quantity_or_missing(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            if object.contains_key("value") {
                return format_quantity_single(value);
            }
            if object.contains_key("minValue") || object.contains_key("maxValue") {
                let min = object
                    .get("minValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                let max = object
                    .get("maxValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                return format!("{min} to {max}");
            }
            "Missing".to_owned()
        }
        Value::Null => "Missing".to_owned(),
        _ => value_string(value).unwrap_or_else(|| value.to_string()),
    }
}
