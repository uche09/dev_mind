use std::{fs::canonicalize, path::PathBuf,};

use clap::{Parser, Subcommand, ValueEnum};

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

    /// Initialize default configuration or set config value(s)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    }
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Generate global and/or project config files with defaults
    Init {
        #[arg(long, value_enum, default_value_t = ConfigScope::Both)]
        scope: ConfigScope,

        /// Ahnlich AI proxy address, e.g. localhost:1370
        #[arg(long)]
        ahnlich_addr: Option<String>,

        /// Ahnlich store name (project layer only)
        #[arg(long)]
        store: Option<String>,

        /// Glob pattern to ignore, repeatable: --ignore "**/target" --ignore "tests/**"
        #[arg(long)]
        ignore: Vec<String>,

        /// Overwrite existing config file(s) if present
        #[arg(long)]
        force: bool,

    },
    Get { key: ConfigOptions },
}

#[derive(Clone, ValueEnum)]
pub enum ConfigOptions {
    AhnlichAddr,
    Store,
    Ignore,
}

#[derive(Clone, ValueEnum)]
pub enum ConfigScope {
    Global,
    Project,
    Both,
}

fn parse_absolute_path(input: &str) -> Result<PathBuf, String> {
    canonicalize(input)
        .map_err(|e| format!("Failed to resolve path for '{}': \n{}", input, e))
}