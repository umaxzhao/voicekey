//! 领域术语 / 开发词汇后处理。
//!
//! 兼容 VoCoType 的 terms.yaml：读取 canonical / aliases，
//! 把识别结果中的音译、英文误写替换成规范写法。hotword/protect 字段保留，
//! 供后续接入支持热词的引擎使用。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
struct TermsFile {
    #[serde(default)]
    terms: Vec<Term>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Term {
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hotword: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub protect: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn term(canonical: &str, aliases: &[&str]) -> Term {
    Term {
        canonical: canonical.into(),
        aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
        hotword: true,
        protect: false,
    }
}

/// 开发场景的常用词条：只放安全、常见的拆词/音译误写。
pub fn presets() -> Vec<Term> {
    vec![
        term("Claude Code", &["claude code", "cloud code", "cloude code"]),
        term("OpenCode", &["open code", "opencode"]),
        term("GitHub", &["git hub", "get hub"]),
        term("GitLab", &["git lab"]),
        term("TypeScript", &["type script", "typescript"]),
        term("JavaScript", &["java script", "javascript"]),
        term("Node.js", &["node js", "nodejs"]),
        term("VS Code", &["vs code", "vscode", "v s code"]),
        term("API", &["a p i"]),
        term("SDK", &["s d k"]),
        term("YAML", &["yaml", "yamil"]),
        term("Linux", &["linux", "linix"]),
        term("Ubuntu", &["ubuntu", "乌班图"]),
        term("Fcitx5", &["fcitx5", "fcitx 5"]),
        term("Docker", &["docker", "道克"]),
        term("Kubernetes", &["kubernetes", "k8s"]),
        term("Redis", &["redis", "瑞迪斯"]),
        term("PostgreSQL", &["postgres", "postgre sql", "postgresql"]),
        term("Tauri", &["tauri", "陶瑞"]),
        term("Svelte", &["svelte", "斯维尔特"]),
        term("Rust", &["rust", "拉斯特"]),
        term("Python", &["python", "派森"]),
        term("Java", &["java", "爪哇"]),
        term("React", &["react", "瑞爱克特"]),
    ]
}

#[derive(Clone, Debug, Default)]
pub struct Terms {
    terms: Vec<Term>,
}

impl Terms {
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = std::fs::read(path) else {
            return Self::default();
        };
        let text = String::from_utf8_lossy(&bytes);
        match serde_yaml::from_str::<TermsFile>(&text) {
            Ok(file) => Self { terms: file.terms },
            Err(_) => Self::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// 依次替换 aliases；长别名优先，避免 `Claude Code` 先被 `Claude` 截断。
    pub fn apply(&self, text: &str) -> String {
        let mut pairs: Vec<(&str, &str)> = self
            .terms
            .iter()
            .flat_map(|term| {
                term.aliases
                    .iter()
                    .map(|alias| (alias.as_str(), term.canonical.as_str()))
            })
            .filter(|(alias, canonical)| !alias.is_empty() && alias != canonical)
            .collect();
        pairs.sort_by_key(|(alias, _)| std::cmp::Reverse(alias.chars().count()));

        let mut out = text.to_string();
        for (alias, canonical) in pairs {
            if alias.is_ascii() {
                // ASCII 别名按整词匹配，避免 OT 命中 NOT、node 命中 anode。
                out = replace_ascii_ci(&out, alias, canonical);
            } else {
                out = out.replace(alias, canonical);
            }
        }
        out
    }

    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    pub fn from_terms(terms: Vec<Term>) -> Self {
        Self { terms }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_yaml::to_string(&TermsFile {
            terms: self.terms.clone(),
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(
            path,
            format!("# VoiceKey 识别词汇：aliases 会在识别后被替换成 canonical\n{body}"),
        )
    }
}

fn replace_ascii_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    let mut scan = 0;
    while let Some(pos) = lower[scan..].find(&needle) {
        let start = scan + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || {
            let b = haystack.as_bytes()[start - 1];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        let after_ok = end >= haystack.len() || {
            let b = haystack.as_bytes()[end];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        if before_ok && after_ok {
            out.push_str(&haystack[last..start]);
            out.push_str(replacement);
            last = end;
            scan = end;
        } else {
            scan = start + 1;
        }
    }
    if last == 0 {
        return haystack.to_string();
    }
    out.push_str(&haystack[last..]);
    out
}

/// VoiceKey 自己的术语表路径（写入时始终用这个）。
pub fn terms_path() -> PathBuf {
    match dirs::config_dir() {
        Some(config) => config.join("voicekey/terms.yaml"),
        None => PathBuf::from(".config/voicekey/terms.yaml"),
    }
}

/// 优先读 VoiceKey 自己的术语表，其次兼容 VoCoType 的旧路径。
pub fn default_path() -> PathBuf {
    let voicekey = terms_path();
    if voicekey.exists() {
        voicekey
    } else {
        match dirs::config_dir() {
            Some(config) => config.join("vocotype/terms.yaml"),
            None => PathBuf::from(".config/vocotype/terms.yaml"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_aliases_and_keeps_ascii_word_boundaries() {
        let yaml = r#"
terms:
  - canonical: Claude Code
    aliases: [claude code, cloud code]
  - canonical: Claude
    aliases: [cloude]
  - canonical: OT
    aliases: [OT]
  - canonical: 挡拆
    aliases: [挡差, 挡猜]
"#;
        let file: TermsFile = serde_yaml::from_str(yaml).unwrap();
        let terms = Terms { terms: file.terms };
        assert_eq!(
            terms.apply("我用 claude code 和 cloud code 写代码"),
            "我用 Claude Code 和 Claude Code 写代码"
        );
        assert_eq!(terms.apply("我用 cloude 写代码"), "我用 Claude 写代码");
        assert_eq!(terms.apply("NOT a word, OT 是加时"), "NOT a word, OT 是加时");
        assert_eq!(terms.apply("这个挡差配合很好"), "这个挡拆配合很好");
    }

    #[test]
    fn saves_and_reloads_terms() {
        let dir = std::env::temp_dir().join(format!("voicekey-terms-{}", std::process::id()));
        let path = dir.join("terms.yaml");
        Terms::from_terms(presets()).save(&path).unwrap();
        let loaded = Terms::load(&path);
        assert!(loaded.apply("我用 cloud code 写代码").contains("Claude Code"));
        assert!(!loaded.terms().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loads_real_terms_file_when_present() {
        let Some(path) = dirs::config_dir().map(|c| c.join("voicekey/terms.yaml")) else {
            return;
        };
        if !path.exists() {
            return;
        }
        let terms = Terms::load(&path);
        assert!(!terms.is_empty());
        assert_eq!(terms.apply("我用 cloude 写代码"), "我用 Claude 写代码");
    }
}
