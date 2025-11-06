# Transaction Processing Engine

Rust-based payments engine that processes CSV transactions, handles disputes/chargebacks, and outputs account states.

## Quick Start

```bash
cargo build
cargo test
cargo run -- transactions.csv > accounts.csv
```

## Architecture

### Core Types (`src/types.rs`)
- `Account` - Client account with available/held/total funds and locked status
- `StoredTransaction` - Transaction history for dispute tracking (only deposits can be disputed)
- `TransactionRecord` - CSV input record (amount optional for dispute/resolve/chargeback)
- `TransactionType` - Deposit, Withdrawal, Dispute, Resolve, Chargeback

### CSV Parser (`src/csv_parser.rs`)
Stream-based parser with whitespace trimming and flexible field handling. Uses `rust_decimal` for 4-decimal precision.

## Test Data

- **simple.csv** - Basic deposits and withdrawals
- **disputes.csv** - Dispute→resolve and dispute→chargeback flows, account locking
- **edge_cases.csv** - Insufficient funds, double disputes, locked account operations, decimal precision
- **whitespace.csv** - Parser whitespace tolerance
- **large_ids.csv** - Boundary testing (u16::MAX, u32::MAX)

## Key Rules

1. **Only deposits can be disputed** (withdrawals cannot)
2. **Disputes hold funds** (available→held, total unchanged)
3. **Chargebacks lock accounts permanently** (all future transactions fail)
4. **Silent failures** (invalid operations don't output errors)
5. **Streaming parser** (memory efficient for large files)

## Assumptions

- Transactions processed in chronological order (file order)
- Transaction IDs globally unique
- Clients created on-demand
- Locked accounts reject ALL operations including deposits
- Decimals display with up to 4 places

## Dependencies

```toml
csv = "1.3"
serde = { version = "1.0", features = ["derive"] }
rust_decimal = { version = "1.35", features = ["serde-float"] }
rust_decimal_macros = "1.35"
```
