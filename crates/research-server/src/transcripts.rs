use research_core::{ReadRequest, Thread};
use research_store::Store;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn file(path: PathBuf) -> Value {
    let Ok(path) = std::fs::canonicalize(path) else {
        return Value::Null;
    };
    if !path.is_file() {
        return Value::Null;
    }
    let path = PathBuf::from(
        path.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string(),
    );
    json!({"path":path})
}

pub fn managed(store: &Store, dir: &Path, thread: &Thread) -> Value {
    let error = store
        .export_thread(thread, dir)
        .err()
        .map(|e| e.to_string());
    let folder = dir.join("transcripts").join(&thread.id);
    json!({"scope":"captured_managed_thread","source_url":thread.url,
        "markdown":file(folder.join("thread.md")),"error":error})
}

pub fn imported(store: &Store, dir: &Path, request: &ReadRequest) -> Value {
    let error = store
        .checkpoint_read(request, dir)
        .err()
        .map(|e| e.to_string());
    let mut summary = request.summary();
    for (index, result) in summary["results"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let folder = dir.join("imports").join(&request.id);
        result["local_transcript"] = json!({"scope":"captured_import_snapshot",
            "markdown":file(folder.join(format!("{index}.md"))),"error":error});
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_paths_only_reference_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat #1 ü.md");
        assert!(file(path.clone()).is_null());
        std::fs::write(&path, "transcript").unwrap();
        let metadata = file(path);
        assert!(metadata.get("url").is_none());
        assert!(Path::new(metadata["path"].as_str().unwrap()).is_absolute());
        assert_eq!(
            std::fs::read_to_string(metadata["path"].as_str().unwrap()).unwrap(),
            "transcript"
        );
    }
}
