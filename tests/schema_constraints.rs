#[path = "support/emit_and_run.rs"]
mod emit_and_run;

use roundhouse::emit::shared::schema_sql::{Dialect, render_schema_statements_for};
use roundhouse::ingest::ingest_schema;

const SCHEMA: &str = include_str!("support/schema_constraints_schema.rb");
const CONTRACT: &str = include_str!("support/schema_constraints_contract.rb");
const EXPECTED: &str = "CHECK and lower(email) enforce writes; NULL is accepted";

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app().write("db/schema.rb", SCHEMA)
}

#[test]
fn check_constraint_reaches_both_ddl_dialects() {
    let schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        let ddl = render_schema_statements_for(&schema, dialect).unwrap().join(";\n");
        assert!(ddl.contains("CHECK (age >= 18)"), "{ddl}");
        assert!(ddl.contains("members_adult"), "{ddl}");
    }
}

#[test]
fn expression_index_reaches_both_ddl_dialects() {
    let schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        let ddl = render_schema_statements_for(&schema, dialect).unwrap().join(";\n");
        assert!(ddl.contains("CREATE UNIQUE INDEX"), "{ddl}");
        assert!(ddl.contains("(lower(email))"), "{ddl}");
    }
}

#[test]
fn emitted_ruby_enforces_check_and_expression_index() {
    let script = format!("def execute(sql)\n  Db.exec(sql)\nend\n{CONTRACT}");
    let run = app().run_ruby(&script);
    run.assert_passes();
    assert!(run.stdout.contains(EXPECTED));
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn native_spinel_enforces_check_and_expression_index() {
    let script = format!("def execute(sql)\n  Db.exec(sql)\nend\n{CONTRACT}\n{}", r#"
refused = false
begin
  Member._unique_index_columns("members_email_lower")
rescue RuntimeError => e
  refused = e.message.include?("expression index")
end
raise "expression index became empty upsert columns" unless refused
"#);
    let run = app().write("app/models/member.rb", "class Member < ActiveRecord::Base\nend\n").run_spinel_with_db(&script);
    run.assert_passes();
    assert!(run.stdout.contains(EXPECTED));
}

#[test]
fn rails_enforces_the_same_check_and_expression_index() {
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
{CONTRACT}
"#);
    let out = emit_and_run::ruby().args(["-e", &script]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(EXPECTED) || stdout.contains("skip:"), "{stdout}");
}

#[test]
fn sqlite_refuses_nonportable_check_and_index_expressions_at_the_source() {
    for declaration in [
        "t.check_constraint \"email::text <> 'blocked'\", name: \"portable_check\"",
        "t.index \"lower(email::text)\", name: \"portable_index\"",
        "t.check_constraint expression_for_check, name: \"computed_check\"",
        "t.index expression_for_index, name: \"computed_index\"",
        "t.index \"lower(email)\", unique: true, where: \"email::text <> 'blocked'\"",
        "t.check_constraint \"email IS NOT NULL\", **constraint_options",
        "t.index \"lower(email)\", **index_options",
        "t.index \"lower(email)\", unique: unique_option",
    ] {
        let source = format!("ActiveRecord::Schema[8.1].define do\n  create_table :members do |t|\n    t.string :email\n    {declaration}\n  end\nend\n");
        let error = ingest_schema(source.as_bytes(), "db/schema.rb").expect_err("unsupported expression must be diagnosed").to_string();
        assert!(error.contains("db/schema.rb"), "{error}");
        if declaration.contains("::text") { assert!(error.contains("line 4"), "{error}"); }
    }
}

#[test]
fn migrations_keep_checks_and_expression_indexes_or_refuse_their_mutation() {
    use roundhouse::ingest::ingest_migration;
    let mut schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    let migration = r#"class AddCheck < ActiveRecord::Migration[8.1]
  def change
    add_check_constraint :members, "age < 100", name: "members_max_age"
    add_index :members, "upper(email)", name: "members_upper"
  end
end"#;
    ingest_migration(migration.as_bytes(), "db/migrate/1_add_check.rb", &mut schema).unwrap();
    let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
    assert!(ddl.contains("CHECK (age < 100)"), "{ddl}");
    assert!(ddl.contains("(upper(email))"), "{ddl}");
    for verb in ["rename_column :members, :email, :address", "remove_column :members, :age"] {
        let source = format!("class ChangeMember < ActiveRecord::Migration[8.1]\n def change\n {verb}\n end\nend");
        assert!(ingest_migration(source.as_bytes(), "db/migrate/2_change.rb", &mut schema.clone()).is_err());
    }
}

#[test]
fn expression_index_removal_matches_sql_keys() {
    use roundhouse::ingest::ingest_migration;
    for arguments in ["\"lower(email)\"", "column: \"lower(email)\""] {
        let mut schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
        let migration = format!("class RemoveIndex < ActiveRecord::Migration[8.1]\n def change\n remove_index :members, {arguments}\n end\nend");
        ingest_migration(migration.as_bytes(), "db/migrate/1_remove.rb", &mut schema).unwrap();
        let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
        assert!(!ddl.contains("CREATE UNIQUE INDEX"), "{ddl}");
        assert!(ddl.contains("CHECK (age >= 18)"), "{ddl}");
    }
}

#[test]
fn roda_migration_keeps_checks_and_expression_index_sql() {
    let mut app = roundhouse::App::default();
    app.schema = ingest_schema(SCHEMA.as_bytes(), "db/schema.rb").unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let migration = files.iter().find(|f| f.path.to_string_lossy().contains("create_members")).unwrap();
    assert!(migration.content.contains("Sequel.lit(\"lower(email)\")"), "{}", migration.content);
    assert!(migration.content.contains("constraint(\"members_adult\", Sequel.lit(\"age >= 18\"))"), "{}", migration.content);
    let script = format!(r#"
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
def execute(sql)
  DB.run(sql)
end
{CONTRACT}
"#);
    let out = emit_and_run::ruby().args(["-e", &script, &migration.content]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(EXPECTED) || stdout.contains("skip:"), "{stdout}");
}

#[test]
fn expression_index_cannot_be_misread_as_upsert_columns() {
    let run = app().write("app/models/member.rb", "class Member < ActiveRecord::Base\nend\n").run_ruby(r#"
refused = false
begin
  Member._unique_index_columns("members_email_lower")
rescue RuntimeError => e
  refused = e.message.include?("expression index")
end
raise "expression index became empty upsert columns" unless refused
puts "expression upsert target refused"
"#);
    run.assert_passes();
    assert!(run.stdout.contains("expression upsert target refused"));
}

#[test]
fn postgres_expression_ddl_keeps_its_casts() {
    use roundhouse::schema::generated::GeneratedExpressionDialect;
    use roundhouse::ingest::schema::ingest_schema_with_generated_expression_dialect;
    let source = SCHEMA.replace("age >= 18", "email::text <> 'blocked'").replace("lower(email)", "lower(email::text)");
    let schema = ingest_schema_with_generated_expression_dialect(source.as_bytes(), "db/schema.rb", GeneratedExpressionDialect::Postgres).unwrap();
    let ddl = render_schema_statements_for(&schema, Dialect::Postgres).unwrap().join(";\n");
    assert!(ddl.contains("CHECK (email::text <> 'blocked')"), "{ddl}");
    assert!(ddl.contains("(lower(email::text))"), "{ddl}");
    assert!(render_schema_statements_for(&schema, Dialect::Sqlite).is_err());
}

#[test]
fn unnamed_expression_index_gets_rails_name() {
    let source = SCHEMA.replace(", name: \"members_email_lower\"", "");
    let schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    let ddl = render_schema_statements_for(&schema, Dialect::Sqlite).unwrap().join(";\n");
    assert!(ddl.contains("index_members_on_lower_email"), "{ddl}");
}

#[test]
fn roda_partial_expression_index_accepts_inactive_duplicates() {
    let source = r#"ActiveRecord::Schema[8.1].define do
  create_table :members do |t|
    t.string :email
    t.integer :active
    t.index 'lower(email)', unique: true, name: 'members_lower', where: 'active = 1'
  end
end"#;
    let mut app = roundhouse::App::default();
    app.schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let migration = files.iter().find(|f| f.path.to_string_lossy().contains("create_members")).unwrap();
    let script = r#"
begin
  gem 'sequel', '>= 5.0'
rescue Gem::LoadError
  puts 'skip: Sequel is unavailable'
  exit
end
require 'sequel'
require 'sequel/extensions/migration'
DB = Sequel.sqlite
eval(ARGV.fetch(0)).apply(DB, :up)
DB.run("INSERT INTO members (email, active) VALUES ('A@B', 0)")
DB.run("INSERT INTO members (email, active) VALUES ('a@b', 0)")
DB.run("INSERT INTO members (email, active) VALUES ('A@B', 1)")
begin
  DB.run("INSERT INTO members (email, active) VALUES ('a@b', 1)")
  raise 'active duplicate was accepted'
rescue Sequel::UniqueConstraintViolation
end
raise 'wrong row count' unless DB[:members].count == 3
puts 'partial expression index passed'
"#;
    let out = emit_and_run::ruby().args(["-e", script, &migration.content]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("partial expression index passed") || stdout.contains("skip:"), "{stdout}");
}

#[test]
fn unnamed_check_can_be_removed_by_rails_generated_name() {
    use roundhouse::ingest::ingest_migration;
    let source = "ActiveRecord::Schema[8.1].define do\n create_table :members do |t|\n t.string :email\n t.check_constraint 'length(email) > 3'\n end\nend";
    let mut schema = ingest_schema(source.as_bytes(), "db/schema.rb").unwrap();
    assert_eq!(schema.tables[&roundhouse::Symbol::from("members")].check_constraints[0].name.as_deref(), Some("chk_rails_43bfe4b039"));
    let migration = "class RemoveCheck < ActiveRecord::Migration[8.1]\n def change\n remove_check_constraint :members, name: 'chk_rails_43bfe4b039'\n end\nend";
    ingest_migration(migration.as_bytes(), "db/migrate/1_remove_check.rb", &mut schema).unwrap();
    for dialect in [Dialect::Sqlite, Dialect::Postgres] {
        let statements = render_schema_statements_for(&schema, dialect).unwrap();
        assert!(!statements.join(";\n").contains("CHECK"));
        if dialect == Dialect::Sqlite {
            let script = "require 'sqlite3'; db = SQLite3::Database.new(':memory:'); ARGV.each { |sql| db.execute(sql) }; db.execute(\"INSERT INTO members (email) VALUES ('x')\"); raise unless db.get_first_value('SELECT COUNT(*) FROM members') == 1";
            let out = emit_and_run::ruby().args(["-e", script]).args(&statements).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
    }
}
