#[path = "support/emit_and_run.rs"]
mod emit_and_run;

use roundhouse::emit::shared::schema_sql::{Dialect, render_schema_statements_for};
use roundhouse::ingest::ingest_schema;

const SCHEMA: &str = include_str!("support/schema_defaults_schema.rb");
const CONTRACT: &str = include_str!("support/schema_defaults_contract.rb");
const EXPECTED: &str = "SQL default fills omitted values; literal and explicit NULL stay distinct";
const EXECUTE: &str = "def execute(sql)\n  Db.exec(sql)\nend\ndef scalar(sql)\n  stmt = Db.prepare(sql)\n  raise 'no row' unless Db.step?(stmt)\n  value = Db.column_int(stmt, 0)\n  Db.finalize(stmt)\n  value\nend\n";

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("db/schema.rb", SCHEMA)
}

const RANDOM_SCHEMA: &str = r#"ActiveRecord::Schema[8.1].define(version: 1) do
  create_table :record_items do |t|
    t.string :token, default: -> { "printf('item_%s', lower(hex(randomblob(16))))" }, null: false
  end
end
"#;
const RANDOM_MODEL: &str = "class RecordItem < ActiveRecord::Base\n  def self.lookup(id)\n    find(id).token\n  end\nend\n";
const RANDOM_CONTRACT: &str = r#"
first = RecordItem.create!
second = RecordItem.create!
raise 'default missing from model' unless first.token.start_with?('item_') && first.token.length == 37
raise 'random default shared between rows' if first.token == second.token
raise 'reader lost' unless RecordItem.lookup(first.id) == first.token
explicit = RecordItem.create!(token: 'explicit')
raise 'explicit value changed' unless explicit.token == 'explicit'
raise 'SQL function default is eager' unless RecordItem.new.token.nil?
nil_input = RecordItem.create!(token: nil)
raise 'nil did not receive SQL function default' unless nil_input.token.start_with?('item_')
raise 'nil input shared another row default' if nil_input.token == first.token
puts 'per-row SQLite default and reader passed'
"#;

fn random_app() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("db/schema.rb", RANDOM_SCHEMA).write("app/models/record_item.rb", RANDOM_MODEL)
}

