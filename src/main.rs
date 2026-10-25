//! dumper: extracts metadata from a container during a build, for the hub and the website.
//!
//! The binary is static, so the workflows can copy it into any image and run it as the entrypoint before init.
//! Each subcommand writes one kind of data.

mod files;

const USAGE: &str = "usage: dumper <command> [options]

Commands:
  files      the file tree of the root filesystem (gzipped JSON, format lsio-files/1)
  version    print the version

Run `dumper <command> --help` for a command's options.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("files") => files::run(&args[1..]),
        Some("version" | "--version" | "-V") => {
            println!("dumper {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
