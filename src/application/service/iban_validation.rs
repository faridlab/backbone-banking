//! IBAN validation for the bank-account write path (hand-authored, user-owned).
//!
//! Fail-closed at the OWNER's write path: any account number that CLAIMS to be an IBAN
//! (two letters + two digits) but whose country prefix is not in the reviewed registry is
//! REFUSED — the inverse of the upstream posture, which passes unknown countries through a
//! generic check. Refusing loudly (typed error + a `warn!` log naming the country) protects
//! statement matching and settlement postings keyed off the account number.
//!
//! ## Local account numbers
//!
//! `BankAccount.account_number` predates IBAN and holds LOCAL account numbers (Indonesian
//! accounts are not IBAN-based). A value that is not IBAN-shaped is accepted as a local
//! number — this mirrors the upstream `acc_type` split (`iban` vs `bank`), with the
//! fail-closed inversion applied where a country IS claimed. There is no "no number"
//! sentinel here: the field is required (a placeholder should be a local-format value,
//! never a fake IBAN).
//!
//! ## Semantics ported (offline structural validation)
//!
//! - ISO 13616 mod-97 checksum verification (move the first four characters to the end,
//!   map A=10..Z=35, interpret mod 97; valid IBANs are congruent to 1).
//! - The per-country IBAN length table (the IBAN registry excerpt).
//! - Per-country BBAN structure checks for the majors (DE, FR, GB, NL, BE, ES, IT, PT,
//!   AT, IE, LU, CH, SE, DK, FI, NO, PL, CZ, SK, HU, RO, GR, TR, AE, SA, CY, MT);
//!   remaining registry countries enforce alphanumeric-only BBANs of the exact length.
//! - Normalization: uppercase, spaces/dashes stripped (paper-form grouping removed).
//!
//! ## Semantics deliberately NOT ported
//!
//! - No online bank-directory lookup (fenced alongside VIES: network verification of
//!   financial identifiers re-enters only via `backbone-integrations` with real billing).
//! - No IBAN registry beyond the static table below — a new country is a reviewed table
//!   row, never a generic pass.
//!
//! ## Named explicit configuration escape
//!
//! `IbanValidationPolicy::allow_unknown_countries()` (wire via
//! `BankingWriteService::with_iban_policy`, or set `BANKING_IBAN_ALLOW_UNKNOWN_COUNTRIES=1`
//! consumed at service construction) accepts IBAN-shaped numbers with unregistered country
//! prefixes, each with a per-write `warn!` log. Checksum and structure failures are NEVER
//! escaped. The default is fail-closed.

use std::fmt;

/// Environment variable that arms the named unknown-country escape at service construction.
pub const ALLOW_UNKNOWN_COUNTRIES_ENV: &str = "BANKING_IBAN_ALLOW_UNKNOWN_COUNTRIES";

/// Why an IBAN-shaped account number was refused. Machine-codeable so the write path can
/// distinguish "unknown country" (fail-closed posture; maybe the escape is warranted) from
/// "known country, bad number" (never escaped — fix the value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IbanError {
    /// The number claims a country prefix that is not in the reviewed registry.
    UnknownCountry(String),
    /// The country is known but the total length is wrong for it.
    InvalidLength { country: String, expected: u8, actual: usize },
    /// The BBAN part does not match the country's structure.
    InvalidStructure { country: String, reason: &'static str },
    /// The mod-97 checksum does not validate.
    InvalidChecksum,
}

impl IbanError {
    pub fn code(&self) -> &'static str {
        match self {
            IbanError::UnknownCountry(_) => "iban_unknown_country",
            IbanError::InvalidLength { .. } => "iban_invalid_length",
            IbanError::InvalidStructure { .. } => "iban_invalid_structure",
            IbanError::InvalidChecksum => "iban_invalid_checksum",
        }
    }
}

