import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getMint,
  getAccount,
} from "@solana/spl-token";
import { assert } from "chai";
import { SolanaNftMint } from "../target/types/solana_nft_mint";

const METADATA_PROGRAM_ID = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

describe("solana-nft-mint", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const program = anchor.workspace.solanaNftMint as Program<SolanaNftMint>;

  const authority = Keypair.generate();
  const buyer = Keypair.generate();

  const COLLECTION_ID = new BN(1);
  const PRICE_LAMPORTS = 0.1 * LAMPORTS_PER_SOL;
  const MAX_SUPPLY = 2;

  let collectionPda: PublicKey;

  const findCollectionPda = (id: BN) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("collection"), authority.publicKey.toBuffer(), id.toArrayLike(Buffer, "le", 8)],
      program.programId
    )[0];

  const findMintAuthorityPda = (collection: PublicKey) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("mint_authority"), collection.toBuffer()],
      program.programId
    )[0];

  const findMintPda = (collection: PublicKey, index: BN) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("nft_mint"), collection.toBuffer(), index.toArrayLike(Buffer, "le", 8)],
      program.programId
    )[0];

  const findMetadataPda = (mint: PublicKey) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("metadata"), METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer()],
      METADATA_PROGRAM_ID
    )[0];

  const findMasterEditionPda = (mint: PublicKey) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("metadata"), METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer(), Buffer.from("edition")],
      METADATA_PROGRAM_ID
    )[0];

  async function mintAccounts(index: BN) {
    const mint = findMintPda(collectionPda, index);
    return {
      buyer: buyer.publicKey,
      collection: collectionPda,
      authority: authority.publicKey,
      mintAuthority: findMintAuthorityPda(collectionPda),
      mint,
      buyerTokenAccount: getAssociatedTokenAddressSync(mint, buyer.publicKey),
      metadata: findMetadataPda(mint),
      masterEdition: findMasterEditionPda(mint),
      tokenMetadataProgram: METADATA_PROGRAM_ID,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
      rent: anchor.web3.SYSVAR_RENT_PUBKEY,
    };
  }

  before(async () => {
    for (const kp of [authority, buyer]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 2 * LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig, "confirmed");
    }
    collectionPda = findCollectionPda(COLLECTION_ID);
  });

  it("rejects creating a collection with max_supply = 0", async () => {
    const badId = new BN(999);
    const badCollection = findCollectionPda(badId);
    try {
      await program.methods
        .createCollection(badId, "Bad", "BAD", "https://example.com/bad.json", new BN(0), new BN(0))
        .accounts({
          authority: authority.publicKey,
          collection: badCollection,
          systemProgram: SystemProgram.programId,
        })
        .signers([authority])
        .rpc();
      assert.fail("expected create_collection to fail with max_supply = 0");
    } catch (err) {
      assert.include(String(err), "InvalidMaxSupply");
    }
  });

  it("rejects a name over the length cap", async () => {
    const badId = new BN(998);
    const badCollection = findCollectionPda(badId);
    try {
      await program.methods
        .createCollection(
          badId,
          "This Name Is Definitely Too Long",
          "BAD",
          "https://example.com/bad.json",
          new BN(0),
          new BN(1)
        )
        .accounts({
          authority: authority.publicKey,
          collection: badCollection,
          systemProgram: SystemProgram.programId,
        })
        .signers([authority])
        .rpc();
      assert.fail("expected create_collection to fail with an over-long name");
    } catch (err) {
      assert.include(String(err), "NameTooLong");
    }
  });

  it("creates a collection", async () => {
    await program.methods
      .createCollection(
        COLLECTION_ID,
        "Test Drop",
        "TST",
        "https://example.com/meta.json",
        new BN(PRICE_LAMPORTS),
        new BN(MAX_SUPPLY)
      )
      .accounts({
        authority: authority.publicKey,
        collection: collectionPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([authority])
      .rpc();

    const collection = await program.account.collection.fetch(collectionPda);
    assert.equal(collection.currentSupply.toNumber(), 0);
    assert.equal(collection.maxSupply.toNumber(), MAX_SUPPLY);
  });

  it("mints an NFT: charges the price, mints exactly 1 token, and creates real Metaplex metadata", async () => {
    const collectionBefore = await program.account.collection.fetch(collectionPda);
    const index = collectionBefore.currentSupply;
    const accounts = await mintAccounts(index);

    const authorityBalanceBefore = await provider.connection.getBalance(authority.publicKey);

    await program.methods.mintNft().accounts(accounts).signers([buyer]).rpc();

    const authorityBalanceAfter = await provider.connection.getBalance(authority.publicKey);
    assert.equal(authorityBalanceAfter - authorityBalanceBefore, PRICE_LAMPORTS);

    const mintInfo = await getMint(provider.connection, accounts.mint);
    assert.equal(mintInfo.supply.toString(), "1");
    assert.equal(mintInfo.decimals, 0);

    const tokenAccount = await getAccount(provider.connection, accounts.buyerTokenAccount);
    assert.equal(tokenAccount.amount.toString(), "1");

    const metadataAccountInfo = await provider.connection.getAccountInfo(accounts.metadata);
    assert.isNotNull(metadataAccountInfo);
    assert.isTrue(metadataAccountInfo!.owner.equals(METADATA_PROGRAM_ID));
    assert.include(metadataAccountInfo!.data.toString("utf8"), "Test Drop #0");

    const masterEditionInfo = await provider.connection.getAccountInfo(accounts.masterEdition);
    assert.isNotNull(masterEditionInfo);
    assert.isTrue(masterEditionInfo!.owner.equals(METADATA_PROGRAM_ID));

    const collectionAfter = await program.account.collection.fetch(collectionPda);
    assert.equal(collectionAfter.currentSupply.toNumber(), 1);
  });

  it("mints the remaining supply and then rejects once sold out", async () => {
    const collectionBefore = await program.account.collection.fetch(collectionPda);
    const index = collectionBefore.currentSupply;
    const accounts = await mintAccounts(index);

    await program.methods.mintNft().accounts(accounts).signers([buyer]).rpc();

    const collectionAfter = await program.account.collection.fetch(collectionPda);
    assert.equal(collectionAfter.currentSupply.toNumber(), MAX_SUPPLY);

    const soldOutAccounts = await mintAccounts(collectionAfter.currentSupply);
    try {
      await program.methods.mintNft().accounts(soldOutAccounts).signers([buyer]).rpc();
      assert.fail("expected mint_nft to fail once sold out");
    } catch (err) {
      assert.include(String(err), "SoldOut");
    }
  });
});
