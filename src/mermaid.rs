//! mermaid 后处理：把渲染层输出的「代码卡 + data-md2html-mermaid 标记」替换为内嵌 PNG。
//! 引擎优先级（auto）：本地 mmdc → mermaid.ink 在线服务，任一引擎失败自动换另一个重试；
//! ink 请求自带指数退避重试，全部失败才保留代码卡兜底。
//! 图片以 data URI 内嵌，HTML 单文件自包含。

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::json;

use crate::themes::Vars;
use crate::utils::{find_section_end, s};

/// 扫描标记块并逐个尝试渲染成图；失败时打印告警并保留代码卡。
/// engine=off 直接跳过（保留代码卡，不告警）。
pub fn inline_mermaid(html: &str, engine: &str, vars: &Vars) -> String {
    if engine == "off" {
        return html.to_string();
    }
    const MARKER: &str = "<section data-md2html-mermaid=\"";
    let mut jobs: Vec<(usize, usize, String)> = Vec::new(); // (start, end, code)
    let mut search_from = 0usize;
    while let Some(at) = html[search_from..].find(MARKER) {
        let start = search_from + at;
        let b64_start = start + MARKER.len();
        let Some(b64_len) = html[b64_start..].find('"') else { break };
        let b64 = &html[b64_start..b64_start + b64_len];
        let code = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok());
        if let (Some(code), Some(end)) = (code, find_section_end(html, start)) {
            jobs.push((start, end, code));
        }
        search_from = b64_start + b64_len;
    }

    // 从后往前替换，避免下标位移
    let mut out = html.to_string();
    for (start, end, code) in jobs.iter().rev() {
        match render_mermaid_png(code, engine, Some(vars), 2) {
            Ok(png) => out.replace_range(*start..*end, &image_card(&png, vars)),
            Err(err) => eprintln!("⚠ mermaid 渲染失败，保留代码卡：{err}"),
        }
    }
    out
}

fn image_card(png: &[u8], vars: &Vars) -> String {
    let r = &vars.radius;
    format!(
        "<section style=\"{}\"><img src=\"data:image/png;base64,{}\" alt=\"mermaid 图表\" style=\"{}\"></section>",
        s(&[
            ("background", "#FFFFFF"),
            ("border-radius", r.card),
            ("padding", "12px 8px"),
            ("margin", "16px 0"),
        ]),
        base64::engine::general_purpose::STANDARD.encode(png),
        s(&[("max-width", "100%"), ("display", "block"), ("margin", "0 auto")]),
    )
}

/// 单图渲染入口（CLI 后处理与 ui-server 共用）：优先本地 mmdc，失败转 ink，反之亦然
pub fn render_mermaid_png(
    code: &str,
    engine: &str,
    vars: Option<&Vars>,
    scale: u32,
) -> Result<Vec<u8>, String> {
    match engine {
        "mmdc" => {
            let mmdc = find_mmdc().ok_or_else(|| {
                "未安装 mmdc（npm i -D @mermaid-js/mermaid-cli），或改用 --mermaid ink".to_string()
            })?;
            render_with_mmdc(&mmdc, code, vars, scale)
        }
        "ink" => render_with_ink(code),
        "auto" => {
            let mut mmdc_err = None;
            if let Some(mmdc) = find_mmdc() {
                match render_with_mmdc(&mmdc, code, vars, scale) {
                    Ok(png) => return Ok(png),
                    Err(err) => mmdc_err = Some(err),
                }
            }
            render_with_ink(code).map_err(|ink_err| match mmdc_err {
                Some(mmdc_err) => {
                    let brief = truncate_chars(mmdc_err.trim(), 300);
                    format!("mmdc 失败：{brief}；ink 兜底也失败：{ink_err}")
                }
                None => ink_err,
            })
        }
        other => Err(format!("未知引擎 \"{other}\"（可选 auto/ink/mmdc/off）")),
    }
}

