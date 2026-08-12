#[derive(Debug)]
pub(crate) struct ImportedWorkspaceEntry {
    pub(crate) path: String,
    pub(crate) mime_type: String,
    pub(crate) content: Option<Vec<u8>>,
}

fn parse_workspace_zip(bytes: &[u8]) -> AppResult<Vec<ImportedWorkspaceEntry>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| AppError::bad_request("INVALID_WORKSPACE_ZIP", "Workspace ZIP is invalid"))?;
    let mut entries = HashMap::<String, ImportedWorkspaceEntry>::new();
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| AppError::bad_request("INVALID_WORKSPACE_ZIP", "Workspace ZIP entry is invalid"))?;
        let raw = file.name().trim_end_matches('/');
        if raw.is_empty() { continue; }
        let path = validate_archive_path(raw)?;
        for parent in parent_paths(&path) { entries.entry(parent.clone()).or_insert(ImportedWorkspaceEntry { path: parent, mime_type: String::new(), content: None }); }
        if file.is_dir() { entries.entry(path.clone()).or_insert(ImportedWorkspaceEntry { path, mime_type: String::new(), content: None }); continue; }
        if file.size() > MAX_FILE_SIZE as u64 { return Err(AppError::unprocessable("SKILL_FILE_TOO_LARGE", "Skill file exceeds 20 MiB")); }
        total_size = total_size.saturating_add(file.size());
        if total_size > MAX_WORKSPACE_SIZE { return Err(AppError::unprocessable("SKILL_WORKSPACE_TOO_LARGE", "Skill workspace exceeds 100 MiB")); }
        let mut content = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut content).map_err(|_| AppError::bad_request("INVALID_WORKSPACE_ZIP", "Workspace ZIP entry cannot be read"))?;
        if entries.insert(path.clone(), ImportedWorkspaceEntry { mime_type: content_type_for_path(&path), path, content: Some(content) }).is_some() { return Err(AppError::bad_request("SKILL_DUPLICATE_PATH", "Workspace ZIP contains duplicate paths")); }
    }
    if entries.len() > MAX_ENTRIES as usize { return Err(AppError::unprocessable("SKILL_ENTRY_LIMIT", "Skill workspace contains more than 1000 entries")); }
    if !matches!(entries.get("SKILL.md"), Some(entry) if entry.content.is_some()) { return Err(AppError::unprocessable("SKILL_ROOT_REQUIRED", "Workspace ZIP must contain a root SKILL.md file")); }
    let mut values = entries.into_values().collect::<Vec<_>>();
    values.sort_by_key(|entry| (entry.path.split('/').count(), entry.content.is_some(), entry.path.clone()));
    Ok(values)
}

fn build_workspace_zip(files: Vec<(String, String, Option<Vec<u8>>)>) -> AppResult<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (path, kind, content) in files {
        if kind == "directory" { writer.add_directory(format!("{path}/"), options).map_err(AppError::internal)?; }
        else { writer.start_file(path, options).map_err(AppError::internal)?; writer.write_all(content.as_deref().unwrap_or_default()).map_err(AppError::internal)?; }
    }
    Ok(writer.finish().map_err(AppError::internal)?.into_inner())
}
