use crate::error::{AppError, Result};

pub(crate) const INSTANCE_ID_LENGTH: usize = 16;
// Lowercase avoids aliases on case-insensitive filesystems such as Windows.
pub(crate) const INSTANCE_ID_ALPHABET: [char; 36] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];

pub(crate) fn validate_instance_id(instance_id: &str) -> Result<()> {
    let is_nanoid = instance_id.len() == INSTANCE_ID_LENGTH
        && instance_id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !is_nanoid && uuid::Uuid::parse_str(instance_id).is_err() {
        return Err(AppError::other("Invalid instance id"));
    }
    Ok(())
}
