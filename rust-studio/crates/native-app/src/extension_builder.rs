//! Offline authoring of a Tauri-compatible `.oppx` package. This module
//! packages source; it never evaluates JavaScript in the PDF process.
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::Path;

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 3 * 1024 * 1024;

pub(crate) struct Draft {
    pub id: String,
    pub name: String,
    pub html: String,
    pub javascript: String,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        && id.as_bytes()[0].is_ascii_alphabetic()
}

/// Import only the three-file source package this editor can round-trip.
/// Refusing extra files avoids silently dropping extension resources.
pub(crate) fn import(path: &Path) -> Result<Draft, String> {
    if path.extension().and_then(|value| value.to_str()) != Some("oppx") {
        return Err("Import path must end in .oppx".into());
    }
    let file = fs::File::open(path).map_err(|error| format!("Cannot open package: {error}"))?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    if size > MAX_PACKAGE_BYTES {
        return Err("Package exceeds 3 MiB import limit".into());
    }
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    if archive.len() != 3 {
        return Err("This editor only imports three-file source packages".into());
    }
    let mut seen = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if !matches!(entry.name(), "plugin.json" | "index.html" | "index.js")
            || !seen.insert(entry.name().to_owned())
        {
            return Err("Unexpected or duplicate package entry".into());
        }
    }
    let read = |archive: &mut zip::ZipArchive<fs::File>, name: &str, max: usize| {
        let entry = archive.by_name(name).map_err(|error| error.to_string())?;
        if entry.size() > max as u64 {
            return Err(format!("{name} exceeds its size limit"));
        }
        let mut contents = String::new();
        entry
            .take(max as u64 + 1)
            .read_to_string(&mut contents)
            .map_err(|error| format!("Cannot read {name}: {error}"))?;
        if contents.len() > max {
            return Err(format!("{name} exceeds its size limit"));
        }
        Ok::<String, String>(contents)
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&read(&mut archive, "plugin.json", 16 * 1024)?)
            .map_err(|error| format!("Invalid manifest: {error}"))?;
    let own_manifest = manifest.as_object().is_some_and(|fields| {
        fields.len() == 4
            && fields.contains_key("id")
            && fields.contains_key("name")
            && fields.get("version").and_then(serde_json::Value::as_str) == Some("0.1.0")
            && fields
                .get("description")
                .and_then(serde_json::Value::as_str)
                == Some("Built with Open PDF Studio Rust")
    });
    if !own_manifest {
        return Err("This editor only imports packages it created without losing metadata".into());
    }
    let id = manifest["id"].as_str().unwrap_or_default();
    let name = manifest["name"].as_str().unwrap_or_default();
    if !valid_id(id) || name.is_empty() || name.len() > 120 || name.chars().any(char::is_control) {
        return Err("Invalid extension ID or name".into());
    }
    Ok(Draft {
        id: id.to_owned(),
        name: name.to_owned(),
        html: read(&mut archive, "index.html", MAX_SOURCE_BYTES)?,
        javascript: read(&mut archive, "index.js", MAX_SOURCE_BYTES)?,
    })
}

pub(crate) fn build(
    destination: &Path,
    id: &str,
    name: &str,
    html: &str,
    javascript: &str,
) -> Result<(), String> {
    if !valid_id(id) {
        return Err(
            "Extension ID must start with a letter and contain only letters, digits, - or _".into(),
        );
    }
    let name = name.trim();
    if name.is_empty() || name.len() > 120 || name.chars().any(char::is_control) {
        return Err("Extension name must be 1–120 printable characters".into());
    }
    if html.len() > MAX_SOURCE_BYTES || javascript.len() > MAX_SOURCE_BYTES {
        return Err("HTML and JavaScript must each be at most 1 MiB".into());
    }
    if destination.extension().and_then(|v| v.to_str()) != Some("oppx") {
        return Err("Output path must end in .oppx".into());
    }
    let manifest = serde_json::json!({
        "id": id,
        "name": name,
        "version": "0.1.0",
        "description": "Built with Open PDF Studio Rust",
    });
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Cannot create package: {error}"))?;
    let result = write_package(file, &manifest.to_string(), html, javascript);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn write_package<W: Write + Seek>(
    output: W,
    manifest: &str,
    html: &str,
    javascript: &str,
) -> Result<(), String> {
    let mut zip = zip::ZipWriter::new(output);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, contents) in [
        ("plugin.json", manifest),
        ("index.js", javascript),
        ("index.html", html),
    ] {
        zip.start_file(name, options)
            .map_err(|error| format!("Cannot write {name}: {error}"))?;
        zip.write_all(contents.as_bytes())
            .map_err(|error| format!("Cannot write {name}: {error}"))?;
    }
    zip.finish()
        .map_err(|error| format!("Cannot finish package: {error}"))?;
    Ok(())
}

pub(crate) fn default_html() -> &'static str {
    "<!doctype html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>Extension</title></head>\n<body><h1>My extension</h1></body>\n</html>\n"
}

pub(crate) fn default_javascript() -> &'static str {
    "// Edit this module and build an .oppx package.\nexport function activate(api) {\n  console.log('Extension activated', api);\n}\nexport function deactivate() {}\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn package_has_tauri_entries_and_preserves_sources() {
        let path =
            std::env::temp_dir().join(format!("opds-extension-test-{}.oppx", std::process::id()));
        let _ = fs::remove_file(&path);
        build(
            &path,
            "sample_ext",
            "Sample",
            "<p>ok</p>",
            "export function activate() {}",
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        for (entry, expected) in [
            ("index.html", "<p>ok</p>"),
            ("index.js", "export function activate() {}"),
        ] {
            let mut actual = String::new();
            zip.by_name(entry)
                .unwrap()
                .read_to_string(&mut actual)
                .unwrap();
            assert_eq!(actual, expected);
        }
        let mut manifest = String::new();
        zip.by_name("plugin.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["id"],
            "sample_ext"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_path_traversal_and_existing_output() {
        let path = std::env::temp_dir().join(format!(
            "opds-extension-existing-{}.oppx",
            std::process::id()
        ));
        fs::write(&path, b"existing").unwrap();
        assert!(build(&path, "../bad", "X", "", "").is_err());
        assert!(build(&path, "valid", "X", "", "").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn imports_own_package_without_losing_source() {
        let path =
            std::env::temp_dir().join(format!("opds-extension-import-{}.oppx", std::process::id()));
        let _ = fs::remove_file(&path);
        build(
            &path,
            "roundtrip",
            "Roundtrip",
            "<p>html</p>",
            "export const x = 1;",
        )
        .unwrap();
        let draft = import(&path).unwrap();
        assert_eq!(draft.id, "roundtrip");
        assert_eq!(draft.name, "Roundtrip");
        assert_eq!(draft.html, "<p>html</p>");
        assert_eq!(draft.javascript, "export const x = 1;");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn import_refuses_manifest_metadata_it_cannot_roundtrip() {
        let path = std::env::temp_dir().join(format!(
            "opds-extension-rich-manifest-{}.oppx",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let file = fs::File::create(&path).unwrap();
        write_package(
            file,
            r#"{"id":"rich","name":"Rich","version":"2.0.0","description":"custom"}"#,
            "<html></html>",
            "export function activate() {}",
        )
        .unwrap();
        assert!(import(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
