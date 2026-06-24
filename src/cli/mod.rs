use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mind", about = "Semantic code search for your own codebase")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create the local Ahnlich store for this project
    Init,

    /// Walk the codebase, parse it, and push embeddings to Ahnlich
    Index {
        #[arg(default_value = ".")]
        path: String,
    },

    /// Query your codebase
    Ask {
        query: String,

        #[arg(short, long, default_value_t = 5)]
        n: usize,
    },
}
