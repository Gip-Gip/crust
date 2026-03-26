use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::io::Error as IoError;
use std::iter::once_with;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use crate::{COMMENT_REGEX, OPERATOR_REGEX, split_whitespace_quote_respecting};

pub const DIRECTIVE_DELEM: char = '#';
pub const PARAM_START_DELEM: char = '(';
pub const PARAM_END_DELEM: char = ')';
pub const PARAM_SEP_DELEM: char = ',';
pub const ESCAPE_DELEM: char = '\\';
pub const MULTILINE_DELEM: char = ESCAPE_DELEM;
pub const STRING_DELEM: char = '"';
pub const INCLUDE_START_DELEM: char = '<';
pub const INCLUDE_END_DELEM: char = '>';
pub const MULTILINE_COMMENT_START_DELEM: &str = "/*";
pub const MULTILINE_COMMENT_END_DELEM: &str = "*/";
pub const PARAM_STRINGIFY_DELEM: char = '#';

fn combine_tokens_into_string<'a, TokenIter: Iterator<Item = &'a String>>(tokens: TokenIter, separator: &str) -> String {
    let mut out = String::with_capacity(80);

    for token in tokens {
        if !out.is_empty() {
            out.push_str(separator);
        }

        out.push_str(token);
    }

    out
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum Directive {
    Unknown,
    Include,
    Define,
    Undef,
    If,
    IfDef,
    IfNDef,
    Else,
    EndIf,
    Error,
}

static DIRECTIVE_MAP: LazyLock<HashMap<&'static str, Directive>> = LazyLock::new(|| {
    HashMap::from_iter([
        ("INCLUDE", Directive::Include),
        ("DEFINE", Directive::Define),
        ("UNDEF", Directive::Undef),
        ("IFDEF", Directive::IfDef),
        ("IFNDEF", Directive::IfNDef),
        ("ELSE", Directive::Else),
        ("ENDIF", Directive::EndIf),
        ("ERROR", Directive::Error),
    ].into_iter())
});

impl From<&str> for Directive {
    fn from(value: &str) -> Self {
        let directive = value.to_uppercase();

        *DIRECTIVE_MAP.get(directive.as_str()).unwrap_or(&Directive::Unknown)
    }
}

pub enum PpErrId {
    Io(IoError),
    UnknownDirective(String),
    DefineMissingToken,
    UnknownParam(String),
    UnexpectedToken(String),
    MissingParenthesis,
    MissingParameter,
    UnclosedParenthesis,
    UnclosedString,
    UnexpectedEof,
    FileNotFound(String),
    UnknownDefine(String),
    ErrorDirective(String)
}

impl PartialEq for PpErrId {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Display for PpErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PpErrId::Io(e) => write!(f, "io error: {}", e),
            PpErrId::UnknownDirective(directive) => write!(f, "unknown directive '{}'", directive),
            PpErrId::UnknownParam(param) => write!(f, "unknown param '{}'", param),
            PpErrId::UnknownDefine(param) => write!(f, "unknown define '{}'", param),
            PpErrId::UnexpectedToken(token) => write!(f, "unexpected token '{}'", token),
            PpErrId::DefineMissingToken => write!(f, "define is missing a token"),
            PpErrId::MissingParenthesis => write!(f, "macro missing parenthesis"),
            PpErrId::MissingParameter => write!(f, "macro missing parameters"),
            PpErrId::UnclosedParenthesis => write!(f, "unclosed parenthesis"),
            PpErrId::UnclosedString => write!(f, "unclosed string"),
            PpErrId::UnexpectedEof => write!(f, "unexpected end of file"),
            PpErrId::FileNotFound(filename) => write!(f, "unable to find '{}'", filename),
            PpErrId::ErrorDirective(error) => write!(f, "error directive: {}", error),
        }
    }
}

impl Debug for PpErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(PartialEq)]
pub struct PreprocessorError {
    line_num: usize,
    id: PpErrId,
}

impl Display for PreprocessorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "preprocessor error on line {}: {}", self.line_num, self.id)
    }
}

