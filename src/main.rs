mod config;
mod error;
mod joplin;
mod mcp;
mod tools;

use clap::Parser;

use crate::config::{Cli, Config};
use crate::joplin::JoplinClient;

fn main() {
    let cli = Cli::parse();
    let config = match Config::from_cli(&cli) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("joplin-mcp: {e}");
            std::process::exit(1);
        }
    };

    let client = JoplinClient::new(config);
    mcp::serve(&client);
}
