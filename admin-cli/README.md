# admin-cli

Command-line admin tool for the floor-program contract.

## Prerequisites
- Rust toolchain
- Solana CLI keypair with admin authority

## Build
- Release: `cargo build --release --locked --bin admin-cli`
- Run from repo root or `admin-cli` directory

## Usage
```
admin-cli [--keypair <PATH> | --keypair-base58-file <PATH>] [--cluster <CLUSTER_OR_RPC_URL>] [--override-program-id <PROGRAM_ID>] <COMMAND>
```
- `--keypair, -k` (default: `~/.config/solana/id.json`)
- `--keypair-base58-file` (file containing base58-encoded keypair string)
- `--cluster, -c` (default: `devnet`; accepts `mainnet`, `devnet`, `localnet`/`localhost`, or full RPC URL)
- `--override-program-id, -o` (override deployed program ID for requests)

### Commands
- `info` — print contract state, vault balances, and investor pool
- `initialize --usdc-mint <PUBKEY> --waln-mint <PUBKEY> --floor-price-usdc <AMOUNT> --round-size-waln <AMOUNT> [--lock-period-seconds <SECONDS>]` — initialize the program
- `set-floor-price <PRICE>` — update floor price (raw units)
- `set-round-size <SIZE>` — update round size in WALN (raw units)
- `set-lock-period <SECONDS>` — update wALN claim lock period in seconds
- `set-usdc-withdraw-lock <SECONDS>` — update the USDC withdrawal lock duration (0 = disabled); applied to each investor on their next deposit
- `set-investor-usdc-unlock <INVESTOR_PUBKEY> <UNIX_TIMESTAMP>` — override a specific investor's USDC unlock timestamp (admin-only early exit)
- `set-paused --paused <true|false>` — pause or unpause the contract
- `cancel-round` — cancel the current active round and refund locked USDC
- `fund-treasury <LAMPORTS>` — send SOL to the treasury PDA for round account rent
- `mint-aat-nft <INVESTOR_PUBKEY> <AAT_VOLUME>` — mint an AAT NFT for an investor
- `transfer-authority <NEW_ADMIN_PUBKEY>` — initiate a two-step admin authority transfer to a new address (current admin only)
- `accept-authority` — accept a pending admin authority transfer (new admin must sign)
- `finalize-claim-for-all <ROUND_INDEX> [--batch-size <N>]` — admin-settle every still-unclaimed wALN allocation in a locked round; re-fetches and skips investors who already self-claimed, sends in batches (default 8/tx), and closes the round account (rent refunded to treasury) once the last allocation is settled. The round must be past its unlock time.

## Examples
- Show state on devnet:
  - `admin-cli info`
- Initialize program:
  - `admin-cli initialize --usdc-mint <USDC_MINT> --waln-mint <WALN_MINT> --floor-price-usdc 100000 --round-size-waln 200000000000`
- Set floor price:
  - `admin-cli set-floor-price 150000`
- Set round size:
  - `admin-cli set-round-size 200000000000`
- Pause the contract:
  - `admin-cli set-paused --paused true`
- Cancel current round:
  - `admin-cli cancel-round`
- Fund treasury with 0.1 SOL:
  - `admin-cli fund-treasury 100000000`
- Mint AAT NFT for an investor:
  - `admin-cli mint-aat-nft <INVESTOR_PUBKEY> 1000`
- Set USDC withdrawal lock to 90 days:
  - `admin-cli set-usdc-withdraw-lock 7776000`
- Disable USDC withdrawal lock:
  - `admin-cli set-usdc-withdraw-lock 0`
- Override unlock timestamp for a specific investor (to allow early withdrawal):
  - `admin-cli set-investor-usdc-unlock <INVESTOR_PUBKEY> 1`
- Transfer admin authority to a new address:
  - `admin-cli transfer-authority <NEW_ADMIN_PUBKEY>`
- Accept pending authority transfer (run with new admin keypair):
  - `admin-cli --keypair ~/.config/solana/new-admin.json accept-authority`
- Finalize all remaining claims for round 0 (e.g. 7 investors never self-claimed):
  - `admin-cli finalize-claim-for-all 0`
- Run on mainnet with custom keypair:
  - `admin-cli --cluster mainnet --keypair ~/.config/solana/admin.json info`
- Use a base58 keypair string from file:
  - `admin-cli --keypair-base58-file ./admin.key info`
- Point to local validator:
  - `admin-cli --cluster http://127.0.0.1:8899 info`