impl Debug for PreprocessorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl PreprocessorError {
    pub fn error_directive(line_num: usize, e: String) -> Self {
        Self { line_num, id: PpErrId::ErrorDirective(e) }
    }

    pub fn io(line_num: usize, e: IoError) -> Self {
        Self { line_num, id: PpErrId::Io(e) }
    }

    pub fn unknown_directive(line_num: usize, directive: &str) -> Self {
        Self { line_num, id: PpErrId::UnknownDirective(directive.to_string()) }
    }

    pub fn unknown_define(line_num: usize, define: &str) -> Self {
        Self { line_num, id: PpErrId::UnknownDefine(define.to_string()) }
    }

    pub fn unknown_param(line_num: usize, param: &str) -> Self {
        Self { line_num, id: PpErrId::UnknownParam(param.to_string()) }
    }
    
    pub fn unexpected_token(line_num: usize, token: &str) -> Self {
        Self { line_num, id: PpErrId::UnexpectedToken(token.to_string()) }
    }

    pub fn define_missing_token(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::DefineMissingToken }
    }

    pub fn missing_parenthesis(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::MissingParenthesis }
    }
    
    pub fn unclosed_parenthesis(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::UnclosedParenthesis }
    }
    
    pub fn unclosed_string(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::UnclosedString }
    }
    
    pub fn missing_parameter(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::MissingParameter }
    }
    
    pub fn unexpected_eof(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::UnexpectedEof }
    }

    pub fn file_not_found(line_num: usize, filename: PathBuf) -> Self {
        Self { line_num, id: PpErrId::FileNotFound(filename.as_os_str().to_os_string().into_string().unwrap()) }
    }
}

pub type Tokens = Vec<String>;

#[derive(Debug, Clone)]
pub struct ParamInstance {
    pub param_key: String,
    pub quote: bool,
    pub i_insert: usize,
}

#[derive(Debug)]
pub struct Define {
    pub param_map: Option<HashMap<String, usize>>,
    /// Params will be placed in between each body vec string
    pub param_instances: Option<Vec<ParamInstance>>,
    pub body: Vec<Tokens>,
}

impl Define {
    pub fn body_to_tokens(&self, line_num: usize, params: Option<Vec<Tokens>>) -> Result<Tokens, PreprocessorError> {
        if params.as_ref().map(|x: &Vec<_>| x.len()).unwrap_or_default() == 0 {
            return Ok(self.body.first().unwrap_or(&Vec::new()).clone());
        }

        let params = params.unwrap();
        let param_map = self.param_map.as_ref().unwrap();
        let param_instances = self.param_instances.as_ref().unwrap();

        let mut out = Vec::with_capacity(80);

        for (i_param_instance, body_tokens) in self.body.iter().enumerate() {
            out.extend_from_slice(body_tokens);

            let param_instance = param_instances.get(i_param_instance).unwrap();

            let i_param = param_map.get(&param_instance.param_key).unwrap();

            let param_tokens = params.get(*i_param).ok_or(PreprocessorError::missing_parameter(line_num))?;

            match param_instance.quote {
                true => {
                    out.push(format!("\"{}\"", combine_tokens_into_string(param_tokens.into_iter(), " ")));
                }
                false => { out.extend_from_slice(param_tokens); }
            }
        }

        Ok(out)
    }
}

#[derive(Debug, PartialEq)]
pub struct LineTokenGroup {
    pub line_num: usize,
    pub file_name: Arc<String>,
    pub tokens: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct PreprocessorOut {
    token_groups: Vec<LineTokenGroup>,
}

impl PreprocessorOut {
    pub fn new() -> Self {
        Self {
            token_groups: Vec::with_capacity(128),
        }
    }

    pub fn push_line(&mut self, line_num: usize, file_name: Arc<String>, tokens: Vec<String>) {
        self.token_groups.push(LineTokenGroup { line_num, file_name, tokens });
    }

