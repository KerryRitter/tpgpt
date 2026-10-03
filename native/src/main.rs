#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

mod app;
mod browser;
mod data;
mod importer;
mod insights;
mod mcp;
mod process;
mod store;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("--login") => browser::run(),
        Some("--mcp") => mcp::run(&args[2..]),
        Some("--import") => importer::run_cli(&args[2..]),
        Some("--overview") => data::overview_cli(&args[2..]),
        Some("--help") => {
            println!("TPGPT\n\nRun without arguments for the native desktop app.\n--mcp --database PATH   Serve local training data over MCP stdio\n--import --database PATH --athlete-id ID [--end YYYY-MM-DD] [--years N]\n  Reads TRAININGPEAKS_TOKEN from the environment, never command-line arguments.\n--overview --database PATH   Print database coverage as JSON\n--no-login             Open chat without the initial login window");
            Ok(())
        }
        _ => app::run(!args.iter().any(|arg| arg == "--no-login")),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
