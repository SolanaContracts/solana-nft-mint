use std::path::PathBuf;
use std::rc::Rc;

use anchor_client::{
    solana_sdk::{
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signer},
    },
    Client, Cluster,
};
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use solana_nft_mint::{accounts, instruction, Collection};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL) as u64
}

#[derive(Parser)]
#[command(name = "nft-mint-cli", about = "CLI client for the solana-nft-mint Anchor program")]
struct Cli {
    /// JSON-RPC URL of the cluster to talk to
    #[arg(long, global = true, default_value = "http://127.0.0.1:8899")]
    url: String,

    /// WebSocket URL of the cluster (used for transaction confirmation)
    #[arg(long, global = true, default_value = "ws://127.0.0.1:8900")]
    ws_url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new NFT collection (a shared metadata template, price, and supply cap)
    CreateCollection {
        /// Keypair file for the collection authority (receives mint proceeds)
        #[arg(long)]
        keypair: PathBuf,
        /// Arbitrary id so one authority can run multiple collections
        #[arg(long)]
        collection_id: u64,
        /// Max 24 characters
        #[arg(long)]
        name: String,
        /// Max 10 characters
        #[arg(long)]
        symbol: String,
        /// Shared off-chain metadata URI, max 180 characters
        #[arg(long)]
        uri: String,
        /// Mint price in SOL
        #[arg(long)]
        price_sol: f64,
        #[arg(long)]
        max_supply: u64,
    },
    /// Mint the next NFT in a collection
    MintNft {
        /// Keypair file for the buyer
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        authority: Pubkey,
        #[arg(long)]
        collection_id: u64,
    },
    /// Print a collection's current state
    Show {
        #[arg(long)]
        authority: Pubkey,
        #[arg(long)]
        collection_id: u64,
    },
}

fn collection_pda(authority: &Pubkey, collection_id: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[b"collection", authority.as_ref(), &collection_id.to_le_bytes()],
        &solana_nft_mint::ID,
    )
    .0
}

fn mint_authority_pda(collection: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"mint_authority", collection.as_ref()], &solana_nft_mint::ID).0
}

fn nft_mint_pda(collection: &Pubkey, index: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[b"nft_mint", collection.as_ref(), &index.to_le_bytes()],
        &solana_nft_mint::ID,
    )
    .0
}

fn metadata_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"metadata", mpl_token_metadata::ID.as_ref(), mint.as_ref()],
        &mpl_token_metadata::ID,
    )
    .0
}

fn master_edition_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"metadata", mpl_token_metadata::ID.as_ref(), mint.as_ref(), b"edition"],
        &mpl_token_metadata::ID,
    )
    .0
}

fn load_keypair(path: &PathBuf) -> Result<Keypair> {
    read_keypair_file(path)
        .map_err(|e| anyhow::anyhow!("failed to read keypair at {}: {e}", path.display()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cluster = Cluster::Custom(cli.url.clone(), cli.ws_url.clone());

    match cli.command {
        Command::CreateCollection {
            keypair,
            collection_id,
            name,
            symbol,
            uri,
            price_sol,
            max_supply,
        } => {
            let authority = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, authority.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_nft_mint::ID)?;
            let collection = collection_pda(&authority.pubkey(), collection_id);

            let sig = program
                .request()
                .accounts(accounts::CreateCollection {
                    authority: authority.pubkey(),
                    collection,
                    system_program: anchor_client::solana_sdk::system_program::ID,
                })
                .args(instruction::CreateCollection {
                    collection_id,
                    name,
                    symbol,
                    uri,
                    price_lamports: sol_to_lamports(price_sol),
                    max_supply,
                })
                .send()
                .context("create_collection transaction failed")?;

            println!("Collection created at {collection}");
            println!("Signature: {sig}");
        }

        Command::MintNft {
            keypair,
            authority,
            collection_id,
        } => {
            let buyer = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, buyer.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_nft_mint::ID)?;
            let collection = collection_pda(&authority, collection_id);
            let collection_state: Collection = program
                .account(collection)
                .context("failed to fetch collection (does it exist?)")?;
            let index = collection_state.current_supply;
            let mint = nft_mint_pda(&collection, index);
            let mint_authority = mint_authority_pda(&collection);
            let buyer_token_account = get_associated_token_address(&buyer.pubkey(), &mint);
            let metadata = metadata_pda(&mint);
            let master_edition = master_edition_pda(&mint);

            let sig = program
                .request()
                .accounts(accounts::MintNft {
                    buyer: buyer.pubkey(),
                    collection,
                    authority,
                    mint_authority,
                    mint,
                    buyer_token_account,
                    metadata,
                    master_edition,
                    token_metadata_program: mpl_token_metadata::ID,
                    token_program: anchor_spl::token::ID,
                    associated_token_program: anchor_spl::associated_token::ID,
                    system_program: anchor_client::solana_sdk::system_program::ID,
                    rent: anchor_client::solana_sdk::sysvar::rent::ID,
                })
                .args(instruction::MintNft {})
                .send()
                .context("mint_nft transaction failed")?;

            println!("Minted \"{} #{}\" at mint {mint}", collection_state.name, index);
            println!("Metadata: {metadata}");
            println!("Master edition: {master_edition}");
            println!("Signature: {sig}");
        }

        Command::Show {
            authority,
            collection_id,
        } => {
            let dummy_payer = Rc::new(Keypair::new());
            let client = Client::new_with_options(cluster, dummy_payer, CommitmentConfig::confirmed());
            let program = client.program(solana_nft_mint::ID)?;
            let collection_key = collection_pda(&authority, collection_id);
            let collection: Collection = program
                .account(collection_key)
                .context("failed to fetch collection (does it exist?)")?;

            println!("Collection: {collection_key}");
            println!("  authority:      {}", collection.authority);
            println!("  name:           {}", collection.name);
            println!("  symbol:         {}", collection.symbol);
            println!("  uri:            {}", collection.uri);
            println!(
                "  price:          {} SOL",
                collection.price_lamports as f64 / LAMPORTS_PER_SOL
            );
            println!(
                "  supply:         {}/{}",
                collection.current_supply, collection.max_supply
            );
        }
    }

    Ok(())
}