impl fmt::Display for IbanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IbanError::UnknownCountry(c) => write!(
                f,
                "iban_unknown_country: {c} is not in the reviewed IBAN registry (fail-closed; the escape is {ALLOW_UNKNOWN_COUNTRIES_ENV} or IbanValidationPolicy::allow_unknown_countries)"
            ),
            IbanError::InvalidLength { country, expected, actual } => write!(
                f,
                "iban_invalid_length: a {country} IBAN has {expected} characters, got {actual}"
            ),
            IbanError::InvalidStructure { country, reason } => {
                write!(f, "iban_invalid_structure: not a valid {country} BBAN: {reason}")
            }
            IbanError::InvalidChecksum => {
                write!(f, "iban_invalid_checksum: mod-97 verification failed")
            }
        }
    }
}
impl std::error::Error for IbanError {}

/// Validation posture for the bank-account write path. Default: fail-closed for
/// IBAN-shaped numbers claiming unregistered countries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IbanValidationPolicy {
    /// The NAMED ESCAPE: accept IBAN-shaped numbers whose country is not registered.
    /// Every acceptance logs a warning naming the country.
    pub allow_unknown_countries: bool,
}

impl IbanValidationPolicy {
    /// The default posture: unknown countries are refused loudly.
    pub const FAIL_CLOSED: Self = Self { allow_unknown_countries: false };

    /// The NAMED EXPLICIT ESCAPE: unknown-country IBANs are accepted with a per-write
    /// warning. Opt in via configuration, never by default.
    pub const ALLOW_UNKNOWN_COUNTRIES: Self = Self { allow_unknown_countries: true };

    /// Read the policy from the environment (`BANKING_IBAN_ALLOW_UNKNOWN_COUNTRIES=1|true`
    /// arms the escape). Unset or any other value stays fail-closed.
    pub fn from_env() -> Self {
        let armed = std::env::var(ALLOW_UNKNOWN_COUNTRIES_ENV)
            .ok()
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
            .unwrap_or(false);
        if armed {
            tracing::warn!(
                env_var = ALLOW_UNKNOWN_COUNTRIES_ENV,
                "banking IBAN validation escape ARMED: unknown-country IBAN-shaped account numbers will be accepted with a per-write warning"
            );
            Self::ALLOW_UNKNOWN_COUNTRIES
        } else {
            Self::FAIL_CLOSED
        }
    }
}

impl Default for IbanValidationPolicy {
    fn default() -> Self {
        Self::FAIL_CLOSED
    }
}

/// Canonical form for storage: uppercase, no spaces or dashes (grouping removed).
pub fn normalize_iban(v: &str) -> String {
    v.trim()
        .to_uppercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-'))
        .collect()
}

