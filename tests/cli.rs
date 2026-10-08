//! CLI 冒烟测试：list-themes / 转换落盘 / stdout / stdin / preview 壳。

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_md2html"))
}

#[test]
fn list_themes_prints_six() {
    let out = bin().arg("--list-themes").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for id in ["moyu", "redwhite", "deepblue", "orange", "grape", "peach"] {
        assert!(text.contains(id), "应列出 {id}");
    }
}

#[test]
fn convert_writes_preview_page() {
    let tmp = tempfile::tempdir().unwrap();
    let md_path = tmp.path().join("article.md");
    std::fs::write(&md_path, "# 你好\n\n正文 **加粗**。").unwrap();

    let out_path = tmp.path().join("article.html");
    let out = bin()
        .arg(md_path.display().to_string())
        .arg("-t")
        .arg("moyu")
        .env("MD2HTML_TEST", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(out_path.exists(), "应生成 {out:?}");

    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.contains("复制到公众号"), "落盘的是预览页");
    assert!(html.contains("你好"));
    assert!(html.contains("#07C160"), "moyu 主色");
    assert!(stderr.contains("✔"), "进度输出到 stderr: {stderr}");
}

#[test]
fn stdout_outputs_bare_fragment() {
    let tmp = tempfile::tempdir().unwrap();
    let md_path = tmp.path().join("a.md");
    std::fs::write(&md_path, "# 标题\n\n正文").unwrap();
    let out = bin().args(["a.md", "--stdout"]).current_dir(tmp.path()).output().unwrap();
    assert!(out.status.success());
    let html = String::from_utf8_lossy(&out.stdout);
    assert!(html.starts_with("<section"), "stdout 是纯正文片段");
    assert!(!html.contains("<!DOCTYPE"), "不带预览壳");
}

#[test]
fn stdin_works() {
    use std::io::Write as _;
    let mut child = bin()
        .args(["-", "--stdout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all("# stdin 标题\n\n内容".as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let html = String::from_utf8_lossy(&out.stdout);
    assert!(html.contains("stdin 标题"));
}

#[test]
fn frontmatter_overrides_theme() {
    let tmp = tempfile::tempdir().unwrap();
    let md_path = tmp.path().join("fm.md");
    std::fs::write(&md_path, "---\ntitle: FM\ntheme: orange\n---\n\n正文").unwrap();
    let out = bin()
        .arg(md_path.display().to_string())
        .arg("--stdout")
        .output()
        .unwrap();
    let html = String::from_utf8_lossy(&out.stdout);
    assert!(html.contains("#F76B1C"), "frontmatter theme: orange 应生效");
}

#[test]
fn unknown_theme_fails_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let md_path = tmp.path().join("a.md");
    std::fs::write(&md_path, "# t\n\nbody").unwrap();
    let out = bin()
        .arg(md_path.display().to_string())
        .args(["--stdout", "-t", "nope"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("未知主题"), "{stderr}");
}
