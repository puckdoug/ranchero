// SPDX-License-Identifier: AGPL-3.0-only
#[path = "it/athletes_get.rs"] mod athletes_get;
#[path = "it/athletes_json_blob.rs"] mod athletes_json_blob;
#[path = "it/athletes_schema.rs"] mod athletes_schema;
#[path = "it/athletes_touch.rs"] mod athletes_touch;
#[path = "it/athletes_upsert.rs"] mod athletes_upsert;
#[path = "it/kv_concurrent.rs"] mod kv_concurrent;
#[path = "it/kv_json_helper.rs"] mod kv_json_helper;
#[path = "it/kv_round_trip.rs"] mod kv_round_trip;
#[path = "it/migrations.rs"] mod migrations;
#[path = "it/segments_evict.rs"] mod segments_evict;
#[path = "it/segments_evict_idempotent.rs"] mod segments_evict_idempotent;
#[path = "it/segments_put_get.rs"] mod segments_put_get;
#[path = "it/segments_schema.rs"] mod segments_schema;
#[path = "it/sqlite_open.rs"] mod sqlite_open;
