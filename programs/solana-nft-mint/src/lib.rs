use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{mint_to, Mint, MintTo, Token, TokenAccount};
use mpl_token_metadata::instructions::{
    CreateMasterEditionV3CpiBuilder, CreateMetadataAccountV3CpiBuilder,
};
use mpl_token_metadata::types::{Creator, DataV2};

declare_id!("2aZdc7LrZM6vETVjyR8iMr1c4Z9VLt9V2DbfXMhJrAVx");

const MAX_NAME_LEN: usize = 24;
const MAX_SYMBOL_LEN: usize = 10;
const MAX_URI_LEN: usize = 180;

#[program]
pub mod solana_nft_mint {
    use super::*;

    pub fn create_collection(
        ctx: Context<CreateCollection>,
        collection_id: u64,
        name: String,
        symbol: String,
        uri: String,
        price_lamports: u64,
        max_supply: u64,
    ) -> Result<()> {
        require!(name.len() <= MAX_NAME_LEN, NftError::NameTooLong);
        require!(symbol.len() <= MAX_SYMBOL_LEN, NftError::SymbolTooLong);
        require!(uri.len() <= MAX_URI_LEN, NftError::UriTooLong);
        require!(max_supply > 0, NftError::InvalidMaxSupply);

        let collection = &mut ctx.accounts.collection;
        collection.authority = ctx.accounts.authority.key();
        collection.collection_id = collection_id;
        collection.name = name;
        collection.symbol = symbol;
        collection.uri = uri;
        collection.price_lamports = price_lamports;
        collection.max_supply = max_supply;
        collection.current_supply = 0;
        collection.bump = ctx.bumps.collection;

        Ok(())
    }

    pub fn mint_nft(ctx: Context<MintNft>) -> Result<()> {
        require!(
            ctx.accounts.collection.current_supply < ctx.accounts.collection.max_supply,
            NftError::SoldOut
        );

        let index = ctx.accounts.collection.current_supply;
        let nft_name = format!("{} #{}", ctx.accounts.collection.name, index);
        require!(
            nft_name.len() <= mpl_token_metadata::MAX_NAME_LENGTH,
            NftError::NameTooLong
        );

        if ctx.accounts.collection.price_lamports > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.buyer.to_account_info(),
                        to: ctx.accounts.authority.to_account_info(),
                    },
                ),
                ctx.accounts.collection.price_lamports,
            )?;
        }

        let collection_key = ctx.accounts.collection.key();
        let mint_authority_bump = ctx.bumps.mint_authority;
        let mint_authority_seeds: &[&[u8]] = &[
            b"mint_authority",
            collection_key.as_ref(),
            &[mint_authority_bump],
        ];

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.buyer_token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[mint_authority_seeds],
            ),
            1,
        )?;

        let expected_metadata = Pubkey::find_program_address(
            &[
                b"metadata",
                mpl_token_metadata::ID.as_ref(),
                ctx.accounts.mint.key().as_ref(),
            ],
            &mpl_token_metadata::ID,
        )
        .0;
        require_keys_eq!(
            ctx.accounts.metadata.key(),
            expected_metadata,
            NftError::InvalidMetadataAccount
        );

        let expected_master_edition = Pubkey::find_program_address(
            &[
                b"metadata",
                mpl_token_metadata::ID.as_ref(),
                ctx.accounts.mint.key().as_ref(),
                b"edition",
            ],
            &mpl_token_metadata::ID,
        )
        .0;
        require_keys_eq!(
            ctx.accounts.master_edition.key(),
            expected_master_edition,
            NftError::InvalidMetadataAccount
        );

        let data = DataV2 {
            name: nft_name,
            symbol: ctx.accounts.collection.symbol.clone(),
            uri: ctx.accounts.collection.uri.clone(),
            seller_fee_basis_points: 0,
            creators: Some(vec![Creator {
                address: ctx.accounts.collection.authority,
                verified: false,
                share: 100,
            }]),
            collection: None,
            uses: None,
        };

        CreateMetadataAccountV3CpiBuilder::new(&ctx.accounts.token_metadata_program)
            .metadata(&ctx.accounts.metadata)
            .mint(&ctx.accounts.mint.to_account_info())
            .mint_authority(&ctx.accounts.mint_authority)
            .payer(&ctx.accounts.buyer)
            .update_authority(&ctx.accounts.mint_authority, true)
            .system_program(&ctx.accounts.system_program)
            .data(data)
            .is_mutable(true)
            .invoke_signed(&[mint_authority_seeds])?;

        CreateMasterEditionV3CpiBuilder::new(&ctx.accounts.token_metadata_program)
            .edition(&ctx.accounts.master_edition)
            .mint(&ctx.accounts.mint.to_account_info())
            .update_authority(&ctx.accounts.mint_authority)
            .mint_authority(&ctx.accounts.mint_authority)
            .payer(&ctx.accounts.buyer)
            .metadata(&ctx.accounts.metadata)
            .token_program(&ctx.accounts.token_program)
            .system_program(&ctx.accounts.system_program)
            .max_supply(0)
            .invoke_signed(&[mint_authority_seeds])?;

        ctx.accounts.collection.current_supply = index
            .checked_add(1)
            .ok_or(NftError::MathOverflow)?;

        Ok(())
    }
}

