use super::*;
use crate::db::{migrate, CreateWorkflowInput};
use crate::templates::contract::{remap_graph_ids, TemplateEnvelope};
use serde_json::json;
use std::sync::{Arc, Barrier};

const OWNER: &str = "synthetic-owner-reference";
const REQUEST: &str = "synthetic-publication-request";
const SOURCE: &str = "https://share.example/s/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fixture() -> TemplateEnvelope {
    serde_json::from_str(include_str!(
        "../../../contracts/workflow-template/fixtures/valid-note-summary.json"
    ))
    .unwrap()
}

fn count(db: &Db, table: &str) -> i64 {
    db.with_conn(|conn| {
        Ok(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?,
        )
    })
    .unwrap()
}

fn source_workflow(db: &Db) -> String {
    db.create_workflow(CreateWorkflowInput {
        name: "Synthetic publisher".into(),
        description: String::new(),
        working_directory: String::new(),
        folder_id: None,
        graph: json!({"nodes": [], "edges": []}),
    })
    .unwrap()
    .id
}

fn pending(db: &Db) -> (String, String, String) {
    let workflow = source_workflow(db);
    let snapshot = serde_json::to_string(&fixture()).unwrap();
    let digest = crate::templates::contract::digest(&fixture()).unwrap();
    db.begin_template_publication(&workflow, REQUEST, &digest, OWNER, &snapshot)
        .unwrap();
    (workflow, digest, snapshot)
}

fn starts() -> Vec<StartRecipe> {
    serde_json::from_value(json!([
        {"kind":"schedule", "config":{"cron":"0 9 * * *", "timezone":"UTC"}},
        {"kind":"file", "config":{"path":"/synthetic/recipient", "label":"Files"}},
        {"kind":"appEvent", "config":{"event":"synthetic.created", "label":"Event"}},
        {"kind":"webhook", "config":{"label":"Webhook"}}
    ]))
    .unwrap()
}

#[test]
fn import_commits_fresh_graph_disabled_starts_and_local_defaults() {
    let db = Db::open_in_memory().unwrap();
    let template = fixture();
    let graph = remap_graph_ids(&template.graph).graph;
    let digest = crate::templates::contract::digest(&template).unwrap();
    let imported = db
        .import_template_transaction(
            "attempt",
            "Recipient copy",
            "Local",
            "/synthetic/recipient",
            &graph,
            &starts(),
            SOURCE,
            &digest,
        )
        .unwrap();
    assert!(!imported.workflow.memory_retrieval_enabled);
    assert_eq!(imported.workflow.working_directory, "/synthetic/recipient");
    assert_eq!(
        imported.workflow.graph["nodes"],
        serde_json::to_value(&graph.nodes).unwrap()
    );
    assert!(graph.nodes.iter().all(|node| !template
        .graph
        .nodes
        .iter()
        .any(|original| original.id == node.id)));
    db.with_conn(|conn| {
        let enabled: i64 = conn.query_row(
            "SELECT enabled FROM workflow_memory_review WHERE workflow_id = ?1",
            [&imported.workflow_id],
            |row| row.get(0),
        )?;
        assert_eq!(enabled, 0);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM schedules WHERE enabled != 0",
                [],
                |row| row.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM triggers WHERE enabled != 0",
                [],
                |row| row.get::<_, i64>(0)
            )?,
            0
        );
        let secret: String = conn.query_row(
            "SELECT secret FROM triggers WHERE source = 'webhook'",
            [],
            |row| row.get(0),
        )?;
        assert!(!secret.is_empty());
        let provenance: (String, String) = conn.query_row(
            "SELECT source_link, digest FROM template_imports",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(provenance, (SOURCE.into(), digest));
        Ok(())
    })
    .unwrap();
    assert_eq!(count(&db, "schedules"), 1);
    assert_eq!(count(&db, "triggers"), 3);
}

#[test]
fn concurrent_import_retries_create_exactly_one_copy_and_one_set_of_starts() {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let barrier = Arc::new(Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let template = fixture();
                let graph = remap_graph_ids(&template.graph).graph;
                let digest = crate::templates::contract::digest(&template).unwrap();
                barrier.wait();
                db.import_template_transaction(
                    "same-attempt",
                    "Copy",
                    "",
                    "",
                    &graph,
                    &starts(),
                    SOURCE,
                    &digest,
                )
                .unwrap()
                .workflow_id
            })
        })
        .collect::<Vec<_>>();
    let ids = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(count(&db, "workflows"), 1);
    assert_eq!(count(&db, "template_imports"), 1);
    assert_eq!(count(&db, "schedules"), 1);
    assert_eq!(count(&db, "triggers"), 3);
}

