use clap::Parser;
use gitexplore::{cli::Cli, run_cli};

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    if let Err(error) = run_cli(cli).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