/// BBAN structure kinds the registry table expresses. `n` = digits, `a` = uppercase
/// letters, `c` = alphanumeric; counts are exact.
#[derive(Clone, Copy)]
enum Bban {
    /// Exact digit/letter/alnum run pattern, e.g. `&[(Kind::D, 10), (Kind::C, 11), (Kind::D, 2)]`.
    Pattern(&'static [(BbanKind, u8)]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BbanKind {
    Digit,
    Letter,
    Alnum,
}

impl BbanKind {
    fn accepts(self, c: char) -> bool {
        match self {
            BbanKind::Digit => c.is_ascii_digit(),
            BbanKind::Letter => c.is_ascii_uppercase(),
            BbanKind::Alnum => c.is_ascii_uppercase() || c.is_ascii_digit(),
        }
    }
}

/// One registry row: country code, total IBAN length, BBAN structure.
struct IbanCountry {
    code: &'static str,
    length: u8,
    bban: Bban,
}

use BbanKind::{Alnum as C, Digit as N, Letter as A};

/// The reviewed IBAN registry excerpt: every ISO 13616 country with its exact total
/// length; the majors carry their full BBAN structure, the rest alphanumeric-only.
/// Adding a country = adding a reviewed row here (never a generic fallback).
const IBAN_REGISTRY: &[IbanCountry] = &[
    IbanCountry { code: "AD", length: 24, bban: Bban::Pattern(&[(N, 8), (C, 12)]) },
    IbanCountry { code: "AE", length: 23, bban: Bban::Pattern(&[(N, 19)]) },
    IbanCountry { code: "AL", length: 28, bban: Bban::Pattern(&[(N, 8), (C, 16)]) },
    IbanCountry { code: "AT", length: 20, bban: Bban::Pattern(&[(N, 16)]) },
    IbanCountry { code: "AZ", length: 28, bban: Bban::Pattern(&[(A, 4), (C, 20)]) },
    IbanCountry { code: "BA", length: 20, bban: Bban::Pattern(&[(N, 16)]) },
    IbanCountry { code: "BE", length: 16, bban: Bban::Pattern(&[(N, 12)]) },
    IbanCountry { code: "BG", length: 22, bban: Bban::Pattern(&[(A, 4), (N, 6), (C, 8)]) },
    IbanCountry { code: "BH", length: 22, bban: Bban::Pattern(&[(A, 4), (C, 14)]) },
    IbanCountry { code: "BR", length: 29, bban: Bban::Pattern(&[(N, 23), (A, 1), (N, 1)]) },
    IbanCountry { code: "CH", length: 21, bban: Bban::Pattern(&[(N, 5), (C, 12)]) },
    IbanCountry { code: "CR", length: 22, bban: Bban::Pattern(&[(N, 18)]) },
    IbanCountry { code: "CY", length: 28, bban: Bban::Pattern(&[(N, 8), (C, 16)]) },
    IbanCountry { code: "CZ", length: 24, bban: Bban::Pattern(&[(N, 20)]) },
    IbanCountry { code: "DE", length: 22, bban: Bban::Pattern(&[(N, 18)]) },
    IbanCountry { code: "DK", length: 18, bban: Bban::Pattern(&[(N, 14)]) },
    IbanCountry { code: "DO", length: 28, bban: Bban::Pattern(&[(C, 4), (N, 20)]) },
    IbanCountry { code: "EE", length: 20, bban: Bban::Pattern(&[(N, 16)]) },
    IbanCountry { code: "EG", length: 29, bban: Bban::Pattern(&[(N, 25)]) },
    IbanCountry { code: "ES", length: 24, bban: Bban::Pattern(&[(N, 20)]) },
    IbanCountry { code: "FI", length: 18, bban: Bban::Pattern(&[(N, 14)]) },
    IbanCountry { code: "FO", length: 18, bban: Bban::Pattern(&[(N, 14)]) },
    IbanCountry { code: "FR", length: 27, bban: Bban::Pattern(&[(N, 10), (C, 11), (N, 2)]) },
    IbanCountry { code: "GB", length: 22, bban: Bban::Pattern(&[(A, 4), (N, 14)]) },
    IbanCountry { code: "GE", length: 22, bban: Bban::Pattern(&[(A, 2), (N, 16)]) },
    IbanCountry { code: "GI", length: 23, bban: Bban::Pattern(&[(A, 4), (C, 15)]) },
    IbanCountry { code: "GL", length: 18, bban: Bban::Pattern(&[(N, 14)]) },
    IbanCountry { code: "GR", length: 27, bban: Bban::Pattern(&[(N, 7), (C, 16)]) },
    IbanCountry { code: "GT", length: 28, bban: Bban::Pattern(&[(C, 24)]) },
    IbanCountry { code: "HR", length: 21, bban: Bban::Pattern(&[(N, 17)]) },
    IbanCountry { code: "HU", length: 28, bban: Bban::Pattern(&[(N, 24)]) },
    IbanCountry { code: "IE", length: 22, bban: Bban::Pattern(&[(C, 4), (N, 14)]) },
    IbanCountry { code: "IL", length: 23, bban: Bban::Pattern(&[(N, 19)]) },
    IbanCountry { code: "IQ", length: 23, bban: Bban::Pattern(&[(A, 4), (N, 15)]) },
    IbanCountry { code: "IS", length: 26, bban: Bban::Pattern(&[(N, 22)]) },
    IbanCountry { code: "IT", length: 27, bban: Bban::Pattern(&[(A, 1), (N, 10), (C, 12)]) },
    IbanCountry { code: "JO", length: 30, bban: Bban::Pattern(&[(A, 4), (N, 4), (C, 18)]) },
    IbanCountry { code: "KW", length: 30, bban: Bban::Pattern(&[(A, 4), (C, 22)]) },
    IbanCountry { code: "KZ", length: 20, bban: Bban::Pattern(&[(N, 3), (C, 13)]) },
    IbanCountry { code: "LB", length: 28, bban: Bban::Pattern(&[(N, 4), (C, 20)]) },
    IbanCountry { code: "LC", length: 32, bban: Bban::Pattern(&[(A, 4), (C, 24)]) },
    IbanCountry { code: "LI", length: 21, bban: Bban::Pattern(&[(N, 5), (C, 12)]) },
    IbanCountry { code: "LT", length: 20, bban: Bban::Pattern(&[(N, 16)]) },
    IbanCountry { code: "LU", length: 20, bban: Bban::Pattern(&[(N, 3), (C, 13)]) },
    IbanCountry { code: "LV", length: 21, bban: Bban::Pattern(&[(A, 4), (C, 13)]) },
    IbanCountry { code: "LY", length: 25, bban: Bban::Pattern(&[(N, 21)]) },
    IbanCountry { code: "MC", length: 27, bban: Bban::Pattern(&[(N, 10), (C, 11), (N, 2)]) },
    IbanCountry { code: "MD", length: 24, bban: Bban::Pattern(&[(C, 2), (C, 18)]) },
    IbanCountry { code: "ME", length: 22, bban: Bban::Pattern(&[(N, 18)]) },
    IbanCountry { code: "MK", length: 19, bban: Bban::Pattern(&[(A, 3), (C, 10), (N, 2)]) },
    IbanCountry { code: "MR", length: 27, bban: Bban::Pattern(&[(N, 23)]) },
    IbanCountry { code: "MT", length: 31, bban: Bban::Pattern(&[(A, 4), (N, 5), (C, 18)]) },
    IbanCountry { code: "MU", length: 30, bban: Bban::Pattern(&[(A, 4), (N, 19), (A, 3)]) },
    IbanCountry { code: "NI", length: 28, bban: Bban::Pattern(&[(A, 4), (N, 20)]) },
    IbanCountry { code: "NL", length: 18, bban: Bban::Pattern(&[(A, 4), (N, 10)]) },
    IbanCountry { code: "NO", length: 15, bban: Bban::Pattern(&[(N, 11)]) },
    IbanCountry { code: "OM", length: 23, bban: Bban::Pattern(&[(C, 3), (C, 16)]) },
    IbanCountry { code: "PK", length: 24, bban: Bban::Pattern(&[(A, 4), (C, 16)]) },
    IbanCountry { code: "PL", length: 28, bban: Bban::Pattern(&[(N, 24)]) },
    IbanCountry { code: "PS", length: 29, bban: Bban::Pattern(&[(C, 4), (C, 21)]) },
    IbanCountry { code: "PT", length: 25, bban: Bban::Pattern(&[(N, 21)]) },
    IbanCountry { code: "QA", length: 29, bban: Bban::Pattern(&[(A, 4), (C, 21)]) },
    IbanCountry { code: "RO", length: 24, bban: Bban::Pattern(&[(A, 4), (C, 16)]) },
    IbanCountry { code: "RS", length: 22, bban: Bban::Pattern(&[(N, 18)]) },
    IbanCountry { code: "SA", length: 24, bban: Bban::Pattern(&[(N, 2), (C, 18)]) },
    IbanCountry { code: "SC", length: 31, bban: Bban::Pattern(&[(A, 4), (N, 20), (A, 3)]) },
    IbanCountry { code: "SD", length: 18, bban: Bban::Pattern(&[(N, 14)]) },
    IbanCountry { code: "SE", length: 24, bban: Bban::Pattern(&[(N, 20)]) },
    IbanCountry { code: "SI", length: 19, bban: Bban::Pattern(&[(N, 15)]) },
    IbanCountry { code: "SK", length: 24, bban: Bban::Pattern(&[(N, 20)]) },
    IbanCountry { code: "SM", length: 27, bban: Bban::Pattern(&[(A, 1), (N, 10), (C, 12)]) },
    IbanCountry { code: "ST", length: 25, bban: Bban::Pattern(&[(N, 21)]) },
    IbanCountry { code: "SV", length: 28, bban: Bban::Pattern(&[(A, 4), (N, 20)]) },
    IbanCountry { code: "TL", length: 23, bban: Bban::Pattern(&[(N, 19)]) },
    IbanCountry { code: "TN", length: 24, bban: Bban::Pattern(&[(N, 20)]) },
    IbanCountry { code: "TR", length: 26, bban: Bban::Pattern(&[(N, 5), (C, 1), (C, 16)]) },
    IbanCountry { code: "UA", length: 29, bban: Bban::Pattern(&[(N, 6), (C, 19)]) },
    IbanCountry { code: "VA", length: 22, bban: Bban::Pattern(&[(N, 18)]) },
    IbanCountry { code: "VG", length: 24, bban: Bban::Pattern(&[(A, 4), (N, 16)]) },
    IbanCountry { code: "XK", length: 20, bban: Bban::Pattern(&[(N, 16)]) },
];

/// ISO 13616 mod-97: rotate the first four chars to the end, map letters to 10..35,
/// interpret the whole run as a number, and require remainder 1.
fn mod97_ok(v: &str) -> bool {
    let rearranged: String = format!("{}{}", &v[4..], &v[..4]);
    let mut acc: u32 = 0;
    for c in rearranged.chars() {
        let digit = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        // Streaming mod: acc = (acc * 10^k + digit) mod 97 with k = 1 for digits, 2 for letters.
        acc = (acc * if c.is_ascii_digit() { 10 } else { 100 } + digit) % 97;
    }
    acc == 1
}

/// Is this value IBAN-shaped (two letters + two digits, then alphanumerics)?
/// Values that are not IBAN-shaped are LOCAL account numbers — out of IBAN scope.
pub fn is_iban_shaped(v: &str) -> bool {
    let b: Vec<char> = v.chars().collect();
    b.len() >= 5
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_uppercase()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4..].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Offline structural IBAN validation, fail-closed. Returns the canonical (normalized)
/// form for storage. This is the policy-free core; the write path calls
/// [`validate_iban_with`].
pub fn validate_iban(raw: &str) -> Result<String, IbanError> {
    let v = normalize_iban(raw);
    if !is_iban_shaped(&v) {
        // Non-IBAN-shaped values are local account numbers; the write path never calls
        // this for them, but keep the core honest: they are not IBANs.
        return Err(IbanError::InvalidStructure {
            country: String::new(),
            reason: "not an IBAN-shaped value",
        });
    }
    let country = &v[..2];
    let row = IBAN_REGISTRY.iter().find(|r| r.code == country);
    let row = match row {
        Some(r) => r,
        None => return Err(IbanError::UnknownCountry(country.to_string())),
    };
    if v.len() != row.length as usize {
        return Err(IbanError::InvalidLength {
            country: country.to_string(),
            expected: row.length,
            actual: v.len(),
        });
    }
    let Bban::Pattern(parts) = row.bban;
    let bban: Vec<char> = v[4..].chars().collect();
    let mut pos = 0usize;
    for (kind, count) in parts {
        let count = *count as usize;
        if pos + count > bban.len()
            || !bban[pos..pos + count].iter().all(|&c| kind.accepts(c))
        {
            return Err(IbanError::InvalidStructure {
                country: country.to_string(),
                reason: "BBAN segment characters do not match the country structure",
            });
        }
        pos += count;
    }
    // A structure row must cover the whole BBAN exactly.
    if pos != bban.len() {
        return Err(IbanError::InvalidStructure {
            country: country.to_string(),
            reason: "BBAN length does not match the country structure",
        });
    }
    if !mod97_ok(&v) {
        return Err(IbanError::InvalidChecksum);
    }
    Ok(v)
}

/// Policy-aware validation for the bank-account write path: unknown countries are refused
/// loudly (warn log) unless the NAMED ESCAPE is armed — in which case they are accepted
/// with a warning naming the country, so the escape is auditable in logs. Structure and
/// checksum failures are NEVER escaped.
pub fn validate_iban_with(policy: &IbanValidationPolicy, raw: &str) -> Result<String, IbanError> {
    match validate_iban(raw) {
        Ok(v) => Ok(v),
        Err(IbanError::UnknownCountry(country)) if policy.allow_unknown_countries => {
            tracing::warn!(
                country = %country,
                "banking IBAN validation escape: accepting an IBAN-shaped number whose country is not in the reviewed registry"
            );
            Ok(normalize_iban(raw))
        }
        Err(e) => {
            tracing::warn!(error = %e.code(), "banking IBAN validation refused a value: {e}");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_valid_ibans() {
        assert_eq!(validate_iban("DE89370400440532013000").unwrap(), "DE89370400440532013000");
        assert_eq!(validate_iban("GB29NWBK60161331926819").unwrap(), "GB29NWBK60161331926819");
        assert_eq!(validate_iban("NL91ABNA0417164300").unwrap(), "NL91ABNA0417164300");
        assert_eq!(validate_iban("BE68539007547034").unwrap(), "BE68539007547034");
        // Normalization: lowercase in, grouping separators out.
        assert_eq!(
            validate_iban("de89 3704 0044 0532 0130 00").unwrap(),
            "DE89370400440532013000"
        );
    }

    #[test]
    fn checksum_failures_refused() {
        assert_eq!(validate_iban("DE89370400440532013001").unwrap_err(), IbanError::InvalidChecksum);
        assert_eq!(validate_iban("GB29NWBK60161331926811").unwrap_err(), IbanError::InvalidChecksum);
    }

    #[test]
    fn length_and_structure_refused() {
        assert!(matches!(
            validate_iban("DE8937040044053201300"),
            Err(IbanError::InvalidLength { ref country, expected: 22, .. }) if country == "DE"
        ));
        // NL structure: 4 letters + 10 digits; letters in the digit run must fail.
        assert!(matches!(
            validate_iban("NL91ABNA04171643AB"),
            Err(IbanError::InvalidStructure { .. })
        ));
    }

    #[test]
    fn unknown_country_refused_fail_closed() {
        assert_eq!(
            validate_iban("XX12345678901234567890").unwrap_err(),
            IbanError::UnknownCountry("XX".into())
        );
        // The US has no IBAN registry entry: an IBAN-shaped US value is refused.
        assert!(matches!(
            validate_iban("US12ABCD123456789012345678"),
            Err(IbanError::UnknownCountry(_))
        ));
    }

    #[test]
    fn iban_shape_detection() {
        assert!(is_iban_shaped("DE89370400440532013000"));
        assert!(is_iban_shaped("XX12345678901234567890"));
        assert!(!is_iban_shaped("1234567890")); // local account number
        assert!(!is_iban_shaped("ACC-1a2b3c4d")); // local number, letters+digits+dash
        assert!(!is_iban_shaped("DE8x370400440532013000")); // lowercase x
    }

    #[test]
    fn escape_accepts_unknown_but_never_bad_numbers() {
        let escape = IbanValidationPolicy::ALLOW_UNKNOWN_COUNTRIES;
        assert_eq!(
            validate_iban_with(&escape, "XX12345678901234567890").unwrap(),
            "XX12345678901234567890"
        );
        // Checksum and structure failures are never escaped.
        assert!(matches!(
            validate_iban_with(&escape, "DE89370400440532013001"),
            Err(IbanError::InvalidChecksum)
        ));
        assert!(matches!(
            validate_iban_with(&IbanValidationPolicy::FAIL_CLOSED, "XX12345678901234567890"),
            Err(IbanError::UnknownCountry(_))
        ));
    }

    #[test]
    fn registry_rows_are_self_consistent() {
        for row in IBAN_REGISTRY {
            // BBAN pattern length + 4 header chars must equal the declared total length.
            if let Bban::Pattern(parts) = row.bban {
                let bban_len: u16 = parts.iter().map(|(_, n)| *n as u16).sum();
                assert_eq!(
                    bban_len + 4,
                    row.length as u16,
                    "registry row {} is internally inconsistent",
                    row.code
                );
            }
        }
    }
}
