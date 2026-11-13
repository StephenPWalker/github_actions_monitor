fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = match args.as_slice() {
        [] => 3000,
        [flag, value] if flag == "--port" => match value.parse::<u16>() {
            Ok(port) if port != 0 => port,
            _ => {
                eprintln!("Port must be a number from 1 to 65535.");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("Usage: github-actions-monitor [--port 3000]");
            std::process::exit(2);
        }
    };

    if let Err(error) = github_actions_monitor::http::run(port) {
        eprintln!("Could not run the monitor: {error}");
        std::process::exit(1);
    }
}
