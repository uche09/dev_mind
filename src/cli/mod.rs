use std::{fs::canonicalize, path::PathBuf,};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "devmind", about = "Semantic code search for your own codebase")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Explicit config file path, overrides global and project config
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create the local Ahnlich store for this project
    Init,

    /// Walk the codebase, parse it, and push embeddings to Ahnlich
    Index {
        #[arg(
            long, short,
            default_value = ".",
            value_parser = parse_absolute_path
        )]
        path: PathBuf,
    },

    /// Query your codebase
    Ask {
        #[arg(long, short)]
        query: String,

        #[arg(short, long, default_value_t = 5)]
        n: usize,
    },
}

fn parse_absolute_path(input: &str) -> Result<PathBuf, String> {
    canonicalize(input)
        .map_err(|e| format!("Failed to resolve path for '{}': \n{}", input, e))
}