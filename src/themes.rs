//! 主题系统：共享一套渲染器，主题 = 色板数据。
//! 6 套主题只覆盖 colors，其余变量（字体/圆角/间距/代码配色）走默认值——与 JS 版 base.js 一致。

pub const DEFAULT_THEME: &str = "deepblue";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub accent: &'static str,
    pub accent_light: &'static str,
    pub heading: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    pub highlight: &'static str,
    pub card_bg: &'static str,
    pub border: &'static str,
    pub code_bg: &'static str,
    pub code_text: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vars {
    pub font: Font,
    pub radius: Radius,
    pub spacing: Spacing,
    pub colors: Colors,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font {
    pub base: &'static str,
    pub title: &'static str,
    pub h2: &'static str,
    pub code: &'static str,
    pub family: &'static str,
    pub mono: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radius {
    pub card: &'static str,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    pub paragraph: &'static str,
    pub block: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub vars: Vars,
}

const DEFAULT: Vars = Vars {
    font: Font {
        base: "15px",
        title: "22px",
        h2: "17px",
        code: "13px",
        family: "-apple-system,BlinkMacSystemFont,'PingFang SC','Hiragino Sans GB','Microsoft YaHei',sans-serif",
        mono: "Menlo,Consolas,'Courier New',monospace",
    },
    radius: Radius { card: "12px", tag: "6px" },
    spacing: Spacing { paragraph: "10px", block: "24px" },
    // 代码块统一深色卡片（VSCode Dark+ 配色），所有主题共用
    colors: Colors {
        accent: "#07C160",
        accent_light: "#E8F8EF",
        heading: "#1A1A1A",
        text: "#3F3F3F",
        muted: "#9CA3AF",
        highlight: "#FFE58F",
        card_bg: "#F6F8FA",
        border: "#E5E7EB",
        code_bg: "#1E1E1E",
        code_text: "#D4D4D4",
    },
};

const fn theme(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    colors: Colors,
) -> Theme {
    Theme { id, name, description, vars: Vars { colors, ..DEFAULT } }
}

pub static THEMES: &[Theme] = &[
    theme(
        "moyu",
        "摸鱼绿",
        "微信绿主色，浅绿点缀，清爽护眼",
        Colors {
            accent: "#07C160",
            accent_light: "#E8F8EF",
            heading: "#1A1A1A",
            text: "#3F3F3F",
            muted: "#9CA3AF",
            highlight: "#FFE58F",
            card_bg: "#F6F8FA",
            border: "#E5E7EB",
            code_bg: "#1E1E1E",
            code_text: "#D4D4D4",
        },
    ),
    theme(
        "redwhite",
        "红白色系",
        "正红主色，热烈醒目，适合观点与热点文",
        Colors {
            accent: "#E34D59",
            accent_light: "#FDECEE",
            heading: "#1F1F1F",
            text: "#444444",
            muted: "#9CA3AF",
            highlight: "#FFE3C2",
            card_bg: "#FAF6F6",
            border: "#EFE3E3",
            code_bg: "#1E1E1E",
            code_text: "#D4D4D4",
        },
    ),
    theme(
        "deepblue",
        "深海蓝",
        "沉稳蓝主色，理性专业，适合技术与科普",
        Colors {
            accent: "#2A6AE9",
            accent_light: "#E8EFFD",
            heading: "#17212B",
            text: "#3D4852",
            muted: "#9AA6B2",
            highlight: "#CCE0FF",
            card_bg: "#F5F8FC",
            border: "#E1E8F0",
            code_bg: "#0F1B2D",
            code_text: "#D6E1F0",
        },
    ),
    theme(
        "orange",
        "活力橙",
        "暖橙主色，明快有食欲，适合生活与美食",
        Colors {
            accent: "#F76B1C",
            accent_light: "#FEF0E6",
            heading: "#26201B",
            text: "#4A423B",
            muted: "#B0A49A",
            highlight: "#FFE3B8",
            card_bg: "#FAF6F1",
            border: "#F0E6DC",
            code_bg: "#221B14",
            code_text: "#E8DFD5",
        },
    ),
    theme(
        "grape",
        "暗夜紫",
        "葡萄紫主色，神秘高级，适合 AI 与前沿话题",
        Colors {
            accent: "#7C5CFC",
            accent_light: "#F0EBFE",
            heading: "#1C1725",
            text: "#423B52",
            muted: "#A29BB3",
            highlight: "#E3D7FF",
            card_bg: "#F8F6FB",
            border: "#E9E4F2",
            code_bg: "#191324",
            code_text: "#DCD6E8",
        },
    ),
    theme(
        "peach",
        "蜜桃粉",
        "蜜桃粉主色，柔软治愈，适合情感与生活方式",
        Colors {
            accent: "#EC6E8B",
            accent_light: "#FDEBF0",
            heading: "#2B2124",
            text: "#4E4145",
            muted: "#B39AA2",
            highlight: "#FFD9E4",
            card_bg: "#FBF6F7",
            border: "#F2E4E8",
            code_bg: "#241A1D",
            code_text: "#EBDEE2",
        },
    ),
];

pub fn get_theme(id: &str) -> Result<&'static Theme, String> {
    THEMES
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| format!("未知主题 \"{id}\"，可用主题: {}", THEMES.iter().map(|t| t.id).collect::<Vec<_>>().join(", ")))
}

pub fn list_themes() -> &'static [Theme] {
    THEMES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_ids_match_js_version() {
        let ids: Vec<_> = list_themes().iter().map(|t| t.id).collect();
        assert_eq!(ids, ["moyu", "redwhite", "deepblue", "orange", "grape", "peach"]);
    }

    #[test]
    fn unknown_theme_errors() {
        assert!(get_theme("nope").is_err());
        assert!(get_theme("deepblue").is_ok());
    }
}
