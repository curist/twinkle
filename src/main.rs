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
    /// Lower a Twinkle source file to ANF IR and display it
    LowerAnf {
        /// Path to the .tw file
        file: String,
    },
    /// Optimise a Twinkle source file and display the resulting ANF IR
    Opt {
        /// Path to the .tw file
        file: String,
        /// Also print the unoptimized ANF before the optimized form
        #[arg(long)]
        show_original: bool,
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
    /// Dump the built-in runtime as WAT
    RuntimeDump,
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
        Commands::LowerAnf { file } => {
            let path = std::path::Path::new(&file);
            twinkle::cli::lower_anf::cmd_lower_anf(path)?;
        }
        Commands::Opt { file, show_original } => {
            let path = std::path::Path::new(&file);
            twinkle::cli::opt::cmd_opt(path, show_original)?;
        }
        Commands::Run { file } => {
            twinkle::cli::run::run_file(&file)?;
        }
        Commands::Build { file, output } => {
            twinkle::cli::build::build_file(&file, output.as_deref())?;
        }
        Commands::RuntimeDump => {
            twinkle::cli::runtime_dump::runtime_dump()?;
        }
    }

    Ok(())
}