#[test]
fn late_import_write_failure_rolls_back_every_created_record() {
    let db = Db::open_in_memory().unwrap();
    db.with_conn(|conn| {
        conn.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON template_imports BEGIN SELECT RAISE(ABORT, 'synthetic import failure'); END;")?;
        Ok(())
    }).unwrap();
    let template = fixture();
    assert!(db
        .import_template_transaction(
            "attempt",
            "Copy",
            "",
            "",
            &template.graph,
            &starts(),
            SOURCE,
            &"a".repeat(64)
        )
        .is_err());
    for table in [
        "workflows",
        "workflow_memory_review",
        "schedules",
        "triggers",
        "template_imports",
    ] {
        assert_eq!(count(&db, table), 0, "left partial {table}");
    }
}

#[test]
fn recipient_memory_links_commit_with_import_and_stale_selection_rolls_back() {
    let db = Db::open_in_memory().unwrap();
    db.with_conn(|conn| {
        conn.execute("INSERT INTO memories (id, scope_type, scope_key, title, body, source, memory_type, created_at, updated_at) VALUES ('recipient-memory', 'user', 'local-user', 'Recipient fact', 'Synthetic fact', 'manual', 'fact', '2026-09-09T00:00:00Z', '2026-09-09T00:00:00Z')", [])?;
        Ok(())
    }).unwrap();
    let graph: TemplateGraph = serde_json::from_value(json!({"nodes":[{"id":"memory-node","type":"memory","position":{"x":0,"y":0},"data":{"memoryIds":["recipient-memory"]}}],"edges":[]})).unwrap();
    let _imported = db
        .import_template_transaction(
            "first",
            "Copy",
            "",
            "",
            &graph,
            &[],
            SOURCE,
            &"a".repeat(64),
        )
        .unwrap();
    db.with_conn(|conn| {
        let links: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_links WHERE memory_id = 'recipient-memory'",
            [],
            |row| row.get(0),
        )?;
        // User-scoped memories are globally visible and must not gain a
        // workflow link as an import side effect.
        assert_eq!(links, 0);
        conn.execute(
            "UPDATE memories SET status = 'retracted' WHERE id = 'recipient-memory'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(db
        .import_template_transaction(
            "second",
            "Stale",
            "",
            "",
            &graph,
            &starts(),
            SOURCE,
            &"a".repeat(64)
        )
        .is_err());
    assert_eq!(count(&db, "workflows"), 1);
    assert_eq!(count(&db, "template_imports"), 1);
    assert_eq!(count(&db, "memory_links"), 0);
    assert_eq!(count(&db, "schedules"), 0);
}

#[test]
fn publication_snapshot_survives_source_deletion_and_can_complete() {
    let db = Db::open_in_memory().unwrap();
    let (workflow, digest, snapshot) = pending(&db);
    db.delete_workflow(&workflow).unwrap();
    let request = db
        .get_template_publication_request(REQUEST)
        .unwrap()
        .unwrap();
    assert_eq!(request.workflow_id, None);
    assert_eq!(request.template_json.as_deref(), Some(snapshot.as_str()));
    db.complete_template_publication(None, REQUEST, &digest, OWNER, &"a".repeat(32))
        .unwrap();
    let publication = db.list_template_shares(None).unwrap().remove(0);
    assert_eq!(publication.workflow_id, None);
    assert_eq!(publication.digest, digest);
    assert!(db.list_pending_template_publications().unwrap().is_empty());
    assert!(db.revoke_template_share(&publication.id).unwrap());
    assert!(db.list_template_shares(None).unwrap()[0]
        .revoked_at
        .is_some());
}

#[test]
fn publication_completion_failure_preserves_pending_request_without_orphan_metadata() {
    let db = Db::open_in_memory().unwrap();
    let (workflow, digest, _) = pending(&db);
    db.with_conn(|conn| {
        conn.execute_batch("CREATE TRIGGER reject_completion BEFORE UPDATE ON template_publication_requests BEGIN SELECT RAISE(ABORT, 'synthetic completion failure'); END;")?;
        Ok(())
    }).unwrap();
    assert!(db
        .complete_template_publication(Some(&workflow), REQUEST, &digest, OWNER, &"a".repeat(32))
        .is_err());
    assert_eq!(count(&db, "template_shares"), 0);
    assert_eq!(db.list_pending_template_publications().unwrap().len(), 1);
}

#[test]
fn publication_completion_rejects_changed_digest() {
    let db = Db::open_in_memory().unwrap();
    let (workflow, digest, _) = pending(&db);
    assert!(db
        .complete_template_publication(
            Some(&workflow),
            REQUEST,
            &"b".repeat(64),
            OWNER,
            &"a".repeat(32)
        )
        .is_err());
    assert_eq!(count(&db, "template_shares"), 0);
    assert_eq!(
        db.get_template_publication_request(REQUEST)
            .unwrap()
            .unwrap()
            .digest,
        digest
    );
}

#[test]
fn publication_completion_rejects_changed_accepted_identity() {
    let db = Db::open_in_memory().unwrap();
    let (workflow, digest, _) = pending(&db);
    db.complete_template_publication(Some(&workflow), REQUEST, &digest, OWNER, &"a".repeat(32))
        .unwrap();
    assert!(db
        .complete_template_publication(Some(&workflow), REQUEST, &digest, OWNER, &"b".repeat(32))
        .is_err());
    assert_eq!(db.list_template_shares(None).unwrap()[0].id, "a".repeat(32));
}

#[test]
fn legacy_publication_metadata_migrates_twice_without_losing_identity() {
    let db = Db::open_in_memory().unwrap();
    let (_, digest, _) = pending(&db);
    db.with_conn(|conn| {
        conn.execute_batch("ALTER TABLE template_publication_requests DROP COLUMN template_json;")?;
        migrate::apply_migrations(conn)?;
        migrate::apply_migrations(conn)?;
        Ok(())
    })
    .unwrap();
    let request = db
        .get_template_publication_request(REQUEST)
        .unwrap()
        .unwrap();
    assert_eq!(request.digest, digest);
    assert_eq!(request.template_json, None);
    assert_eq!(count(&db, "workflows"), 1);
}

fn memory_graph(memory_id: &str) -> crate::templates::contract::TemplateGraph {
    serde_json::from_value(json!({
        "nodes": [{
            "id": "memory-node",
            "type": "memory",
            "data": {"label": "Context", "memoryIds": [memory_id]},
            "position": {"x": 0.0, "y": 0.0}
        }],
        "edges": []
    }))
    .unwrap()
}

#[test]
fn import_rejects_workspace_memory_from_a_different_directory() {
    let db = Db::open_in_memory().unwrap();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO memories
             (id, workflow_id, scope_type, scope_key, kind, memory_type, source, title, body,
              pinned, confidence, salience, status, created_at, updated_at)
             VALUES ('workspace-memory', NULL, 'workspace', '/workspace/other', 'text', 'fact',
                     'manual', 'Other', 'body', 0, 1.0, 50, 'active', ?1, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
    .unwrap();
    let before = count(&db, "workflows");
    assert!(db
        .import_template_transaction(
            "workspace-attempt",
            "Recipient",
            "",
            "/workspace/recipient",
            &memory_graph("workspace-memory"),
            &[],
            SOURCE,
            &"a".repeat(64),
        )
        .is_err());
    assert_eq!(count(&db, "workflows"), before);
    assert_eq!(count(&db, "memory_links"), 0);
}

#[test]
fn import_keeps_user_memory_global_without_creating_a_workflow_link() {
    let db = Db::open_in_memory().unwrap();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO memories
             (id, workflow_id, scope_type, scope_key, kind, memory_type, source, title, body,
              pinned, confidence, salience, status, created_at, updated_at)
             VALUES ('user-memory', NULL, 'user', 'local-user', 'text', 'fact', 'manual',
                     'User', 'body', 0, 1.0, 50, 'active', ?1, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
    .unwrap();
    let imported = db
        .import_template_transaction(
            "user-attempt",
            "Recipient",
            "",
            "",
            &memory_graph("user-memory"),
            &[],
            SOURCE,
            &"b".repeat(64),
        )
        .unwrap();
    assert_eq!(count(&db, "memory_links"), 0);
    assert!(db
        .list_memories_for_context(
            &crate::db::MemoryContext {
                workflow_id: imported.workflow_id,
                working_directory: None,
            },
            false,
        )
        .unwrap()
        .iter()
        .any(|item| item.memory.id == "user-memory"));
}
