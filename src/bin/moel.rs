use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::ExitCode;

use moel::parse;
use moel::schema::{parse_schema, schema_path_for, validate};

const EXIT_USAGE_OR_IO: u8 = 2;
const EXIT_DOCUMENT: u8 = 3;
const EXIT_SCHEMA: u8 = 4;
const EXIT_VALIDATION: u8 = 5;

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<String, CliError> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [command, path] if command == OsStr::new("check") => check(Path::new(path)),
        _ => Err(CliError::Usage),
    }
}

fn check(document_path: &Path) -> Result<String, CliError> {
    let document_source = read(document_path, FileKind::Document)?;
    let document = parse(&document_source).map_err(|error| CliError::DocumentParse {
        path: document_path.display().to_string(),
        message: error.to_string(),
    })?;

    let Some(schema_path) = schema_path_for(document_path) else {
        return Ok(format!("{}: valid MOEL", document_path.display()));
    };

    match fs::symlink_metadata(&schema_path) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(format!(
                "{}: valid MOEL (no schema.moel)",
                document_path.display()
            ));
        }
        Err(error) => {
            return Err(CliError::Io {
                path: schema_path.display().to_string(),
                kind: FileKind::Schema,
                message: error.to_string(),
            });
        }
    }

    let schema_source = read(&schema_path, FileKind::Schema)?;
    let schema = parse_schema(&schema_source).map_err(|error| CliError::SchemaParse {
        path: schema_path.display().to_string(),
        message: error.to_string(),
    })?;
    let diagnostics = validate(&document, &schema);

    if diagnostics.is_empty() {
        Ok(format!(
            "{}: valid MOEL; schema {} passed",
            document_path.display(),
            schema_path.display()
        ))
    } else {
        Err(CliError::Validation {
            document_path: document_path.display().to_string(),
            schema_path: schema_path.display().to_string(),
            diagnostics: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum FileKind {
    Document,
    Schema,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document => f.write_str("document"),
            Self::Schema => f.write_str("schema"),
        }
    }
}

fn read(path: &Path, kind: FileKind) -> Result<String, CliError> {
    fs::read_to_string(path).map_err(|error| CliError::Io {
        path: path.display().to_string(),
        kind,
        message: error.to_string(),
    })
}

#[derive(Debug)]
enum CliError {
    Usage,
    Io {
        path: String,
        kind: FileKind,
        message: String,
    },
    DocumentParse {
        path: String,
        message: String,
    },
    SchemaParse {
        path: String,
        message: String,
    },
    Validation {
        document_path: String,
        schema_path: String,
        diagnostics: Vec<String>,
    },
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage | Self::Io { .. } => EXIT_USAGE_OR_IO,
            Self::DocumentParse { .. } => EXIT_DOCUMENT,
            Self::SchemaParse { .. } => EXIT_SCHEMA,
            Self::Validation { .. } => EXIT_VALIDATION,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => write!(f, "usage: moel check <file>"),
            Self::Io {
                path,
                kind,
                message,
            } => write!(f, "{path}: could not read {kind}: {message}"),
            Self::DocumentParse { path, message } => {
                write!(f, "{path}: document parse error: {message}")
            }
            Self::SchemaParse { path, message } => write!(f, "{path}: {message}"),
            Self::Validation {
                document_path,
                schema_path,
                diagnostics,
            } => {
                write!(
                    f,
                    "{document_path}: document failed schema validation against {schema_path}"
                )?;
                for diagnostic in diagnostics {
                    write!(f, "\n- {diagnostic}")?;
                }
                Ok(())
            }
        }
    }
}