#[test]
fn sqlite_random_sql_default_keeps_exact_expression() {
    let schema = ingest_schema(RANDOM_SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
    assert!(ddl.contains("DEFAULT (printf('item_%s', lower(hex(randomblob(16)))))"), "{ddl}");
    assert!(render_schema_statements_for(&schema, Dialect::Postgres).is_err());
}

#[test]
fn sqlite_default_scalar_functions_are_closed_and_column_free() {
    for sql in ["upper('x')", "lower('X')", "hex('x')", "randomblob(16)", "json_valid('{}')", "length('abc')", "substr('abc', 2, 1)", "coalesce(NULL, 'x')", "abs(-2)", "printf('%s', 'it''s')", "'x' || lower('Y')"] {
        let source = RANDOM_SCHEMA.replace("printf('item_%s', lower(hex(randomblob(16))))", sql);
        let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
        let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
        assert!(ddl.contains(&format!("DEFAULT ({sql})")), "{ddl}");
    }
    for sql in ["unknown_function('x')", "load_extension('file')", "token", "randomblob_size(16)", "'x'; SELECT 1", "'unterminated", "1", "(1)", "(1 + 2)", "TRUE", "FALSE", "NULL", "(lower ('X'))"] {
        let source = RANDOM_SCHEMA.replace("printf('item_%s', lower(hex(randomblob(16))))", sql);
        let error = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap_err().to_string();
        assert!(error.contains("db/schema.rb") && error.contains("line 3") && error.contains("record_items.token"), "{error}");
    }
}

#[test]
fn postgres_refuses_sqlite_calls_but_ignores_quoted_data_and_comments() {
    for sql in ["printf('%s', 'x')", "HEX('x')", "randomblob(16)", "json_valid('{}')", "substr('abc', -1)"] {
        let source = RANDOM_SCHEMA.replace("printf('item_%s', lower(hex(randomblob(16))))", sql);
        let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
        assert!(render_schema_statements_for(&schema, Dialect::Postgres).is_err(), "{sql}");
    }
    let source = RANDOM_SCHEMA.replace("printf('item_%s', lower(hex(randomblob(16))))", "coalesce('printf( json_valid(', 'x')");
    let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    render_schema_statements_for(&schema, Dialect::Postgres).unwrap();
}

#[test]
fn sql_defaults_with_unmodeled_key_options_refuse_at_the_source() {
    for declaration in ["t.string :token, primary_key: true,", "t.column :token, :string, primary_key: true,"] {
        let source = RANDOM_SCHEMA.replace("t.string :token,", declaration);
        let error = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap_err().to_string();
        assert!(error.contains("db/schema.rb") && error.contains("line 3") && error.contains("primary_key"), "{error}");
    }
    let mut schema = ingest_schema(RANDOM_SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    schema.tables.get_mut(&roundhouse::Symbol::from("record_items")).unwrap().columns[1].primary_key = true;
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        assert!(render_schema_statements_for(&schema, dialect).unwrap_err().contains("primary-key default"));
    }
}

#[test]
fn emitted_ruby_applies_per_row_sqlite_default_and_keeps_reader() {
    random_app().run_ruby(RANDOM_CONTRACT).assert_passes();
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn native_spinel_applies_per_row_sqlite_default_and_keeps_reader() {
    let script = format!("Db.configure(\":memory:\")\nSchema.statements.each {{ |sql| Db.exec(sql) }}\nActiveRecord.adapter = SqliteAdapter\n{RANDOM_CONTRACT}");
    random_app().run_spinel_with_rbs(&script).assert_passes();
}

#[test]
fn rails_applies_per_row_sqlite_default_and_keeps_reader() {
    let helper = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/rails_oracle.rb");
    let script = format!(r#"
require {helper:?}
unless RailsOracle.available?("railties", "activerecord", sqlite3: ">= 2.9")
  puts 'skip: Rails oracle, railties or sqlite3 unavailable'
  exit
end
RailsOracle.activate("railties", "activerecord", sqlite3: ">= 2.9")
require 'rails'
require 'active_record/railtie'
class OracleApp < Rails::Application
  config.load_defaults 8.1
  config.eager_load = false
  config.secret_key_base = 'test'
  config.logger = Logger.new(File::NULL)
end
ENV['DATABASE_URL'] = 'sqlite3::memory:'
OracleApp.initialize!
ActiveRecord::Base.establish_connection(adapter: 'sqlite3', database: ':memory:')
ActiveRecord::Schema.verbose = false
{RANDOM_SCHEMA}
{RANDOM_MODEL}
{RANDOM_CONTRACT}
puts "oracle Ruby=#{{RUBY_VERSION}} Rails=#{{Rails.version}} defaults=8.1 SQLite=#{{SQLite3::SQLITE_VERSION}}"
"#);
    let out = emit_and_run::ruby().args(["-e", &script]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("per-row SQLite default and reader passed") || stdout.contains("skip:"), "{stdout}");
    println!("{stdout}");
}

#[test]
fn sql_default_reaches_both_ddl_dialects_without_quoting() {
    let schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        let ddl = render_schema_statements_for(&schema, dialect).unwrap().join(";\n");
        assert!(ddl.contains("DEFAULT (CURRENT_TIMESTAMP)"), "{ddl}");
        assert!(ddl.contains("DEFAULT 'CURRENT_TIMESTAMP'"), "{ddl}");
    }
}

#[test]
fn sqlite_clock_spelling_is_portable_and_sql_literals_keep_their_quotes() {
    let source = SCHEMA.replace("-> { \"CURRENT_TIMESTAMP\" }", "-> { \"DATETIME('now')\" }");
    let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        let ddl = render_schema_statements_for(&schema, dialect).unwrap().join(";\n");
        assert!(ddl.contains("DEFAULT (CURRENT_TIMESTAMP)"), "{ddl}");
        assert!(ddl.contains("DEFAULT ('[]')"), "{ddl}");
        assert!(ddl.contains("DEFAULT ('O''Reilly')"), "{ddl}");
    }
}

#[test]
fn unsupported_or_computed_defaults_have_source_diagnostics() {
    for default in [
        "-> { \"random()\" }", "-> { computed_sql }", "->(arg) { \"CURRENT_TIMESTAMP\" }",
        "-> { side_effect; \"CURRENT_TIMESTAMP\" }", "SQL_DEFAULT", "Defaults::SQL_DEFAULT",
        "-> { \"'[]' || random()\" }",
    ] {
        let source = SCHEMA.replace("-> { \"CURRENT_TIMESTAMP\" }", default);
        let error = ingest_schema(source.as_bytes(), "db/schema.rb").expect_err("unsupported default must be diagnosed").to_string();
        assert!(error.contains("db/schema.rb") && error.contains("recorded_at"), "{error}");
        assert!(error.contains("line 3"), "{error}");
    }
}

#[test]
fn equivalent_literal_proc_spellings_keep_database_defaults_separate() {
    for default in ["->() { \"CURRENT_TIMESTAMP\" }", "lambda { \"CURRENT_TIMESTAMP\" }", "proc { \"CURRENT_TIMESTAMP\" }", "(-> { \"CURRENT_TIMESTAMP\" })"] {
        let source = SCHEMA.replace("-> { \"CURRENT_TIMESTAMP\" }", default);
        let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
        let column = &schema.tables[&roundhouse::Symbol::from("events")].columns[1];
        assert_eq!(column.default, None);
        assert_eq!(column.default_expression.as_deref(), Some("CURRENT_TIMESTAMP"));
    }
}

#[test]
fn changing_a_column_retains_or_replaces_the_sql_default() {
    use roundhouse::ingest::ingest_migration;
    let mut schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    for change in [
        "change_column :events, :recorded_at, :datetime, precision: 6",
        "change_column_default :events, :recorded_at, from: nil, to: -> { \"CURRENT_DATE\" }",
    ] {
        let migration = format!("class ChangeDefault < ActiveRecord::Migration[8.1]\n def change\n {change}\n end\nend");
        ingest_migration(migration.as_bytes(), "db/migrate/1_default.rb", &mut schema).unwrap();
        let expected = if change.contains("CURRENT_DATE") { "CURRENT_DATE" } else { "CURRENT_TIMESTAMP" };
        let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
        assert!(ddl.contains(&format!("DEFAULT ({expected})")), "{ddl}");
    }
}

#[test]
fn roda_migration_keeps_the_sql_timestamp_default() {
    let mut app = roundhouse::App::default();
    app.schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let migration = files.iter().find(|f| f.path.to_string_lossy().contains("create_events")).unwrap();
    let script = r#"
begin
  gem "sequel", ">= 5.0"
rescue Gem::LoadError
  puts "skip: Sequel is unavailable"
  exit
end
require "sequel"
require "sequel/extensions/migration"
DB = Sequel.sqlite
eval(ARGV.fetch(0)).apply(DB, :up)
DB.run("INSERT INTO events DEFAULT VALUES")
raise "SQL timestamp default was lost" unless DB.fetch("SELECT COUNT(*) AS n FROM events WHERE recorded_at IS NOT NULL").first[:n] == 1
raise "SQL JSON default was lost" unless DB.fetch("SELECT COUNT(*) AS n FROM events WHERE payload = '[]'").first[:n] == 1
DB.run("INSERT INTO events (recorded_at) VALUES (NULL)")
raise "explicit NULL acquired a default" unless DB.fetch("SELECT COUNT(*) AS n FROM events WHERE recorded_at IS NULL").first[:n] == 1
puts "Roda SQL timestamp default passed"
"#;
    let out = emit_and_run::ruby().args(["-e", script, &migration.content]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Roda SQL timestamp default passed") || stdout.contains("skip:"), "{stdout}");
}

#[test]
fn duplicate_default_options_keep_only_the_last_value() {
    for (last, expected) in [("'last'", Some("DEFAULT 'last'")), ("nil", None), ("-> { \"'second'\" }", Some("DEFAULT ('second')"))] {
        let source = format!("ActiveRecord::Schema[8.1].define do\n create_table :events do |t|\n t.string :tag, default: -> {{ \"'first'\" }}, default: {last}\n end\nend");
        let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
        for dialect in [Dialect::Sqlite, Dialect::Postgres] {
            let ddl = render_schema_statements_for(&schema, dialect).unwrap().join(";\n");
            assert!(!ddl.contains("first"), "{ddl}");
            match expected {
                Some(value) => assert!(ddl.contains(value), "{ddl}"),
                None => assert!(!ddl.contains("DEFAULT"), "{ddl}"),
            }
        }
    }
}

#[test]
fn roda_jsonb_default_keeps_nullability_and_sql_options() {
    let source = "ActiveRecord::Schema[8.1].define do\n create_table :events do |t|\n t.jsonb :payload, null: false, default: -> { \"'[]'\" }\n end\nend";
    let mut app = roundhouse::App::default();
    app.schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let migration = files.iter().find(|f| f.path.to_string_lossy().contains("create_events")).unwrap();
    let script = "begin; gem 'sequel', '>= 5.0'; rescue Gem::LoadError; puts 'skip: Sequel is unavailable'; exit; end; require 'sequel'; require 'sequel/extensions/migration'; DB = Sequel.sqlite; eval(ARGV.fetch(0)).apply(DB, :up); DB.run('INSERT INTO events DEFAULT VALUES'); raise 'JSONB default lost' unless DB[:events].first[:payload] == '[]'; begin; DB.run('INSERT INTO events (payload) VALUES (NULL)'); raise 'NULL was accepted'; rescue Sequel::NotNullConstraintViolation; end";
    let out = emit_and_run::ruby().args(["-e", script, &migration.content]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn emitted_ruby_applies_sql_default() {
    let run = app().run_ruby(&format!("{EXECUTE}{CONTRACT}"));
    run.assert_passes();
    assert!(run.stdout.contains(EXPECTED));
}

#[test]
fn roda_uuid_default_keeps_nullability_and_sql_options() {
    let source = "ActiveRecord::Schema[8.1].define do\n create_table :events do |t|\n t.column :token, :uuid, null: false, default: -> { \"'550e8400-e29b-41d4-a716-446655440000'\" }\n end\nend";
    let mut app = roundhouse::App::default();
    app.schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let migration = files.iter().find(|f| f.path.to_string_lossy().contains("create_events")).unwrap();
    let script = "begin; gem 'sequel', '>= 5.0'; rescue Gem::LoadError; puts 'skip: Sequel is unavailable'; exit; end; require 'sequel'; require 'sequel/extensions/migration'; DB = Sequel.sqlite; eval(ARGV.fetch(0)).apply(DB, :up); DB.run('INSERT INTO events DEFAULT VALUES'); raise 'UUID default lost' unless DB[:events].first[:token] == '550e8400-e29b-41d4-a716-446655440000'; begin; DB.run('INSERT INTO events (token) VALUES (NULL)'); raise 'NULL was accepted'; rescue Sequel::NotNullConstraintViolation; end";
    let out = emit_and_run::ruby().args(["-e", script, &migration.content]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn native_spinel_applies_sql_default() {
    let run = app().run_spinel_with_db(&format!("{EXECUTE}{CONTRACT}"));
    run.assert_passes();
    assert!(run.stdout.contains(EXPECTED));
}

#[test]
fn rails_applies_the_same_sql_default() {
    let helper = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/rails_oracle.rb");
    let script = format!(r#"
require {helper:?}
unless RailsOracle.available?("activerecord", sqlite3: ">= 2.9")
  puts "skip: Rails oracle and sqlite3 are unavailable"
  exit
end
RailsOracle.activate("activerecord", sqlite3: ">= 2.9")
require "active_record"
ActiveRecord::Base.establish_connection(adapter: "sqlite3", database: ":memory:")
ActiveRecord::Schema.verbose = false
{SCHEMA}
def execute(sql)
  ActiveRecord::Base.connection.execute(sql)
end
def scalar(sql)
  ActiveRecord::Base.connection.select_value(sql).to_i
end
{CONTRACT}
"#);
    let out = emit_and_run::ruby().args(["-e", &script]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(EXPECTED) || stdout.contains("skip:"), "{stdout}");
}
