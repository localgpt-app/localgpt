//! Serves test 3 to a phone on this network: the page, its WebAssembly
//! (`pkg/`, from wasm-pack), and a CJK font from this Mac for what egui's
//! own faces lack (on the phone the system's fonts are out of its reach).
//!
//! ```text
//! cd spikes/egui-phone
//! wasm-pack build --target web --release --no-typescript
//! cargo run --release --bin serve        # prints the address to open
//! ```
//!
//! `SPIKE_PORT` moves it off 9890; `SPIKE_CJK_FONT` names another font
//! file (a collection's first face is used); `SPIKE_BIND=127.0.0.1` keeps
//! it to this Mac. Otherwise it answers anyone on the network for as long
//! as it runs, so stop it when the test is done.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">
<title>Test 3: egui on a phone</title>
<style>
  html, body { margin: 0; height: 100%; background: #0f1218; overflow: hidden; }
  canvas { position: fixed; inset: 0; width: 100%; height: 100%; }
</style>
</head>
<body>
<canvas id="phone"></canvas>
<script type="module">import init from "./pkg/spike_egui_phone.js"; init();</script>
</body>
</html>
"#;

fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("SPIKE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9890);
    let font = std::env::var_os("SPIKE_CJK_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"));
    let pkg = Path::new(env!("CARGO_MANIFEST_DIR")).join("pkg");
    if !pkg.join("spike_egui_phone_bg.wasm").exists() {
        eprintln!(
            "No build in {}: wasm-pack build --target web --release --no-typescript",
            pkg.display()
        );
        std::process::exit(1);
    }
    let bind = std::env::var("SPIKE_BIND").unwrap_or_else(|_| "0.0.0.0".into());
    let listener = TcpListener::bind((bind.as_str(), port))?;
    for interface in ["en0", "en1"].iter().filter(|_| bind == "0.0.0.0") {
        if let Ok(out) = Command::new("ipconfig")
            .args(["getifaddr", interface])
            .output()
            && out.status.success()
        {
            let address = String::from_utf8_lossy(&out.stdout).trim().to_string();
            println!("Open on the phone: http://{address}:{port}/");
        }
    }
    println!("(and here: http://localhost:{port}/); Ctrl-C stops it");
    for stream in listener.incoming().flatten() {
        let (pkg, font) = (pkg.clone(), font.clone());
        std::thread::spawn(move || {
            if let Err(e) = answer(stream, &pkg, &font) {
                eprintln!("{e}");
            }
        });
    }
    Ok(())
}

fn answer(stream: TcpStream, pkg: &Path, font: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    // The rest of the request says nothing this needs.
    let mut line = String::new();
    while reader.read_line(&mut line)? > 2 {
        line.clear();
    }
    let path = first.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    println!("{path}");
    let found = match path {
        "/" | "/index.html" => Some((PAGE.as_bytes().to_vec(), "text/html; charset=utf-8")),
        "/cjk.ttc" => std::fs::read(font)
            .ok()
            .map(|bytes| (bytes, "font/collection")),
        _ => path
            .strip_prefix("/pkg/")
            .filter(|name| !name.contains("..") && !name.contains('/'))
            .and_then(|name| {
                std::fs::read(pkg.join(name))
                    .ok()
                    .map(|bytes| (bytes, mime(name)))
            }),
    };
    let mut stream = stream;
    match found {
        Some((body, kind)) => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)
        }
        None => stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    }
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
