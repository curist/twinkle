use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use url::Url;

use crate::lsp::definition::definition_at_workspace;
use crate::lsp::position::{PositionUtf16, file_byte_offset_to_position_utf16};
use crate::lsp::session::AnalysisSession;

pub fn cmd_lsp() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());

    let mut state = LspState::default();

    loop {
        let Some(message) = read_lsp_message(&mut reader)? else {
            break;
        };

        let payload: Value = match serde_json::from_str(&message) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let should_exit = handle_lsp_message(&mut state, &payload, &mut writer)?;
        writer.flush()?;
        if should_exit {
            break;
        }
    }

    Ok(())
}

#[derive(Default)]
struct LspState {
    session: Option<AnalysisSession>,
    shutdown_requested: bool,
}

fn handle_lsp_message(
    state: &mut LspState,
    payload: &Value,
    writer: &mut impl Write,
) -> Result<bool> {
    let Some(method) = payload.get("method").and_then(Value::as_str) else {
        return Ok(false);
    };
    let id = payload.get("id").cloned();
    let params = payload.get("params");

    match method {
        "initialize" => match initialize_session(params) {
            Ok(session) => {
                state.session = Some(session);
                state.shutdown_requested = false;

                if let Some(id) = id {
                    let result = json!({
                        "capabilities": {
                            "textDocumentSync": 1,
                            "hoverProvider": true,
                            "definitionProvider": true
                        },
                        "serverInfo": {
                            "name": "twk"
                        }
                    });
                    write_response(writer, id, result)?;
                }
            }
            Err(err) => {
                if let Some(id) = id {
                    write_error_response(writer, id, -32002, &err.to_string())?;
                }
            }
        },
        "initialized" => {}
        "shutdown" => {
            state.shutdown_requested = true;
            if let Some(id) = id {
                write_response(writer, id, Value::Null)?;
            }
        }
        "exit" => {
            return Ok(true);
        }
        "textDocument/didOpen" => {
            if let (Some(session), Some(params)) = (state.session.as_mut(), params) {
                if let (Some(path), Some(text)) = (
                    extract_text_document_path(params),
                    params
                        .get("textDocument")
                        .and_then(|td| td.get("text"))
                        .and_then(Value::as_str),
                ) {
                    session.did_open(path, text.to_string());
                }
            }
        }
        "textDocument/didChange" => {
            if let (Some(session), Some(params)) = (state.session.as_mut(), params) {
                if let Some(path) = extract_text_document_path(params) {
                    let text = params
                        .get("contentChanges")
                        .and_then(Value::as_array)
                        .and_then(|changes| changes.last())
                        .and_then(|change| change.get("text"))
                        .and_then(Value::as_str);
                    if let Some(text) = text {
                        session.did_change(path, text.to_string());
                    }
                }
            }
        }
        "textDocument/didClose" => {
            if let (Some(session), Some(params)) = (state.session.as_mut(), params) {
                if let Some(path) = extract_text_document_path(params) {
                    session.did_close(path);
                }
            }
        }
        "textDocument/hover" => {
            if let Some(id) = id {
                let response = match state.session.as_ref() {
                    Some(session) => handle_hover_request(session, params).unwrap_or(Value::Null),
                    None => Value::Null,
                };
                write_response(writer, id, response)?;
            }
        }
        "textDocument/definition" => {
            if let Some(id) = id {
                let response = match state.session.as_ref() {
                    Some(session) => {
                        handle_definition_request(session, params).unwrap_or(Value::Null)
                    }
                    None => Value::Null,
                };
                write_response(writer, id, response)?;
            }
        }
        _ => {
            if let Some(id) = id {
                write_error_response(writer, id, -32601, "Method not found")?;
            }
        }
    }

    Ok(false)
}

fn handle_hover_request(session: &AnalysisSession, params: Option<&Value>) -> Result<Value> {
    let Some(params) = params else {
        return Ok(Value::Null);
    };
    let Some(path) = extract_text_document_path(params) else {
        return Ok(Value::Null);
    };
    let Some(position) = extract_position(params) else {
        return Ok(Value::Null);
    };

    let hover = session.hover(&path, &path, position)?;
    if let Some(contents) = hover {
        Ok(json!({
            "contents": {
                "kind": "plaintext",
                "value": contents
            }
        }))
    } else {
        Ok(Value::Null)
    }
}

fn handle_definition_request(session: &AnalysisSession, params: Option<&Value>) -> Result<Value> {
    let Some(params) = params else {
        return Ok(Value::Null);
    };
    let Some(path) = extract_text_document_path(params) else {
        return Ok(Value::Null);
    };
    let Some(position) = extract_position(params) else {
        return Ok(Value::Null);
    };

    let analysis = session.analyze_entry(&path)?;
    let Some(target) = definition_at_workspace(&analysis, &path, position) else {
        return Ok(Value::Null);
    };
    let Some(target_module) = analysis.modules.get(&target.path) else {
        return Ok(Value::Null);
    };

    let start = file_byte_offset_to_position_utf16(
        &target_module.file_registry,
        target.span.file_id,
        target.span.start,
    );
    let end = file_byte_offset_to_position_utf16(
        &target_module.file_registry,
        target.span.file_id,
        target.span.end,
    );

    let (Some(start), Some(end)) = (start, end) else {
        return Ok(Value::Null);
    };

    let uri = path_to_file_uri(&target.path)?;
    Ok(json!({
        "uri": uri,
        "range": {
            "start": { "line": start.line, "character": start.character },
            "end": { "line": end.line, "character": end.character }
        }
    }))
}

