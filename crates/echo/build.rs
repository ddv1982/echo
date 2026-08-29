fn main() {
    println!("cargo:rerun-if-env-changed=ECHO_BUILD_COMMIT");
}
