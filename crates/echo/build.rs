fn main() {
    println!("cargo:rerun-if-env-changed=ECHO_BUILD_COMMIT");
    let commit =
        std::env::var("ECHO_BUILD_COMMIT").unwrap_or_else(|_| format!("unbound{}", "_".repeat(33)));
    if commit.len() != 40
        || (commit != format!("unbound{}", "_".repeat(33))
            && !commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    {
        panic!("ECHO_BUILD_COMMIT must be 40 lowercase hexadecimal characters");
    }
    println!("cargo:rustc-env=ECHO_BUILD_COMMIT={commit}");
}
