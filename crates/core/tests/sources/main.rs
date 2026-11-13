#![allow(clippy::duplicate_mod)]

use hito_core::test_support::support;

mod manual_source_choice_upgrade;
#[path = "../fixtures/resolution_deleted_receipt.rs"]
mod resolution_deleted_receipt_fixture;
#[path = "../fixtures/resolution_failover.rs"]
mod resolution_failover_fixture;
#[path = "../fixtures/resolution_musical.rs"]
mod resolution_musical_fixture;
mod source_choice_reset_upgrade;
mod source_preference;
mod source_preparation_foreign_keys;
mod source_preparation_history;
mod source_preparation_query_plans;
mod source_resolution_admission;
mod source_resolution_bound_candidate;
mod source_resolution_canonical_live;
mod source_resolution_capacity_outcome;
mod source_resolution_catalog_video_ranking;
mod source_resolution_credentials;
mod source_resolution_credentials_alternate;
mod source_resolution_credentials_detail;
mod source_resolution_credentials_metadata;
mod source_resolution_credentials_ranked;
mod source_resolution_credentials_sources;
mod source_resolution_deleted_receipt;
mod source_resolution_evidence_upgrade;
mod source_resolution_failover;
mod source_resolution_failure_upgrade;
mod source_resolution_featured_title;
mod source_resolution_global_admission;
mod source_resolution_musical_evidence;
mod source_resolution_provider_order;
mod source_resolution_provider_rank_fallback;
mod source_resolution_refusal;
mod source_resolution_revocation;
mod source_resolution_search_order;
mod source_resolution_transient_disposition;
mod source_resolution_unavailable;
mod source_resolution_unavailable_upgrade;
mod source_resolution_upgrade;
mod source_resolution_upgrade_work;
mod source_resolution_versions;
mod source_resolution_video_versions;
