pub fn non_empty<T: AsRef<str>>(s: T) -> Option<T> {
    (!s.as_ref().is_empty()).then_some(s)
}

pub fn get_disc_subtitle(disc_raw: String) -> String {
    let mut disc_name = disc_raw.as_str();
    if disc_name.starts_with(|c: char| c.is_ascii_digit()) {
        disc_name = disc_name.trim_start_matches(|c: char| c.is_ascii_digit());
        if let Some(rest) = disc_name.strip_prefix('/') {
            let after_slash_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if after_slash_digits.len() != rest.len() {
                disc_name = after_slash_digits;
            }
        }
    }
    disc_name.trim().to_owned()
}