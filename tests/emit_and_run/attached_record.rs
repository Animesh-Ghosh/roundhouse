//! `has_one_attached` follows its record, as Rails' `Attached::One`
//! does: an attachment belongs to the record's id at the time of the
//! write, `reload` forgets what the proxy remembers, and `purge` on a
//! record found again removes the attachment.

use super::emit_and_run;

fn schema() -> &'static str {
    r#"ActiveRecord::Schema.define do
  create_table "docs", force: :cascade do |t|
    t.string "name"
  end
  create_table "active_storage_blobs", force: :cascade do |t|
    t.string "key", null: false
    t.string "filename", null: false
    t.string "content_type"
    t.text "metadata"
    t.string "service_name", null: false
    t.bigint "byte_size", null: false
    t.string "checksum"
    t.datetime "created_at", null: false
  end
  create_table "active_storage_attachments", force: :cascade do |t|
    t.string "name", null: false
    t.string "record_type", null: false
    t.bigint "record_id", null: false
    t.bigint "blob_id", null: false
    t.datetime "created_at", null: false
  end
  create_table "active_storage_variant_records", force: :cascade do |t|
    t.integer "blob_id", null: false
    t.string "variation_digest", null: false
  end
end
"#
}

/// `Doc` reads `file.attached?` in a validation, so the first read of
/// `file` happens before the insert gives the record its id.
fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write(
            "app/models/application_record.rb",
            "class ApplicationRecord < ActiveRecord::Base\n  self.abstract_class = true\nend\n",
        )
        .write(
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        )
        .write("db/schema.rb", schema())
        .write(
            "app/models/doc.rb",
            r#"class Doc < ApplicationRecord
  has_one_attached :file

  validate :file_is_small

  def self.attachment_count
    ActiveStorage::Attachment.count
  end

  def self.attachment_record_ids
    ActiveStorage::Attachment.all.map { |attachment| attachment.record_id }
  end

  def self.attach_after_create
    doc = create!(name: "after create")
    doc.file.attach("hello-bytes", "cover.png", "image/png")
    doc
  end

  def self.create_with_upload
    File.write("upload.bin", "hello-bytes")
    File.open("upload.bin", "rb") do |io|
      create!(name: "with upload", file: { io: io, filename: "upload.png" })
    end
  end

  def self.found_attached?(id)
    find(id).file.attached?
  end

  def self.purge_found(id)
    find(id).file.purge
  end

  def self.attach_through_another_copy(id)
    find(id).file.attach("other-bytes", "other.png", "image/png")
  end

  def self.reloaded_attached?(doc)
    doc.reload.file.attached?
  end

  def self.assign_then_reload_then_save(doc)
    File.write("dropped.bin", "dropped-bytes")
    File.open("dropped.bin", "rb") do |io|
      doc.file = { io: io, filename: "dropped.png" }
    end
    doc.reload
    doc.save!
    doc
  end

  private
    def file_is_small
      errors.add(:file, "is too big") if file.attached? && file.byte_size > 1_000
    end
end
"#,
        )
        .write("config/routes.rb", "Rails.application.routes.draw do\nend\n")
}

/// The validation reads `file` while the id is not yet set. The
/// attachment that follows the insert still belongs to the new id.
#[test]
fn an_attachment_after_create_belongs_to_the_new_record() {
    app()
        .run_ruby(
            r##"
doc = Doc.attach_after_create
ids = Doc.attachment_record_ids
raise "record ids #{ids.inspect}, expected [#{doc.id}]" unless ids == [doc.id]
raise "a found copy sees no file" unless Doc.found_attached?(doc.id)
puts "attach after create passed"
"##,
        )
        .assert_passes();
}

/// `create!(file: …)` saves the pending upload in `after_save`, after
/// the insert. The row belongs to the new id.
#[test]
fn an_upload_given_to_create_belongs_to_the_new_record() {
    app()
        .run_ruby(
            r##"
doc = Doc.create_with_upload
ids = Doc.attachment_record_ids
raise "record ids #{ids.inspect}, expected [#{doc.id}]" unless ids == [doc.id]
raise "a found copy sees no file" unless Doc.found_attached?(doc.id)
puts "upload on create passed"
"##,
        )
        .assert_passes();
}

/// Dontfile's `FilesController#destroy`: a record found by its key
/// purges the attachment that another copy of the record added.
#[test]
fn purge_on_a_found_record_removes_the_attachment() {
    app()
        .run_ruby(
            r##"
doc = Doc.attach_after_create
raise "count before #{Doc.attachment_count}" unless Doc.attachment_count == 1
Doc.purge_found(doc.id)
raise "count after #{Doc.attachment_count}" unless Doc.attachment_count == 0
raise "a found copy still sees a file" if Doc.found_attached?(doc.id)
puts "purge on found record passed"
"##,
        )
        .assert_passes();
}

/// The proxy remembers "nothing attached" from the validation. Another
/// copy of the record then attaches a file, and `reload` sees it.
#[test]
fn reload_sees_an_attachment_another_copy_added() {
    app()
        .run_ruby(
            r##"
doc = Doc.create!(name: "plain")
raise "attached before" if doc.file.attached?
Doc.attach_through_another_copy(doc.id)
raise "reload does not see the new attachment" unless Doc.reloaded_attached?(doc)
puts "reload attachment passed"
"##,
        )
        .assert_passes();
}

/// Rails' `reload` drops the attachment changes that were not saved.
#[test]
fn reload_drops_an_unsaved_assignment() {
    app()
        .run_ruby(
            r##"
doc = Doc.create!(name: "plain")
Doc.assign_then_reload_then_save(doc)
raise "count #{Doc.attachment_count}: the dropped upload was saved" unless Doc.attachment_count == 0
puts "reload pending passed"
"##,
        )
        .assert_passes();
}
