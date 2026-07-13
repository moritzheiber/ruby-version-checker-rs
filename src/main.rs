mod cli;
mod client;
mod docker;
mod release;

#[cfg(test)]
mod test_support;

use clap::Parser;

use crate::cli::{Cli, Command};

#[tokio::main]
async fn main() {
    match Cli::parse().command {
        Command::Check => release::run_check().await,
        Command::Docker(args) => docker::run(args).await,
    }
}