    pub fn write_to<Out: Write>(&self, mut out: Out) -> Result<(), IoError>{
        let mut line_written = false;

        for token_group in self.token_groups.iter() {
            if line_written {
                writeln!(out, "")?;
            } else {
                line_written = true;
            }

            write!(out, "{}", combine_tokens_into_string(token_group.tokens.iter(), " "))?
        }

        Ok(())
    }
}

pub struct Preprocessor<'istr, In: Read> {
    in_stream: &'istr mut BufReader<In>,
    in_file_name: Arc<String>,
    defines: HashMap<String, Define>,
    include_dirs: Vec<PathBuf>,
    /// Incremented as the preprocessor goes through the in stream
    line_num: usize,
}

impl<'istr, In: Read> Preprocessor<'istr, In> {
    pub fn new(in_stream: &'istr mut BufReader<In>, in_file_name: &str) -> Self {
        let include_dirs = vec!["./include/".into()];

        Self {
            in_stream: in_stream,
            defines: HashMap::with_capacity(16),
            include_dirs,
            in_file_name: Arc::new(in_file_name.to_string()),
            line_num: 0,
        }
    }

    pub fn take_defines(self) -> HashMap<String, Define> {
        self.defines
    }

    pub fn run(&mut self) -> Result<PreprocessorOut, PreprocessorError> {
        let mut in_line = String::with_capacity(80);
        let mut is_in_multiline = false;

        let mut out = PreprocessorOut::new();

        loop {
            self.line_num += 1;
            let read_size = self.in_stream.read_line(&mut in_line).map_err(|e| PreprocessorError::io(self.line_num, e))?;

            if read_size == 0 {
                if in_line.is_empty() {
                    PreprocessorError::unexpected_eof(self.line_num);
                }

                return Ok(out);
            }

            // Prevent mangling of multiline comments
            if (is_in_multiline ||in_line.contains(MULTILINE_COMMENT_START_DELEM)) && !in_line.contains(MULTILINE_COMMENT_END_DELEM) {
                is_in_multiline = true;
                continue;
            } else {
                is_in_multiline = false;
            }

            let in_line_no_comments = COMMENT_REGEX.replace_all(&in_line, "");
            let in_line_trimmed = in_line_no_comments.trim();
            let in_line_split = split_whitespace_quote_respecting(&in_line_trimmed);

            if in_line_trimmed.is_empty() {
                continue;
            }

            let mut tokens: VecDeque<_> = Self::split_operators(in_line_split.map(|x| x.unwrap()).collect());

            if let Some(last_token) = tokens.back() && last_token.ends_with(MULTILINE_DELEM) {
                let i_line_end = in_line.rfind(MULTILINE_DELEM).unwrap();
                in_line.truncate(i_line_end);
                continue;
            }

            if let Some(first_token) = tokens.front() && first_token.starts_with(DIRECTIVE_DELEM) {
                let first_token = tokens.pop_front().unwrap();
                let directive_str = &first_token[1..];

                match Directive::from(directive_str) {
                    Directive::Unknown => {
                        return Err(PreprocessorError::unknown_directive(self.line_num, directive_str));
                    }
                    Directive::Define => { self.process_define(tokens)?; },
                    Directive::Undef => {
                        let token = tokens.pop_front().ok_or(PreprocessorError::define_missing_token(self.line_num))?;

                        if self.defines.remove(token).is_none() {
                            return Err(PreprocessorError::unknown_define(self.line_num, token))
                        }
                    }
                    Directive::Include => {
                        let rev_priority = tokens.front().unwrap_or(&"").starts_with(STRING_DELEM);

                        let filename = PathBuf::from(self.get_string_from_tokens(tokens, true)?);

                        log::debug!("Including file {}", filename.as_os_str().to_str().unwrap());

                        self.include_file(filename, rev_priority)?;
                    }
                    Directive::Error => {
                        let error = self.get_string_from_tokens(tokens, false)?;

                        return Err(PreprocessorError::error_directive(self.line_num, error));
                    }
                    _ => {
                        todo!()
                    }
                }
            } else {
                let out_tokens = self.preprocess_tokens(tokens, false)?;

                out.push_line(self.line_num, self.in_file_name.clone(), out_tokens);
            }

            in_line.clear();
        } 
    }

