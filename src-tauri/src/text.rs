use crate::error::{AppError, AppResult, ErrorCode};
use unicode_normalization::UnicodeNormalization;

pub const NAME_SCALAR_LIMIT: usize = 120;
pub const NOTES_SCALAR_LIMIT: usize = 4_000;

pub fn normalize_required_text(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > NAME_SCALAR_LIMIT
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(AppError::new(
            ErrorCode::InvalidText,
            "Enter 1–120 characters without line breaks or controls.",
        ));
    }
    Ok(value.to_owned())
}

pub fn normalize_notes(value: &str) -> AppResult<String> {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    let value = value.trim();
    if value.chars().count() > NOTES_SCALAR_LIMIT
        || value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(AppError::new(
            ErrorCode::InvalidText,
            "Notes may contain up to 4,000 characters, with tabs and line breaks.",
        ));
    }
    Ok(value.to_owned())
}

pub fn stable_text_order_key(value: &str) -> String {
    value
        .nfc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .nfc()
        .collect()
}

pub fn compare_text_with_ids(
    left: &str,
    left_id: &str,
    right: &str,
    right_id: &str,
) -> std::cmp::Ordering {
    stable_text_order_key(left)
        .cmp(&stable_text_order_key(right))
        .then_with(|| left_id.cmp(right_id))
}
