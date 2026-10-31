use crate::error::{AppError, Result};

pub(crate) const INSTANCE_ID_LENGTH: usize = 16;
// Lowercase avoids aliases on case-insensitive filesystems such as Windows.
pub(crate) const INSTANCE_ID_ALPHABET: [char; 36] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];

/// Keep IDs within one child path component; naming rules belong to the OS.
pub(crate) fn validate_instance_id(instance_id: &str) -> Result<()> {
    // Windows can normalize trailing spaces/dots into an empty or parent path.
    // Reject these aliases as well, even when running on another platform.
    let normalized = instance_id.trim_end_matches(' ');
    if normalized.trim_end_matches('.').is_empty() || instance_id.contains(['/', '\\', ':', '\0']) {
        return Err(AppError::other(
            "Instance id must be a single child directory name",
        ));
    }
    Ok(())
}
