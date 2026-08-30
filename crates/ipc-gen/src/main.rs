use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(argument) => return Err(format!("unknown argument: {argument}")),
    };
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/generated/ipc.ts");
    let expected = echo_ipc::typescript_contract();
    if check {
        let actual = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if actual != expected {
            return Err(format!(
                "{} is stale; run `cargo run -p echo-ipc-gen`",
                path.display()
            ));
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("ts.tmp");
    std::fs::write(&temporary, expected)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("replace {}: {error}", path.display()))
}
