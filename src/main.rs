//! CLI 入口（薄壳）：转换 / ui 编辑器 / publish 草稿箱三个形态。

use std::io::Read as _;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use clap::{CommandFactory, Parser, Subcommand};

use md2html_rs::RenderOptions;
use md2html_rs::mermaid;
use md2html_rs::open_browser::open_browser;
use md2html_rs::preview::preview_page;
use md2html_rs::publish::{publish, PublishOptions};
use md2html_rs::themes::list_themes;
use md2html_rs::ui::{start_editor_server, UiOptions};
use md2html_rs::render_markdown;

use notify::Watcher as _;

#[derive(Parser)]
#[command(
    name = "md2html",
    version,
    about = "Markdown → 微信公众号排版 HTML（样式全内联，可直接复制粘贴发布）"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 输入 md 文件（'-' 读 stdin；可多个）
    files: Vec<String>,
    /// 主题 id，默认 deepblue
    #[arg(short, long)]
    theme: Option<String>,
    /// 输出路径（单文件时有效，默认同名 .html）
    #[arg(short, long)]
    out: Option<String>,
    /// 仅输出正文 HTML 片段（管道友好）
    #[arg(long)]
    stdout: bool,
    /// 生成后自动打开预览页
    #[arg(long)]
    open: bool,
    /// 不渲染文首标题卡（复制到公众号时本就会自动剔除）
    #[arg(long)]
    no_header: bool,
    /// 追加文末一键三连页脚（默认不追加）
    #[arg(long)]
    footer: bool,
    /// 列出可用主题
    #[arg(long)]
    list_themes: bool,
    /// mermaid 渲染引擎：auto（本地 mmdc → 在线 ink）| ink | mmdc | off
    #[arg(long, default_value = "auto")]
    mermaid: String,
    /// 监听文件变更并重新生成（单文件）
    #[arg(short, long)]
    watch: bool,
}

#[derive(Subcommand)]
enum Command {
    /// 浏览器编辑器：编辑 md 实时预览、⌘S 保存、一键复制
    Ui {
        /// 要编辑的 md 文件（不给则只预览，保存禁用）
        file: Option<String>,
        /// 默认主题（页面内可随时切换）
        #[arg(short, long)]
        theme: Option<String>,
        /// mermaid 引擎：auto（mmdc → ink）| ink | mmdc | off
        #[arg(long, default_value = "auto")]
        mermaid: String,
        /// 启动后不自动打开浏览器
        #[arg(long)]
        no_open: bool,
    },
    /// 渲染并发布到公众号草稿箱（不群发，需配置凭据）
    Publish {
        file: String,
        /// 主题
        #[arg(short, long)]
        theme: Option<String>,
        /// 标题（默认取文首 h1 或文件名）
        #[arg(long)]
        title: Option<String>,
        /// 作者（默认取 frontmatter author）
        #[arg(long)]
        author: Option<String>,
        /// 摘要（默认 frontmatter digest 或首段前 120 字）
        #[arg(long)]
        digest: Option<String>,
        /// 封面：本地路径或图片 URL（默认文内第一张图）
        #[arg(long)]
        cover: Option<String>,
        /// 草稿正文不含头卡（默认即剥离）
        #[arg(long)]
        no_header: bool,
        /// 草稿正文附一键三连页脚（默认不追加）
        #[arg(long)]
        footer: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Command::Ui { file, theme, mermaid, no_open }) => {
            run_ui(file, theme, mermaid, no_open)
        }
        Some(Command::Publish { file, theme, title, author, digest, cover, no_header, footer }) => {
            run_publish(
                PublishOptions { theme, title, author, digest, cover, footer, no_header },
                &file,
            )
        }
        None => run_convert(&cli),
    };
    std::process::exit(code);
}

fn run_convert(cli: &Cli) -> i32 {
    if cli.list_themes {
        for t in list_themes() {
            println!("{:<12}{}  {}", t.id, t.name, t.description);
        }
        return 0;
    }
    if cli.files.is_empty() {
        Cli::command().print_help().ok();
        return 1;
    }
    if let Err(err) = validate_mermaid(&cli.mermaid) {
        eprintln!("✖ {err}");
        return 1;
    }

    let mut first_out: Option<String> = None;
    for file in &cli.files {
        match convert_one(file, cli) {
            Ok(out) => {
                if out.is_some() && first_out.is_none() {
                    first_out = out;
                }
            }
            Err(err) => {
                eprintln!("✖ {err}");
                return 1;
            }
        }
    }
    if cli.open {
        if let Some(out) = &first_out {
            open_browser(out);
        }
    }

    if cli.watch {
        if cli.stdout {
            eprintln!("⚠ --watch 与 --stdout 不兼容，已忽略 --watch");
        } else if cli.files.len() != 1 || cli.files[0] == "-" {
            eprintln!("⚠ --watch 仅支持监听单个文件");
        } else {
            return run_watch(&cli.files[0], cli);
        }
    }
    0
}

