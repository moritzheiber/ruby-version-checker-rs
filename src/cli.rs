use clap::{Parser, Subcommand};

use crate::docker::DockerArgs;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Report the latest regular Ruby releases from cache.ruby-lang.org.
    Check,
    /// List the regular Ruby releases available in a container repository.
    Docker(DockerArgs),
}