fn extract_text_document_path(params: &Value) -> Option<PathBuf> {
    let uri = params
        .get("textDocument")
        .and_then(|td| td.get("uri"))
        .and_then(Value::as_str)?;
    file_uri_to_path(uri).ok()
}

fn extract_position(params: &Value) -> Option<PositionUtf16> {
    let position = params.get("position")?;
    let line = position.get("line")?.as_u64()?;
    let character = position.get("character")?.as_u64()?;
    Some(PositionUtf16::new(line as u32, character as u32))
}

fn initialize_root_path(params: Option<&Value>) -> Result<PathBuf> {
    if let Some(params) = params {
        if let Some(uri) = params.get("rootUri").and_then(Value::as_str) {
            return file_uri_to_path(uri);
        }
        if let Some(folder_uri) = params
            .get("workspaceFolders")
            .and_then(Value::as_array)
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.get("uri"))
            .and_then(Value::as_str)
        {
            return file_uri_to_path(folder_uri);
        }
    }
    std::env::current_dir().map_err(|e| anyhow!("failed to read cwd: {e}"))
}

fn initialize_session(params: Option<&Value>) -> Result<AnalysisSession> {
    let requested_root = initialize_root_path(params)?;
    let requested_root = canonicalize_or_self(&requested_root);
    let project_root = canonicalize_or_self(&crate::module::find_project_root(&requested_root));

    let stdlib_root = detect_stdlib_root(&project_root);
    let prelude_root = stdlib_root
        .parent()
        .map(|p| p.join("prelude"))
        .unwrap_or_else(|| stdlib_root.join("../prelude"));
    let source_roots = dedup_roots(vec![
        project_root.clone(),
        stdlib_root.clone(),
        prelude_root,
    ]);
    let base_sources = load_tw_sources_from_roots(&source_roots)?;
    Ok(AnalysisSession::new(
        &project_root,
        &stdlib_root,
        base_sources,
    ))
}

fn detect_stdlib_root(project_root: &Path) -> PathBuf {
    let project_stdlib = project_root.join("stdlib");
    if project_stdlib.exists() {
        return canonicalize_or_self(&project_stdlib);
    }
    canonicalize_or_self(&crate::module::loader::resolve_stdlib_root_default())
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn dedup_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in roots {
        let canonical = canonicalize_or_self(&root);
        if seen.insert(canonical.clone()) {
            out.push(canonical);
        }
    }
    out
}

fn load_tw_sources_from_roots(roots: &[PathBuf]) -> Result<HashMap<PathBuf, String>> {
    let mut out = HashMap::new();
    for root in roots {
        collect_tw_sources(root, &mut out)?;
    }
    Ok(out)
}

fn collect_tw_sources(dir: &Path, out: &mut HashMap<PathBuf, String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name == "target" || name.starts_with('.'))
            {
                continue;
            }
            collect_tw_sources(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "tw") {
            let source = fs::read_to_string(&path)?;
            out.insert(path, source);
        }
    }
    Ok(())
}

fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }

        let line_trimmed = line.trim_end_matches(['\r', '\n']);
        if line_trimmed.is_empty() {
            break;
        }

        if let Some((name, value)) = line_trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }

    let Some(content_length) = content_length else {
        return Ok(None);
    };

    let mut content = vec![0u8; content_length];
    reader.read_exact(&mut content)?;
    let payload = String::from_utf8(content).map_err(|e| anyhow!("invalid UTF-8 payload: {e}"))?;
    Ok(Some(payload))
}

fn write_response(writer: &mut impl Write, id: Value, result: Value) -> Result<()> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    write_lsp_payload(writer, &payload)
}

fn write_error_response(
    writer: &mut impl Write,
    id: Value,
    code: i64,
    message: &str,
) -> Result<()> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    });
    write_lsp_payload(writer, &payload)
}

fn write_lsp_payload(writer: &mut impl Write, payload: &Value) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    Ok(())
}

fn file_uri_to_path(uri: &str) -> Result<PathBuf> {
    let parsed = Url::parse(uri).map_err(|e| anyhow!("invalid file URI '{}': {}", uri, e))?;
    parsed
        .to_file_path()
        .map_err(|_| anyhow!("cannot convert URI to file path: {}", uri))
}

fn path_to_file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map_err(|_| anyhow!("cannot convert file path to URI: {}", path.display()))
        .map(|u| u.to_string())
}