/// mmdc 查找顺序：MD2HTML_MMDC 环境变量 → ./node_modules/.bin → PATH
fn find_mmdc() -> Option<PathBuf> {
    let name = if cfg!(windows) { "mmdc.cmd" } else { "mmdc" };
    if let Ok(p) = std::env::var("MD2HTML_MMDC") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let local = std::env::current_dir().ok()?.join("node_modules").join(".bin").join(name);
    if local.is_file() {
        return Some(local);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn render_with_mmdc(mmdc: &PathBuf, code: &str, vars: Option<&Vars>, scale: u32) -> Result<Vec<u8>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = dir.path().join("in.mmd");
    let output = dir.path().join("out.png");
    std::fs::write(&input, code).map_err(|e| e.to_string())?;

    let mut args: Vec<String> = vec![
        "-i".into(),
        input.display().to_string(),
        "-o".into(),
        output.display().to_string(),
        "-s".into(),
        scale.to_string(),
        "-b".into(),
        "white".into(),
    ];
    // 主题联动：mermaid 变量映射自 designVars（图与文章配色一致）
    if let Some(v) = vars {
        let theme_variables = json!({
            "primaryColor": v.colors.accent_light,
            "primaryBorderColor": v.colors.accent,
            "primaryTextColor": v.colors.heading,
            "lineColor": v.colors.accent,
            "textColor": v.colors.text,
            "background": "#FFFFFF",
            "fontFamily": v.font.family,
        });
        args.push("--themeVariables".into());
        args.push(theme_variables.to_string());
    }

    let mut cmd = Command::new(mmdc);
    cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = run_with_timeout(&mut cmd, Duration::from_secs(60))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(if stderr.is_empty() { format!("mmdc 退出码 {:?}", out.status.code()) } else { stderr.to_string() });
    }
    std::fs::read(&output).map_err(|e| format!("读取渲染产物失败: {e}"))
}

/// 带超时的子进程执行（超时 kill）。输出量小，先等退出再读管道即可。
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<std::process::Output, String> {
    let mut child = cmd.spawn().map_err(|e| format!("启动 mmdc 失败: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("mmdc 执行超时（{}s）", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    Ok(std::process::Output { status, stdout, stderr })
}

fn render_with_ink(code: &str) -> Result<Vec<u8>, String> {
    // mermaid.ink 路径要求 URL-safe base64（+/ 会 404）；源码必须以换行结尾，否则 400
    let normalized = if code.ends_with('\n') { code.to_string() } else { format!("{code}\n") };
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(normalized.as_bytes());
    // 注意：scale 参数必须配合 width/height，单独使用返回 400
    let url = format!("https://mermaid.ink/img/{b64}?type=png");
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build();
    // 在线服务抖动 / 429 限流很常见，单次请求失败不足以放弃整张图：最多 3 次，退避 1s → 3s
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(if attempt == 1 { 1 } else { 3 }));
        }
        match fetch_ink(&agent, &url) {
            Ok(png) => return Ok(png),
            Err((retryable, msg)) => {
                last_err = msg;
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(last_err)
}

/// 单次请求 mermaid.ink。第二个返回值表示是否值得重试：
/// 400/404 说明源码或 base64 有问题，重试无意义；429/5xx/网络错误/响应截断均可重试。
fn fetch_ink(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, (bool, String)> {
    let res = agent
        .get(url)
        .set("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) md2html-rs")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(status, _) => {
                let retryable = status == 429 || status >= 500;
                (retryable, format!("mermaid.ink HTTP {status}"))
            }
            other => (true, format!("mermaid.ink 请求失败: {other}")),
        })?;
    let mut bytes = Vec::new();
    res.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| (true, format!("mermaid.ink 读取失败: {e}")))?;
    Ok(bytes)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut brief: String = s.chars().take(max).collect();
    brief.push('…');
    brief
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::get_theme;

    #[test]
    fn marker_replacement_and_fallback() {
        let vars = get_theme("moyu").unwrap().vars;
        let b64 = base64::engine::general_purpose::STANDARD.encode("graph LR\nA-->B\n".as_bytes());
        let html = format!("<p>前</p><section data-md2html-mermaid=\"{b64}\"><section>代码卡</section></section><p>后</p>");
        // off 引擎必然报错 → 保留代码卡
        let kept = inline_mermaid(&html, "off", &vars);
        assert!(kept.contains("代码卡"));
        assert!(kept.contains("前"));
    }

    #[test]
    fn find_section_end_bound() {
        let html = "<a><section data-md2html-mermaid=\"x\"><section>y</section></section>z";
        let end = find_section_end(html, 3).unwrap();
        assert_eq!(&html[3..end], "<section data-md2html-mermaid=\"x\"><section>y</section></section>");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("中文错误信息很长", 4), "中文错误…");
        let long = "渲染失败nginx/1.2 gateway timeout".repeat(50);
        assert_eq!(truncate_chars(&long, 300).chars().count(), 301);
    }
}
