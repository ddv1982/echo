use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("rec") => rec(args.collect()),
        Some("--help" | "-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::from(2)
        }
        None => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn rec(args: Vec<String>) -> ExitCode {
    if args.iter().any(|a| a == "--once") {
        ExitCode::from(echo::rec::run_rec_once() as u8)
    } else if args.first().map(String::as_str) == Some("down") {
        println!("edge down");
        ExitCode::SUCCESS
    } else if args.first().map(String::as_str) == Some("up") {
        println!("edge up");
        ExitCode::SUCCESS
    } else {
        eprintln!("usage: echo rec --once");
        ExitCode::from(2)
    }
}

fn print_usage() {
    eprintln!("usage: echo rec --once");
}
