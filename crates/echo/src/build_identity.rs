pub(crate) const COMMIT: &str = env!("ECHO_BUILD_COMMIT");

#[used]
pub static MARKER: &str = concat!("\0echo-build-commit-v1\0", env!("ECHO_BUILD_COMMIT"), "\0");

pub(crate) fn qualified_commit() -> Option<&'static str> {
    (COMMIT.len() == 40
        && COMMIT
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(COMMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_frames_the_compile_time_commit() {
        assert_eq!(MARKER, format!("\0echo-build-commit-v1\0{COMMIT}\0"));
        assert_eq!(qualified_commit().is_some(), !COMMIT.starts_with("unbound"));
    }
}
