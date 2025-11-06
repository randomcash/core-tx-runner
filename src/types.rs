use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Client ID type (u16 as defined on the spec)
pub type ClientId = u16;

/// Transaction ID type (u32 as defined on the spec)
pub type TransactionId = u32;

/// Type of transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

/// Input transaction record from CSV
/// Handles all transaction types with optional amount field
#[derive(Debug, Deserialize)]
pub struct TransactionRecord {
    #[serde(rename = "type")]
    pub tx_type: TransactionType,
    pub client: ClientId,
    pub tx: TransactionId,
    #[serde(deserialize_with = "deserialize_optional_decimal")]
    pub amount: Option<Decimal>,
}

/// Custom deserializer for optional decimal fields
/// Handles empty strings in CSV (for dispute/resolve/chargeback)
fn deserialize_optional_decimal<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeDecimal {
        Value(Decimal),
        EmptyString(String),
    }

    // Numbers might be strings, integers, etc
    match Option::<MaybeDecimal>::deserialize(deserializer)? {
        Some(MaybeDecimal::Value(v)) => Ok(Some(v)),
        Some(MaybeDecimal::EmptyString(s)) if s.trim().is_empty() => Ok(None),
        Some(MaybeDecimal::EmptyString(s)) => s
            .parse::<Decimal>()
            .map(Some)
            .map_err(|e| Error::custom(format!("Invalid decimal: {}", e))),
        None => Ok(None),
    }
}

/// Stored transaction for dispute tracking
/// Only deposits can be disputed, so we store them
#[derive(Debug, Clone)]
pub struct StoredTransaction {
    pub client_id: ClientId,
    pub tx_type: TransactionType,
    pub amount: Decimal,
    pub disputed: bool,
}

impl StoredTransaction {
    /// Create a new stored transaction
    pub fn new(client_id: ClientId, tx_type: TransactionType, amount: Decimal) -> Self {
        Self {
            client_id,
            tx_type,
            amount,
            disputed: false,
        }
    }

    /// Check if this transaction can be disputed
    /// Only deposits can be disputed and only if not already disputed
    pub fn can_dispute(&self) -> bool {
        self.tx_type == TransactionType::Deposit && !self.disputed
    }

    /// Mark transaction as disputed
    pub fn mark_disputed(&mut self) {
        self.disputed = true;
    }

    /// Mark transaction as resolved (no longer disputed)
    pub fn mark_resolved(&mut self) {
        self.disputed = false;
    }

    /// Check if transaction is currently disputed
    pub fn is_disputed(&self) -> bool {
        self.disputed
    }
}

/// Client account state
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub client: ClientId,
    #[serde(serialize_with = "serialize_decimal_4dp")]
    pub available: Decimal,
    #[serde(serialize_with = "serialize_decimal_4dp")]
    pub held: Decimal,
    #[serde(serialize_with = "serialize_decimal_4dp")]
    pub total: Decimal,
    pub locked: bool,
}

impl Account {
    /// Create a new account with zero balances
    pub fn new(client: ClientId) -> Self {
        Self {
            client,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            total: Decimal::ZERO,
            locked: false,
        }
    }

    /// Deposit funds (increases available and total)
    pub fn deposit(&mut self, amount: Decimal) {
        self.available += amount;
        self.total += amount;
    }

    /// Withdraw funds (decreases available and total)
    /// Returns true if successful, false if insufficient funds
    pub fn withdraw(&mut self, amount: Decimal) -> bool {
        if self.available >= amount {
            self.available -= amount;
            self.total -= amount;
            true
        } else {
            false
        }
    }

    /// Move funds from available to held (dispute)
    /// Total remains unchanged
    pub fn hold_funds(&mut self, amount: Decimal) {
        self.available -= amount;
        self.held += amount;
    }

    /// Move funds from held to available (resolve)
    /// Total remains unchanged
    pub fn release_funds(&mut self, amount: Decimal) {
        self.held -= amount;
        self.available += amount;
    }

    /// Remove held funds and decrease total (chargeback)
    /// Locks the account permanently
    pub fn chargeback(&mut self, amount: Decimal) {
        self.held -= amount;
        self.total -= amount;
        self.locked = true;
    }

    /// Check if account is locked
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

/// Custom serializer for Decimal with 4 decimal places
fn serialize_decimal_4dp<S>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use rust_decimal::prelude::ToPrimitive;

    // Round to 4 decimal places
    let rounded = value.round_dp(4);
    serializer.serialize_f64(rounded.to_f64().unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_account_deposit() {
        let mut account = Account::new(1);
        account.deposit(dec!(100.5));

        assert_eq!(account.available, dec!(100.5));
        assert_eq!(account.total, dec!(100.5));
        assert_eq!(account.held, dec!(0));
    }

    #[test]
    fn test_account_withdrawal_success() {
        let mut account = Account::new(1);
        account.deposit(dec!(100.0));

        let success = account.withdraw(dec!(50.0));

        assert!(success);
        assert_eq!(account.available, dec!(50.0));
        assert_eq!(account.total, dec!(50.0));
    }

    #[test]
    fn test_account_withdrawal_insufficient_funds() {
        let mut account = Account::new(1);
        account.deposit(dec!(100.0));

        let success = account.withdraw(dec!(150.0));

        assert!(!success);
        assert_eq!(account.available, dec!(100.0));
        assert_eq!(account.total, dec!(100.0));
    }

    #[test]
    fn test_account_dispute_flow() {
        let mut account = Account::new(1);
        account.deposit(dec!(100.0));

        // Dispute
        account.hold_funds(dec!(100.0));
        assert_eq!(account.available, dec!(0));
        assert_eq!(account.held, dec!(100.0));
        assert_eq!(account.total, dec!(100.0));

        // Resolve
        account.release_funds(dec!(100.0));
        assert_eq!(account.available, dec!(100.0));
        assert_eq!(account.held, dec!(0));
        assert_eq!(account.total, dec!(100.0));
    }

    #[test]
    fn test_account_chargeback() {
        let mut account = Account::new(1);
        account.deposit(dec!(100.0));
        account.hold_funds(dec!(100.0));

        // Chargeback
        account.chargeback(dec!(100.0));

        assert_eq!(account.available, dec!(0));
        assert_eq!(account.held, dec!(0));
        assert_eq!(account.total, dec!(0));
        assert!(account.is_locked());
    }

    #[test]
    fn test_stored_transaction_can_dispute() {
        let tx = StoredTransaction::new(1, TransactionType::Deposit, dec!(100.0));
        assert!(tx.can_dispute());

        let mut tx_disputed = tx.clone();
        tx_disputed.mark_disputed();
        assert!(!tx_disputed.can_dispute());

        let tx_withdrawal = StoredTransaction::new(1, TransactionType::Withdrawal, dec!(50.0));
        assert!(!tx_withdrawal.can_dispute());
    }
}
