use super::*;

fn workspace_zip(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (path, content) in entries {
        if let Some(content) = content {
            writer.start_file(*path, options).expect("start ZIP file");
            writer.write_all(content).expect("write ZIP file");
        } else {
            writer
                .add_directory(format!("{path}/"), options)
                .expect("add ZIP directory");
        }
    }
    writer.finish().expect("finish ZIP").into_inner()
}

#[test]
fn markdown_links_follow_moved_files_and_sources() {
    let content = "[guide](docs/guide.md)\n![logo](assets/logo.png)\n";
    let rewritten = rewrite_markdown_links("SKILL.md", "SKILL.md", content, "docs", "manual")
        .expect("rewrite links")
        .expect("content changed");
    assert!(rewritten.contains("(manual/guide.md)"));
    assert!(rewritten.contains("(assets/logo.png)"));
    let moved = rewrite_markdown_links(
        "docs/guide.md",
        "manual/guide.md",
        "[root](../SKILL.md)\n",
        "docs",
        "manual",
    )
    .expect("rewrite moved source")
    .expect("changed");
    assert!(moved.contains("(../SKILL.md)"));
}

#[test]
fn workspace_zip_validation() {
    let missing = workspace_zip(&[("README.md", Some(b"# Readme"))]);
    assert_eq!(
        parse_workspace_zip(&missing).unwrap_err().code,
        "SKILL_ROOT_REQUIRED"
    );
    let traversal = workspace_zip(&[("../SKILL.md", Some(b"# Unsafe"))]);
    assert_eq!(
        parse_workspace_zip(&traversal).unwrap_err().code,
        "SKILL_PATH_INVALID"
    );
    let duplicate = workspace_zip(&[
        ("SKILL.md", Some(b"# One")),
        ("docs/guide.md", Some(b"# Guide")),
        ("docs", Some(b"conflict")),
    ]);
    assert_eq!(
        parse_workspace_zip(&duplicate).unwrap_err().code,
        "SKILL_DUPLICATE_PATH"
    );
}

#[test]
fn workspace_zip_accepts_a_valid_workspace() {
    let archive = workspace_zip(&[
        (
            "SKILL.md",
            Some(b"---\nname: Skill\ndescription: Reusable instructions\n---\n\n# Skill"),
        ),
        ("docs", None),
        ("docs/guide.md", Some(b"# Guide")),
    ]);
    assert_eq!(parse_workspace_zip(&archive).expect("valid ZIP").len(), 3);
}

#[test]
fn skill_document_round_trips_and_requires_description() {
    let content = render_skill_document("browser-helper", "Use tools safely", "# Instructions\n")
        .expect("render");
    let document = parse_skill_document(&content).expect("parse");
    assert_eq!(document.metadata.name, "browser-helper");
    assert_eq!(document.metadata.description, "Use tools safely");
    assert_eq!(
        parse_skill_document("---\nname: helper\ndescription: ''\n---\n\n# Body")
            .unwrap_err()
            .code,
        "SKILL_DESCRIPTION_REQUIRED"
    );
}

#[test]
fn published_content_hash_ignores_revision_and_dependency_order() {
    let file = SkillWorkspaceEntry {
        id: Uuid::now_v7(),
        parent_id: None,
        name: "SKILL.md".to_owned(),
        path: "SKILL.md".to_owned(),
        entry_type: "file".to_owned(),
        mime_type: Some("text/markdown".to_owned()),
        artifact_id: Some(Uuid::now_v7()),
        content_hash: Some("a".repeat(64)),
        size_bytes: 42,
        editable: true,
        updated_at: OffsetDateTime::UNIX_EPOCH,
    };
    let first = SkillDependencyInput {
        resource_type: "model".to_owned(),
        resource_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        resource_version_id: None,
        operation: "use".to_owned(),
    };
    let second = SkillDependencyInput {
        resource_type: "mcp_tool".to_owned(),
        resource_id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
        resource_version_id: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()),
        operation: "invoke".to_owned(),
    };
    let initial = skill_version_content_hash(
        &json!({"name":"helper","description":"Reusable","sourceRevision":1}),
        std::slice::from_ref(&file),
        &[first.clone(), second.clone()],
    )
    .unwrap();
    let later_revision = skill_version_content_hash(
        &json!({"name":"helper","description":"Reusable","sourceRevision":99}),
        &[file],
        &[second, first],
    )
    .unwrap();
    assert_eq!(initial, later_revision);
}