    fn process_define(&mut self, mut tokens: VecDeque<&str>) -> Result<(), PreprocessorError> {
        let token = tokens.pop_front().ok_or(PreprocessorError::define_missing_token(self.line_num))?;
        let has_params = tokens.front().unwrap_or(&"").starts_with(PARAM_START_DELEM);

        let params = match has_params {
            true => {
                let params = self.get_params(&mut tokens)?;

                let params_iter = params
                    .iter()
                    .enumerate()
                    .map(|(i_param, param)| {
                        let param_name: String = param.iter().map(|x| x.clone()).collect();
                        (param_name, i_param)
                    });
                
                Some(HashMap::from_iter(params_iter))
            },
            false => None,
        };

        let param_count = params.as_ref().map(|x: &HashMap<_, _>| x.len()).unwrap_or_default();
        let mut body = Vec::with_capacity(param_count + 1);
        let mut param_instances = None;
        
        if let Some(params) = &params {
            let (mut tokens, instances) = self.remove_params(tokens, params)?; 

            let mut i_last_split = 0;

            for param_instance in instances {
                let i_split = param_instance.i_insert - i_last_split;
                i_last_split = param_instance.i_insert;

                let new_tokens = tokens.split_off(i_split);
                let body_string = self.preprocess_tokens(tokens, false)?;

                body.push(body_string);
                param_instances.get_or_insert(Vec::with_capacity(param_count)).push(param_instance);

                tokens = new_tokens;
            }
        } else {
            body.push(self.preprocess_tokens(tokens, false)?)
        }

        let define = Define { param_map: params, param_instances, body };

        self.defines.insert(token.to_string(), define);

        Ok(())
    }

    fn get_string_from_tokens(&self, tokens: VecDeque<&str>, is_in_include_dir: bool) -> Result<String, PreprocessorError> {
        let string_seq: String = self.preprocess_tokens(tokens, true)?.iter().map(|x| x.clone()).collect();

        let seq_close_char = match string_seq.starts_with(STRING_DELEM) {
            true => STRING_DELEM,
            false => {
                if !is_in_include_dir || !string_seq.starts_with(INCLUDE_START_DELEM) {
                    return Err(PreprocessorError::unexpected_token(self.line_num, &string_seq));
                }

                INCLUDE_END_DELEM
            }
        };

        if !string_seq.ends_with(seq_close_char) {
            return Err(PreprocessorError::unclosed_string(self.line_num));
        }

        let string = string_seq[1..string_seq.len() - 1].to_owned();

        Ok(string)
    }

    /// `#include <foo.h>` is normal priority and searches include directories first
    /// `#include "foo.h"` is reverse priority and searches local directories first
    fn include_file(&mut self, filename: PathBuf, rev_priority: bool) -> Result<(), PreprocessorError> {
        let paths: Vec<_> = match rev_priority {
            true => self.include_dirs.clone().into_iter().chain(once_with(|| PathBuf::from("./"))).collect(),
            false => once_with(|| PathBuf::from("./")).chain(self.include_dirs.clone().into_iter().rev()).collect(),
        };
        
        for mut path in paths.into_iter() {
            if path.is_dir() {
                path.push(filename.clone());

                if path.is_file() {
                    let mut in_stream = BufReader::new(File::open(path.clone()).map_err(|e| PreprocessorError::io(self.line_num, e))?);

                    let mut include_preprocessor = Preprocessor::new(&mut in_stream, path.to_str().unwrap_or_default());

                    include_preprocessor.run()?;

                    for (id, define) in include_preprocessor.take_defines() {
                        self.defines.insert(id, define);
                    }

                    return Ok(())
                }
            }
        }

        Err(PreprocessorError::file_not_found(self.line_num, filename))
    }

