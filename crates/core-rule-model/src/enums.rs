use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::name_normalization::normalize_name;

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Exists,
    NotExists,
    EqualTo,
    NotEqualTo,
    EqualToCaseInsensitive,
    NotEqualToCaseInsensitive,
    Contains,
    DoesNotContain,
    ContainsCaseInsensitive,
    DoesNotContainCaseInsensitive,
    IsContainedBy,
    IsNotContainedBy,
    IsContainedByCaseInsensitive,
    IsNotContainedByCaseInsensitive,
    ContainsAll,
    NotContainsAll,
    SharesNoElementsWith,
    IsNotOrderedSubsetOf,
    LessThan,
    LessThanOrEqualTo,
    GreaterThan,
    GreaterThanOrEqualTo,
    MatchesRegex,
    DoesNotMatchRegex,
    DoesNotMatchRegexFullString,
    LongerThan,
    StartsWith,
    PrefixEqualTo,
    PrefixNotEqualTo,
    NotPrefixMatchesRegex,
    PrefixIsNotContainedBy,
    EndsWith,
    SuffixMatchesRegex,
    NotSuffixMatchesRegex,
    SuffixIsNotContainedBy,
    DateEqualTo,
    DateNotEqualTo,
    DateLessThan,
    DateLessThanOrEqualTo,
    DateGreaterThan,
    DateGreaterThanOrEqualTo,
    InvalidDate,
    InvalidDuration,
    IsCompleteDate,
    IsIncompleteDate,
    TargetIsNotSortedBy,
    EmptyWithinExceptLastRow,
    DoesNotHaveNextCorrespondingRecord,
    NotPresentOnMultipleRowsWithin,
    InconsistentEnumeratedColumns,
    IsNotUniqueSet,
    IsUniqueSet,
    IsNotUniqueRelationship,
    IsInconsistentAcrossDataset,
    DoesNotEqualStringPart,
    IsEmpty,
    IsNotEmpty,
    Unsupported(String),
}

