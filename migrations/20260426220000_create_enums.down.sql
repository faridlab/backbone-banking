-- Down: drop enum types for banking module
DROP TYPE IF EXISTS txn_status CASCADE;
DROP TYPE IF EXISTS import_status CASCADE;
DROP TYPE IF EXISTS source_format CASCADE;
DROP TYPE IF EXISTS recon_status CASCADE;
DROP TYPE IF EXISTS match_method CASCADE;
DROP TYPE IF EXISTS matched_source_type CASCADE;
DROP TYPE IF EXISTS bank_account_type CASCADE;
