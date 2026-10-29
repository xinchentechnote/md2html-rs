//! 浏览器编辑器服务：仅绑定 127.0.0.1，只服务启动时指定的那一个文件，
//! 不接受任意路径读写。
//! GET / 页面、GET /editor.js 内嵌脚本、GET /api/state 初始内容、
//! POST /api/render 服务端渲染（core 在 Rust 侧，浏览器无法直跑）、
//! POST /api/mermaid 图表渲染、PUT /api/file 保存。

pub const PAGE: &str = include_str!("page.html");
pub const EDITOR_JS: &str = include_str!("editor.js");

use std::io::Read;
use std::path::PathBuf;

use base64::Engine as _;
use serde_json::json;
use tiny_http::{Header, Method, Response, Server};

use crate::RenderOptions;
use crate::mermaid::render_mermaid_png;
use crate::themes;
use crate::{render_markdown, themes::Vars};

const MAX_BODY: usize = 10 * 1024 * 1024;
const MAX_MERMAID_BODY: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub struct UiOptions {
    pub file: Option<PathBuf>,
    pub theme: Option<String>,
    pub mermaid: String,
}

/// 启动服务：返回 (工作线程, 监听地址)。线程随进程退出而结束。
pub fn start_editor_server(opts: UiOptions) -> Result<(std::thread::JoinHandle<()>, String), String> {
    let server = Server::http("127.0.0.1:0").map_err(|e| format!("端口绑定失败: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let url = format!("http://127.0.0.1:{port}");
    println!("✔ 编辑器已启动: {url}（Ctrl+C 退出）");

    let handle = std::thread::spawn(move || serve(server, opts));
    Ok((handle, url))
}

fn serve(server: Server, opts: UiOptions) {
    let file = opts.file.clone();
    let default_theme = opts.theme.clone();
    let mermaid = opts.mermaid.clone();

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/");

        let mut body = Vec::new();
        if matches!(method, Method::Post | Method::Put) {
            let limit = if path == "/api/mermaid" { MAX_MERMAID_BODY } else { MAX_BODY };
            let _ = request.as_reader().take(limit as u64 + 1).read_to_end(&mut body);
            if body.len() > limit {
                respond(request, 413, "application/json; charset=utf-8", r#"{"error":"body too large"}"#);
                continue;
            }
        }

        let response = route(&method, path, &body, &file, &default_theme, &mermaid);
        match response {
            Some((status, ctype, payload)) => respond(request, status, ctype, &payload),
            None => respond(request, 404, "text/plain; charset=utf-8", "not found"),
        }
    }
}

fn route(
    method: &Method,
    path: &str,
    body: &[u8],
    file: &Option<PathBuf>,
    default_theme: &Option<String>,
    mermaid: &str,
) -> Option<(u16, &'static str, String)> {
    let send = |status: u16, ctype: &'static str, payload: String| Some((status, ctype, payload));
    match (method, path) {
        (Method::Get, "/") => return send(200, "text/html; charset=utf-8", PAGE.to_string()),
        (Method::Get, "/editor.js") => return send(200, "text/javascript; charset=utf-8", EDITOR_JS.to_string()),
        (Method::Get, "/api/state") => {
            let content = file
                .as_deref()
                .and_then(|f| std::fs::read_to_string(f).ok())
                .unwrap_or_default();
            let themes: Vec<_> = themes::list_themes()
                .iter()
                .map(|t| json!({ "id": t.id, "name": t.name }))
                .collect();
            return send(
                200,
                "application/json; charset=utf-8",
                json!({
                    "file": file.as_ref().map(|f| f.display().to_string()),
                    "content": content,
                    "theme": default_theme,
                    "defaultTheme": themes::DEFAULT_THEME,
                    "mermaid": mermaid,
                    "themes": themes,
                })
                .to_string(),
            );
        }
        // 保存的是原始文本，不走 JSON 解析
        (Method::Put, "/api/file") => match file {
            Some(path) => match std::fs::write(path, body) {
                Ok(()) => return send(200, "application/json; charset=utf-8", r#"{"ok":true}"#.into()),
                Err(e) => return send(500, "application/json; charset=utf-8", json!({ "error": e.to_string() }).to_string()),
            },
            None => return send(400, "application/json; charset=utf-8", r#"{"error":"no file"}"#.into()),
        },
        _ => {}
    }

    let payload: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return send(400, "application/json; charset=utf-8", r#"{"error":"invalid json"}"#.into()),
    };

    match (method, path) {
        (Method::Post, "/api/render") => {
            let markdown = payload["markdown"].as_str().unwrap_or_default();
            let theme_id = payload["theme"].as_str().unwrap_or(themes::DEFAULT_THEME);
            match render_markdown(markdown, &RenderOptions { theme: Some(theme_id.to_string()), ..Default::default() }) {
                Ok(result) => send(
                    200,
                    "application/json; charset=utf-8",
                    json!({ "html": result.html }).to_string(),
                ),
                Err(err) => send(422, "application/json; charset=utf-8", json!({ "error": err }).to_string()),
            }
        }
        (Method::Post, "/api/mermaid") => {
            if mermaid == "off" {
                return send(400, "application/json; charset=utf-8", r#"{"error":"mermaid disabled"}"#.into());
            }
            let code = payload["code"].as_str().unwrap_or_default();
            if code.trim().is_empty() {
                return send(400, "application/json; charset=utf-8", r#"{"error":"no code"}"#.into());
            }
            let theme_id = payload["theme"].as_str().unwrap_or(themes::DEFAULT_THEME);
            let vars: Option<&Vars> = themes::get_theme(theme_id).ok().map(|t| &t.vars);
            match render_mermaid_png(code, mermaid, vars, 2) {
                Ok(png) => send(
                    200,
                    "application/json; charset=utf-8",
                    json!({ "png": base64::engine::general_purpose::STANDARD.encode(&png) }).to_string(),
                ),
                Err(err) => send(422, "application/json; charset=utf-8", json!({ "error": err }).to_string()),
            }
        }
        _ => None,
    }
}

fn respond(request: tiny_http::Request, status: u16, ctype: &str, payload: &str) {
    let header = Header::from_bytes("Content-Type", ctype).unwrap();
    let response = Response::from_string(payload)
        .with_header(header)
        .with_status_code(status);
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_state_render_and_save() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("note.md");
        std::fs::write(&file, "# 初稿").unwrap();

        let (_handle, url) = start_editor_server(UiOptions {
            file: Some(file.clone()),
            theme: Some("moyu".into()),
            mermaid: "off".into(),
        })
        .unwrap();

        // state：初始内容与主题
        let state: serde_json::Value = ureq::get(&format!("{url}/api/state")).call().unwrap().into_json().unwrap();
        assert_eq!(state["content"], "# 初稿");
        assert_eq!(state["theme"], "moyu");
        assert_eq!(state["defaultTheme"], themes::DEFAULT_THEME);
        assert_eq!(state["themes"].as_array().unwrap().len(), 6);

        // render：服务端渲染
        let res = ureq::post(&format!("{url}/api/render"))
            .send_json(json!({ "markdown": "# 你好\n\n正文", "theme": "moyu" }))
            .unwrap();
        let rendered: serde_json::Value = res.into_json().unwrap();
        assert!(rendered["html"].as_str().unwrap().contains("你好"));

        // file：保存原始文本（非 JSON body）
        let res = ureq::put(&format!("{url}/api/file")).send_string("# 已保存").unwrap();
        let saved: serde_json::Value = res.into_json().unwrap();
        assert_eq!(saved["ok"], true);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# 已保存");

        // mermaid off → 400
        let res = ureq::post(&format!("{url}/api/mermaid")).send_json(json!({ "code": "graph LR\nA-->B" }));
        assert!(res.is_err());
    }
}
