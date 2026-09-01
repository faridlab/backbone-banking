# IBAN validation (bank-account write path)

`bank_accounts.account_number` historically held free text. It still accepts **local**
account numbers (Indonesian accounts are not IBAN-based), but any value that CLAIMS to be
an IBAN — two letters + two digits, then alphanumerics — is validated **fail-closed at the
owner's write path** (`BankingWriteService::create_bank_account`, see
`src/application/service/iban_validation.rs`). This is the deliberate inverse of the
upstream `base_iban` posture, which passes unknown countries through a generic check:
here, an IBAN-shaped number claiming a country that is not in the reviewed registry is
**refused**, loudly.

This matters downstream: statement matching, settlement bounds, and clearance postings all
key off the account, and a wrong IBAN discovered there is far more expensive than a
refused create.

## Behaviour

| Input | Outcome |
|---|---|
| `DE89 3704 0044 0532 0130 00` | accepted, stored **canonicalized** (uppercased; spaces/dashes stripped) |
| `GB29NWBK60161331926819`, `NL91ABNA0417164300`, `BE68539007547034` | accepted (registry length + BBAN structure + mod-97 all pass) |
| `DE89370400440532013001` | refused — `iban_invalid_checksum` (mod-97 fails) |
| `DE893704004405320130001` | refused — `iban_invalid_length` |
| `NL91ABNA04171643AB` | refused — `iban_invalid_structure` (letters in the digit run) |
| `XX12345678901234567890` | refused — `iban_unknown_country` (the fail-closed posture) |
| `US12ABCD…` | refused — `US` has no IBAN registry entry; an IBAN-shaped US value is a data error |
| `0123456789`, `ACC-1a2b3c4d` | **accepted unchanged** — local account numbers (not IBAN-shaped) |

Every refusal logs a `warn!` naming the reason. Unknown-country refusals carry their own
error code (`iban_unknown_country`, HTTP 422) so operators can distinguish "the escape may
be warranted" from "the number is wrong" — checksum, length, and structure failures are
never escape-able.

There is **no "no number" sentinel** on this field (unlike party's `/` no-VAT sentinel):
`account_number` is required. A stub account should carry a local-format placeholder —
never a fabricated IBAN, which the checksum would refuse anyway.

## What was ported (offline structural validation)

- **ISO 13616 mod-97 checksum** — rotate the first four characters to the end, map
  `A=10..Z=35`, take mod 97; a valid IBAN is congruent to 1.
- **The per-country length table** — the IBAN registry excerpt, every ISO 13616 country
  with its exact total length.
- **BBAN structure checks** — exact digit/letter/alphanumeric runs for the majors (DE,
  FR, GB, NL, BE, ES, IT, PT, AT, IE, LU, CH, SE, DK, FI, NO, PL, CZ, SK, HU, RO, GR,
  TR, AE, SA, CY, MT, and the rest of the table); every row is internally consistency-
  checked by a unit test (pattern length + 4 = declared length).
- **Normalization** — uppercase, strip spaces and dashes (paper-form grouping).

## What was deliberately NOT ported

- **Online bank-directory verification stays FENCED** (alongside VIES on the party side):
  no network calls from this module; re-entry is only via `backbone-integrations` with
  real billing behind it.
- **No registry beyond the static table** — a new country is a reviewed row, never a
  generic pass.
- **Odoo's `acc_type` triage is simplified to a binary**: IBAN-shaped → strict IBAN
  validation; anything else → local number. Odoo's third "else" bucket (neither iban nor
  known bank format) is not modeled.

## The named configuration escape

`BANKING_IBAN_ALLOW_UNKNOWN_COUNTRIES=1` (or `IbanValidationPolicy::ALLOW_UNKNOWN_COUNTRIES`
wired via `BankingWriteService::with_iban_policy`) accepts IBAN-shaped numbers with
unregistered country prefixes. The escape:

- is **never implicit** — the default is fail-closed; arming it logs a `warn!` at service
  construction and a `warn!` per accepted unknown-country value;
- **never widens correctness** — checksum, length, and structure failures are still
  refused under the escape.

## Probes

`tests/iban_probes.rs` proves the posture end-to-end against a migrated database: a valid
IBAN accepted + canonicalized; unknown country refused with the distinct code and no row
written; checksum and length refused for known countries; local numbers pass unchanged;
the escape honored (and still refusing bad checksums). Unit tests in `iban_validation.rs`
cover the checksum, the registry rows' self-consistency, and shape detection.
