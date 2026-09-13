use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after Unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("moel-cli-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("temporary test directory must be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: impl AsRef<Path>, source: &str) {
    fs::write(path, source).expect("test fixture must be writable");
}

fn check(path: impl AsRef<Path>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moel"))
        .arg("check")
        .arg(path.as_ref())
        .output()
        .expect("moel CLI must run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn check_accepts_schema_less_moel() {
    let dir = TempDir::new("schema-less");
    let document = dir.path().join("config.moel");
    write(
        &document,
        "id = uuid\"550e8400-e29b-41d4-a716-446655440000\"\n",
    );

    let output = check(&document);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("valid MOEL (no schema.moel)"));
}

#[test]
fn check_loads_only_the_sibling_schema() {
    let dir = TempDir::new("sibling-only");
    let nested = dir.path().join("nested");
    fs::create_dir_all(&nested).expect("nested fixture directory must be creatable");
    write(dir.path().join("schema.moel"), "name = \"integer\"\n");
    let document = nested.join("config.moel");
    write(&document, "name = \"MOEL\"\n");

    let output = check(&document);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("valid MOEL (no schema.moel)"));
}

#[test]
fn check_validates_against_the_sibling_schema() {
    let dir = TempDir::new("valid-schema");
    let document = dir.path().join("config.moel");
    write(
        &document,
        "id = uuid\"550e8400-e29b-41d4-a716-446655440000\"\nstatus = \"draft\"\n",
    );
    write(
        dir.path().join("schema.moel"),
        "id = \"uuid\"\nstatus = { enum = [\"draft\", \"published\"] }\n",
    );

    let output = check(&document);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("schema"));
    assert!(stdout(&output).contains("passed"));
}

#[test]
fn check_reports_document_parse_failures_separately() {
    let dir = TempDir::new("document-error");
    let document = dir.path().join("config.moel");
    write(&document, "id = uuid\"not-a-uuid\"\n");

    let output = check(&document);

    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("document parse error"));
    assert!(stderr(&output).contains("invalid UUID literal"));
}

#[test]
fn check_reports_schema_parse_failures_separately() {
    let dir = TempDir::new("schema-error");
    let document = dir.path().join("config.moel");
    write(&document, "name = \"MOEL\"\n");
    write(dir.path().join("schema.moel"), "name = \"any\"\n");

    let output = check(&document);

    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("unknown schema type"));
}

#[test]
fn check_reports_validation_failures_with_diagnostics() {
    let dir = TempDir::new("validation-error");
    let document = dir.path().join("config.moel");
    write(&document, "count = \"many\"\n");
    write(dir.path().join("schema.moel"), "count = \"integer\"\n");

    let output = check(&document);

    assert_eq!(output.status.code(), Some(5));
    assert!(stderr(&output).contains("failed schema validation"));
    assert!(stderr(&output).contains("$[\"count\"]: expected integer, found string"));
}

#[test]
fn checking_schema_moel_does_not_recursively_discover_itself() {
    let dir = TempDir::new("schema-self");
    let schema = dir.path().join("schema.moel");
    write(&schema, "name = \"string\"\n");

    let output = check(&schema);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("valid MOEL"));
    assert!(!stdout(&output).contains("passed"));
}

#[test]
fn unsupported_invocation_returns_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_moel"))
        .arg("unknown")
        .output()
        .expect("moel CLI must run");

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("usage: moel check <file>"));
}