#[account]
pub struct Collection {
    pub authority: Pubkey,
    pub collection_id: u64,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub price_lamports: u64,
    pub max_supply: u64,
    pub current_supply: u64,
    pub bump: u8,
}

impl Collection {
    pub const MAX_SIZE: usize = 8 // discriminator
        + 32 // authority
        + 8 // collection_id
        + 4 + MAX_NAME_LEN // name
        + 4 + MAX_SYMBOL_LEN // symbol
        + 4 + MAX_URI_LEN // uri
        + 8 // price_lamports
        + 8 // max_supply
        + 8 // current_supply
        + 1; // bump
}

#[derive(Accounts)]
#[instruction(collection_id: u64)]
pub struct CreateCollection<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = Collection::MAX_SIZE,
        seeds = [b"collection", authority.key().as_ref(), collection_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub collection: Account<'info, Collection>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintNft<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"collection", collection.authority.as_ref(), collection.collection_id.to_le_bytes().as_ref()],
        bump = collection.bump,
    )]
    pub collection: Account<'info, Collection>,

    /// CHECK: receives mint proceeds; validated against collection.authority.
    #[account(mut, address = collection.authority)]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: pure PDA signing authority for this collection's mints; holds no data.
    #[account(seeds = [b"mint_authority", collection.key().as_ref()], bump)]
    pub mint_authority: UncheckedAccount<'info>,

    #[account(
        init,
        payer = buyer,
        mint::decimals = 0,
        mint::authority = mint_authority,
        mint::freeze_authority = mint_authority,
        seeds = [b"nft_mint", collection.key().as_ref(), collection.current_supply.to_le_bytes().as_ref()],
        bump,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = buyer,
        associated_token::mint = mint,
        associated_token::authority = buyer,
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,

    /// CHECK: Metaplex Metadata PDA; address validated manually in the handler.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: Metaplex Master Edition PDA; address validated manually in the handler.
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,

    /// CHECK: the Metaplex Token Metadata program.
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[error_code]
pub enum NftError {
    #[msg("Collection name must be 24 characters or fewer")]
    NameTooLong,
    #[msg("Collection symbol must be 10 characters or fewer")]
    SymbolTooLong,
    #[msg("Collection uri must be 180 characters or fewer")]
    UriTooLong,
    #[msg("max_supply must be greater than zero")]
    InvalidMaxSupply,
    #[msg("This collection is fully minted")]
    SoldOut,
    #[msg("Provided metadata/edition account does not match the expected PDA")]
    InvalidMetadataAccount,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