    fn remove_params<'a>(&mut self, tokens: VecDeque<&'a str>, params: &HashMap<String, usize>) -> Result<(VecDeque<&'a str>, Vec<ParamInstance>), PreprocessorError> {
        let mut out_tokens = VecDeque::with_capacity(tokens.len());
        let mut param_instances = Vec::with_capacity(params.len());

        let mut split_count = 0;
        let mut is_in_string = false;
        let mut escape_next = false;

        for (i_token, mut token) in tokens.into_iter().enumerate() {
            if token.starts_with(STRING_DELEM) && !escape_next {
                is_in_string = !is_in_string;
            }
            
            let quote = token.starts_with(PARAM_STRINGIFY_DELEM);

            if quote {
                token = &token[1..];
            }

            if !is_in_string && params.contains_key(token) {
                param_instances.push(ParamInstance { param_key: token.to_string(), quote, i_insert: i_token - split_count });
                split_count += 1;
            } else {
                if quote {
                    return Err(PreprocessorError::unexpected_token(self.line_num, PARAM_STRINGIFY_DELEM.to_string().as_str()))
                }

                out_tokens.push_back(token);
            }
            
            if token.starts_with(ESCAPE_DELEM) && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }

        }

        Ok((out_tokens, param_instances))
    }

    /// Set is_in_include_dir to treat "<" and ">" as string delemiters
    fn preprocess_tokens(&self, mut tokens: VecDeque<&str>, is_in_include_dir: bool) -> Result<Tokens, PreprocessorError> {
        let mut out = Vec::with_capacity(80);

        // Ignore defines when in strings
        let mut string_combination_buffer = String::with_capacity(80);
        let mut escape_next = false;

        loop {
            let token = match tokens.pop_front() {
                Some(token) => token,
                None => {break;},
            };

            let is_string_delem = token.starts_with(STRING_DELEM) || (is_in_include_dir && (token.starts_with(INCLUDE_START_DELEM) || token.starts_with(INCLUDE_END_DELEM)));

            // Combine strings into one token
            if is_string_delem && !escape_next {
                if !string_combination_buffer.is_empty() {
                    string_combination_buffer.push_str(token);
                    if is_in_include_dir && token.starts_with(INCLUDE_START_DELEM) {
                        return Err(PreprocessorError::unexpected_token(self.line_num, &INCLUDE_START_DELEM.to_string()));
                    }
                    
                    if is_in_include_dir && token.starts_with(INCLUDE_END_DELEM) {
                        return Err(PreprocessorError::unexpected_token(self.line_num, &INCLUDE_END_DELEM.to_string()));
                    }

                    out.push(string_combination_buffer.clone());

                    string_combination_buffer.clear();

                    continue;
                }
                
                string_combination_buffer.push_str(token);
            } else if !string_combination_buffer.is_empty() {
                string_combination_buffer.push_str(token);
            }

            let is_in_string = !string_combination_buffer.is_empty();
                
            if let Some(define) = self.defines.get(token) && !is_in_string {
                let params = match define.param_map.is_some() {
                    true => Some(self.get_params(&mut tokens)?),
                    false => None,
                };
                out.append(&mut define.body_to_tokens(self.line_num, params)?);
            } else if !is_in_string {
                out.push(token.to_string());
            }

            if token.starts_with(ESCAPE_DELEM) && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }
        }

        Ok(out)
    }

    fn get_params(&self, tokens: &mut VecDeque<&str>) -> Result<Vec<Tokens>, PreprocessorError> {
        if tokens.pop_front().map(|x: &str| !x.starts_with(PARAM_START_DELEM)).unwrap_or(true) {
            return Err(PreprocessorError::missing_parenthesis(self.line_num));
        }

        let mut params = Vec::with_capacity(4);
        let mut current_param = Vec::with_capacity(16);
        let mut depth = 0;

        loop {
            let token = tokens.pop_front().ok_or(PreprocessorError::unclosed_parenthesis(self.line_num))?;

            if token.starts_with(PARAM_START_DELEM) {
                depth += 1;
                continue;
            }

            if token.starts_with(PARAM_END_DELEM) {
                if depth == 0 {
                    params.push(current_param);

                    return Ok(params);
                }

                depth -= 1;
                continue;
            }

            if token.starts_with(PARAM_SEP_DELEM) && depth == 0 {
                params.push(current_param.clone());
                current_param.clear();
                continue;
            }

            current_param.push(token.to_owned());
        }
    }

    fn split_operators(tokens: VecDeque<&str>) -> VecDeque<&str> { 
        let mut out_tokens = VecDeque::with_capacity(tokens.len());

        for token in tokens {
            let match_indexes = OPERATOR_REGEX.find_iter(token)
                .map(|m| {
                    let m = m.unwrap();
                    [m.start(), m.end()]
                })
                .flatten()
                .chain(once_with(|| token.len()));

            let mut token_str = token;
            let mut i_match_last: usize = 0;

            for i_match in match_indexes {
                let (out_token, new_token_string) = token_str.split_at(i_match - i_match_last);

                if !out_token.is_empty() {
                    out_tokens.push_back(out_token);
                }
                token_str = new_token_string;
                i_match_last = i_match;
            }
        }

        out_tokens
    }
}

