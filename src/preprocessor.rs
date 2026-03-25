use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter, write};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::io::Error as IoError;
use std::iter::once_with;
use std::path::PathBuf;
use std::sync::{LazyLock};

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

#[derive(Debug, Clone, Copy)]
pub enum Directive {
    Unknown,
    Include,
    Define,
    Undef,
    If,
    IfDef,
    IfNDef,
    Error,
}

static DIRECTIVE_MAP: LazyLock<HashMap<&'static str, Directive>> = LazyLock::new(|| {
    HashMap::from_iter([
        ("INCLUDE", Directive::Include),
        ("DEFINE", Directive::Define)
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
}

impl Display for PpErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PpErrId::Io(e) => write!(f, "io error: {}", e),
            PpErrId::UnknownDirective(directive) => write!(f, "unknown directive '{}'", directive),
            PpErrId::UnknownParam(param) => write!(f, "unknown param '{}'", param),
            PpErrId::UnexpectedToken(token) => write!(f, "unexpected token '{}'", token),
            PpErrId::DefineMissingToken => write!(f, "define is missing a token"),
            PpErrId::MissingParenthesis => write!(f, "macro missing parenthesis"),
            PpErrId::MissingParameter => write!(f, "macro missing parameters"),
            PpErrId::UnclosedParenthesis => write!(f, "unclosed parenthesis"),
            PpErrId::UnclosedString => write!(f, "unclosed string"),
            PpErrId::UnexpectedEof => write!(f, "unexpected end of file"),
            PpErrId::FileNotFound(filename) => write!(f, "unable to find '{}'", filename),
        }
    }
}

