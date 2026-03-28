use std::sync::LazyLock;

use fancy_regex::{Regex, Split};

pub mod preprocessor;
pub mod parser;

pub static COMMENT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(//.*)|(/\*\O*(?=\*/)\*/(?!/\*))"#).unwrap()
});

pub static OPERATOR_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(==|>>|<<|->|\+\+|--|[+-/\\*%=><|&^;!(){}"])"#).unwrap()
});

pub static WHITESPACE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+(?=((\\[\\"]|[^\\"])*"(\\[\\"]|[^\\"])*")*(\\[\\"]|[^\\"])*$)"#).unwrap()
});
