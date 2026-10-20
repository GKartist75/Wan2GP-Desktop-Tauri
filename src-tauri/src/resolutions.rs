//! Custom Wan2GP resolutions.
//!
//! Wan2GP loads `resolutions.json` from its working directory (next to
//! `wgp.py`) and appends the entries to its built-in resolution picker. The
//! file is a JSON array of two-element arrays: a display label and a
//! `"WIDTHxHEIGHT"` value. This module is the launcher's safe editor for that
//! file: it validates every row before writing, never accepts a path, and
//! leaves Wan2GP's own defaults untouched.
use crate::base::*;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;

/// The file Wan2GP's `shared.resolutions` module looks for. It is resolved
/// relative to the process working directory, which the launcher sets to the
/// repo root (the folder containing `wgp.py`).
fn resolutions_path() -> PathBuf {
    get_repo_dir().join("resolutions.json")
}

fn parse_dimensions(value: &str) -> Result<(u32, u32), String> {
    let (w, h) = value
        .split_once('x')
        .ok_or_else(|| format!("\"{value}\" is not WIDTHxHEIGHT (for example 1600x1200)"))?;
    if w.is_empty() || h.is_empty() || w.starts_with('0') || h.starts_with('0') {
        return Err(format!(
            "\"{value}\" is not WIDTHxHEIGHT with positive, non-zero numbers"
        ));
    }
    let w: u32 = w
        .parse()
        .map_err(|_| format!("\"{value}\" has a non-numeric width"))?;
    let h: u32 = h
        .parse()
        .map_err(|_| format!("\"{value}\" has a non-numeric height"))?;
    // Keep obvious paste accidents out without imposing Wan2GP's model limits:
    // those depend on the selected model and are checked by Wan2GP itself.
    if w == 0 || h == 0 || w > 65535 || h > 65535 {
        return Err(format!(
            "\"{value}\" is outside the supported 1..65535 range"
        ));
    }
    Ok((w, h))
}

/// Validate Wan2GP's `[[label, "WxH"], ...]` shape. Returns the normalized rows
/// (labels trimmed, sizes lower-cased) so the writer and reader agree.
fn validate_resolutions(value: &Value) -> Result<Vec<(String, String)>, String> {
    let rows = value
        .as_array()
        .ok_or("resolutions must be a JSON array of [label, \"WxH\"] pairs")?;
    if rows.len() > 500 {
        return Err("too many resolutions (the limit is 500)".into());
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut labels: HashSet<String> = HashSet::new();
    let mut sizes: HashSet<String> = HashSet::new();
    for (i, row) in rows.iter().enumerate() {
        let n = i + 1;
        let pair = row
            .as_array()
            .ok_or_else(|| format!("row {n} must be a [label, \"WxH\"] pair"))?;
        if pair.len() != 2 {
            return Err(format!("row {n} must contain exactly a label and a size"));
        }
        let label = pair[0]
            .as_str()
            .ok_or_else(|| format!("row {n} label must be text"))?
            .trim();
        if label.is_empty() {
            return Err(format!("row {n} needs a label"));
        }
        if label.chars().count() > 200 {
            return Err(format!("row {n} label is longer than 200 characters"));
        }
        let size = pair[1]
            .as_str()
            .ok_or_else(|| format!("row {n} size must be text like \"1600x1200\""))?
            .trim()
            .to_ascii_lowercase();
        parse_dimensions(&size).map_err(|e| format!("row {n}: {e}"))?;
        if !labels.insert(label.to_lowercase()) {
            return Err(format!("duplicate label \"{label}\""));
        }
        if !sizes.insert(size.clone()) {
            return Err(format!(
                "duplicate resolution \"{size}\" — Wan2GP keeps only the first entry for a size"
            ));
        }
        out.push((label.to_string(), size));
    }
    Ok(out)
}

fn rows_to_json(rows: &[(String, String)]) -> Value {
    Value::Array(
        rows.iter()
            .map(|(label, size)| json!([label, size]))
            .collect(),
    )
}

/// Wan2GP accepts any JSON formatting, but the documented shape is one pair per
/// line. Keeping that shape makes a hand edit and a diff readable.
fn format_resolutions(rows: &[(String, String)]) -> Result<String, String> {
    let mut out = String::from("[\n");
    for (i, (label, size)) in rows.iter().enumerate() {
        let label = serde_json::to_string(label).map_err(|e| e.to_string())?;
        let size = serde_json::to_string(size).map_err(|e| e.to_string())?;
        out.push_str(&format!("    [{label}, {size}]"));
        if i + 1 != rows.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("]\n");
    Ok(out)
}

/// Read the current custom list without failing when the file is absent.
/// `ok: false` means the file exists but is invalid; the UI can still offer a
/// deliberate overwrite via `custom_resolutions_write`.
#[tauri::command]
pub fn custom_resolutions_read() -> Value {
    let path = resolutions_path();
    let path_text = path.to_string_lossy().to_string();
    let installed = get_repo_dir().join("wgp.py").is_file();
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({
            "ok": true,
            "path": path_text,
            "exists": false,
            "installed": installed,
            "resolutions": [],
        }),
        Err(e) => json!({
            "ok": false,
            "path": path_text,
            "exists": false,
            "installed": installed,
            "resolutions": [],
            "error": format!("cannot read {path_text}: {e}"),
        }),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Err(e) => json!({
                "ok": false,
                "path": path_text,
                "exists": true,
                "installed": installed,
                "resolutions": [],
                "error": format!("resolutions.json is not valid JSON: {e}"),
            }),
            Ok(value) => match validate_resolutions(&value) {
                Ok(rows) => json!({
                    "ok": true,
                    "path": path_text,
                    "exists": true,
                    "installed": installed,
                    "resolutions": rows_to_json(&rows),
                }),
                Err(e) => json!({
                    "ok": false,
                    "path": path_text,
                    "exists": true,
                    "installed": installed,
                    "resolutions": [],
                    "error": e,
                }),
            },
        },
    }
}

