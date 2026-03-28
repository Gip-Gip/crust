use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::io::Error as IoError;
use std::iter::once_with;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use crate::{COMMENT_REGEX, OPERATOR_REGEX, WHITESPACE_REGEX};

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

pub fn preprocess<In: Read>(in_stream: &mut In, in_file_name: &str) -> Result<PreprocessorOut, PreprocessorError> {
    let mut buff_in = BufReader::new(in_stream);
    let mut preprocessor = Preprocessor::new(&mut buff_in, in_file_name);

    preprocessor.run()
}

fn combine_tokens_into_string<'a, PpTokenIter: Iterator<Item = &'a PpToken>>(tokens: PpTokenIter, separator: &str) -> String {
    let mut out = String::with_capacity(80);

    for token in tokens {
        if !out.is_empty() {
            out.push_str(separator);
        }

        out.push_str(&token.value);
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
    DefineMissingPpToken,
    UnknownParam(String),
    UnexpectedPpToken(String),
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
            PpErrId::UnexpectedPpToken(token) => write!(f, "unexpected token '{}'", token),
            PpErrId::DefineMissingPpToken => write!(f, "define is missing a token"),
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
        Self { line_num, id: PpErrId::UnexpectedPpToken(token.to_string()) }
    }

    pub fn define_missing_token(line_num: usize) -> Self {
        Self { line_num, id: PpErrId::DefineMissingPpToken }
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

#[repr(u8)]
#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
pub enum ValueType {
    #[default]
    Operator,
    Numeric,
    NonNumeric,
    StringLiteral,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PpToken {
    pub value: String,
    pub ws_leading: bool,
    pub ws_tailing: bool,
    pub value_type: ValueType,
}

impl PpToken {
    pub fn new(value: String) -> Self {
        Self { value, ..Default::default() }
    }

    pub fn ws_leading(mut self, ws_leading: bool) -> Self {
        self.ws_leading = ws_leading;

        self
    }

    pub fn ws_tailing(mut self, ws_tailing: bool) -> Self {
        self.ws_tailing = ws_tailing;

        self
    }

    pub fn value_type(mut self, value_type: ValueType) -> Self {
        self.value_type = value_type;

        self
    }
}

pub type PpTokens = Vec<PpToken>;

#[derive(Debug, Clone)]
pub struct ParamInstance {
    pub param_key: String,
    pub stringify: bool,
    pub i_insert: usize,
}

#[derive(Debug)]
pub struct Define {
    pub param_map: Option<HashMap<String, usize>>,
    /// Params will be placed in between each body vec string
    pub param_instances: Option<Vec<ParamInstance>>,
    pub body: Vec<PpTokens>,
}

impl Define {
    pub fn body_to_tokens(&self, line_num: usize, params: Option<Vec<PpTokens>>) -> Result<PpTokens, PreprocessorError> {
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

            match param_instance.stringify {
                true => {
                    let token = PpToken::new(format!("\"{}\"", combine_tokens_into_string(param_tokens.into_iter(), " ")))
                        .value_type(ValueType::StringLiteral);
                    out.push(token);
                }
                false => { out.extend_from_slice(param_tokens); }
            }
        }

        Ok(out)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct LineIndex {
    pub line_num: usize,
    pub file_name: Arc<String>,
    pub i_line_end: usize,
}

#[derive(Debug, PartialEq)]
pub struct PreprocessorOut {
    pub line_indexes: Vec<LineIndex>,
    pub tokens: PpTokens,
}

impl PreprocessorOut {
    pub fn new() -> Self {
        Self {
            line_indexes: Vec::with_capacity(128),
            tokens: Vec::with_capacity(1024),
        }
    }

    pub fn push_line(&mut self, line_num: usize, file_name: Arc<String>, mut tokens: PpTokens) {
        self.line_indexes.push(LineIndex { line_num, file_name, i_line_end: self.tokens.len() + tokens.len() });
        self.tokens.append(&mut tokens);
    }

    pub fn write_to<Out: Write>(&self, mut out: Out) -> Result<(), IoError>{
        let mut line_written = false;
        let mut i_line_start = 0;

        for line in self.line_indexes.iter() {
            if line_written {
                writeln!(out, "")?;
            } else {
                line_written = true;
            }

            write!(out, "{}", combine_tokens_into_string(self.tokens[i_line_start..line.i_line_end].iter(), " "))?;

            i_line_start = line.i_line_end;
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

    fn tokenize(in_string: &str) -> VecDeque<PpToken> {
        let mut out_tokens = VecDeque::with_capacity(16);

        let whitespace_split = WHITESPACE_REGEX.split(in_string);

        for ws_stripped_tokens in whitespace_split {
            let mut tokens = Self::split_operators(ws_stripped_tokens.unwrap());

            if let Some(front) = tokens.front_mut() {
                front.ws_leading = true;
            }

            if let Some(back) = tokens.back_mut() {
                back.ws_tailing = true;
            }

            for token in tokens.iter_mut() {
                if token.value.starts_with(|c: char| c.is_numeric()) {
                    token.value_type = ValueType::Numeric;
                    continue;
                }

                if token.value.starts_with(|c: char| c == '_' || c.is_alphabetic()) {
                    token.value_type = ValueType::NonNumeric;
                }
            }

            out_tokens.append(&mut tokens);
        }

        out_tokens
    }

    fn split_operators(in_string: &str) -> VecDeque<PpToken> { 
        let mut out_tokens = VecDeque::with_capacity(4);

        let match_indexes = OPERATOR_REGEX.find_iter(in_string)
            .map(|m| {
                let m = m.unwrap();
                [m.start(), m.end()]
            })
            .flatten()
            .chain(once_with(|| in_string.len()));

        let mut window_str = in_string;
        let mut i_match_last: usize = 0;

        for i_match in match_indexes {
            let (out_token, new_window_string) = window_str.split_at(i_match - i_match_last);

            if !out_token.is_empty() {
                out_tokens.push_back(PpToken::new(out_token.to_string()));
            }
            window_str = new_window_string;
            i_match_last = i_match;
        }

        out_tokens
    }

    pub fn run(&mut self) -> Result<PreprocessorOut, PreprocessorError> {
        let mut in_line = String::with_capacity(80);
        let mut is_in_multiline = false;

        let mut out = PreprocessorOut::new();

        loop {
            self.line_num += 1;
            let read_size = self.in_stream.read_line(&mut in_line).map_err(|e| PreprocessorError::io(self.line_num, e))?;

            if read_size == 0 {
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

            if in_line_trimmed.is_empty() {
                continue;
            }

            let mut tokens = Self::tokenize(in_line_trimmed);

            if let Some(last_token) = tokens.back() && last_token.value.ends_with(MULTILINE_DELEM) {
                let i_line_end = in_line.rfind(MULTILINE_DELEM).unwrap();
                in_line.truncate(i_line_end);
                continue;
            }

            if let Some(first_token) = tokens.front() && first_token.value.starts_with(DIRECTIVE_DELEM) {
                let first_token = tokens.pop_front().unwrap();
                let directive_str = &first_token.value[1..];

                match Directive::from(directive_str) {
                    Directive::Unknown => {
                        return Err(PreprocessorError::unknown_directive(self.line_num, directive_str));
                    }
                    Directive::Define => { self.process_define(tokens)?; },
                    Directive::Undef => {
                        let token = tokens.pop_front().ok_or(PreprocessorError::define_missing_token(self.line_num))?;

                        if self.defines.remove(&token.value).is_none() {
                            return Err(PreprocessorError::unknown_define(self.line_num, &token.value))
                        }
                    }
                    Directive::Include => {
                        let rev_priority = tokens.front().unwrap_or(&PpToken::default()).value.starts_with(STRING_DELEM);

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

    fn process_define(&mut self, mut tokens: VecDeque<PpToken>) -> Result<(), PreprocessorError> {
        let token = tokens.pop_front().ok_or(PreprocessorError::define_missing_token(self.line_num))?;
        let has_params = !token.ws_tailing && tokens.front().unwrap_or(&PpToken::default()).value.starts_with(PARAM_START_DELEM);

        let params = match has_params {
            true => {
                let params = self.get_params(&mut tokens)?;

                let params_iter = params
                    .iter()
                    .enumerate()
                    .map(|(i_param, param)| {
                        let param_name: String = param.iter().map(|x| x.value.clone()).collect();
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

        self.defines.insert(token.value, define);

        Ok(())
    }

    fn get_string_from_tokens(&self, tokens: VecDeque<PpToken>, is_in_include_dir: bool) -> Result<String, PreprocessorError> {
        let string_seq: String = self.preprocess_tokens(tokens, true)?.iter().map(|x| x.value.clone()).collect();

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

    fn remove_params(&mut self, tokens: VecDeque<PpToken>, params: &HashMap<String, usize>) -> Result<(VecDeque<PpToken>, Vec<ParamInstance>), PreprocessorError> {
        let mut out_tokens = VecDeque::with_capacity(tokens.len());
        let mut param_instances = Vec::with_capacity(params.len());

        let mut split_count = 0;
        let mut is_in_string = false;
        let mut escape_next = false;

        for (i_token, mut token) in tokens.into_iter().enumerate() {
            if token.value.starts_with(STRING_DELEM) && !escape_next {
                is_in_string = !is_in_string;
            }
            
            let stringify = token.value.starts_with(PARAM_STRINGIFY_DELEM);
            let escape_delem = token.value.starts_with(ESCAPE_DELEM);

            if stringify {
                token.value = token.value.strip_prefix(PARAM_STRINGIFY_DELEM).unwrap_or(&token.value).to_string();
            }

            if !is_in_string && params.contains_key(&token.value) {
                param_instances.push(ParamInstance { param_key: token.value.clone(), stringify, i_insert: i_token - split_count });
                split_count += 1;
            } else {
                if stringify {
                    return Err(PreprocessorError::unexpected_token(self.line_num, PARAM_STRINGIFY_DELEM.to_string().as_str()))
                }

                out_tokens.push_back(token);
            }
            
            if escape_delem && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }

        }

        Ok((out_tokens, param_instances))
    }

    /// Set is_in_include_dir to treat "<" and ">" as string delemiters
    fn preprocess_tokens(&self, mut tokens: VecDeque<PpToken>, is_in_include_dir: bool) -> Result<PpTokens, PreprocessorError> {
        let mut out = Vec::with_capacity(80);

        // Ignore defines when in strings
        let mut string_combination_buffer_opt: Option<PpToken> = None;
        let mut escape_next = false;

        loop {
            let token = match tokens.pop_front() {
                Some(token) => token,
                None => {break;},
            };

            let is_string_delem = token.value.starts_with(STRING_DELEM) || (is_in_include_dir && (token.value.starts_with(INCLUDE_START_DELEM) || token.value.starts_with(INCLUDE_END_DELEM)));
            let is_escape_delem = token.value.starts_with(ESCAPE_DELEM);

            // Combine strings into one token
            if is_string_delem && !escape_next {
                if let Some(string_combination_buffer) = string_combination_buffer_opt.as_mut() {
                    string_combination_buffer.value.push_str(&token.value);
                    string_combination_buffer.ws_tailing = token.ws_tailing;

                    if is_in_include_dir && token.value.starts_with(INCLUDE_START_DELEM) {
                        return Err(PreprocessorError::unexpected_token(self.line_num, &token.value));
                    }

                    out.push(string_combination_buffer_opt.take().unwrap());

                    continue;
                }
                    
                if is_in_include_dir && token.value.starts_with(INCLUDE_END_DELEM) {
                    return Err(PreprocessorError::unexpected_token(self.line_num, &token.value));
                }

                string_combination_buffer_opt = Some(token.clone().ws_tailing(false).value_type(ValueType::StringLiteral));
                
                continue;
            } else if let Some(string_combination_buffer) = string_combination_buffer_opt.as_mut() {
                string_combination_buffer.value.push_str(&token.value);


                if is_escape_delem && !escape_next {
                    escape_next = true;
                } else {
                    escape_next = false;
                }

                continue;
            }

            if let Some(define) = self.defines.get(&token.value) {
                let params = match define.param_map.is_some() {
                    true => Some(self.get_params(&mut tokens)?),
                    false => None,
                };
                let mut body_tokens = define.body_to_tokens(self.line_num, params)?;

                if let Some(first) = body_tokens.first_mut() {
                    first.ws_leading = token.ws_leading;
                }

                if let Some(last) = body_tokens.last_mut() {
                    last.ws_tailing = token.ws_tailing;
                }

                out.append(&mut body_tokens);
            } else {
                out.push(token);
            }

            if is_escape_delem && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }
        }


        Ok(out)
    }

    fn get_params(&self, tokens: &mut VecDeque<PpToken>) -> Result<Vec<PpTokens>, PreprocessorError> {
        if tokens.pop_front().map(|x| !x.value.starts_with(PARAM_START_DELEM)).unwrap_or(true) {
            return Err(PreprocessorError::missing_parenthesis(self.line_num));
        }

        let mut params = Vec::with_capacity(4);
        let mut current_param = Vec::with_capacity(16);
        let mut depth = 0;

        loop {
            let token = tokens.pop_front().ok_or(PreprocessorError::unclosed_parenthesis(self.line_num))?;

            if token.value.starts_with(PARAM_START_DELEM) {
                depth += 1;
                continue;
            }

            if token.value.starts_with(PARAM_END_DELEM) {
                if depth == 0 {
                    params.push(current_param);

                    return Ok(params);
                }

                depth -= 1;
                continue;
            }

            if token.value.starts_with(PARAM_SEP_DELEM) && depth == 0 {
                params.push(current_param.clone());
                current_param.clear();
                continue;
            }

            current_param.push(token.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::{BufReader, BufWriter}, sync::Arc};

    use crate::preprocessor::{LineIndex, PpToken, Preprocessor, PreprocessorError, PreprocessorOut, ValueType};
    
    #[test]
    fn test_preprocessor_hello_world() {
        let mut test_in = BufReader::new(r#"#include "include/test.h"
/* This is a test of all basic
 * preprocessor functionality
 * and stuff */

int main(void) {
    printf(HELLO);
}"#.as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        // The preprocessor also removes redundant whitespace
        let string_expected =
r#"int main ( void ) {
printf ( "Hello, World!" ) ;
}"#;

        let file_name = Arc::new("".to_string());

        let ppout_expected = PreprocessorOut {
            line_indexes: vec! [
                LineIndex { line_num: 6, file_name: file_name.clone(), i_line_end: 6 },
                LineIndex { line_num: 7, file_name: file_name.clone(), i_line_end: 11 },
                LineIndex { line_num: 8, file_name: file_name.clone(), i_line_end: 12 },
            ],
            tokens: vec![
                PpToken::new("int".to_string()).ws_leading(true).ws_tailing(true).value_type(ValueType::NonNumeric),
                PpToken::new("main".to_string()).ws_leading(true).value_type(ValueType::NonNumeric),
                PpToken::new("(".to_string()),
                PpToken::new("void".to_string()).value_type(ValueType::NonNumeric),
                PpToken::new(")".to_string()).ws_tailing(true),
                PpToken::new("{".to_string()).ws_leading(true).ws_tailing(true),
                PpToken::new("printf".to_string()).ws_leading(true).value_type(ValueType::NonNumeric),
                PpToken::new("(".to_string()),
                PpToken::new("\"Hello, World!\"".to_string()).value_type(ValueType::StringLiteral),
                PpToken::new(")".to_string()),
                PpToken::new(";".to_string()).ws_tailing(true),
                PpToken::new("}".to_string()).ws_leading(true).ws_tailing(true),
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
        let mut test_in = BufReader::new("#define bar (1)\nstatic char foo = bar;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = ( 1 ) ;";

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
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) #bax #boo #boing #boo\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = \"1\" \"123\" \"12345\" \"123\" ;";
        let file_name = Arc::new("".to_string());

        let ppout_expected = PreprocessorOut {
            line_indexes: vec! [
                LineIndex { line_num: 2, file_name: file_name.clone(), i_line_end: 9 },
            ], 
            tokens: vec![
                PpToken::new("static".to_string()).ws_leading(true).ws_tailing(true).value_type(ValueType::NonNumeric),
                PpToken::new("char".to_string()).ws_leading(true).ws_tailing(true).value_type(ValueType::NonNumeric),
                PpToken::new("foo".to_string()).ws_leading(true).ws_tailing(true).value_type(ValueType::NonNumeric),
                PpToken::new("=".to_string()).ws_leading(true).ws_tailing(true),
                PpToken::new("\"1\"".to_string()).ws_leading(true).value_type(ValueType::StringLiteral),
                PpToken::new("\"123\"".to_string()).value_type(ValueType::StringLiteral),
                PpToken::new("\"12345\"".to_string()).value_type(ValueType::StringLiteral),
                PpToken::new("\"123\"".to_string()).value_type(ValueType::StringLiteral),
                PpToken::new(";".to_string()).ws_tailing(true),
            ]
        };

        let mut preprocessor = Preprocessor::new(&mut test_in, "");

        let ppout = preprocessor.run().unwrap();

        assert_eq!(ppout, ppout_expected);

        ppout.write_to(&mut test_out).unwrap();

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
