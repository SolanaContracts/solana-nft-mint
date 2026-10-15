# solana-nft-mint

An Anchor program for a simple numbered NFT collection: fixed max supply, fixed price, sequential numbering — the shape behind most NFT drops.

## Why this one's different from the rest of this org

Every other repo here — the 8 I've built plus the 10 pre-existing/forked ones — only ever deals with fungible tokens or raw SOL. Not one mints a unique, metadata-bearing NFT, despite that being arguably the single most common thing built on Solana. This is the first program here that:

- **CPIs into a real third-party program** — [`mpl-token-metadata`](https://github.com/metaplex-foundation/mpl-token-metadata), the actual Metaplex Token Metadata program — rather than only `spl-token`/`system_program`/itself.
- **Implements the real master-edition "true NFT" mechanism**: a mint capped at exactly 1 unit, an on-chain `Metadata` account (name/symbol/uri/creators), and a `MasterEdition` account that permanently takes over the mint's authorities so it can never be printed or inflated — the actual mechanism that makes something "an NFT" rather than a 1-of-1 fungible token someone could technically mint more of later.

## Instructions

| Instruction | Signer | Description |
|---|---|---|
| `create_collection(collection_id, name, symbol, uri, price_lamports, max_supply)` | authority | Creates the `Collection` config (a shared metadata template, price, and supply cap). |
| `mint_nft()` | buyer | Pays `price_lamports` SOL to the collection authority, mints token `#{current_supply}`, and creates its Metaplex `Metadata` + `MasterEdition`. Fails once `current_supply == max_supply`. |

## Accounts

**`Collection`** — PDA at `["collection", authority, collection_id]`
- `authority`, `collection_id`, `name` (≤24 chars), `symbol` (≤10), `uri` (≤180)
- `price_lamports`, `max_supply`, `current_supply`

Each mint creates its own `Mint` + Metaplex `Metadata` + `MasterEdition` accounts — there's no separate per-NFT account of ours; that's where the actual NFT data lives, matching how real Metaplex NFTs work.

## A simplification worth knowing

All NFTs in a collection share the same `symbol`/`uri` — only the on-chain **name** is unique (`"{name} #{index}"`). A real per-token-unique-art drop needs an off-chain metadata server generating distinct JSON per token; that's out of scope here. This program is the on-chain half of a drop, not a full Candy-Machine replacement.

## Building and testing

Requires `solana-cli`, `anchor-cli`, and Rust already installed. This machine needed `platform-tools` v1.57 to avoid an `edition2024` build error (same issue as the other repos in this org).

Testing this program requires the **real** Metaplex Token Metadata program on the local validator — `Anchor.toml` clones it from mainnet-beta automatically:
```toml
[test.validator]
url = "https://api.mainnet-beta.solana.com"

[[test.validator.clone]]
address = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
```

```bash
anchor build --no-idl -- --tools-version v1.57
anchor idl build -o target/idl/solana_nft_mint.json -t target/types/solana_nft_mint.ts
anchor test --skip-build --no-idl
```

`cargo clippy` (run from `programs/solana-nft-mint`) is clean.

**A real correctness bug worth knowing about**: Metaplex's `CreateMasterEditionV3` rejects the mint with `"Cannot create NFT with no Freeze Authority"` unless the `Mint` was created with a freeze authority set — the Master Edition program needs to take it over to freeze the token after minting. The `mint` account in `MintNft` sets both `mint::authority` and `mint::freeze_authority` to the collection's mint-authority PDA for this reason.

## CLI client

`cli/` is a Rust CLI (`nft-mint-cli`, built with `anchor-client` + `clap`) covering both instructions, plus a `show` command for a collection's state. Defaults to a local validator (`http://127.0.0.1:8899` / `ws://127.0.0.1:8900`) — override with `--url`/`--ws-url` for devnet or mainnet.

`mint-nft` fetches the collection on-chain to figure out the next token index and derives every PDA (mint, mint authority, Metaplex metadata, master edition) automatically — you never need to compute or pass them.

**Running this against a local validator requires the real Metaplex program to be present** — clone it with the full upgradeable-program flag (plain `--clone` only copies the thin executable pointer, not the actual bytecode, and will fail with `"Program is not deployed"`):

```bash
solana-test-validator --reset --quiet \
  --url https://api.mainnet-beta.solana.com \
  --clone-upgradeable-program metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s &

cargo build -p nft-mint-cli
BIN=./target/debug/nft-mint-cli

solana program deploy target/deploy/solana_nft_mint.so \
  --program-id target/deploy/solana_nft_mint-keypair.json

$BIN create-collection --keypair ~/authority.json --collection-id 1 \
  --name "Test Drop" --symbol "TST" --uri "https://example.com/meta.json" \
  --price-sol 0.1 --max-supply 100

$BIN mint-nft --keypair ~/buyer.json --authority <AUTHORITY_PUBKEY> --collection-id 1

$BIN show --authority <AUTHORITY_PUBKEY> --collection-id 1
```

Run `$BIN --help` or `$BIN <command> --help` for the full flag list.