impl Debug for PpErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

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
    pub fn io(line_num: usize, e: IoError) -> Self {
        Self { line_num, id: PpErrId::Io(e) }
    }

    pub fn unknown_directive(line_num: usize, directive: &str) -> Self {
        Self { line_num, id: PpErrId::UnknownDirective(directive.to_string()) }
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

#[derive(Debug)]
pub struct Define {
    pub params: Option<HashMap<String, usize>>,
    /// Params will be placed in between each body vec string
    pub param_instances: Option<Vec<String>>,
    pub body: Vec<String>,
}

impl Define {
    pub fn body_to_string(&self, line_num: usize, params: Option<Vec<String>>) -> Result<String, PreprocessorError> {
        if params.as_ref().map(|x: &Vec<_>| x.len()).unwrap_or_default() == 0 {
            return Ok(self.body.first().unwrap_or(&String::new()).clone());
        }

        let params = params.unwrap();

        let mut out = String::with_capacity(80);

        for (i_param, body_string) in self.body.iter().enumerate() {
            if !out.is_empty() {
                out.push(' ');
            }

            out.push_str(body_string);

            if !out.is_empty() {
                out.push(' ');
            }

            let param_string = params.get(i_param).ok_or(PreprocessorError::missing_parameter(line_num))?;

            out.push_str(param_string);
        }

        Ok(out)
    }
}

pub struct Preprocessor<'istr, 'ostr, In: Read, Out: Write> {
    in_stream: &'istr mut BufReader<In>,
    out_stream: &'ostr mut BufWriter<Out>,
    defines: HashMap<String, Define>,
    include_dirs: Vec<PathBuf>,
    /// Incremented as the preprocessor goes through the in stream
    line_num: usize,
}

impl<'istr, 'ostr, In: Read, Out: Write + Debug> Preprocessor<'istr, 'ostr, In, Out> {
    pub fn new(in_stream: &'istr mut BufReader<In>, out_stream: &'ostr mut BufWriter<Out>) -> Self {
        let include_dirs = vec!["./include/".into()];

        Self {
            in_stream: in_stream,
            out_stream: out_stream,
            defines: HashMap::with_capacity(16),
            include_dirs,
            line_num: 0,
        }
    }

    pub fn take_defines(self) -> HashMap<String, Define> {
        self.defines
    }

    pub fn run(&mut self) -> Result<(), PreprocessorError> {
        let mut in_line = String::with_capacity(80);
        let mut line_written = false;

        loop {
            self.line_num += 1;
            let read_size = self.in_stream.read_line(&mut in_line).map_err(|e| PreprocessorError::io(self.line_num, e))?;

            if read_size == 0 {
                if in_line.is_empty() {
                    PreprocessorError::unexpected_eof(self.line_num);
                }

                return Ok(());
            }

            // Prevent mangling of multiline comments
            if in_line.contains(MULTILINE_COMMENT_START_DELEM) && !in_line.contains(MULTILINE_COMMENT_END_DELEM) {
                continue;
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
                    Directive::Define => {
                        let token = tokens.pop_front().ok_or(PreprocessorError::define_missing_token(self.line_num))?;
                        let has_params = tokens.front().unwrap_or(&"").contains(PARAM_START_DELEM);

                        let params = match has_params {
                            true => {
                                let params = self.get_params(&mut tokens)?;

                                let params_iter = params
                                    .iter()
                                    .enumerate()
                                    .map(|(i_param, param)| (param.clone(), i_param));
                                
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

                            for (param, i_param) in instances {
                                let i_split = i_param - i_last_split;

                                let new_tokens = tokens.split_off(i_split);
                                let body_string = self.preprocess_tokens(tokens, false)?;

                                body.push(body_string);
                                param_instances.get_or_insert(Vec::with_capacity(param_count)).push(param);

                                tokens = new_tokens;
                                i_last_split = i_param;
                            }
                        } else {
                            body.push(self.preprocess_tokens(tokens, false)?)
                        }

                        let define = Define { params, param_instances, body };

                        self.defines.insert(token.to_string(), define);
                    }
                    Directive::Include => {
                        let filename_seq = self.preprocess_tokens(tokens, true)?;

                        let seq_close_char = match filename_seq.starts_with(STRING_DELEM) {
                            true => STRING_DELEM,
                            false => {
                                if !filename_seq.starts_with(INCLUDE_START_DELEM) {
                                    return Err(PreprocessorError::unexpected_token(self.line_num, &filename_seq));
                                }

                                INCLUDE_END_DELEM
                            }
                        };

                        if !filename_seq.ends_with(seq_close_char) {
                            return Err(PreprocessorError::unclosed_string(self.line_num));
                        }

                        let rev_priority = seq_close_char == STRING_DELEM;

                        let filename = PathBuf::from(&filename_seq[1..filename_seq.len() - 1]);

                        log::debug!("Including file {}", filename.as_os_str().to_str().unwrap());

                        self.include_file(filename, rev_priority)?;
                    }
                    _ => {
                        todo!()
                    }
                }
            } else {
                let out_string = self.preprocess_tokens(tokens, false)?;

                // If we have already written a line, prefix this line with a newline
                if line_written {
                    write!(self.out_stream, "\n").map_err(|e| PreprocessorError::io(self.line_num, e))?;
                }

                write!(self.out_stream, "{}", out_string).map_err(|e| PreprocessorError::io(self.line_num, e))?;

                line_written = true;
            }

            in_line.clear();
        } 
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
                    let mut in_stream = BufReader::new(File::open(path).map_err(|e| PreprocessorError::io(self.line_num, e))?);

                    let mut include_preprocessor = Preprocessor::new(&mut in_stream, self.out_stream);

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

    fn remove_params<'a>(&mut self, tokens: VecDeque<&'a str>, params: &HashMap<String, usize>) -> Result<(VecDeque<&'a str>, Vec<(String, usize)>), PreprocessorError> {
        let mut out_tokens = VecDeque::with_capacity(tokens.len());
        let mut param_instances = Vec::with_capacity(params.len());

        let mut split_count = 0;
        let mut is_in_string = false;
        let mut escape_next = false;

        for (i_token, token) in tokens.into_iter().enumerate() {
            if token.contains(STRING_DELEM) && !escape_next {
                is_in_string = !is_in_string;
            }

            if !is_in_string && params.contains_key(token) {

                param_instances.push((token.to_string(), i_token - split_count));
                split_count += 1;
            } else {
                out_tokens.push_back(token);
            }
            
            if token.contains(ESCAPE_DELEM) && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }
        }

        Ok((out_tokens, param_instances))
    }

    /// Set is_in_include_dir to treat "<" and ">" as string delemiters
    fn preprocess_tokens(&mut self, mut tokens: VecDeque<&str>, is_in_include_dir: bool) -> Result<String, PreprocessorError> {
        let mut out = String::with_capacity(80);

        // Ignore defines when in strings
        let mut is_in_string = false;
        let mut escape_next = false;

        loop {
            let token = match tokens.pop_front() {
                Some(token) => token,
                None => {break;},
            };

            if !out.is_empty() && !out.ends_with(' ') && !is_in_string {
                out.push(' ');
            }

            if token.contains(STRING_DELEM) && !escape_next {
                is_in_string = !is_in_string;
            }

            if is_in_include_dir && token.contains(INCLUDE_START_DELEM) {
                if is_in_string {
                    return Err(PreprocessorError::unexpected_token(self.line_num, &INCLUDE_START_DELEM.to_string()));
                }

                is_in_string = true;
            }
            
            if is_in_include_dir && token.contains(INCLUDE_END_DELEM) {
                if !is_in_string {
                    return Err(PreprocessorError::unexpected_token(self.line_num, &INCLUDE_END_DELEM.to_string()));
                }

                is_in_string = false;
            }
                
            if let Some(define) = self.defines.get(token) && !is_in_string {
                let params = match define.params.is_some() {
                    true => Some(self.get_params(&mut tokens)?),
                    false => None,
                };
                out.push_str(define.body_to_string(self.line_num, params)?.as_str());
            } else {
                out.push_str(token);
            }

            if token.contains(ESCAPE_DELEM) && !escape_next {
                escape_next = true;
            } else {
                escape_next = false;
            }
        }

        Ok(out)
    }

    fn get_params(&self, tokens: &mut VecDeque<&str>) -> Result<Vec<String>, PreprocessorError> {
        if tokens.pop_front().map(|x: &str| !x.contains(PARAM_START_DELEM)).unwrap_or(true) {
            return Err(PreprocessorError::missing_parenthesis(self.line_num));
        }

        let mut params = Vec::with_capacity(4);
        let mut current_param = String::with_capacity(16);
        let mut depth = 0;

        loop {
            let token = tokens.pop_front().ok_or(PreprocessorError::unclosed_parenthesis(self.line_num))?;

            if token.contains(PARAM_START_DELEM) {
                depth += 1;
                continue;
            }

            if token.contains(PARAM_END_DELEM) {
                if depth == 0 {
                    params.push(current_param);

                    return Ok(params);
                }

                depth -= 1;
                continue;
            }

            if token.contains(PARAM_SEP_DELEM) && depth == 0 {
                params.push(current_param.clone());
                current_param.clear();
                continue;
            }

            current_param.push_str(token);
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
    use std::io::{BufReader, BufWriter};

    use crate::preprocessor::Preprocessor;
    
    #[test]
    fn test_preprocessor_hello_world() {
        let mut test_in =
BufReader::new(r#"#include "include/test.h"

/* This is a test of all basic
 * preprocessor functionality and stuff */

int main ( void ) {
    printf ( HELLO ) ;
}"#.as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        // The preprocessor also removes redundant whitespace
        let test_expected =
r#"int main ( void ) {
printf ( "Hello, World!" ) ;
}"#;

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }

    #[test]
    fn test_preprocessor_no_directive() {
        let mut test_in = BufReader::new("static char foo = 1;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        // The preprocessor places spaces between all operators
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }

    #[test]
    fn test_preprocessor_define_no_param() {
        let mut test_in = BufReader::new("#define bar 1\nstatic char foo = bar;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_one_param() {
        let mut test_in = BufReader::new("#define bar(bax) bax\nstatic char foo = bar(1);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_param() {
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) bax + boo + boing\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 + 123 + 12345 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_define() {
        let mut test_in = BufReader::new("#define bar 1\n#define boo bar+1\nstatic char foo = boo;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 + 1 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_define_strings() {
        let mut test_in = BufReader::new("#define bar \"\\\"bat\"\n#define boo \"boo\" bar\nstatic char* foo = boo;".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char * foo = \"boo\" \"\\\"bat\" ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
    
    #[test]
    fn test_preprocessor_define_multi_line_multi_param() {
        let mut test_in = BufReader::new("#define bar(bax, boo, boing) bax + boo + \\\n boing\nstatic char foo = bar(1, 123, 12345);".as_bytes());
        let mut test_out = BufWriter::new(Vec::new());
        let test_expected = "static char foo = 1 + 123 + 12345 ;";

        let mut preprocessor = Preprocessor::new(&mut test_in, &mut test_out);

        preprocessor.run().unwrap();

        assert_eq!(test_expected, str::from_utf8(&test_out.into_inner().unwrap()).unwrap());
    }
}
