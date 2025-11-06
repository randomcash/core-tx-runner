# Transaction Processing Engine

Rust-based payments engine that processes CSV transactions, handles disputes/chargebacks, and outputs account states.

## Quick Start

```bash
cargo build          # No warnings/errors
cargo test           # 21 tests passing
cargo run -- transactions.csv > accounts.csv
```

## Implementation
1. **Deposits only disputed** - Withdrawals cannot be disputed
2. **Disputes hold funds** - available→held (total unchanged)
3. **Chargebacks lock permanently** - All future ops fail including deposits
4. **Silent failures** - Invalid ops ignored (insufficient funds, double disputes, etc.)
5. **Streaming** - Memory efficient, handles large files

## Test Coverage
**Test files:**
- `simple.csv` - Basic deposits/withdrawals (2 clients)
- `disputes.csv` - Dispute→resolve and dispute→chargeback flows with account locking
- `edge_cases.csv` - Insufficient funds, double disputes, locked accounts, 4dp precision
- `invalid_references.csv` - Non-existent tx, non-disputed tx, wrong client operations
- `whitespace.csv` - CSV parser whitespace tolerance
- `large_ids.csv` - Boundary values (u16::MAX client, u32::MAX transaction)

## Assumptions

- Transactions processed in file order (chronological)
- Transaction IDs globally unique
- Clients lazy-created on first transaction
- Negative available allowed (withdraw then dispute deposit)
- Output row order non-deterministic