//! Every request of the sample suite (`samples/workspace`) must be canonically formatted, so a
//! hand written file never produces a diff the first time Postino saves it.
//! See `docs/sample-suite.md`.
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use postino_format::{env, parse, serialize};
use pretty_assertions::assert_eq;

fn samples_workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/workspace")
}

fn files_with_extension(dir: &Path, extension: &str, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            files_with_extension(&path, extension, found);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            found.push(path);
        }
    }
}

#[test]
fn every_sample_request_round_trips() {
    let mut files = Vec::new();
    files_with_extension(&samples_workspace(), "postino", &mut files);
    assert!(files.len() > 100, "found only {} requests", files.len());
    for path in files {
        let text = fs::read_to_string(&path).expect("readable request");
        let request = parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            serialize(&request),
            text,
            "{} is not canonical",
            path.display()
        );
    }
}

#[test]
fn every_sample_environment_parses() {
    let mut files = Vec::new();
    files_with_extension(&samples_workspace(), "env", &mut files);
    assert!(files.len() >= 2);
    for path in files {
        let text = fs::read_to_string(&path).expect("readable environment");
        env::parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
}
