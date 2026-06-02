use admin_cli::handlers::{
    handle_accept_authority, handle_cancel_round, handle_fund_treasury, handle_info,
    handle_initialize, handle_mint_aat_nft, handle_set_floor_price, handle_set_investor_usdc_unlock,
    handle_set_frozen, handle_set_lock_period, handle_set_round_size, handle_set_sell_paused,
    handle_set_usdc_withdraw_lock,
    handle_transfer_authority, handle_withdraw_treasury,
};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{
    solana_sdk::signature::{read_keypair_file, Keypair},
    Client, Cluster, Program,
};
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use floor_program::id;
use std::fs;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "admin-cli")]
#[command(about = "Admin CLI tool for floor-program", long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "~/.config/solana/id.json")]
    keypair: String,

    #[arg(long, conflicts_with = "keypair")]
    keypair_base58_file: Option<String>,

    #[arg(short, long, default_value = "devnet")]
    cluster: String,

    #[arg(long, short)]
    override_program_id: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Info,
    Initialize {
        #[arg(long)]
        usdc_mint: String,
        #[arg(long)]
        waln_mint: String,
        #[arg(long)]
        floor_price_usdc: u64,
        #[arg(long)]
        round_size_waln: u64,
        #[arg(long, default_value = "0")]
        lock_period_seconds: i64,
    },
    SetFloorPrice {
        new_price_usdc: u64,
    },
    SetRoundSize {
        new_round_size_waln: u64,
    },
    SetLockPeriod {
        new_lock_period: i64,
    },
    SetUsdcWithdrawLock {
        new_lock_seconds: i64,
    },
    SetInvestorUsdcUnlock {
        investor: String,
        new_unlock_ts: i64,
    },
    SetSellPaused {
        #[arg(long, action = clap::ArgAction::Set)]
        paused: bool,
    },
    SetFrozen {
        #[arg(long, action = clap::ArgAction::Set)]
        frozen: bool,
    },
    CancelRound,
    FundTreasury {
        amount_lamports: u64,
    },
    WithdrawTreasury {
        amount_lamports: u64,
    },
    MintAatNft {
        investor: String,
        aat_volume: u64,
    },
    TransferAuthority {
        new_admin: String,
    },
    AcceptAuthority,
}

fn get_program_client(
    keypair_path: &str,
    keypair_base58_file: Option<&str>,
    cluster_str: &str,
    program_id: Pubkey,
) -> Result<(Program<Arc<Keypair>>, Arc<Keypair>)> {
    let keypair = if let Some(base58_path) = keypair_base58_file {
        let content = fs::read_to_string(shellexpand::tilde(base58_path).to_string())
            .map_err(|e| anyhow!("Failed to read keypair file: {}", e))?;
        let bytes = bs58::decode(content.trim())
            .into_vec()
            .map_err(|e| anyhow!("Failed to decode base58 keypair: {}", e))?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| anyhow!("Failed to parse keypair: {}", e))?
    } else {
        read_keypair_file(shellexpand::tilde(keypair_path).to_string())
            .map_err(|e| anyhow!("Failed to read keypair file: {}", e))?
    };
    println!("Using keypair: {}", keypair.pubkey());

    let cluster = match cluster_str {
        "mainnet" => Cluster::Mainnet,
        "devnet" => Cluster::Devnet,
        "localnet" | "localhost" => Cluster::Localnet,
        url => Cluster::Custom(url.to_string(), url.to_string()),
    };

    let keypair = Arc::new(keypair);
    let client = Client::new_with_options(
        cluster.clone(),
        keypair.clone(),
        CommitmentConfig::confirmed(),
    );

    let program = client.program(program_id)?;
    Ok((program, keypair))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let program_id = if let Some(override_id) = cli.override_program_id.as_deref() {
        override_id
            .parse::<Pubkey>()
            .map_err(|e| anyhow!("Invalid override program id: {}", e))?
    } else {
        id()
    };

    let (program, _keypair) = get_program_client(
        &cli.keypair,
        cli.keypair_base58_file.as_deref(),
        &cli.cluster,
        program_id,
    )?;

    match cli.command {
        Commands::Info => handle_info(&program)?,
        Commands::Initialize {
            usdc_mint,
            waln_mint,
            floor_price_usdc,
            round_size_waln,
            lock_period_seconds,
        } => {
            let usdc_mint = usdc_mint
                .parse::<Pubkey>()
                .map_err(|e| anyhow!("Invalid USDC mint: {}", e))?;
            let waln_mint = waln_mint
                .parse::<Pubkey>()
                .map_err(|e| anyhow!("Invalid WALN mint: {}", e))?;
            handle_initialize(
                &program,
                usdc_mint,
                waln_mint,
                floor_price_usdc,
                round_size_waln,
                lock_period_seconds,
            )?
        }
        Commands::SetFloorPrice { new_price_usdc } => {
            handle_set_floor_price(&program, new_price_usdc)?
        }
        Commands::SetRoundSize { new_round_size_waln } => {
            handle_set_round_size(&program, new_round_size_waln)?
        }
        Commands::SetLockPeriod { new_lock_period } => {
            handle_set_lock_period(&program, new_lock_period)?
        }
        Commands::SetUsdcWithdrawLock { new_lock_seconds } => {
            handle_set_usdc_withdraw_lock(&program, new_lock_seconds)?
        }
        Commands::SetInvestorUsdcUnlock {
            investor,
            new_unlock_ts,
        } => {
            let investor = investor
                .parse::<Pubkey>()
                .map_err(|e| anyhow!("Invalid investor pubkey: {}", e))?;
            handle_set_investor_usdc_unlock(&program, investor, new_unlock_ts)?
        }
        Commands::SetSellPaused { paused } => handle_set_sell_paused(&program, paused)?,
        Commands::SetFrozen { frozen } => handle_set_frozen(&program, frozen)?,
        Commands::CancelRound => handle_cancel_round(&program)?,
        Commands::FundTreasury { amount_lamports } => {
            handle_fund_treasury(&program, amount_lamports)?
        }
        Commands::WithdrawTreasury { amount_lamports } => {
            handle_withdraw_treasury(&program, amount_lamports)?
        }
        Commands::MintAatNft {
            investor,
            aat_volume,
        } => {
            let investor = investor
                .parse::<Pubkey>()
                .map_err(|e| anyhow!("Invalid investor pubkey: {}", e))?;
            handle_mint_aat_nft(&program, investor, aat_volume)?
        }
        Commands::TransferAuthority { new_admin } => {
            let new_admin = new_admin
                .parse::<Pubkey>()
                .map_err(|e| anyhow!("Invalid new admin pubkey: {}", e))?;
            handle_transfer_authority(&program, new_admin)?
        }
        Commands::AcceptAuthority => handle_accept_authority(&program)?,
    }

    Ok(())
}
