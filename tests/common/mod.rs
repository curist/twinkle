#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureExpectation {
    Output {
        stdout: Vec<String>,
        stderr: Vec<String>,
    },
    Trap {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFixture {
    pub path: PathBuf,
    pub expectation: FixtureExpectation,
}

pub fn discover_run_fixtures(root: &Path) -> Vec<RunFixture> {
    let mut files = Vec::new();
    collect_tw_files(root, &mut files);
    files.sort();

    let mut out = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
        if let Some(expectation) = parse_fixture_expectation(&source) {
            out.push(RunFixture { path, expectation });
        }
    }
    out
}

pub fn load_run_fixture(path: &Path) -> RunFixture {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    let expectation = parse_fixture_expectation(&source)
        .unwrap_or_else(|| panic!("missing expected output/trap block in {}", path.display()));
    RunFixture {
        path: path.to_path_buf(),
        expectation,
    }
}

pub fn run_wasm_capture(path: &Path) -> anyhow::Result<(String, String)> {
    let path_text = path.to_string_lossy();
    twinkle::cli::run_wasm::run_wasm_capture(path_text.as_ref())
}

pub fn assert_wasm_fixture(path: &Path) {
    let fixture = load_run_fixture(path);
    match fixture.expectation {
        FixtureExpectation::Output { stdout, stderr } => {
            let (actual_stdout, actual_stderr) = run_wasm_capture(path)
                .unwrap_or_else(|e| panic!("wasm run failed for {}: {e}", path.display()));
            assert_output_lines(path, "stdout", &actual_stdout, &stdout);
            assert_output_lines(path, "stderr", &actual_stderr, &stderr);
        }
        FixtureExpectation::Trap { .. } => {
            run_wasm_capture(path)
                .expect_err(&format!("expected wasm trap for {}", path.display()));
        }
    }
}

fn parse_fixture_expectation(source: &str) -> Option<FixtureExpectation> {
    if let Some(message) = source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("// Expected trap: ")
            .map(str::to_string)
    }) {
        return Some(FixtureExpectation::Trap { message });
    }

    let stdout = parse_expected_block(source, "// Expected output:");
    let stderr = parse_expected_block(source, "// Expected stderr:");
    if stdout.is_empty() && stderr.is_empty() {
        None
    } else {
        Some(FixtureExpectation::Output { stdout, stderr })
    }
}

fn parse_expected_block(source: &str, header: &str) -> Vec<String> {
    let mut lines = source.lines();
    let found = lines.by_ref().any(|line| line.trim() == header);
    if !found {
        return vec![];
    }

    let mut out = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("//   ") {
            out.push(rest.to_string());
            continue;
        }
        if line.trim() == "//" {
            out.push(String::new());
            continue;
        }
        break;
    }
    while out.last().is_some_and(|line| line.is_empty()) {
        out.pop();
    }
    out
}

fn assert_output_lines(path: &Path, stream: &str, actual: &str, expected: &[String]) {
    if expected.is_empty() {
        assert!(
            actual.is_empty(),
            "{}: expected empty {}, got:\n{}",
            path.display(),
            stream,
            actual
        );
        return;
    }

    let actual_lines: Vec<&str> = actual.lines().collect();
    assert_eq!(
        actual_lines,
        expected,
        "{}: {} mismatch\nExpected:\n{}\nActual:\n{}",
        path.display(),
        stream,
        expected.join("\n"),
        actual
    );
}

fn collect_tw_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| panic!("failed to enumerate directory {}: {e}", dir.display()));
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_tw_files(path.as_path(), out);
        } else if path.extension().is_some_and(|ext| ext == "tw") {
            out.push(path);
        }
    }
}
