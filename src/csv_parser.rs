use crate::types::TransactionRecord;
use csv::{ReaderBuilder, Trim};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;

/// CSV parser for transaction records
/// Supports streaming to handle large files efficiently
pub struct TransactionReader<R: io::Read> {
    reader: csv::Reader<R>,
}

impl TransactionReader<BufReader<File>> {
    /// Create a new reader from a file path
    /// Returns error if file cannot be opened
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let buf_reader = BufReader::new(file);
        Ok(Self::from_reader(buf_reader))
    }
}

impl<R: io::Read> TransactionReader<R> {
    /// Create a new reader from any readable source
    pub fn from_reader(reader: R) -> Self {
        let csv_reader = ReaderBuilder::new()
            .trim(Trim::All) // Trim whitespace from all fields
            .flexible(true) // Allow variable number of fields (amount can be empty)
            .from_reader(reader);

        Self {
            reader: csv_reader,
        }
    }

    /// Get an iterator over transaction records
    /// Streams records one at a time for memory efficiency
    pub fn records(self) -> TransactionRecordIterator<R> {
        TransactionRecordIterator {
            inner: self.reader.into_deserialize(),
        }
    }
}

/// Iterator over transaction records
/// Yields Result<TransactionRecord, csv::Error> for error handling
pub struct TransactionRecordIterator<R: io::Read> {
    inner: csv::DeserializeRecordsIntoIter<R, TransactionRecord>,
}

impl<R: io::Read> Iterator for TransactionRecordIterator<R> {
    type Item = Result<TransactionRecord, csv::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransactionType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_simple_transactions() {
        let data = "\
type,client,tx,amount
deposit,1,1,1.0
withdrawal,1,2,0.5
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 2);

        // Check deposit
        assert_eq!(records[0].tx_type, TransactionType::Deposit);
        assert_eq!(records[0].client, 1);
        assert_eq!(records[0].tx, 1);
        assert_eq!(records[0].amount, Some(dec!(1.0)));

        // Check withdrawal
        assert_eq!(records[1].tx_type, TransactionType::Withdrawal);
        assert_eq!(records[1].client, 1);
        assert_eq!(records[1].tx, 2);
        assert_eq!(records[1].amount, Some(dec!(0.5)));
    }

    #[test]
    fn test_parse_dispute_transactions() {
        let data = "\
type,client,tx,amount
deposit,1,1,100.0
dispute,1,1,
resolve,1,1,
chargeback,1,1,
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 4);

        // Check deposit
        assert_eq!(records[0].tx_type, TransactionType::Deposit);
        assert_eq!(records[0].amount, Some(dec!(100.0)));

        // Check dispute (no amount)
        assert_eq!(records[1].tx_type, TransactionType::Dispute);
        assert_eq!(records[1].client, 1);
        assert_eq!(records[1].tx, 1);
        assert_eq!(records[1].amount, None);

        // Check resolve (no amount)
        assert_eq!(records[2].tx_type, TransactionType::Resolve);
        assert_eq!(records[2].amount, None);

        // Check chargeback (no amount)
        assert_eq!(records[3].tx_type, TransactionType::Chargeback);
        assert_eq!(records[3].amount, None);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let data = "\
type, client, tx, amount
deposit, 1, 1, 1.0
withdrawal,  2,  2,  0.5
dispute,  1,  1,
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 3);

        // Verify whitespace was trimmed
        assert_eq!(records[0].client, 1);
        assert_eq!(records[0].tx, 1);
        assert_eq!(records[1].client, 2);
        assert_eq!(records[2].amount, None);
    }

    #[test]
    fn test_parse_decimal_precision() {
        let data = "\
type,client,tx,amount
deposit,1,1,1.1234
deposit,2,2,10.5
deposit,3,3,100
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].amount, Some(dec!(1.1234)));
        assert_eq!(records[1].amount, Some(dec!(10.5)));
        assert_eq!(records[2].amount, Some(dec!(100)));
    }

    #[test]
    fn test_parse_multiple_clients() {
        let data = "\
type,client,tx,amount
deposit,1,1,100.0
deposit,2,2,200.0
deposit,1,3,50.0
withdrawal,2,4,100.0
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 4);
        assert_eq!(records[0].client, 1);
        assert_eq!(records[1].client, 2);
        assert_eq!(records[2].client, 1);
        assert_eq!(records[3].client, 2);
    }

    #[test]
    fn test_parse_large_transaction_ids() {
        // Test u32 max range
        let data = "\
type,client,tx,amount
deposit,1,4294967295,100.0
deposit,2,1,50.0
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tx, u32::MAX);
        assert_eq!(records[1].tx, 1);
    }

    #[test]
    fn test_invalid_transaction_type() {
        let data = "\
type,client,tx,amount
invalid,1,1,100.0
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let result: Result<Vec<_>, _> = reader.records().collect();

        // Should fail to deserialize
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_client_id() {
        // u16 max is 65535
        let data = "\
type,client,tx,amount
deposit,65536,1,100.0
";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let result: Result<Vec<_>, _> = reader.records().collect();

        // Should fail to deserialize
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_csv() {
        let data = "type,client,tx,amount\n";
        let reader = TransactionReader::from_reader(data.as_bytes());
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_parse_from_file() {
        // Test reading from actual file
        let reader = TransactionReader::from_file("test_data/simple.csv")
            .expect("Failed to open test file");
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        assert!(records.len() > 0);
        assert_eq!(records[0].tx_type, TransactionType::Deposit);
    }

    #[test]
    fn test_parse_disputes_file() {
        let reader = TransactionReader::from_file("test_data/disputes.csv")
            .expect("Failed to open test file");
        let records: Result<Vec<_>, _> = reader.records().collect();
        let records = records.expect("Failed to parse CSV");

        // Count transaction types
        let disputes = records
            .iter()
            .filter(|r| r.tx_type == TransactionType::Dispute)
            .count();
        let chargebacks = records
            .iter()
            .filter(|r| r.tx_type == TransactionType::Chargeback)
            .count();

        assert_eq!(disputes, 2);
        assert_eq!(chargebacks, 1);
    }
}
