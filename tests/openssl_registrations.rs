//! OpenSSL's native library surface must pass the strict constant boundary.
#[path = "support/emit_and_run.rs"]
mod emit_and_run;

use roundhouse::project::BuildTarget;
use std::process::Command;

const SOURCE: &str = include_str!("support/openssl_registrations.rb");
const OVERRIDE: &str = r#"require "openssl"
module OpenSSL
  module KDF
    def self.hkdf(value, salt:, info:, length:, hash:)
      length
    end
  end
end
class OpenSSL::PKey::EC
  def group(value)
    value
  end
end
"#;
const OVERRIDE_ASSERTIONS: &str = r#"
raise "source class override" unless OpenSSL::KDF.hkdf("key", salt: "", info: "", length: 7, hash: "SHA256") == 7
raise "source instance override" unless OpenSSL::PKey::EC.generate("prime256v1").group(9) == 9
puts "OpenSSL source overrides OK"
"#;
const SUBCLASSES: &str = r#"require "openssl"
class FactoryDigest < OpenSSL::Digest::SHA384
  def hexdigest(data)
    17
  end
end
class FactoryEC < OpenSSL::PKey::EC
  def group
    19
  end
end
class FactoryConsumer
  def self.digest
    FactoryDigest.new.hexdigest("message")
  end
  def self.group
    FactoryEC.generate("prime256v1").group
  end
end
"#;
const SUBCLASS_ASSERTIONS: &str = r#"
raise "Digest subclass" unless FactoryConsumer.digest == 17
raise "EC subclass" unless FactoryConsumer.group == 19
puts "OpenSSL factory subclasses OK"
"#;
const ERROR_ALIAS: &str = r#"require "openssl"
class OpenSSL::PKey::PKeyError
end
class OpenSSL::PKey::ECError
  def message
    42
  end
end
class AliasConsumer
  def self.value
    OpenSSL::PKey::PKeyError.new("error").message
  end
end
"#;
const ASSERTIONS: &str = r#"
raise "HKDF RFC 5869" unless CryptoBoundary.hkdf ==
  "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
raise "comparisons" unless CryptoBoundary.comparisons && CryptoBoundary.comparison_length_error
sha384 = "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
sha512 = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a" +
  "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
raise "SHA384" unless CryptoBoundary.sha384 == ["SHA384", sha384, sha384, sha384, sha384]
raise "SHA512" unless CryptoBoundary.sha512 == ["SHA512", sha512, sha512, sha512, sha512]
hmac384 = "30ddb9c8f347cffbfb44e519d814f074cf4047a55d6f563324f1c6a33920e5edfb2a34bac60bdc96cd33a95623d7d638"
hmac512 = "3926a207c8c42b0c41792cbd3e1a1aaaf5f7a25704f62dfc939c4987dd7ce060" +
  "009c5bb1c2447355b3216f10b537e9afa7b64a4e5391b0d631172d07939e087a"
raise "HMAC384" unless CryptoBoundary.hmac384 == [hmac384, hmac384]
raise "HMAC512" unless CryptoBoundary.hmac512 == [hmac512, hmac512]
raise "EC round trip" unless CryptoBoundary.ec
raise "Ed25519 RFC 8032" unless CryptoBoundary.ed25519 ==
  "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155" +
  "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
raise "errors" unless CryptoBoundary.bad_key && CryptoBoundary.bad_curve &&
  CryptoBoundary.bad_digest && CryptoBoundary.bad_hkdf
puts "OpenSSL registrations OK"
"#;

fn overlay() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("app/models/crypto_boundary.rb", SOURCE)
}

#[test]
fn openssl_constants_pass_strict_emission() {
    let (_, errors) = overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn openssl_emitted_ruby_matches_vectors() {
    overlay().run_ruby(ASSERTIONS).assert_passes();
}

#[test]
fn openssl_registration_preserves_source_methods() {
    override_overlay().run_ruby(OVERRIDE_ASSERTIONS).assert_passes();
    let (tree, errors) = override_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    for path in ["open_ssl/kdf", "open_ssl/pkey/ec"] {
        let sidecar = std::fs::read_to_string(tree.join(format!("app/models/{path}.rbs"))).unwrap();
        assert!(sidecar.contains("-> untyped"), "source override retains a native seed: {sidecar}");
    }
}

fn override_overlay() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("app/models/local_crypto.rb", OVERRIDE)
}

fn subclass_overlay() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("app/models/factory_consumer.rb", SUBCLASSES)
}

#[test]
fn openssl_factories_do_not_erase_subclass_overrides() {
    subclass_overlay().run_ruby(SUBCLASS_ASSERTIONS).assert_passes();
    let (tree, errors) = subclass_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    let sidecar = std::fs::read_to_string(tree.join("app/models/factory_consumer.rbs")).unwrap();
    assert!(!sidecar.contains("-> String") && !sidecar.contains("-> OpenSSL::PKey::EC::Group"), "{sidecar}");
}

