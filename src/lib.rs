use std::sync::LazyLock;

use fancy_regex::{Regex, Split};

pub mod preprocessor;

pub static COMMENT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(//.*)|(/\*\O*(?=\*/)\*/(?!/\*))"#).unwrap()
});

pub static OPERATOR_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[+-/\\*%=><|&^;!()"]"#).unwrap()
});

pub static WHITESPACE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+(?=((\\[\\"]|[^\\"])*"(\\[\\"]|[^\\"])*")*(\\[\\"]|[^\\"])*$)"#).unwrap()
});

pub fn split_whitespace_quote_respecting<'a>(target: &'a str) -> Split<'a, 'a> {
    WHITESPACE_REGEX.split(target)
}