fn convert_one(file: &str, cli: &Cli) -> Result<Option<String>, String> {
    let md = read_input(file)?;
    let base = if file == "-" { "stdin".to_string() } else { strip_md_ext(file) };
    let result = render_markdown(
        &md,
        &RenderOptions {
            theme: cli.theme.clone(),
            header: Some(!cli.no_header),
            footer: Some(cli.footer),
            ..Default::default()
        },
    )?;
    let theme = result.theme;
    let final_html = mermaid::inline_mermaid(&result.html, &cli.mermaid, &theme.vars);
    if cli.stdout {
        println!("{final_html}");
        return Ok(None);
    }
    let out = if cli.files.len() == 1 {
        cli.out.clone().unwrap_or_else(|| format!("{base}.html"))
    } else {
        format!("{base}.html")
    };
    std::fs::write(&out, preview_page(&final_html, theme.name))
        .map_err(|e| format!("写出 {out} 失败: {e}"))?;
    match &result.meta.title {
        Some(title) => eprintln!("✔ {file} → {out}（主题: {} · {title}）", theme.name),
        None => eprintln!("✔ {file} → {out}（主题: {}）", theme.name),
    }
    Ok(Some(out))
}

/// 监听单文件变更，防抖 200ms 后重新生成（编辑器保存时常触发多次事件）
fn run_watch(file: &str, cli: &Cli) -> i32 {
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("✖ 初始化监听失败: {e}");
            return 1;
        }
    };
    if let Err(e) = watcher.watch(Path::new(file), notify::RecursiveMode::NonRecursive) {
        eprintln!("✖ 无法监听 {file}: {e}");
        return 1;
    }
    eprintln!("👁 正在监听 {file}，保存后自动重新生成（Ctrl+C 退出）");
    loop {
        match rx.recv() {
            Ok(_) => {
                while rx.recv_timeout(Duration::from_millis(200)).is_ok() {}
                if let Err(e) = convert_one(file, cli) {
                    eprintln!("✖ {e}");
                }
            }
            Err(e) => {
                eprintln!("✖ 监听错误: {e}");
                return 1;
            }
        }
    }
}

fn run_ui(file: Option<String>, theme: Option<String>, mermaid: String, no_open: bool) -> i32 {
    if let Err(err) = validate_mermaid(&mermaid) {
        eprintln!("✖ {err}");
        return 1;
    }
    let resolved = file.map(|f| std::fs::canonicalize(&f).unwrap_or_else(|_| f.into()));
    let opts = UiOptions { file: resolved, theme, mermaid };
    let (_handle, url) = match start_editor_server(opts) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("✖ {err}");
            return 1;
        }
    };
    if !no_open {
        open_browser(&url);
    }
    // 服务线程已就绪，主线程保活
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn run_publish(options: PublishOptions, file: &str) -> i32 {
    match publish(Path::new(file), &options) {
        Ok(report) => {
            eprintln!("✔ 已存入草稿箱：{}", report.title);
            eprintln!("  草稿 media_id: {}", report.media_id);
            if report.skipped.is_empty() {
                eprintln!("  正文图转存: {} 张", report.images);
            } else {
                eprintln!(
                    "  正文图转存: {} 张，跳过 {} 张（{}）",
                    report.images,
                    report.skipped.len(),
                    report.skipped.join("、")
                );
            }
            eprintln!("  凭据来源: {}", report.config_source);
            eprintln!("  到公众号后台「草稿箱」查看，确认无误后手动群发。");
            0
        }
        Err(err) => {
            eprintln!("✖ 发布失败: {err}");
            1
        }
    }
}

fn read_input(file: &str) -> Result<String, String> {
    if file == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("读取 stdin 失败: {e}"))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(file).map_err(|e| format!("{file} 不可读: {e}"))
    }
}

fn validate_mermaid(engine: &str) -> Result<(), String> {
    match engine {
        "auto" | "ink" | "mmdc" | "off" => Ok(()),
        other => Err(format!("未知 mermaid 引擎 \"{other}\"（可选 auto/ink/mmdc/off）")),
    }
}

fn strip_md_ext(file: &str) -> String {
    match file.strip_suffix(".md").or_else(|| file.strip_suffix(".MD")) {
        Some(base) => base.to_string(),
        None => file.to_string(),
    }
}
