use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("rec") => rec(args.collect()),
        Some("dict") => dict(args.collect()),
        Some("history") => history(),
        Some("status") => status(),
        Some("--hud-demo") => match echo::ui::hud::run_hud_demo() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("hud-demo: {err}");
                ExitCode::from(1)
            }
        },
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

fn dict(args: Vec<String>) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("add") => {
            let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
            let (spoken, written) = match rest.as_slice() {
                [one] if one.contains('=') => {
                    let (s, w) = one.split_once('=').unwrap();
                    (s.to_string(), w.to_string())
                }
                [one] => (one.to_ascii_lowercase(), (*one).to_string()),
                [spoken, written] => ((*spoken).to_string(), (*written).to_string()),
                _ => {
                    eprintln!("usage: echo dict add \"Claude Code\"");
                    return ExitCode::from(2);
                }
            };
            match echo_core::Dictionary::load().and_then(|mut d| d.add(spoken, written)) {
                Ok(entry) => {
                    println!("{} -> {}", entry.spoken, entry.written);
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("dict: {err}");
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            eprintln!("usage: echo dict add \"Claude Code\"");
            ExitCode::from(2)
        }
    }
}

fn history() -> ExitCode {
    match echo_core::History::load() {
        Ok(store) => {
            echo::ui::history::print_history(&store);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("history: {err}");
            ExitCode::from(1)
        }
    }
}

fn status() -> ExitCode {
    match echo::ui::tray::read_status() {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(_) => {
            println!("state=Idle");
            ExitCode::SUCCESS
        }
    }
}

fn print_usage() {
    eprintln!("usage: echo rec --once");
    eprintln!("       echo dict add \"Claude Code\"");
    eprintln!("       echo history");
    eprintln!("       echo status");
    eprintln!("       echo --hud-demo");
}
