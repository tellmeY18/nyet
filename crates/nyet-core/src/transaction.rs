use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const TRANSACTION_VERSION: u32 = 1;
pub const TRANSACTION_DIR: &str = "/var/lib/nyet/transactions";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u32,
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub uid: Option<u32>,
    pub started: u64,
    pub finished: u64,
    pub status: String,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

pub fn history() -> Result<Vec<Transaction>> {
    let directory = Path::new(TRANSACTION_DIR);
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    let mut transactions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(transaction) = serde_json::from_slice::<Transaction>(&bytes) else {
            continue;
        };
        if transaction.version == TRANSACTION_VERSION {
            transactions.push(transaction);
        }
    }
    transactions.sort_by(|left, right| {
        left.finished
            .cmp(&right.finished)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(transactions)
}

pub fn write_transaction(directory: &Path, transaction: &Transaction) -> Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join(format!("{}.json", transaction.id));
    let temporary = directory.join(format!(".{}.{}.tmp", transaction.id, std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(transaction)?)?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

pub fn validate(transaction: &Transaction) -> Result<()> {
    if transaction.version != TRANSACTION_VERSION {
        return Err(Error::Usage(format!(
            "unsupported transaction version {}",
            transaction.version
        )));
    }
    if transaction.id.is_empty() {
        return Err(Error::Usage("transaction id is empty".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction(id: &str, finished: u64) -> Transaction {
        Transaction {
            version: TRANSACTION_VERSION,
            id: id.to_string(),
            kind: "apply".to_string(),
            plan_id: Some("abc".to_string()),
            uid: Some(1000),
            started: finished.saturating_sub(2),
            finished,
            status: "success".to_string(),
            before: Some("system-1-link".to_string()),
            after: Some("system-2-link".to_string()),
            message: None,
        }
    }

    #[test]
    fn writes_and_reads_transactions_in_order() {
        let directory = std::env::temp_dir().join(format!(
            "nyet-transactions-{}-{}",
            std::process::id(),
            crate::apply::unix_time().unwrap()
        ));
        write_transaction(&directory, &transaction("b", 20)).unwrap();
        write_transaction(&directory, &transaction("a", 10)).unwrap();

        let mut transactions = Vec::new();
        for entry in std::fs::read_dir(&directory).unwrap().flatten() {
            let bytes = std::fs::read(entry.path()).unwrap();
            transactions.push(serde_json::from_slice::<Transaction>(&bytes).unwrap());
        }
        transactions.sort_by_key(|transaction: &Transaction| transaction.finished);
        assert_eq!(
            transactions
                .iter()
                .map(|transaction| transaction.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn validates_a_transaction() {
        assert!(validate(&transaction("a", 10)).is_ok());
        let mut invalid = transaction("", 10);
        assert!(validate(&invalid).is_err());
        invalid.id = "a".to_string();
        invalid.version = 99;
        assert!(validate(&invalid).is_err());
    }
}
