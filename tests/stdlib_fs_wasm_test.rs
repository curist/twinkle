use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn tw_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[test]
fn stdlib_fs_works_via_wasmtime_host_imports() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "twinkle-stdlib-fs-test-{}-{stamp}",
        std::process::id()
    ));
    let io_dir = root.join("io");
    fs::create_dir_all(&io_dir).expect("create temp io directory");

    let src_path: PathBuf = root.join("prog.tw");
    let src = format!(
        r#"
use @std.fs
use @std.path

dir := "{dir}"
txt := path.join(dir, "hello.txt")
bin := path.join(dir, "bytes.bin")
bad := path.join(dir, "bad.bin")

case fs.mkdirp(dir) {{
  .Ok(_) => println("mkdir-ok"),
  .Err(_) => println("mkdir-err"),
}}

case fs.write_text(txt, "hello stdlib") {{
  .Ok(_) => println("write-text-ok"),
  .Err(_) => println("write-text-err"),
}}

case fs.write_bytes(bin, [65, 66, 67]) {{
  .Ok(_) => println("write-bytes-ok"),
  .Err(_) => println("write-bytes-err"),
}}

case fs.write_bytes(bad, [255, 254]) {{
  .Ok(_) => println("write-bad-ok"),
  .Err(_) => println("write-bad-err"),
}}

case fs.read_text(txt) {{
  .Ok(s) => println(s),
  .Err(_) => println("read-err"),
}}

case fs.read_bytes(bin) {{
  .Ok(bs) => println("${{bs.len()}}:${{Byte.to_int(bs[0])}}:${{Byte.to_int(bs[2])}}"),
  .Err(_) => println("read-bytes-err"),
}}

case fs.read_text(bad) {{
  .Ok(_) => println("invalid-utf8-missed"),
  .Err(e) => case e {{
    .InvalidUtf8 => println("invalid-utf8-ok"),
    _ => println("invalid-utf8-other"),
  }},
}}

case fs.list_dir(dir) {{
  .Ok(entries) => println("${{entries.len()}}"),
  .Err(_) => println("list-err"),
}}

println("${{fs.exists(txt)}}")
println("${{fs.exists(bin)}}")
"#,
        dir = tw_escape(&io_dir.to_string_lossy()),
    );
    fs::write(&src_path, src).expect("write temp program");

    let (stdout, stderr) = twinkle::cli::run_wasm::run_wasm_capture(src_path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("run_wasm_capture failed: {e:#}"));
    let _ = fs::remove_dir_all(&root);

    assert!(stderr.is_empty(), "expected empty stderr, got:\n{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "mkdir-ok",
            "write-text-ok",
            "write-bytes-ok",
            "write-bad-ok",
            "hello stdlib",
            "3:65:67",
            "invalid-utf8-ok",
            "3",
            "true",
            "true"
        ]
    );
}
