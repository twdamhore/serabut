use clap::{Parser, Subcommand};
use serabut::{arm, disarm, init_db, list, open_db};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "serabut")]
#[command(about = "PXE boot management CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Arm a MAC address for PXE boot
    Arm {
        /// MAC address in hex-hyp format (e.g., aa-bb-cc-dd-ee-ff)
        mac: String,
    },
    /// Disarm a MAC address
    Disarm {
        /// MAC address in hex-hyp format
        mac: String,
        /// Force disarm (idempotent, no error if not exists)
        #[arg(short, long)]
        force: bool,
    },
    /// List all armed MAC addresses
    List,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let conn = match open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to open database: {}", e);
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = init_db(&conn) {
        eprintln!("error: failed to initialize database: {}", e);
        return ExitCode::FAILURE;
    }

    match cli.command {
        Commands::Arm { mac } => {
            match arm(&conn, &mac) {
                Ok(true) => {
                    println!("armed: {}", mac);
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    println!("already armed: {}", mac);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    ExitCode::FAILURE
                }
            }
        }
        Commands::Disarm { mac, force } => {
            match disarm(&conn, &mac, force) {
                Ok(true) => {
                    println!("disarmed: {}", mac);
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    eprintln!("error: {} is not armed", mac);
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    ExitCode::FAILURE
                }
            }
        }
        Commands::List => match list(&conn) {
            Ok(macs) => {
                for mac in macs {
                    println!("{}", mac);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {}", e);
                ExitCode::FAILURE
            }
        },
    }
}
