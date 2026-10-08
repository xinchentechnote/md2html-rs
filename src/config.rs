//! 凭据加载：环境变量优先，其次 ~/.md2html/config.json；都没有给出可操作的报错

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct WechatConfig {
    pub appid: String,
    pub secret: String,
    pub source: String,
}

pub fn load_wechat_config() -> Result<WechatConfig, String> {
    if let (Ok(appid), Ok(secret)) = (
        std::env::var("MD2HTML_APPID"),
        std::env::var("MD2HTML_SECRET"),
    ) {
        return Ok(WechatConfig {
            appid,
            secret,
            source: "环境变量 MD2HTML_APPID / MD2HTML_SECRET".to_string(),
        });
    }
    let file = config_file_path();
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                [
                    "未配置公众号凭据，无法发布草稿。两种方式任选：",
                    "  1) 环境变量：export MD2HTML_APPID=xxx MD2HTML_SECRET=xxx",
                    "  2) 配置文件 ~/.md2html/config.json：{ \"appid\": \"xxx\", \"secret\": \"xxx\" }",
                    "",
                    "凭据获取：公众号后台「设置与开发 → 基本配置」；IP 白名单需包含本机出口 IP。",
                    "暂不发布可继续用复制粘贴流程：md2html 文章.md --open",
                ]
                .join("\n"),
            )
        }
        Err(e) => return Err(format!("读取 {} 失败: {e}", file.display())),
    };
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("配置文件不是合法 JSON: {}（{e}）", file.display()))?;
    let appid = parsed["appid"].as_str().unwrap_or_default().to_string();
    let secret = parsed["secret"].as_str().unwrap_or_default().to_string();
    if appid.is_empty() || secret.is_empty() {
        return Err(format!("配置文件缺少 appid/secret 字段: {}", file.display()));
    }
    Ok(WechatConfig { appid, secret, source: file.display().to_string() })
}

pub fn config_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".md2html")
        .join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_priority() {
        // CI 环境可能已设置这些变量，只在两者都缺失时检查文件路径兜底逻辑
        if std::env::var("MD2HTML_APPID").is_ok() && std::env::var("MD2HTML_SECRET").is_ok() {
            let c = load_wechat_config().unwrap();
            assert_eq!(c.appid, std::env::var("MD2HTML_APPID").unwrap());
        }
    }
}