#[cfg(test)]
mod tests {
    use std::{io::{BufReader, BufWriter}, sync::Arc};

    use crate::preprocessor::{LineTokenGroup, Preprocessor, PreprocessorError, PreprocessorOut};
    
    #[test]
    fn test_preprocessor_hello_world() {
        let mut test_in = BufReader::new(r#"#include "include/test.h"
/* This is a test of all basic
 * preprocessor functionality
 * and stuff */

int main ( void ) {
    printf ( HELLO ) ;
}"#.as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        // The preprocessor also removes redundant whitespace
        let string_expected =
r#"int main ( void ) {
printf ( "Hello, World!" ) ;
}"#;

        let file_name = Arc::new("".to_string());

        let ppout_expected = PreprocessorOut { token_groups: vec![
            LineTokenGroup { line_num: 6, file_name: file_name.clone(), tokens: vec![
                "int".to_string(),
                "main".to_string(),
                "(".to_string(),
                "void".to_string(),
                ")".to_string(),
                "{".to_string(),
            ] },
            LineTokenGroup { line_num: 7, file_name: file_name.clone(), tokens: vec![
                "printf".to_string(),
                "(".to_string(),
                "\"Hello, World!\"".to_string(),
                ")".to_string(),
                ";".to_string(),
            ] },
            LineTokenGroup { line_num: 8, file_name: file_name.clone(), tokens: vec![
                "}".to_string(),
            ] },
        ]};

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        let ppout = preprocessor.run().unwrap();

        assert_eq!(ppout, ppout_expected);

        ppout.write_to(&mut test_out).unwrap();

        assert_eq!(string_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }

    #[test]
    fn test_preprocessor_no_directive() {
        let mut test_in = BufReader::new("static char foo = 1;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        // The preprocessor places spaces between all operators
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }

    #[test]
    fn test_preprocessor_define_no_param() {
        let mut test_in = BufReader::new("#define bar 1\nstatic char foo = bar;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_one_param() {
        let mut test_in = BufReader::new("#define bar(bax) bax\nstatic char foo = bar(1);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_param() {
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) bax + boo + boing + boo\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 + 123 + 12345 + 123 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
#[test]
    fn test_preprocessor_define_multi_param_stringify() {
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) #bax + #boo + #boing + #boo\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = \"1\" + \"123\" + \"12345\" + \"123\" ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_define() {
        let mut test_in = BufReader::new("#define bar 1\n#define boo bar+1\nstatic char foo = boo;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 + 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_define_strings() {
        let mut test_in = BufReader::new("#define bar \"\\\"bat\"\n#define boo \"boo\" bar\nstatic char* foo = boo;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char * foo = \"boo\" \"\\\"bat\" ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_line_multi_param() {
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) boo + bax + \\\n boing\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 123 + 1 + 12345 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        preprocessor.run().unwrap().write_to(&mut test_out).unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }

    #[test]
    fn test_preprocessor_error_directive() {
        let mut test_in = BufReader::new("#error \"foo\"".as_bytes());
        let test_expected = PreprocessorError::error_directive(1, "foo".to_string());

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        let result = preprocessor.run().unwrap_err();

        assert_eq!(test_expected, result);
    }
}
