//! A hash written only through a memoizing reader (`_kinds[label] =
//! kind`, `def _kinds = @kinds ||= {}`) types its values, on the CRuby
//! overlay; the native twin is `spinel_toolchain/hash_write_through_reader.rs`.

use super::hash_write_through_reader_contract as contract;

#[test]
fn a_hash_written_through_its_reader_runs() {
    let run = contract::overlay().run_ruby(contract::SCRIPT);
    run.assert_passes();
    assert_eq!(run.stdout, contract::EXPECTED, "stderr:\n{}", run.stderr);
}

/// The value read back through the reader is the union of the classes
/// `add` stored, so the Spinel emit has a class to call `new` on.
#[test]
fn a_hash_written_through_its_reader_types_its_values_for_spinel() {
    let (_emitted, errors) = contract::overlay().emit(roundhouse::project::BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
}
