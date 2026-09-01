//! Integration Tests for banking
//!
//! User-owned module stub: declares the generated integration scaffold and carries the
//! crate-root lint expectation covering it (the scaffold itself stays generator-owned).
//!
//! Run with: cargo test --package backbone-banking --test integration_tests

#![recursion_limit = "512"]
#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]

mod integration;

use integration::tests::*;

#[tokio::test]
async fn test_bank_api() {
    let mut test = BankApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_bank_account_api() {
    let mut test = BankAccountApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_bank_clearance_api() {
    let mut test = BankClearanceApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_bank_reconciliation_api() {
    let mut test = BankReconciliationApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_bank_statement_import_api() {
    let mut test = BankStatementImportApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_bank_transaction_api() {
    let mut test = BankTransactionApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_fx_gain_loss_api() {
    let mut test = FxGainLossApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}

#[tokio::test]
async fn test_reconcile_preset_api() {
    let mut test = ReconcilePresetApiTest::new();
    let results = test.run_all().await;

    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("FAILED: {} - {}", f.test_name, f.details);
        }
        panic!("{} tests failed", failed.len());
    }
}
