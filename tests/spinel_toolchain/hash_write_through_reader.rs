//! A hash written only through a memoizing reader, compiled natively;
//! the CRuby twin is `emit_and_run/hash_write_through_reader.rs`.

use super::hash_write_through_reader_contract as contract;

#[test]
#[ignore = "requires the Spinel toolchain, run in its CI lane"]
fn a_hash_written_through_its_reader_runs_natively() {
    let run = contract::overlay().run_spinel(contract::SCRIPT);
    run.assert_passes();
    assert_eq!(run.stdout, contract::EXPECTED, "stderr:\n{}", run.stderr);
}
