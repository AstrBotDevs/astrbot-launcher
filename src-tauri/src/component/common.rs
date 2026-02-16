pub fn normalize_default_index(pypi_mirror: &str) -> String {
    if pypi_mirror.trim().is_empty() {
        return "https://pypi.org/simple".to_string();
    }

    let mirror = pypi_mirror.trim().trim_end_matches('/');
    if mirror.ends_with("/simple") {
        mirror.to_string()
    } else {
        format!("{}/simple", mirror)
    }
}
