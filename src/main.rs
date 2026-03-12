use clap::Parser;
use cytah_core::cli::cli::{Cli, Commands, CliHandler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║ Cytah-Speed Blockchain Node                       ║");
    println!("║ BlockDAG + GHOSTDAG + State Execution + Finality ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    // Parse CLI
    let cli = Cli::parse();
    let handler = CliHandler::new();

    // Handle commands
    match cli.command {
        Commands::Node { node_command } => {
            match node_command {
                cytah_core::cli::cli::NodeCommands::Start { listen_addr, rpc_addr } => {
                    handler.handle_node_start(&listen_addr, &rpc_addr).await?;
                }
            }
        }
        Commands::Wallet { wallet_command } => {
            match wallet_command {
                cytah_core::cli::cli::WalletCommands::Create { output } => {
                    handler.handle_wallet_create(output.as_deref()).await?;
                }
                cytah_core::cli::cli::WalletCommands::Balance { address, rpc_url } => {
                    handler.handle_wallet_balance(&address, &rpc_url).await?;
                }
            }
        }
        Commands::Tx { tx_command } => {
            match tx_command {
                cytah_core::cli::cli::TxCommands::Send { wallet, to, amount, rpc_url } => {
                    handler.handle_tx_send(&wallet, &to, amount, &rpc_url).await?;
                }
            }
        }
        Commands::Contract { contract_command } => {
            match contract_command {
                cytah_core::cli::cli::ContractCommands::Deploy { wasm, wallet, rpc_url } => {
                    handler.handle_contract_deploy(&wasm, wallet.as_deref(), &rpc_url).await?;
                }
                cytah_core::cli::cli::ContractCommands::Call { contract, method, args, wallet, rpc_url } => {
                    handler.handle_contract_call(&contract, &method, args.as_deref(), wallet.as_deref(), &rpc_url).await?;
                }
            }
        }
    }

    Ok(())
}

