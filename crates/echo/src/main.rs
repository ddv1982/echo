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
    match args.as_slice() {
        [arg] if arg == "--once" => ExitCode::from(echo::rec::run_rec_once() as u8),
        [arg] if arg == "--toggle" => ExitCode::from(echo::rec::run_rec_toggle() as u8),
        [arg] if arg == "--hold" => ExitCode::from(echo::rec::run_rec_hold() as u8),
        _ => {
            eprintln!("usage: echo-app rec --once|--toggle|--hold");
            ExitCode::from(2)
        }
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
                    eprintln!("usage: echo-app dict add \"Claude Code\"");
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
            eprintln!("usage: echo-app dict add \"Claude Code\"");
            ExitCode::from(2)
        }
    }
}

fn history() -> ExitCode {
    match echo_core::History::load() {
        Ok(store) => {
            print_history(&store);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("history: {err}");
            ExitCode::from(1)
        }
    }
}

fn print_history(store: &echo_core::History) {
    if store.rows().is_empty() {
        println!("(empty)");
        return;
    }
    for row in store.rows() {
        println!("{}  {}  {}  {}", row.id, row.engine, row.infer_ms, row.text);
    }
}

fn status() -> ExitCode {
    let status = echo::status::read();
    println!("state={}", status.state);
    if let Some(last) = status.last {
        println!("last={last}");
    }
    ExitCode::SUCCESS
}

fn print_usage() {
    eprintln!("usage: echo-app rec --once");
    eprintln!("       echo-app rec --toggle");
    eprintln!("       echo-app rec --hold");
    eprintln!("       echo-app dict add \"Claude Code\"");
    eprintln!("       echo-app history");
    eprintln!("       echo-app status");
    eprintln!("       echo-app --hud-demo");
}