fn alias_overlay() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("app/models/alias_consumer.rb", ERROR_ALIAS)
}

#[test]
fn openssl_error_alias_does_not_restore_a_concrete_native_return() {
    alias_overlay().run_ruby("raise \"alias override\" unless AliasConsumer.value == 42").assert_passes();
    let (tree, errors) = alias_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    let sidecar = std::fs::read_to_string(tree.join("app/models/alias_consumer.rbs")).unwrap();
    assert!(!sidecar.contains("-> String"), "{sidecar}");
}

#[test]
fn openssl_registration_leaves_other_methods_untyped() {
    let (tree, errors) = emit_and_run::empty_app().write("app/models/missing_crypto_api.rb",
        "class MissingCryptoApi\n  def self.value\n    OpenSSL::KDF.unprovided_crypto_api\n  end\nend\n")
        .emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    let sidecar = std::fs::read_to_string(tree.join("app/models/missing_crypto_api.rbs")).unwrap();
    assert!(sidecar.contains("def self.value: () -> untyped"), "{sidecar}");
}

#[test]
fn openssl_constants_remain_unavailable_on_crystal() {
    let (_, errors) = overlay().emit(BuildTarget::Crystal);
    for name in ["OpenSSL", "OpenSSL::KDF", "OpenSSL::PKey", "OpenSSL::PKey::EC",
        "OpenSSL::Digest::SHA384", "OpenSSL::Digest::SHA512"] {
        assert!(errors.iter().any(|e| e.contains(&format!("{name} is not available as a class/module value on crystal"))),
            "{name}: {errors:#?}");
    }
}

fn run_native(target: BuildTarget) {
    let sidecars = if matches!(target, BuildTarget::Ruby) { "sig/app/models" } else { "app/models" };
    let (tree, errors) = overlay().emit(target);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(tree.join(sidecars).join("crypto_boundary.rbs").is_file(), "missing generated sidecar");
    compile_and_run(&tree, sidecars,
        &format!("require_relative \"app/models/crypto_boundary\"\n{ASSERTIONS}"), "OpenSSL registrations OK");
}

fn compile_and_run(tree: &emit_and_run::Emitted, sidecars: &str, script: &str, expected: &str) {
    std::fs::write(tree.join("contract.rb"),
        script).unwrap();
    eprintln!("native fixture: {}", tree.display());
    let compiler = std::env::var("SPINEL").unwrap_or_else(|_| "spinel".into());
    let build = Command::new(compiler).args(["--rbs", sidecars, "contract.rb", "-o", "contract"])
        .current_dir(&tree).output().expect("compile emitted program");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(tree.join("contract")).current_dir(&tree).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), expected);
}

#[test]
#[ignore = "requires Spinel with OpenSSL support (da4485a58 or later)"]
fn openssl_native_through_ruby_emission() {
    run_native(BuildTarget::Ruby);
}

#[test]
#[ignore = "requires Spinel with OpenSSL support (da4485a58 or later)"]
fn openssl_native_through_strict_emission() {
    run_native(BuildTarget::Spinel);
}

#[test]
#[ignore = "requires Spinel with OpenSSL support (da4485a58 or later)"]
fn openssl_source_overrides_run_natively_with_sidecars() {
    let (tree, errors) = override_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    compile_and_run(&tree, "app/models", &format!(
        "require \"openssl\"\nrequire_relative \"app/models/open_ssl/kdf\"\n\
         require_relative \"app/models/open_ssl/pkey/ec\"\n{OVERRIDE_ASSERTIONS}"), "OpenSSL source overrides OK");
}

#[test]
#[ignore = "requires Spinel with OpenSSL support (da4485a58 or later)"]
fn openssl_subclass_overrides_run_natively_with_sidecars() {
    let (tree, errors) = subclass_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    compile_and_run(&tree, "app/models", &format!(
        "require \"openssl\"\nrequire_relative \"app/models/factory_digest\"\n\
         require_relative \"app/models/factory_ec\"\n\
         require_relative \"app/models/factory_consumer\"\n{SUBCLASS_ASSERTIONS}"), "OpenSSL factory subclasses OK");
}

#[test]
#[ignore = "requires Spinel with OpenSSL support (da4485a58 or later)"]
fn openssl_error_alias_override_runs_natively_with_sidecars() {
    let (tree, errors) = alias_overlay().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    compile_and_run(&tree, "app/models", "require \"openssl\"\n\
        require_relative \"app/models/open_ssl/pkey/pkey_error\"\n\
        require_relative \"app/models/open_ssl/pkey/ecerror\"\n\
        require_relative \"app/models/alias_consumer\"\n\
        raise \"alias override\" unless AliasConsumer.value == 42\n\
        puts \"OpenSSL alias override OK\"\n", "OpenSSL alias override OK");
}
