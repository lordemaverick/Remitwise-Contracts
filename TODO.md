# TODO (encoding stability task)

- [ ] Add `#[cfg(test)]` encoding stability + round-trip tests for `Category`, `CoverageType`, `FamilyRole` in `remitwise-common/src/lib.rs`
- [ ] Pin discriminant/encoding for every variant and assert equality after `IntoVal` -> `TryFromVal`
- [ ] Add exhaustiveness enforcement (match covering every variant) so new variants require test updates
- [ ] Add container edge-case tests (Vec + Map round-trips)
- [x] Add/Update docs under `docs/` documenting cross-contract ABI for these enums

- [ ] Run `cargo test -p remitwise-common encoding -- --nocapture`
- [ ] Run `cargo clippy -p remitwise-common` and fix any issues
