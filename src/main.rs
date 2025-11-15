pub mod csv_parser;
pub mod types;

use csv_parser::TransactionReader;
use std::collections::{HashMap, HashSet};
use std::env;
use std::process;
use types::{Account, ClientId, StoredTransaction, TransactionId, TransactionType};

fn main() {
    // Parse command line arguments
    // Since we have 2 arguments only, no need for any fancy library
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <transactions.csv>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    // Process transactions and get final account states
    match process_file(filename) {
        Ok(accounts) => {
            // Output results to stdout
            if let Err(e) = output_accounts(accounts) {
                eprintln!("Error writing output: {}", e);
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error processing transactions: {}", e);
            process::exit(1);
        }
    }
}

/// Read CSV file and process all transactions, streaming one record at a time
fn process_file(filename: &str) -> Result<HashMap<ClientId, Account>, Box<dyn std::error::Error>> {
    // Account storage - created on demand
    let mut accounts: HashMap<ClientId, Account> = HashMap::new();

    // Transaction storage - only deposits stored for dispute tracking
    // Note: Withdrawals are not stored since they cannot be disputed
    let mut transactions: HashMap<TransactionId, StoredTransaction> = HashMap::new();

    // Track all seen transaction IDs to enforce uniqueness
    let mut seen_tx_ids: HashSet<TransactionId> = HashSet::new();

    // Open CSV file and stream records
    let reader = TransactionReader::from_file(filename)?;

    // Process each transaction record one at a time
    for result in reader.records() {
        let record = match result {
            Ok(r) => r,
            Err(_) => continue, // Skip malformed records silently
        };

        // Process this single transaction
        process_transaction(record, &mut accounts, &mut transactions, &mut seen_tx_ids);
    }

    Ok(accounts)
}

/// Process a single transaction record
fn process_transaction(
    record: types::TransactionRecord,
    accounts: &mut HashMap<ClientId, Account>,
    transactions: &mut HashMap<TransactionId, StoredTransaction>,
    seen_tx_ids: &mut HashSet<TransactionId>,
) {
    // For deposits and withdrawals, enforce transaction ID uniqueness
    match record.tx_type {
        TransactionType::Deposit | TransactionType::Withdrawal => {
            if !seen_tx_ids.insert(record.tx) {
                // Transaction ID already exists - silently ignore this duplicate
                return;
            }
        }
        // Dispute/Resolve/Chargeback reference existing transactions, so don't check uniqueness
        _ => {}
    }

    // Get or create account for this client
    let account = accounts
        .entry(record.client)
        .or_insert_with(|| Account::new(record.client));

    // Skip all operations if account is locked
    if account.is_locked() {
        return;
    }

    // Process transaction based on type
    match record.tx_type {
        TransactionType::Deposit => {
            if let Some(amount) = record.amount {
                // Credit account
                account.deposit(amount);

                // Store transaction for potential disputes
                transactions.insert(
                    record.tx,
                    StoredTransaction::new(record.client, TransactionType::Deposit, amount),
                );
            }
            // Skip if amount is missing (malformed)
        }

        TransactionType::Withdrawal => {
            if let Some(amount) = record.amount {
                // Attempt to debit account (fails silently if insufficient funds)
                account.withdraw(amount);
                // Note: Don't store withdrawals - only deposits can be disputed
            }
            // Skip if amount is missing (malformed)
        }

        TransactionType::Dispute => {
            // Look up the referenced transaction
            if let Some(stored_tx) = transactions.get_mut(&record.tx) {
                // Verify client matches
                if stored_tx.client_id != record.client {
                    return; // Wrong client, ignore
                }

                // Only deposits can be disputed, and only if not already disputed
                if stored_tx.can_dispute() {
                    // Hold the funds
                    account.hold_funds(stored_tx.amount);

                    // Mark transaction as disputed
                    stored_tx.mark_disputed();
                }
            }
            // If tx doesn't exist or can't be disputed, ignore silently
        }

        TransactionType::Resolve => {
            // Look up the referenced transaction
            if let Some(stored_tx) = transactions.get_mut(&record.tx) {
                // Verify client matches
                if stored_tx.client_id != record.client {
                    return; // Wrong client, ignore
                }

                // Only resolve if transaction is currently disputed
                if stored_tx.is_disputed() {
                    // Release the held funds
                    account.release_funds(stored_tx.amount);

                    // Mark transaction as resolved (no longer disputed)
                    stored_tx.mark_resolved();
                }
            }
            // If tx doesn't exist or isn't disputed, ignore silently
        }

        TransactionType::Chargeback => {
            // Look up the referenced transaction
            if let Some(stored_tx) = transactions.get_mut(&record.tx) {
                // Verify client matches
                if stored_tx.client_id != record.client {
                    return; // Wrong client, ignore
                }

                // Only chargeback if transaction is currently disputed
                if stored_tx.is_disputed() {
                    // Remove held funds and lock account
                    account.chargeback(stored_tx.amount);

                    // Transaction remains disputed (terminal state)
                    // Note: We don't remove the transaction from storage
                }
            }
            // If tx doesn't exist or isn't disputed, ignore silently
        }
    }
}

/// Output account states to stdout as CSV
fn output_accounts(accounts: HashMap<ClientId, Account>) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = csv::Writer::from_writer(std::io::stdout());

    // Write all accounts (order doesn't matter per spec)
    for account in accounts.values() {
        writer.serialize(account)?;
    }

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_simple_transactions() {
        use rust_decimal_macros::dec;

        let accounts = process_file("test_data/simple.csv").expect("Failed to process");

        // Client 1: deposit 100 + deposit 50 - withdraw 25 = 125
        let client1 = accounts.get(&1).expect("Client 1 not found");
        assert_eq!(client1.available, dec!(125));
        assert_eq!(client1.held, dec!(0));
        assert_eq!(client1.total, dec!(125));
        assert!(!client1.locked);

        // Client 2: deposit 200 - withdraw 100 = 100
        let client2 = accounts.get(&2).expect("Client 2 not found");
        assert_eq!(client2.available, dec!(100));
        assert_eq!(client2.held, dec!(0));
        assert_eq!(client2.total, dec!(100));
        assert!(!client2.locked);
    }

    #[test]
    fn test_process_disputes() {
        use rust_decimal_macros::dec;

        let accounts = process_file("test_data/disputes.csv").expect("Failed to process");

        // Client 1: Should have resolved dispute
        let client1 = accounts.get(&1).expect("Client 1 not found");
        assert_eq!(client1.available, dec!(200));
        assert_eq!(client1.held, dec!(0));
        assert_eq!(client1.total, dec!(200));
        assert!(!client1.locked);

        // Client 2: Should be locked with 0 balance after chargeback
        let client2 = accounts.get(&2).expect("Client 2 not found");
        assert_eq!(client2.available, dec!(0));
        assert_eq!(client2.held, dec!(0));
        assert_eq!(client2.total, dec!(0));
        assert!(client2.locked);
    }

    #[test]
    fn test_process_edge_cases() {
        use rust_decimal_macros::dec;

        let accounts = process_file("test_data/edge_cases.csv").expect("Failed to process");

        // Client 1: 1000.5678 - 100.0 = 900.5678
        let client1 = accounts.get(&1).expect("Client 1 not found");
        assert_eq!(client1.available, dec!(900.5678));
        assert_eq!(client1.held, dec!(0));
        assert_eq!(client1.total, dec!(900.5678));
        assert!(!client1.locked);

        // Client 2: 500.0 with dispute resolved
        let client2 = accounts.get(&2).expect("Client 2 not found");
        assert_eq!(client2.available, dec!(500));
        assert_eq!(client2.held, dec!(0));
        assert_eq!(client2.total, dec!(500));
        assert!(!client2.locked);

        // Client 3: Chargedback, account locked
        let client3 = accounts.get(&3).expect("Client 3 not found");
        assert_eq!(client3.available, dec!(0));
        assert_eq!(client3.held, dec!(0));
        assert_eq!(client3.total, dec!(0));
        assert!(client3.locked);
    }

    #[test]
    fn test_invalid_references() {
        use rust_decimal_macros::dec;

        let accounts = process_file("test_data/invalid_references.csv").expect("Failed to process");

        // Client 1: Only deposit, all invalid dispute/resolve/chargeback ignored
        let client1 = accounts.get(&1).expect("Client 1 not found");
        assert_eq!(client1.available, dec!(100));
        assert_eq!(client1.held, dec!(0));
        assert_eq!(client1.total, dec!(100));
        assert!(!client1.locked);

        // Client 2: Deposit, resolve on non-disputed ignored, then dispute+resolve
        let client2 = accounts.get(&2).expect("Client 2 not found");
        assert_eq!(client2.available, dec!(200));
        assert_eq!(client2.held, dec!(0));
        assert_eq!(client2.total, dec!(200));
        assert!(!client2.locked);

        // Client 3: Deposit, chargeback on non-disputed ignored, then dispute + chargeback on non-existent
        let client3 = accounts.get(&3).expect("Client 3 not found");
        assert_eq!(client3.available, dec!(0));
        assert_eq!(client3.held, dec!(300));
        assert_eq!(client3.total, dec!(300));
        assert!(!client3.locked); // Not locked because chargeback referenced non-existent tx
    }
}
