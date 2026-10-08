//! 微信公众号 API 客户端：stable_token + 图片上传 + 草稿箱。
//! HTTP 客户端可注入 api_base 便于测试；token 缓存 + 40001/42001 强刷重试一次。

use std::cell::RefCell;
use std::time::Duration;

use serde_json::{json, Value};

const DEFAULT_API: &str = "https://api.weixin.qq.com";

/// 常见错误码中文映射（完整列表见微信公众平台文档）
fn errcode_tips(code: i64) -> &'static str {
    match code {
        -1 => "系统繁忙，稍后重试",
        40001 => "AppSecret 错误，或不属于该公众号",
        40002 => "凭证类型不合法",
        40013 => "AppID 不合法",
        40004 => "媒体文件类型不合法（支持 jpg/png）",
        40005 => "不支持的文件类型",
        40009 => "图片尺寸过大（封面建议 900×383，正文图 ≤10MB）",
        40125 => "无效的 AppSecret（可能在后台被重置）",
        40164 => "IP 不在白名单（错误信息里通常带了本机出口 IP，去后台加上）",
        41001 => "缺少 access_token",
        42001 => "access_token 已过期",
        43104 => "AppID 与账号不匹配",
        44002 => "POST 内容为空",
        45002 => "内容超过大小限制（正文 ≤2 万字符 / 20 万字节内保险）",
        45009 => "接口调用频率超限，稍后重试",
        48001 => "api 未授权——该账号大概率没有草稿/素材接口权限（未认证个人订阅号通常没有，需微信认证）",
        _ => "微信接口错误",
    }
}

#[derive(Debug)]
pub struct WechatError {
    pub code: Option<i64>,
    pub message: String,
}

impl std::fmt::Display for WechatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "[{code}] {}（{}）", errcode_tips(code), self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

pub struct WechatClient {
    agent: ureq::Agent,
    appid: String,
    secret: String,
    api_base: String,
    token: RefCell<Option<String>>,
}

/// 上传素材（multipart 里的一个文件）
pub struct MediaFile<'a> {
    pub field: &'a str,
    pub filename: &'a str,
    pub bytes: &'a [u8],
}

impl WechatClient {
    pub fn new(appid: impl Into<String>, secret: impl Into<String>) -> Self {
        Self::with_base(appid, secret, DEFAULT_API)
    }

    /// 测试可注入 api_base
    pub fn with_base(appid: impl Into<String>, secret: impl Into<String>, api_base: &str) -> Self {
        WechatClient {
            agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build(),
            appid: appid.into(),
            secret: secret.into(),
            api_base: api_base.to_string(),
            token: RefCell::new(None),
        }
    }

    fn get_token(&self, force: bool) -> Result<String, WechatError> {
        if !force {
            if let Some(token) = self.token.borrow().as_deref() {
                return Ok(token.to_string());
            }
        }
        let body = json!({ "grant_type": "client_credential", "appid": self.appid, "secret": self.secret });
        let data = self.post_json(&format!("{}/cgi-bin/stable_token", self.api_base), &body)?;
        assert_ok(&data)?;
        let token = data["access_token"].as_str().unwrap_or_default().to_string();
        *self.token.borrow_mut() = Some(token.clone());
        Ok(token)
    }

    fn post_json(&self, url: &str, body: &Value) -> Result<Value, WechatError> {
        let res = self.agent.post(url).send_json(body).map_err(http_err)?;
        parse_json(res)
    }

    fn post_multipart(&self, url: &str, media: &MediaFile) -> Result<Value, WechatError> {
        let boundary = format!("----md2html{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos());
        let mut body: Vec<u8> = Vec::with_capacity(media.bytes.len() + 512);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                media.field,
                media.filename,
                content_type_of(media.filename),
            )
            .as_bytes(),
        );
        body.extend_from_slice(media.bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let res = self
            .agent
            .post(url)
            .set("content-type", &format!("multipart/form-data; boundary={boundary}"))
            .send_bytes(&body)
            .map_err(http_err)?;
        parse_json(res)
    }

    /// 带 token 调用；40001/42001 时强制刷新 token 重试一次
    fn call(&self, pathname: &str, retried: bool, send: impl Fn(&str) -> Result<Value, WechatError>) -> Result<Value, WechatError> {
        let token = self.get_token(false)?;
        let sep = if pathname.contains('?') { '&' } else { '?' };
        match send(&format!("{}{pathname}{sep}access_token={token}", self.api_base)) {
            Ok(data) => {
                let code = data["errcode"].as_i64().unwrap_or(0);
                if (code == 40001 || code == 42001) && !retried {
                    self.get_token(true)?;
                    return self.call(pathname, true, send);
                }
                Ok(data)
            }
            Err(e) => Err(e),
        }
    }

    /// 正文图片转存，返回微信 URL（草稿正文不允许外链图）
    pub fn upload_content_image(&self, media: &MediaFile) -> Result<String, WechatError> {
        let data = self.call("/cgi-bin/media/uploadimg", false, |url| self.post_multipart(url, media))?;
        assert_ok(&data)?;
        data["url"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| WechatError { code: None, message: format!("uploadimg 未返回 url: {data}") })
    }

