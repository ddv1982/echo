#[used]
pub static MARKER: &str = concat!("\0echo-build-commit-v1\0", env!("ECHO_BUILD_COMMIT"), "\0");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_frames_the_compile_time_commit() {
        let commit = env!("ECHO_BUILD_COMMIT");
        assert_eq!(MARKER, format!("\0echo-build-commit-v1\0{commit}\0"));
    }
}