/// Replace `<repo>/resolutions.json` with the supplied list. This is the only
/// write path: no caller can choose a different file, and an invalid list is
/// rejected before the existing file is touched.
#[tauri::command]
pub fn custom_resolutions_write(resolutions: Value) -> Result<Value, String> {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").is_file() {
        return Err(
            "Wan2GP is not installed yet — install it first, then add custom resolutions.".into(),
        );
    }
    let rows = validate_resolutions(&resolutions)?;
    let path = repo.join("resolutions.json");
    let text = format_resolutions(&rows)?;
    atomic_write(&path, &text)
        .map_err(|e| format!("cannot write {}: {e}", path.to_string_lossy()))?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "count": rows.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::TestDataDir;

    fn with_install() -> TestDataDir {
        let d = TestDataDir::new();
        std::fs::write(get_repo_dir().join("wgp.py"), "# test").expect("seed wgp.py");
        d
    }

    fn example() -> Value {
        json!([
            ["1600x1200 (4:3, 1080p)", "1600x1200"],
            ["1200x1600 (3:4, 1080p)", "1200x1600"]
        ])
    }

    #[test]
    fn accepts_the_wan2gp_pair_shape() {
        let rows = validate_resolutions(&example()).unwrap();
        assert_eq!(rows[0].0, "1600x1200 (4:3, 1080p)");
        assert_eq!(rows[0].1, "1600x1200");
    }

    #[test]
    fn rejects_bad_shapes_and_sizes() {
        assert!(validate_resolutions(&json!({"label": "x"})).is_err());
        assert!(validate_resolutions(&json!([["only a label"]])).is_err());
        assert!(validate_resolutions(&json!([["x", "1600"]])).is_err());
        assert!(validate_resolutions(&json!([["x", "0x1200"]])).is_err());
        assert!(validate_resolutions(&json!([["x", "1600x0"]])).is_err());
        assert!(validate_resolutions(&json!([["x", "1600x1200x2"]])).is_err());
        assert!(validate_resolutions(&json!([["x", "1920x1080"], ["y", "1920x1080"]])).is_err());
    }

    #[test]
    fn rejects_duplicate_labels_and_blank_labels() {
        assert!(
            validate_resolutions(&json!([["same", "1600x1200"], ["SAME", "1200x1600"]])).is_err()
        );
        assert!(validate_resolutions(&json!([["   ", "1600x1200"]])).is_err());
    }

    #[test]
    fn writes_one_pair_per_line_in_the_documented_shape() {
        let rows = validate_resolutions(&example()).unwrap();
        let text = format_resolutions(&rows).unwrap();
        assert!(text.starts_with("[\n    [\"1600x1200 (4:3, 1080p)\", \"1600x1200\"],\n"));
        assert!(text.ends_with("    [\"1200x1600 (3:4, 1080p)\", \"1200x1600\"]\n]\n"));
        // Empty is the clear-everything case and must still be valid JSON.
        assert_eq!(format_resolutions(&[]).unwrap(), "[\n]\n");
    }

    #[test]
    fn round_trips_to_the_repo_root() {
        let _data = with_install();
        let written = custom_resolutions_write(example()).expect("write");
        assert_eq!(written["ok"], true);
        let path = PathBuf::from(written["path"].as_str().unwrap());
        assert_eq!(path.parent(), Some(get_repo_dir().as_path()));
        assert_eq!(path.file_name().unwrap(), "resolutions.json");

        let read = custom_resolutions_read();
        assert_eq!(read["ok"], true);
        assert_eq!(read["exists"], true);
        assert_eq!(read["installed"], true);
        assert_eq!(read["resolutions"], example());

        // Wan2GP's own parser lower-cases values; the written file must too.
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved[0][1], "1600x1200");
    }

    #[test]
    fn missing_file_is_fine_but_invalid_json_is_reported() {
        let _data = with_install();
        let read = custom_resolutions_read();
        assert_eq!(read["ok"], true);
        assert_eq!(read["exists"], false);

        std::fs::write(resolutions_path(), "{not json").unwrap();
        let read = custom_resolutions_read();
        assert_eq!(read["ok"], false);
        assert!(read["error"].as_str().unwrap().contains("not valid JSON"));
    }

    #[test]
    fn refuses_to_write_before_wan2gp_is_installed() {
        let _data = TestDataDir::new();
        assert!(custom_resolutions_write(example()).is_err());
        assert!(!resolutions_path().exists());
    }
}