    /// 封面图上传为永久素材，返回 { media_id, url }（草稿必填 thumb_media_id）
    pub fn upload_cover(&self, media: &MediaFile) -> Result<(String, Option<String>), WechatError> {
        let data = self.call("/cgi-bin/material/add_material?type=image", false, |url| {
            self.post_multipart(url, media)
        })?;
        assert_ok(&data)?;
        let media_id = data["media_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| WechatError { code: None, message: format!("add_material 未返回 media_id: {data}") })?;
        Ok((media_id, data["url"].as_str().map(String::from)))
    }

    /// 新建草稿，返回草稿 media_id
    pub fn add_draft(&self, article: Value) -> Result<String, WechatError> {
        let body = json!({ "articles": [article] });
        let data = self.call("/cgi-bin/draft/add", false, |url| self.post_json(url, &body))?;
        assert_ok(&data)?;
        data["media_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| WechatError { code: None, message: format!("draft/add 未返回 media_id: {data}") })
    }
}

fn assert_ok(data: &Value) -> Result<(), WechatError> {
    match data["errcode"].as_i64() {
        Some(code) if code != 0 => Err(WechatError {
            code: Some(code),
            message: data["errmsg"].as_str().unwrap_or_default().to_string(),
        }),
        _ => Ok(()),
    }
}

fn parse_json(res: ureq::Response) -> Result<Value, WechatError> {
    use std::io::Read as _;
    let mut text = String::new();
    res.into_reader()
        .read_to_string(&mut text)
        .map_err(|e| WechatError { code: None, message: e.to_string() })?;
    serde_json::from_str(&text)
        .map_err(|e| WechatError { code: None, message: format!("响应不是合法 JSON: {text:.200}（{e}）") })
}

fn http_err(e: ureq::Error) -> WechatError {
    // 微信错误语义在 JSON body 里，非 2xx 响应也取 body 解析
    match e {
        ureq::Error::Status(_, res) => match parse_json(res) {
            Ok(data) => match data["errcode"].as_i64() {
                Some(code) => WechatError {
                    code: Some(code),
                    message: data["errmsg"].as_str().unwrap_or_default().to_string(),
                },
                None => WechatError { code: None, message: format!("HTTP 状态错误且无 errcode: {data}") },
            },
            Err(err) => err,
        },
        other => WechatError { code: None, message: format!("HTTP 请求失败: {other}") },
    }
}

fn content_type_of(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 起一个假微信 API：按脚本依次返回 canned 响应
    fn fake_server(script: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for expected in script {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 65536];
                let mut request = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        // 粗略判满：头部之后按 content-length 收 body
                        let text = String::from_utf8_lossy(&request).to_string();
                        let len = text
                            .split("\r\n")
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)));
                        let header_end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                        match len {
                            Some(l) if request.len() >= header_end + l => break,
                            None => break,
                            _ => continue,
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&request).to_string());
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{expected}", expected.len());
                stream.write_all(response.as_bytes()).unwrap();
            }
            seen
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn token_cache_and_force_refresh_retry() {
        let (base, server) = fake_server(vec![
            r#"{"access_token":"t1"}"#.into(),
            r#"{"errcode":40001,"errmsg":"invalid credential"}"#.into(),
            r#"{"access_token":"t2"}"#.into(),
            r#"{"media_id":"M1"}"#.into(),
        ]);
        let client = WechatClient::with_base("appid", "secret", &base);
        // 第一次拿 token（t1）→ 草稿返回 40001 → 强刷（t2）→ 重试成功
        let media_id = client.add_draft(json!({ "title": "t" })).unwrap();
        assert_eq!(media_id, "M1");
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 4);
        assert!(seen[0].contains("stable_token"));
        assert!(seen[1].contains("draft/add"));
        assert!(seen[2].contains("stable_token"));
        assert!(seen[3].contains("draft/add"));
    }

    #[test]
    fn errcode_maps_to_chinese_tip() {
        let (base, server) = fake_server(vec![
            r#"{"access_token":"t1"}"#.into(),
            r#"{"errcode":48001,"errmsg":"api unauthorized"}"#.into(),
        ]);
        let client = WechatClient::with_base("appid", "secret", &base);
        let err = client.add_draft(json!({})).unwrap_err();
        assert!(err.to_string().contains("api 未授权"), "实际: {err}");
        server.join().unwrap();
    }

    #[test]
    fn multipart_carves_file_part() {
        let (base, server) = fake_server(vec![
            r#"{"access_token":"t1"}"#.into(),
            r#"{"url":"https://mmbiz.qpic.cn/x"}"#.into(),
        ]);
        let client = WechatClient::with_base("appid", "secret", &base);
        let url = client
            .upload_content_image(&MediaFile { field: "media", filename: "a.png", bytes: b"PNGDATA" })
            .unwrap();
        assert_eq!(url, "https://mmbiz.qpic.cn/x");
        let seen = server.join().unwrap();
        assert!(seen[1].contains("name=\"media\"; filename=\"a.png\""));
        assert!(seen[1].contains("PNGDATA"));
    }
}
