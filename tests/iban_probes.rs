//! IBAN fail-closed probes for the bank-account write path (hand-authored, user-owned).
//!
//! Proves the posture end-to-end against a migrated database:
//!   - a valid IBAN is accepted and stored in canonical (uppercased, separator-free) form;
//!   - an IBAN-shaped number claiming an UNREGISTERED country is refused with the distinct
//!     `iban_unknown_country` error and NOTHING is written (fail-closed inversion);
//!   - a wrong mod-97 checksum and a wrong country length are both refused;
//!   - a LOCAL-format account number (the field's pre-existing contract — Indonesian
//!     accounts are not IBAN-based) passes unchanged;
//!   - the NAMED configuration escape (`IbanValidationPolicy::ALLOW_UNKNOWN_COUNTRIES`)
//!     accepts unknown-country IBANs — and still refuses bad checksums.
//!
//! Requires DATABASE_URL (defaults to local dev Postgres on :5433/backbone_banking).

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount,
};
use backbone_banking::application::service::iban_validation::IbanValidationPolicy;

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string()
    });
    PgPool::connect(&url).await.unwrap()
}

fn uq(p: &str) -> String {
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}

async fn bank_id(pool: &PgPool, svc: &BankingWriteService) -> Uuid {
    svc.create_bank(NewBank {
        name: uq("IBAN Bank"),
        swift_bic: None,
        country: Some("ID".into()),
    })
    .await
    .unwrap()
}

fn account(bank: Uuid, number: &str) -> NewBankAccount {
    NewBankAccount {
        branch_id: None,
        bank_id: bank,
        account_name: "Test Account".into(),
        account_number: number.into(),
        gl_account_id: Uuid::new_v4(),
        clearing_account_id: Uuid::new_v4(),
        currency: Some("IDR".into()),
        account_type: Some("checking".into()),
    }
}

async fn stored_number(pool: &PgPool, id: Uuid) -> String {
    let row: (String,) =
        sqlx::query_as("SELECT account_number FROM banking.bank_accounts WHERE id=$1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
    row.0
}

// IBAN-P1: a valid IBAN is accepted and stored canonically (grouping separators removed).
#[tokio::test]
async fn valid_iban_accepted_and_canonicalized() {
    let pool = pool().await;
    let svc = BankingWriteService::new(pool.clone());
    let bank = bank_id(&pool, &svc).await;
    let id = svc
        .create_bank_account(account(bank, "de89 3704 0044 0532 0130 00"))
        .await
        .expect("a valid IBAN must be accepted");
    assert_eq!(stored_number(&pool, id).await, "DE89370400440532013000");
}

// IBAN-P2: an unknown-country IBAN-shaped number is refused fail-closed, distinct error
// code, nothing written. (The upstream posture passes these through a generic check.)
// The probe number uses a prefix no OTHER probe ever writes, so the nothing-written
// count cannot race the parallel escape probe (which does persist an XX-prefixed row).
#[tokio::test]
async fn unknown_country_refused_loudly_fail_closed() {
    let pool = pool().await;
    let svc = BankingWriteService::new(pool.clone());
    let bank = bank_id(&pool, &svc).await;
    let err = svc
        .create_bank_account(account(bank, "ZZ12345678901234567890"))
        .await
        .unwrap_err();
    match &err {
        BankingError::IbanUnknownCountry(country) => assert_eq!(country, "ZZ"),
        other => panic!("expected IbanUnknownCountry, got {other:?}"),
    }
    assert_eq!(err.code(), "iban_unknown_country");
    assert_eq!(err.http_status(), 422);
    let n: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM banking.bank_accounts WHERE account_number='ZZ12345678901234567890'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n.0, 0, "a refused account number must not be written");
}

// IBAN-P3: checksum and length failures are refused for KNOWN countries.
#[tokio::test]
async fn checksum_and_length_refused() {
    let pool = pool().await;
    let svc = BankingWriteService::new(pool.clone());
    let bank = bank_id(&pool, &svc).await;
    for bad in ["DE89370400440532013001", "DE893704004405320130001"] {
        let err = svc.create_bank_account(account(bank, bad)).await.unwrap_err();
        assert!(matches!(err, BankingError::InvalidIban(_)), "expected InvalidIban for {bad}");
    }
}

// IBAN-P4: local-format account numbers (letters+digits+separators, not IBAN-shaped)
// pass unchanged — the field's pre-existing contract.
#[tokio::test]
async fn local_account_numbers_pass_unchanged() {
    let pool = pool().await;
    let svc = BankingWriteService::new(pool.clone());
    let bank = bank_id(&pool, &svc).await;
    let id = svc
        .create_bank_account(account(bank, "0123456789"))
        .await
        .expect("a local account number must be accepted");
    assert_eq!(stored_number(&pool, id).await, "0123456789");
    svc.create_bank_account(account(bank, &uq("ACC")))
        .await
        .expect("letters+digits local numbers (test-fixture shape) must be accepted");
}

// IBAN-P5: the NAMED escape accepts unknown-country IBANs; bad checksums stay refused.
#[tokio::test]
async fn named_escape_honored() {
    let pool = pool().await;
    let escaped = BankingWriteService::new(pool.clone())
        .with_iban_policy(IbanValidationPolicy::ALLOW_UNKNOWN_COUNTRIES);
    let bank = bank_id(&pool, &escaped).await;
    let id = escaped
        .create_bank_account(account(bank, "XX12345678901234567890"))
        .await
        .expect("the named escape must accept an unknown-country IBAN");
    assert_eq!(stored_number(&pool, id).await, "XX12345678901234567890");
    assert!(matches!(
        escaped
            .create_bank_account(account(bank, "DE89370400440532013001"))
            .await
            .unwrap_err(),
        BankingError::InvalidIban(_)
    ));
}
