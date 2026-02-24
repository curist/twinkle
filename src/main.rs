use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "twk")]
#[command(about = "Twinkle compiler - A statically typed language targeting WebAssembly GC", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a Twinkle source file and display the AST
    Parse {
        /// Path to the .tw file
        file: String,
    },
    /// Type-check a Twinkle source file
    Check {
        /// Path to the .tw file
        file: String,
    },
    /// Lower a Twinkle source file to Core IR and display it
    Lower {
        /// Path to the .tw file
        file: String,
    },
    /// Run a Twinkle program using the interpreter
    Run {
        /// Path to the .tw file
        file: String,
    },
    /// Compile a Twinkle program to WAT/Wasm
    Build {
        /// Path to the .tw file
        file: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse { file } => {
            twinkle::cli::parse::parse_file(&file)?;
        }
        Commands::Check { file } => {
            twinkle::cli::check::check_file(&file)?;
        }
        Commands::Lower { file } => {
            twinkle::cli::lower::lower_file(&file)?;
        }
        Commands::Run { file } => {
            twinkle::cli::run::run_file(&file)?;
        }
        Commands::Build { file, output } => {
            twinkle::cli::build::build_file(&file, output.as_deref())?;
        }
    }

    Ok(())
}
