#!/usr/bin/env bash
# Extension-contract §5 for the banking↔payment clearing seam: prove the cross-module ACL/consumer
# wiring survives a regeneration of BOTH modules. Snapshots the seam files, regenerates banking AND
# payment with --force, asserts byte-identical, and re-runs the end-to-end seam test green.
# Usage: DATABASE_URL=... bash scripts/clearing_seam_roundtrip.sh
set -euo pipefail
cd "$(dirname "$0")/.."

BANK_FILES=(
  src/application/service/banking_write_service.rs
  src/application/service/banking_events.rs
  src/application/service/banking_gl.rs
  src/presentation/http/guarded_routes.rs
  tests/clearing_seam.rs
)
PAY_FILES=(
  ../backbone-payment/src/application/service/payment_write_service.rs
)

echo "→ snapshot seam consumer/ACL files (both modules)"
before=$(shasum -a 256 "${BANK_FILES[@]}" "${PAY_FILES[@]}")

echo "→ regenerate BOTH modules (§5) — payment then banking"
( cd ../backbone-payment && metaphor schema schema generate --force >/dev/null )
metaphor schema schema generate --force >/dev/null

echo "→ verify every seam file is byte-identical after regen"
after=$(shasum -a 256 "${BANK_FILES[@]}" "${PAY_FILES[@]}")
if [ "$before" != "$after" ]; then
  echo "✗ FAIL: a seam file changed during regen"; diff <(echo "$before") <(echo "$after") || true; exit 1
fi
echo "  ✓ all ${#BANK_FILES[@]}+${#PAY_FILES[@]} seam files unchanged"

echo "→ re-run the end-to-end clearing seam post-regen"
cargo test --test clearing_seam -- --test-threads=1 >/dev/null
echo "  ✓ payment→accounting→banking→accounting seam still green after regenerating both modules"
echo "✓ §5 round-trip proven for the clearing seam."
