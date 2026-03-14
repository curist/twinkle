use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use url::Url;

use crate::lsp::completion::CompletionItem;
use crate::lsp::definition::definition_at_workspace;
use crate::lsp::diagnostics::{LspDiagnostic, LspRange, LspSeverity};
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
                            "definitionProvider": true,
                            "completionProvider": {
                                "triggerCharacters": ["."]
                            }
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
                    session.did_open(&path, text.to_string());
                    publish_diagnostics_for_file(session, &path, writer)?;
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
                        session.did_change(&path, text.to_string());
                        publish_diagnostics_for_file(session, &path, writer)?;
                    }
                }
            }
        }
        "textDocument/didClose" => {
            if let (Some(session), Some(params)) = (state.session.as_mut(), params) {
                if let Some(path) = extract_text_document_path(params) {
                    session.did_close(&path);
                    // Clear diagnostics for the closed file
                    let uri = path_to_file_uri(&path)?;
                    write_notification(
                        writer,
                        "textDocument/publishDiagnostics",
                        json!({ "uri": uri, "diagnostics": [] }),
                    )?;
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
        "textDocument/completion" => {
            if let Some(id) = id {
                let response = match state.session.as_ref() {
                    Some(session) => {
                        handle_completion_request(session, params).unwrap_or_else(|_| {
                            json!({
                                "isIncomplete": false,
                                "items": []
                            })
                        })
                    }
                    None => json!({
                        "isIncomplete": false,
                        "items": []
                    }),
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
        let markdown = hover_to_markdown(&contents);
        Ok(json!({
            "contents": {
                "kind": "markdown",
                "value": markdown
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

fn handle_completion_request(session: &AnalysisSession, params: Option<&Value>) -> Result<Value> {
    let Some(params) = params else {
        return Ok(json!({ "isIncomplete": false, "items": [] }));
    };
    let Some(path) = extract_text_document_path(params) else {
        return Ok(json!({ "isIncomplete": false, "items": [] }));
    };
    let Some(position) = extract_position(params) else {
        return Ok(json!({ "isIncomplete": false, "items": [] }));
    };

    let items = session.completion(&path, &path, position)?;
    let lsp_items: Vec<Value> = items.iter().map(completion_item_to_json).collect();
    Ok(json!({
        "isIncomplete": false,
        "items": lsp_items
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

fn write_notification(writer: &mut impl Write, method: &str, params: Value) -> Result<()> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    write_lsp_payload(writer, &payload)
}

fn publish_diagnostics_for_file(
    session: &AnalysisSession,
    path: &Path,
    writer: &mut impl Write,
) -> Result<()> {
    let diags = match session.diagnostics(path, path) {
        Ok(diags) => diags,
        Err(err) => vec![LspDiagnostic {
            range: LspRange {
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 0,
            },
            severity: LspSeverity::Error,
            code: "E_ANALYSIS".to_string(),
            message: format!("analysis failed: {err}"),
        }],
    };
    let uri = path_to_file_uri(path)?;
    let lsp_diags: Vec<Value> = diags.iter().map(lsp_diagnostic_to_json).collect();
    write_notification(
        writer,
        "textDocument/publishDiagnostics",
        json!({ "uri": uri, "diagnostics": lsp_diags }),
    )
}

fn lsp_diagnostic_to_json(diag: &LspDiagnostic) -> Value {
    json!({
        "range": {
            "start": { "line": diag.range.start_line, "character": diag.range.start_character },
            "end": { "line": diag.range.end_line, "character": diag.range.end_character }
        },
        "severity": diag.severity.to_lsp_number(),
        "code": diag.code,
        "source": "twk",
        "message": diag.message
    })
}

fn completion_item_to_json(item: &CompletionItem) -> Value {
    let mut value = json!({
        "label": item.label,
        "kind": item.kind as u32,
    });
    if let Some(detail) = &item.detail {
        value["detail"] = json!(detail);
    }
    if let Some(doc) = &item.documentation {
        value["documentation"] = json!({
            "kind": "plaintext",
            "value": doc
        });
    }
    value
}

fn hover_to_markdown(contents: &str) -> String {
    let (signature, docs) = if let Some((sig, rest)) = contents.split_once("\n\n") {
        (sig, Some(rest))
    } else {
        (contents, None)
    };

    let mut markdown = format!("```twinkle\n{signature}\n```");
    if let Some(doc_text) = docs {
        if !doc_text.trim().is_empty() {
            markdown.push_str("\n\n");
            markdown.push_str(doc_text);
        }
    }
    markdown
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::position::byte_offset_to_position_utf16;
    use crate::query::cache::reset_global_cache;

    #[test]
    fn completion_request_returns_lsp_completion_items() {
        reset_global_cache();

        let project_root = PathBuf::from("/virtual/lsp_cli_completion");
        let stdlib_root = project_root.join("stdlib");
        let entry = project_root.join("main.tw");
        let source = "text := \"hello\"\nsize := text.len()\n";

        let mut base_sources = HashMap::new();
        base_sources.insert(entry.clone(), source.to_string());
        let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

        let mut state = LspState {
            session: Some(session),
            shutdown_requested: false,
        };

        let cursor = source.find("text.").expect("text receiver") + "text.".len();
        let pos = byte_offset_to_position_utf16(source, cursor).expect("position");
        let uri = path_to_file_uri(&entry).expect("uri");
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": pos.line, "character": pos.character }
            }
        });

        let mut writer = Vec::new();
        let should_exit = handle_lsp_message(&mut state, &payload, &mut writer).expect("handle");
        assert!(!should_exit, "completion request should not exit");

        let response = decode_lsp_body(&writer);
        assert_eq!(response["id"], json!(1));
        assert_eq!(response["result"]["isIncomplete"], json!(false));

        let items = response["result"]["items"]
            .as_array()
            .expect("completion items array");
        assert!(
            items
                .iter()
                .any(|item| item.get("label") == Some(&json!("len"))
                    && item.get("kind") == Some(&json!(2))),
            "expected method completion item for `len`, got: {:?}",
            items
        );
        assert!(
            items.iter().any(|item| {
                item.get("label") == Some(&json!("len")) && item.get("documentation").is_some()
            }),
            "expected completion item docs in protocol response, got: {:?}",
            items
        );
    }

    #[test]
    fn hover_request_returns_markdown_fenced_signature() {
        reset_global_cache();

        let project_root = PathBuf::from("/virtual/lsp_cli_hover_markdown");
        let stdlib_root = project_root.join("stdlib");
        let entry = project_root.join("main.tw");
        let source = "value := range(10)\n";

        let mut base_sources = HashMap::new();
        base_sources.insert(entry.clone(), source.to_string());
        let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

        let mut state = LspState {
            session: Some(session),
            shutdown_requested: false,
        };

        let cursor = source.find("range").expect("range symbol");
        let pos = byte_offset_to_position_utf16(source, cursor).expect("position");
        let uri = path_to_file_uri(&entry).expect("uri");
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": pos.line, "character": pos.character }
            }
        });

        let mut writer = Vec::new();
        let should_exit = handle_lsp_message(&mut state, &payload, &mut writer).expect("handle");
        assert!(!should_exit, "hover request should not exit");

        let response = decode_lsp_body(&writer);
        assert_eq!(response["id"], json!(2));
        assert_eq!(response["result"]["contents"]["kind"], json!("markdown"));
        let value = response["result"]["contents"]["value"]
            .as_str()
            .expect("hover markdown value");
        assert!(
            value.starts_with("```twinkle\n"),
            "hover markdown should start with twinkle fence, got: {value}"
        );
        assert!(
            value.contains("\n```"),
            "hover markdown should close fence, got: {value}"
        );
    }

    #[test]
    fn hover_request_includes_builtin_doc_markdown() {
        reset_global_cache();

        let project_root = PathBuf::from("/virtual/lsp_cli_hover_builtin_doc");
        let stdlib_root = project_root.join("stdlib");
        let entry = project_root.join("main.tw");
        let source = "println(\"hi\")\n";

        let mut base_sources = HashMap::new();
        base_sources.insert(entry.clone(), source.to_string());
        let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

        let mut state = LspState {
            session: Some(session),
            shutdown_requested: false,
        };

        let pos = byte_offset_to_position_utf16(source, 0).expect("position");
        let uri = path_to_file_uri(&entry).expect("uri");
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": pos.line, "character": pos.character }
            }
        });

        let mut writer = Vec::new();
        let should_exit = handle_lsp_message(&mut state, &payload, &mut writer).expect("handle");
        assert!(!should_exit, "hover request should not exit");

        let response = decode_lsp_body(&writer);
        assert_eq!(response["id"], json!(3));
        let value = response["result"]["contents"]["value"]
            .as_str()
            .expect("hover markdown value");
        assert!(
            value.contains("fn(text: String) Void"),
            "expected named builtin parameter in hover markdown, got: {value}"
        );
        assert!(
            value.contains("Print a string to stdout followed by a newline."),
            "expected builtin doc text in hover markdown, got: {value}"
        );
    }

    #[test]
    fn hover_request_includes_builtin_doc_after_multibyte_line() {
        reset_global_cache();

        let project_root = PathBuf::from("/virtual/lsp_cli_hover_builtin_doc_multibyte");
        let stdlib_root = project_root.join("stdlib");
        let entry = project_root.join("main.tw");
        let source = "// ASCII \u{2014} each character is its own grapheme\nprintln(\"foo bar\")\n";

        let mut base_sources = HashMap::new();
        base_sources.insert(entry.clone(), source.to_string());
        let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

        let mut state = LspState {
            session: Some(session),
            shutdown_requested: false,
        };

        let cursor = source.find("println").expect("println call");
        let pos = byte_offset_to_position_utf16(source, cursor).expect("position");
        let uri = path_to_file_uri(&entry).expect("uri");
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": pos.line, "character": pos.character }
            }
        });

        let mut writer = Vec::new();
        let should_exit = handle_lsp_message(&mut state, &payload, &mut writer).expect("handle");
        assert!(!should_exit, "hover request should not exit");

        let response = decode_lsp_body(&writer);
        assert_eq!(response["id"], json!(4));
        let value = response["result"]["contents"]["value"]
            .as_str()
            .expect("hover markdown value");
        assert!(
            value.contains("fn(text: String) Void"),
            "expected named builtin parameter in hover markdown, got: {value}"
        );
        assert!(
            value.contains("Print a string to stdout followed by a newline."),
            "expected builtin doc text in hover markdown, got: {value}"
        );
    }

    #[test]
    fn publish_diagnostics_reports_analysis_errors_instead_of_silently_clearing() {
        reset_global_cache();

        let project_root = PathBuf::from("/virtual/lsp_cli_diag_error");
        let stdlib_root = project_root.join("stdlib");
        let path = project_root.join("missing.tw");
        let session = AnalysisSession::new(&project_root, &stdlib_root, HashMap::new());

        let mut writer = Vec::new();
        publish_diagnostics_for_file(&session, &path, &mut writer).expect("publish diagnostics");

        let response = decode_lsp_body(&writer);
        assert_eq!(response["method"], json!("textDocument/publishDiagnostics"));
        let diagnostics = response["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(
            diagnostics.len(),
            1,
            "should publish synthetic analysis error"
        );
        assert_eq!(diagnostics[0]["code"], json!("E_ANALYSIS"));
        let message = diagnostics[0]["message"]
            .as_str()
            .expect("diagnostic message");
        assert!(
            message.contains("analysis failed"),
            "expected analysis failure message, got: {message}"
        );
    }

    fn decode_lsp_body(buffer: &[u8]) -> Value {
        let raw = std::str::from_utf8(buffer).expect("utf8 response");
        let (_, body) = raw.split_once("\r\n\r\n").expect("header/body split");
        serde_json::from_str(body).expect("valid json body")
    }
}