impl Operator {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "exists" => Self::Exists,
            "not_exists" => Self::NotExists,
            "equal_to" => Self::EqualTo,
            "not_equal_to" => Self::NotEqualTo,
            "equal_to_case_insensitive" => Self::EqualToCaseInsensitive,
            "not_equal_to_case_insensitive" => Self::NotEqualToCaseInsensitive,
            "contains" => Self::Contains,
            "does_not_contain" => Self::DoesNotContain,
            "contains_case_insensitive" => Self::ContainsCaseInsensitive,
            "does_not_contain_case_insensitive" => Self::DoesNotContainCaseInsensitive,
            "is_contained_by" => Self::IsContainedBy,
            "is_not_contained_by" => Self::IsNotContainedBy,
            "is_contained_by_case_insensitive" => Self::IsContainedByCaseInsensitive,
            "is_not_contained_by_case_insensitive" => Self::IsNotContainedByCaseInsensitive,
            "contains_all" => Self::ContainsAll,
            "not_contains_all" => Self::NotContainsAll,
            "shares_no_elements_with" => Self::SharesNoElementsWith,
            "is_not_ordered_subset_of" => Self::IsNotOrderedSubsetOf,
            "less_than" => Self::LessThan,
            "less_than_or_equal_to" => Self::LessThanOrEqualTo,
            "greater_than" => Self::GreaterThan,
            "greater_than_or_equal_to" => Self::GreaterThanOrEqualTo,
            "matches_regex" => Self::MatchesRegex,
            "does_not_match_regex" => Self::DoesNotMatchRegex,
            "not_matches_regex" => Self::DoesNotMatchRegexFullString,
            "longer_than" => Self::LongerThan,
            "starts_with" => Self::StartsWith,
            "prefix_equal_to" => Self::PrefixEqualTo,
            "prefix_not_equal_to" => Self::PrefixNotEqualTo,
            "not_prefix_matches_regex" => Self::NotPrefixMatchesRegex,
            "prefix_is_not_contained_by" => Self::PrefixIsNotContainedBy,
            "ends_with" => Self::EndsWith,
            "suffix_matches_regex" => Self::SuffixMatchesRegex,
            "not_suffix_matches_regex" => Self::NotSuffixMatchesRegex,
            "suffix_is_not_contained_by" => Self::SuffixIsNotContainedBy,
            "date_equal_to" => Self::DateEqualTo,
            "date_not_equal_to" => Self::DateNotEqualTo,
            "date_less_than" => Self::DateLessThan,
            "date_less_than_or_equal_to" => Self::DateLessThanOrEqualTo,
            "date_greater_than" => Self::DateGreaterThan,
            "date_greater_than_or_equal_to" => Self::DateGreaterThanOrEqualTo,
            "invalid_date" => Self::InvalidDate,
            "invalid_duration" => Self::InvalidDuration,
            "is_complete_date" => Self::IsCompleteDate,
            "is_incomplete_date" => Self::IsIncompleteDate,
            "target_is_not_sorted_by" => Self::TargetIsNotSortedBy,
            "empty_within_except_last_row" => Self::EmptyWithinExceptLastRow,
            "does_not_have_next_corresponding_record" => Self::DoesNotHaveNextCorrespondingRecord,
            "not_present_on_multiple_rows_within" => Self::NotPresentOnMultipleRowsWithin,
            "inconsistent_enumerated_columns" => Self::InconsistentEnumeratedColumns,
            "is_not_unique_set" => Self::IsNotUniqueSet,
            "is_unique_set" => Self::IsUniqueSet,
            "is_not_unique_relationship" => Self::IsNotUniqueRelationship,
            "is_inconsistent_across_dataset" => Self::IsInconsistentAcrossDataset,
            "does_not_equal_string_part" => Self::DoesNotEqualStringPart,
            "is_empty" | "empty" => Self::IsEmpty,
            "is_not_empty" | "non_empty" => Self::IsNotEmpty,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Exists => "exists",
            Self::NotExists => "not_exists",
            Self::EqualTo => "equal_to",
            Self::NotEqualTo => "not_equal_to",
            Self::EqualToCaseInsensitive => "equal_to_case_insensitive",
            Self::NotEqualToCaseInsensitive => "not_equal_to_case_insensitive",
            Self::Contains => "contains",
            Self::DoesNotContain => "does_not_contain",
            Self::ContainsCaseInsensitive => "contains_case_insensitive",
            Self::DoesNotContainCaseInsensitive => "does_not_contain_case_insensitive",
            Self::IsContainedBy => "is_contained_by",
            Self::IsNotContainedBy => "is_not_contained_by",
            Self::IsContainedByCaseInsensitive => "is_contained_by_case_insensitive",
            Self::IsNotContainedByCaseInsensitive => "is_not_contained_by_case_insensitive",
            Self::ContainsAll => "contains_all",
            Self::NotContainsAll => "not_contains_all",
            Self::SharesNoElementsWith => "shares_no_elements_with",
            Self::IsNotOrderedSubsetOf => "is_not_ordered_subset_of",
            Self::LessThan => "less_than",
            Self::LessThanOrEqualTo => "less_than_or_equal_to",
            Self::GreaterThan => "greater_than",
            Self::GreaterThanOrEqualTo => "greater_than_or_equal_to",
            Self::MatchesRegex => "matches_regex",
            Self::DoesNotMatchRegex => "does_not_match_regex",
            Self::DoesNotMatchRegexFullString => "not_matches_regex",
            Self::LongerThan => "longer_than",
            Self::StartsWith => "starts_with",
            Self::PrefixEqualTo => "prefix_equal_to",
            Self::PrefixNotEqualTo => "prefix_not_equal_to",
            Self::NotPrefixMatchesRegex => "not_prefix_matches_regex",
            Self::PrefixIsNotContainedBy => "prefix_is_not_contained_by",
            Self::EndsWith => "ends_with",
            Self::SuffixMatchesRegex => "suffix_matches_regex",
            Self::NotSuffixMatchesRegex => "not_suffix_matches_regex",
            Self::SuffixIsNotContainedBy => "suffix_is_not_contained_by",
            Self::DateEqualTo => "date_equal_to",
            Self::DateNotEqualTo => "date_not_equal_to",
            Self::DateLessThan => "date_less_than",
            Self::DateLessThanOrEqualTo => "date_less_than_or_equal_to",
            Self::DateGreaterThan => "date_greater_than",
            Self::DateGreaterThanOrEqualTo => "date_greater_than_or_equal_to",
            Self::InvalidDate => "invalid_date",
            Self::InvalidDuration => "invalid_duration",
            Self::IsCompleteDate => "is_complete_date",
            Self::IsIncompleteDate => "is_incomplete_date",
            Self::TargetIsNotSortedBy => "target_is_not_sorted_by",
            Self::EmptyWithinExceptLastRow => "empty_within_except_last_row",
            Self::DoesNotHaveNextCorrespondingRecord => "does_not_have_next_corresponding_record",
            Self::NotPresentOnMultipleRowsWithin => "not_present_on_multiple_rows_within",
            Self::InconsistentEnumeratedColumns => "inconsistent_enumerated_columns",
            Self::IsNotUniqueSet => "is_not_unique_set",
            Self::IsUniqueSet => "is_unique_set",
            Self::IsNotUniqueRelationship => "is_not_unique_relationship",
            Self::IsInconsistentAcrossDataset => "is_inconsistent_across_dataset",
            Self::DoesNotEqualStringPart => "does_not_equal_string_part",
            Self::IsEmpty => "is_empty",
            Self::IsNotEmpty => "is_not_empty",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for Operator {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for Operator {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleType {
    RecordData,
    DatasetMetadata,
    VariableMetadata,
    DomainPresence,
    ValueLevelMetadata,
    JsonSchema,
    Jsonata,
    Unsupported(String),
}

impl RuleType {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "record_data" => Self::RecordData,
            "dataset_metadata" | "dataset_metadata_check" => Self::DatasetMetadata,
            "variable_metadata"
            | "variable_metadata_check"
            | "variable_metadata_check_against_define_xml"
            | "define_item_metadata_check_against_library_metadata"
            | "variable_metadata_check_against_library_metadata" => Self::VariableMetadata,
            "domain_presence" | "domain_presence_check" => Self::DomainPresence,
            "value_level_metadata"
            | "value_check_with_variable_metadata"
            | "value_check_with_dataset_metadata" => Self::ValueLevelMetadata,
            "json_schema_check" => Self::JsonSchema,
            "jsonata" => Self::Jsonata,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::RecordData => "record_data",
            Self::DatasetMetadata => "dataset_metadata",
            Self::VariableMetadata => "variable_metadata",
            Self::DomainPresence => "domain_presence",
            Self::ValueLevelMetadata => "value_level_metadata",
            Self::JsonSchema => "json_schema_check",
            Self::Jsonata => "jsonata",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for RuleType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for RuleType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sensitivity {
    Record,
    Dataset,
    Group,
    Study,
    Unsupported(String),
}

impl Sensitivity {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "record" => Self::Record,
            "dataset" => Self::Dataset,
            "group" => Self::Group,
            "study" => Self::Study,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Record => "record",
            Self::Dataset => "dataset",
            Self::Group => "group",
            Self::Study => "study",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for Sensitivity {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for Sensitivity {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleStatus {
    Published,
    Draft,
    Retired,
    Disabled,
    Unsupported(String),
}

impl RuleStatus {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "published" => Self::Published,
            "draft" => Self::Draft,
            "retired" => Self::Retired,
            "disabled" => Self::Disabled,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Retired => "retired",
            Self::Disabled => "disabled",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for RuleStatus {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for RuleStatus {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}
