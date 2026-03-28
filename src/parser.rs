use std::{collections::HashMap, fmt::{Debug, Display, Formatter}, sync::{Arc, LazyLock, Mutex}};

use crate::preprocessor::{LineIndex, PpToken, PpTokens, PreprocessorOut, ValueType};

static KEYWORD_MAP: LazyLock<HashMap<&'static str, Keyword>> = LazyLock::new(|| {
    HashMap::from_iter([
        ("auto", Keyword::Auto),
        ("break", Keyword::Break),
        ("case", Keyword::Case),
        ("char", Keyword::Char),
        ("const", Keyword::Const),
        ("continue", Keyword::Continue),
        ("default", Keyword::Default),
        ("do", Keyword::Do),
        ("double", Keyword::Double),
        ("else", Keyword::Else),
        ("enum", Keyword::Enum),
        ("extern", Keyword::Extern),
        ("float", Keyword::Float),
        ("for", Keyword::For),
        ("goto", Keyword::Goto),
        ("if", Keyword::If),
        ("int", Keyword::Int),
        ("long", Keyword::Long),
        ("register", Keyword::Register),
        ("return", Keyword::Return),
        ("short", Keyword::Short),
        ("signed", Keyword::Signed),
        ("sizeof", Keyword::Sizeof),
        ("static", Keyword::Static),
        ("struct", Keyword::Struct),
        ("switch", Keyword::Switch),
        ("typedef", Keyword::Typedef),
        ("union", Keyword::Union),
        ("unsigned", Keyword::Unsigned),
        ("void", Keyword::Void),
        ("volatile", Keyword::Volatile),
        ("while", Keyword::While),
    ].into_iter())
});

#[repr(u8)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Keyword {
    Auto,
    Break,
    Case,
    Char,
    Const,
    Continue,
    Default,
    Do,
    Double,
    Else,
    Enum,
    Extern,
    Float,
    For,
    Goto,
    If,
    Int,
    Long,
    Register,
    Return,
    Short,
    Signed,
    Sizeof,
    Static,
    Struct,
    Switch,
    Typedef,
    Union,
    Unsigned,
    Void,
    Volatile,
    While,
}

impl Keyword {
    pub fn from_token(token: &PpToken) -> Option<Self> {
        KEYWORD_MAP.get(token.value.as_str()).map(|x| *x)
    }
}

#[derive(Debug, PartialEq)]
pub struct StructDefinition {
}

#[derive(Debug, PartialEq)]
pub struct UnionDefinition {
}

#[derive(Debug, PartialEq)]
pub struct ArrayDefinition {
}

#[derive(Debug, PartialEq)]
pub enum Type {
    Array(Box<ArrayDefinition>),
    Char,
    Double,
    Float,
    Int,
    Long,
    LongLong,
    Short,
    StructDefinition(Box<StructDefinition>),
    StructReference(String),
    TypedefReference(String),
    UnionDefinition(Box<UnionDefinition>),
    UnionReference(String),
    UnsignedChar,
    UnsignedInt,
    UnsignedLong,
    UnsignedLongLong,
    UnsignedShort,
    Void,
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, PartialEq)]
pub struct TypeSpecifier {
    /// If the varible is constant, or in the case of pointers, if the pointer
    /// is constant
    pub is_const: bool,
    /// If the indirection to this pointer is const
    pub indir_is_const: bool,
    pub is_volatile: bool,
    /// 1 if the variable is a pointer, 2 if the variable is a pointer to a pointer..
    pub pointer_indirections: u8,
    pub ctype: Type,
}

impl TypeSpecifier {
    pub fn parse(parser: &Parser, tokens: &[PpToken]) -> Result<Self, ParserError> {
        let mut base_type_opt = None;
        let mut is_signed = false;
        let mut is_unsigned = false;
        let mut is_const = false;
        let mut indir_is_const = false;
        let mut is_volatile = false;
        let mut pointer_indirections = 0;
        let mut i_next = 0;
        
        for (i_token, token) in tokens.iter().enumerate() {
            i_next = i_token + 1;

            let keyword = match Keyword::from_token(token) {
                Some(keyword) => keyword,
                None => {
                    if  i_next > 2 || (i_next > 1 && (!is_const || !is_volatile)) {
                        return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                    }

                    base_type_opt = Some(Type::TypedefReference(token.value.clone()));
                    break;
                }
            };

            base_type_opt = Some(match keyword {
                Keyword::Char => Type::Char,
                Keyword::Const => {
                    if is_const { 
                        return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                    }

                    is_const = true;

                    continue;
                }
                Keyword::Double => Type::Double,
                Keyword::Float => Type::Float,
                Keyword::Int => Type::Int,
                Keyword::Long => {
                    if let Some(next_token) = tokens.get(i_token + 1) &&
                        let Some(keyword) = Keyword::from_token(next_token) &&
                        keyword == Keyword::Long {

                        Type::LongLong
                    } else {
                        Type::Long
                    }
                }
                Keyword::Short => Type::Short,
                Keyword::Signed => {
                    if is_signed || is_unsigned {
                        return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                    }

                    is_signed = true;

                    continue;
                }
                Keyword::Struct => {
                    let (i_new_next, struct_type) = Self::parse_struct(parser, &tokens[i_next..])?;

                    i_next = i_new_next;

                    struct_type
                },
                Keyword::Union => {
                    let (i_new_next, union_type) = Self::parse_union(parser, &tokens[i_next..])?;

                    i_next = i_new_next;

                    union_type
                },
                Keyword::Unsigned => {
                    if is_signed || is_unsigned {
                        return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                    }

                    is_unsigned = true;

                    continue;
                }
                Keyword::Void => Type::Void,
                Keyword::Volatile => {
                    if is_volatile {
                        return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                    }

                    is_volatile = true;

                    continue;
                }
                _ => {
                    return Err(ParserError::unexpected_token(parser.get_line_num(), &token.value));
                }
            });
        }

        let base_type = match base_type_opt {
            Some(base_type) => base_type,
            None => {
                return Err(ParserError::unexpected_token(parser.get_line_num(), &tokens.last().unwrap().value));
            }
        };

        let ctype = match (is_unsigned, base_type) {
            (true, Type::Char) => Type::UnsignedChar,
            (true, Type::Int) => Type::UnsignedInt,
            (true, Type::Long) => Type::UnsignedLong,
            (true, Type::LongLong) => Type::UnsignedLong,
            (true, Type::Short) => Type::UnsignedShort,
            (false, ctype) => ctype,
            (true, ctype) => {
                return Err(ParserError::unexpected_token(parser.get_line_num(), &ctype.to_string()));
            }
        };

        // There should only be pointer specifiers and const keywords
        for token in &tokens[i_next..] {
            if matches!(token.value_type, ValueType::Operator) && matches!(Operator::parse(&token.value), Some(Operator::Star)) {
                pointer_indirections += 1;
                continue;
            }

            if matches!(token.value_type, ValueType::NonNumeric) && matches!(Keyword::from_token(token), Some(Keyword::Const)) && !indir_is_const {
                indir_is_const = true;
                continue;
            }

            return Err(ParserError::unexpected_token(parser.get_line_num(), &tokens[i_next].value));
        }

        return Ok(Self {
            is_const,
            is_volatile,
            indir_is_const,
            pointer_indirections,
            ctype,
        })
    }

    pub fn parse_struct(parser: &Parser, tokens: &[PpToken]) -> Result<(usize, Type), ParserError> {
        todo!()
    }
    
    pub fn parse_union(parser: &Parser, tokens: &[PpToken]) -> Result<(usize, Type), ParserError> {
        todo!()
    }
}

pub static OPERATOR_MAP: LazyLock<HashMap<&'static str, Operator>> = LazyLock::new(|| {
    HashMap::from_iter([
        ("(", Operator::ParStart),
        (")", Operator::ParEnd),
        ("{", Operator::BlockStart),
        ("}", Operator::BlockEnd),
        (";", Operator::StatementEnd),
        ("*", Operator::Star),
        (",", Operator::Comma),
    ].into_iter())
});

#[derive(Debug, PartialEq)]
pub enum Associativity {
    LeftToRight,
    RightToLeft,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum Operator {
    ParStart,
    ParEnd,
    BlockStart,
    BlockEnd,
    StatementEnd,
    Star,
    Comma,
    FunctionCall,
    TypeCast,
}

impl Operator {
    pub fn parse(string: &str) -> Option<Operator> {
        OPERATOR_MAP.get(string).map(|x| *x)
    }

    pub fn precidence_and_associativity(&self) -> (u8, Associativity) {
        match self {
            Self::ParStart => (0, Associativity::LeftToRight),
            _ => {
                panic!("{:?}", self);
            }
        }
    }

    pub fn promote_from_context(self, i_self: usize, context: &[PpToken]) -> Self {
        match self {
            Self::ParStart => {
                let last_token = &context[i_self - 1];
                let next_token = &context[i_self + 1];

                if last_token.value_type == ValueType::NonNumeric && Keyword::from_token(last_token).is_none() {
                    Self::FunctionCall
                } else if Keyword::from_token(next_token).is_some() {
                    Self::TypeCast
                // !TODO! Make case for fuction pointers that are in parenthesis
                }else {
                    self
                }
            },
            _ => {
                self
            }
        }
    }
}

pub enum ParseErrId {
    UnknownParam(String),
    UnknownOperator(String),
    UnexpectedToken(String),
    MissingParenthesis,
    MissingParameter,
    MissingStatementEnd,
    MissingIdentifier,
    UnclosedParenthesis,
    UnexpectedEof,
    InvalidIdentifier(String),
}

impl PartialEq for ParseErrId {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Display for ParseErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseErrId::UnknownParam(param) => write!(f, "unknown param '{}'", param),
            ParseErrId::UnknownOperator(operator) => write!(f, "unknown operator '{}'", operator),
            ParseErrId::UnexpectedToken(token) => write!(f, "unexpected token '{}'", token),
            ParseErrId::MissingParenthesis => write!(f, "missing parenthesis"),
            ParseErrId::MissingParameter => write!(f, "missing parameters"),
            ParseErrId::MissingIdentifier => write!(f, "missing identifier for parameter"),
            ParseErrId::MissingStatementEnd => write!(f, "no ';' at end of statement"),
            ParseErrId::UnclosedParenthesis => write!(f, "unclosed parenthesis"),
            ParseErrId::UnexpectedEof => write!(f, "unexpected end of file"),
            ParseErrId::InvalidIdentifier(x) => write!(f, "invalid identifier '{}'", x),
        }
    }
}

impl Debug for ParseErrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(PartialEq)]
pub struct ParserError {
    line_num: usize,
    id: ParseErrId,
}

impl Display for ParserError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "parser error on line {}: {}", self.line_num, self.id)
    }
}

impl Debug for ParserError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl ParserError {
    pub fn unknown_param(line_num: usize, param: &str) -> Self {
        Self { line_num, id: ParseErrId::UnknownParam(param.to_string()) }
    }
    
    pub fn unknown_operator(line_num: usize, operator: &str) -> Self {
        Self { line_num, id: ParseErrId::UnknownOperator(operator.to_string()) }
    }
    
    
    pub fn unexpected_token(line_num: usize, token: &str) -> Self {
        Self { line_num, id: ParseErrId::UnexpectedToken(token.to_string()) }
    }

    pub fn missing_statement_end(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::MissingStatementEnd }
    }

    pub fn missing_parenthesis(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::MissingParenthesis }
    }
    
    pub fn unclosed_parenthesis(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::UnclosedParenthesis }
    }
    
    pub fn missing_parameter(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::MissingParameter }
    }
    
    pub fn missing_identifier(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::MissingIdentifier }
    }
    
    pub fn unexpected_eof(line_num: usize) -> Self {
        Self { line_num, id: ParseErrId::UnexpectedEof }
    }
    
    pub fn invalid_identifier(line_num: usize, x: &str) -> Self {
        Self { line_num, id: ParseErrId::InvalidIdentifier(x.to_string()) }
    }
}

pub type ParserTokens = Vec<ParserToken>;

#[derive(Debug, PartialEq)]
pub struct IfStatement {
    pub condition: ParserToken,
    pub statement: ParserToken,
    pub else_statement: ParserToken,
}

#[derive(Debug, PartialEq)]
pub struct ParameterDefinition {
    pub type_specifier: TypeSpecifier,
    pub identifier: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct FuncDefinition {
    pub return_type_specifier: TypeSpecifier,
    pub identifier: String,
    pub parameters: Vec<ParameterDefinition>,
    pub block: Vec<ParserToken>,
}

#[derive(Debug, PartialEq)]
pub struct FuncCall {
    // Can either be an identifier or a pointer deref
    pub call: ParserToken,
    pub params: ParserTokens,
}

#[derive(Debug, PartialEq)]
pub enum ParserToken {
    Block(Vec<Self>),
    IfStatement(Box<IfStatement>),
    FuncDefinition(Box<FuncDefinition>),
    FuncCall(Box<FuncCall>),
    Identifier(String),
    Number(String),
    StringLiteral(String),
    Return(Box<ParserToken>),
}

#[derive(Debug)]
pub struct Parser {
    i_window_start: usize,
    i_window_end_lock: Mutex<usize>,
    line_indexes: Vec<LineIndex>,
    tokens: PpTokens,
}

impl Parser {
    pub fn new(ppout: PreprocessorOut) -> Self {
        Self { i_window_start: 0, i_window_end_lock: Mutex::new(0), line_indexes: ppout.line_indexes, tokens: ppout.tokens }
    }

    pub fn parse(mut self) -> Result<ParserTokens, ParserError> {
        let mut out = Vec::with_capacity(self.tokens.len());

        while !self.at_eof() {
            let (i_token, token) = self.next_token()?;

            // We can only make decisions if there's an operator
            if !matches!(token.value_type, ValueType::Operator) {
                continue;
            }

            let operator = self.parse_op(&token.value)?;

            match operator {
                // The first operator being "(" corresponds to either a function
                // declaration, definition, or pointer.
                Operator::ParStart => {
                    let i_par_start = i_token;
                    let i_par_end = self.jump_to_matching(Operator::ParStart, Operator::ParEnd)?;

                    // This operator determines what kind of statement this is.
                    let (i_token, token) = self.next_token()?;

                    let operator = self.parse_op(&token.value)?;

                    match operator {
                        // Function declaration
                        Operator::StatementEnd => {
                            self.parse_function_declaration(&mut out, i_par_start, i_par_end)?;
                        },
                        // Function definition
                        Operator::BlockStart => {
                            let i_block_start = i_token;
                            let i_block_end = self.jump_to_matching(Operator::BlockStart, Operator::BlockEnd)?;
                            self.parse_function_definition(&mut out, i_par_start, i_par_end, i_block_start, i_block_end)?;
                        },
                        // Function pointer
                        Operator::ParStart => {
                            todo!()
                        },
                        _ => {
                            return Err(ParserError::unexpected_token(self.get_line_num(), &token.value));
                        },
                    }
                },
                _ => {
                    return Err(ParserError::unexpected_token(self.get_line_num(), &token.value));
                },
            }

            self.i_window_start = self.get_i_window_end();
        }

        Ok(out)
    }

    fn parse_function_declaration(&mut self, out: &mut Vec<ParserToken>, i_param_start: usize, i_param_end: usize) -> Result<(), ParserError> {
        todo!()
    }

    fn parse_function_definition(&self, out: &mut Vec<ParserToken>, i_param_start: usize, i_param_end: usize, i_block_start: usize, i_block_end: usize) -> Result<(), ParserError> {
        let i_window_start = self.i_window_start;

        // Minimum 2 tokens infront of a function definition: 1 token for the type,
        // one for the identity
        if i_param_start < i_window_start + 2 {
            return Err(ParserError::unexpected_token(self.get_line_num(), &self.tokens.first().unwrap().value))
        }

        let i_identifier = i_param_start - 1;
        let i_type_start = i_window_start;
        let i_type_end = i_identifier;

        let identifier = self.parse_identitifier(i_identifier)?;
        let return_type_specifier = self.parse_type(i_type_start, i_type_end)?;
        let parameters = self.parse_params(i_param_start, i_param_end, true)?;
        let block = self.parse_block(i_block_start, i_block_end)?;

        let func_defintion =  FuncDefinition {return_type_specifier, identifier, parameters, block };

        out.push(ParserToken::FuncDefinition(Box::new(func_defintion)));
    
        Ok(())
    }

    fn parse_identitifier(&self, i_identifier: usize) -> Result<String, ParserError> {
        let identifier = &self.tokens[i_identifier];
        if !matches!(identifier.value_type, ValueType::NonNumeric) {
            return Err(ParserError::invalid_identifier(self.get_line_num(), &identifier.value));
        }

        Ok(identifier.value.clone())
    }

    fn parse_type(&self, i_type_start: usize, i_type_end: usize) -> Result<TypeSpecifier, ParserError> {
        TypeSpecifier::parse(self, &self.tokens[i_type_start..i_type_end])
    }
   
    /// Will return none if there are either no parameters or there is one
    /// unidentified parameter of the "void" type
    /// If force_ident is true, all parameters will be required to have identifiers
    fn parse_params(&self, i_param_start: usize, i_param_end: usize, force_ident: bool) -> Result<Vec<ParameterDefinition>, ParserError> {
        let i_first_param = i_param_start + 1;
        let param_count = i_param_end - i_first_param;
        let mut parameters = Vec::with_capacity(4);

        if param_count == 0 || matches!(Keyword::from_token(&self.tokens[i_first_param]), Some(Keyword::Void)) {
            return Ok(parameters);
        }

        let mut i_token = i_first_param;
        let mut i_next_token = i_token;

        let mut i_declare_start = i_token;

        while i_next_token < i_param_end {
            i_token = i_next_token;
            i_next_token += 1;

            let token = &self.tokens[i_token];

            if token.value_type != ValueType::Operator {
                continue;
            }

            let operator = Operator::parse(&token.value).ok_or(ParserError::unexpected_token(self.get_token_meta(i_token).line_num, &token.value))?;

            match operator {
                Operator::Comma => {
                    parameters.push(self.parse_param_declaration(i_declare_start, i_token + 1, force_ident)?);
                    i_declare_start = i_token;
                }
                _ => {
                    return Err(ParserError::unexpected_token(self.get_token_meta(i_token).line_num, &token.value));
                }
            }
        }

        if i_declare_start == i_param_end {
            return Err(ParserError::unexpected_token(self.get_token_meta(i_token).line_num, &self.tokens[i_token].value));
        }

        parameters.push(self.parse_param_declaration(i_declare_start, i_param_end, force_ident)?);

        Ok(parameters)
    }

    fn parse_param_declaration(&self, i_declare_start: usize, i_declare_end: usize, force_ident: bool) -> Result<ParameterDefinition, ParserError> {
        let i_last_token = i_declare_end - 1;
        let last_token = &self.tokens[i_last_token];
        let (identifier, i_type_end) = match Keyword::from_token(last_token).is_none() {
            true => (Some(last_token.value.clone()), i_last_token),
            false => (None, i_declare_end),
        };

        log::info!("{:?}\n{:?}", &self.tokens[i_declare_start..i_declare_end], &self.tokens[i_declare_start..i_type_end]);
        if identifier.is_none() && force_ident {
            return Err(ParserError::missing_identifier(self.get_token_meta(i_type_end).line_num))
        }

        Ok(ParameterDefinition {
            identifier,
            type_specifier: TypeSpecifier::parse(self, &self.tokens[i_declare_start..i_type_end])?,
        })
    }
    
    fn parse_block(&self, i_block_start: usize, i_block_end: usize) -> Result<ParserTokens, ParserError> {
        let mut i_statement_start = i_block_start + 1;
        let mut i_statement_end;

        let mut statements = Vec::with_capacity(16);

        while i_statement_start < i_block_end {
            i_statement_end = self.find_end_of_statement(i_statement_start)?;

            let statement = self.parse_statement(i_statement_start, i_statement_end)?;
            statements.push(statement);

            i_statement_start = i_statement_end + 1;
        }

        Ok(statements)
    }

    fn parse_statement(&self, i_statement_start: usize, i_statement_end: usize) -> Result<ParserToken, ParserError> {
        let first_token = &self.tokens[i_statement_start];
        let i_next_token = i_statement_start + 1;
        let statement_len = i_statement_end - i_statement_start;

        if statement_len == 1 {
            if matches!(first_token.value_type, ValueType::Numeric) {
                return Ok(ParserToken::Number(first_token.value.clone()));
            }
            
            if matches!(first_token.value_type, ValueType::StringLiteral) {
                return Ok(self.parse_string_literals(i_statement_start, i_statement_end))
            }

            if matches!(first_token.value_type, ValueType::Operator) {
                return Err(ParserError::unexpected_token(self.get_token_meta(i_statement_start).line_num, &first_token.value));
            }

            match Keyword::from_token(first_token) {
                None => {
                    return Ok(ParserToken::Identifier(first_token.value.clone()));
                }
                _ => {
                    return Err(ParserError::unexpected_token(self.get_token_meta(i_statement_start).line_num, &first_token.value));
                }
            }
        }

        if matches!(first_token.value_type, ValueType::NonNumeric) &&
            let Some(keyword) = Keyword::from_token(&first_token) {

            match keyword {
                Keyword::Return => { return Ok(ParserToken::Return(Box::new(self.parse_statement(i_next_token, i_statement_end)?))); },
                _ => {
                    todo!();
                }
            }
        }

        let mut operator_opt = None;
        let mut i_max_precedence = 0;
        let mut max_precidence = 0;
        let mut op_associativity = Associativity::LeftToRight;

        let mut i_token = i_statement_start;
        let mut i_next_token = i_token;

        while i_next_token < i_statement_end {
            i_token = i_next_token;
            i_next_token += 1;
            let token = &self.tokens[i_token];

            if !matches!(token.value_type, ValueType::Operator) {
                continue;
            }

            let operator = Operator::parse(&token.value).ok_or(ParserError::unexpected_token(self.get_token_meta(i_token).line_num, &token.value))?;

            let (precidence, associativity) = operator.precidence_and_associativity();

            if operator_opt.is_none() || precidence > max_precidence || (precidence == max_precidence && op_associativity == Associativity::RightToLeft) {
                operator_opt = Some(operator.promote_from_context(i_token, &self.tokens));
                i_max_precedence = i_token;
                max_precidence = precidence;
                op_associativity = associativity;
            }

            if matches!(operator, Operator::ParStart) {
                i_next_token = self.find_matching_token(i_token + 1, Operator::ParStart, Operator::ParEnd)? + 1;
            }
            
            if matches!(operator, Operator::BlockStart) {
                i_next_token = self.find_matching_token(i_token + 1, Operator::BlockStart, Operator::BlockEnd)? + 1;
            }
        }

        let operator = match operator_opt {
            Some(operator) => operator,
            None => {
                panic!("{:?}", &self.tokens[i_statement_start..i_statement_end]);
                todo!()
            }
        };

        match operator {
            Operator::ParStart => {
                let i_par_start = i_max_precedence + 1;
                let i_par_end = self.find_matching_token(i_par_start, Operator::ParStart, Operator::ParEnd)?;

                self.parse_statement(i_par_start, i_par_end)
            }
            Operator::FunctionCall => {
                self.parse_function_call(i_statement_start, i_max_precedence, i_statement_end)
            }
            _ => {
                todo!();
            }
        }
    }

    fn parse_string_literals(&self, i_first_literal: usize, i_end: usize) -> ParserToken {
        let mut result_string = String::with_capacity(16);

        for string_literal in self.tokens[i_first_literal..i_end].iter().map(|x| &x.value) {
            let i_string_start = 1;
            let i_string_end = string_literal.len() - 1;

            result_string.push_str(&string_literal[i_string_start..i_string_end]);
        }

        ParserToken::StringLiteral(result_string)
    }

    fn parse_function_call(&self, i_fc_start: usize, i_fc_params: usize, i_fc_end: usize) -> Result<ParserToken, ParserError> {
        let call = self.parse_statement(i_fc_start, i_fc_params)?;

        let i_last_token = i_fc_end - 1;
        
        let mut params = Vec::new();

        let mut i_token = i_fc_params + 1;

        if i_token == i_last_token {
            let func_call = FuncCall {
                call,
                params,
            };

            return Ok(ParserToken::FuncCall(Box::new(func_call)));
        }

        let mut i_next_token = i_token;
        let mut i_param_start = i_token;

        while i_next_token < i_fc_end {
            i_token = i_next_token;
            i_next_token += 1;
            let token = &self.tokens[i_token];

            match Operator::parse(&token.value) {
                Some(Operator::Comma) => {
                    let statement = self.parse_statement(i_param_start, i_token)?;
                    params.push(statement);

                    i_next_token = i_token + 1;
                    i_param_start = i_next_token;
                }
                Some(Operator::ParStart) => {
                    i_next_token = self.find_matching_token(i_next_token, Operator::ParStart, Operator::ParEnd)?;
                }
                _ => {},
            }
        }
        
        let statement = self.parse_statement(i_param_start, i_token)?;

        params.push(statement);

        let func_call = FuncCall {
            call,
            params,
        };

        Ok(ParserToken::FuncCall(Box::new(func_call)))
    }

    fn parse_op(&self, string: &str) -> Result<Operator, ParserError> {
        Operator::parse(&string).ok_or(ParserError::unknown_operator(self.get_line_num(), &string))
    }

    fn jump_to_matching(&mut self, p_start: Operator, p_end: Operator) -> Result<usize, ParserError> {
        let i_matching = self.find_matching_token(self.get_i_window_end(), p_start, p_end)?;
        *self.i_window_end_lock.lock().unwrap() = i_matching + 1;

        Ok(i_matching)
    }
    /// Ensure the token you want to match with is before i_start
    fn find_matching_token(&self, i_start: usize, p_start: Operator, p_end: Operator) -> Result<usize, ParserError> {
        let mut depth = 0;

        let mut i_token = i_start;
        let mut i_next_token = i_token;

        loop {
            i_token = i_next_token;
            let token = match self.tokens.get(i_token) {
                Some(token) => token,
                None => {
                    return Err(ParserError::missing_statement_end(self.get_token_meta(i_start).line_num));
                }
            };
            
            i_next_token += 1;

            if !matches!(token.value_type, ValueType::Operator) {
                continue;
            }

            let operator = self.parse_op(&token.value)?;


            if operator == p_start {
                depth += 1;
            } else if operator == p_end {
                if depth == 0 {
                    return Ok(i_token);
                }

                depth -= 1;
            }
        }
    }

    fn find_end_of_statement(&self, i_start: usize) -> Result<usize, ParserError> {
        let mut i_token = i_start;
        let mut i_next_token = i_token;

        loop {
            i_token = i_next_token;
            let token = match self.tokens.get(i_token) {
                Some(token) => token,
                None => {
                    return Err(ParserError::missing_statement_end(self.get_token_meta(i_start).line_num));
                }
            };

            i_next_token += 1;

            if !matches!(token.value_type, ValueType::Operator) {
                continue;
            }

            let operator = Operator::parse(&token.value).ok_or(ParserError::unexpected_token(self.get_token_meta(i_token).line_num, &token.value))?;

            match operator {
                Operator::ParStart => {
                    i_next_token = self.find_matching_token(i_token + 1, Operator::ParStart, Operator::ParEnd)? + 1;
                }
                Operator::BlockStart => {
                    i_token = self.find_matching_token(i_token + 1, Operator::BlockStart, Operator::BlockEnd)?;
                    return Ok(i_token)
                }
                Operator::StatementEnd => {
                    return Ok(i_token);
                }
                _ => {},
            }
        }
    }

    fn next_token<'a>(&'a self) -> Result<(usize, &'a PpToken), ParserError> {
        let i_token = self.get_i_window_end();
        match self.tokens.get(i_token) {
            Some(token) => {
                *self.i_window_end_lock.lock().unwrap() += 1;
                
                Ok((i_token, &token))
            }
            None => { return Err(ParserError::unexpected_eof(self.get_line_num())); }
        }
    }

    fn at_eof(&self) -> bool {
        !(self.get_i_window_end() < self.tokens.len())
    }

    fn get_token_meta(&self, i_token: usize) -> LineIndex {
        let mut i_line_start = 0;
        for line_index in self.line_indexes.iter() {
            if i_line_start < i_token && line_index.i_line_end > i_token {
                return line_index.clone();
            }

            i_line_start = line_index.i_line_end;
        }

        self.line_indexes.last().unwrap_or(&LineIndex { line_num: 0, file_name: Arc::new("".to_string()), i_line_end: 0 }).clone()
    }

    fn get_line_num(&self) -> usize {
        self.get_token_meta(self.get_i_window_end()).line_num
    }

    fn get_i_window_end(&self) -> usize {
        *self.i_window_end_lock.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::{parser::{FuncCall, FuncDefinition, ParameterDefinition, Parser, ParserToken, Type, TypeSpecifier}, preprocessor::preprocess};
    
    #[test]
    fn test_parser_func_hello_world() {
        let mut test_in = r#"#include "include/test.h"
int main(void) {
    printf(HELLO);
}"#.as_bytes();
        
        let func_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Int },
            identifier: "main".to_string(),
            parameters: Vec::new(),
            block: vec![
                ParserToken::FuncCall(Box::new(FuncCall {
                    call: ParserToken::Identifier("printf".to_string()),
                    params: vec![
                        ParserToken::StringLiteral("Hello, World!".to_string()),
                    ]
                })),
            ],
        };

        let test_expected = vec![ParserToken::FuncDefinition(Box::new(func_def))];

        let test_out = Parser::new(preprocess(&mut test_in, "").unwrap()).parse().unwrap();

        assert_eq!(test_expected, test_out);
    }

    #[test]
    fn test_parser_func_definition_no_param_no_return() {
        let mut test_in_1 = "void foo(){}".as_bytes();
        let mut test_in_2 = "void foo( void ){}".as_bytes();
        
        let func_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Void },
            identifier: "foo".to_string(),
            parameters: Vec::new(),
            block: Vec::new(),
        };

        let test_expected = vec![ParserToken::FuncDefinition(Box::new(func_def))];

        let test_out_1 = Parser::new(preprocess(&mut test_in_1, "").unwrap()).parse().unwrap();
        let test_out_2 = Parser::new(preprocess(&mut test_in_2, "").unwrap()).parse().unwrap();

        assert_eq!(test_expected, test_out_1);
        assert_eq!(test_expected, test_out_2);
    }
    
    #[test]
    fn test_parser_func_definition_no_param_int_return() {
        let mut test_in_1 = "int foo(void){return 2;}".as_bytes();
        let mut test_in_2 = "int foo(void){return(2);}".as_bytes();
        
        let func_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Int },
            identifier: "foo".to_string(),
            parameters: Vec::new(),
            block: vec![ParserToken::Return(Box::new(ParserToken::Number("2".to_string())))],
        };

        let test_expected = vec![ParserToken::FuncDefinition(Box::new(func_def))];

        let test_out_1 = Parser::new(preprocess(&mut test_in_1, "").unwrap()).parse().unwrap();
        let test_out_2 = Parser::new(preprocess(&mut test_in_2, "").unwrap()).parse().unwrap();

        assert_eq!(test_expected, test_out_1);
        assert_eq!(test_expected, test_out_2);
    }
    
    #[test]
    fn test_parser_func_definition_int_param_int_return() {
        colog::init();
        let mut test_in = "int foo(int i){return i;}".as_bytes();
        
        let func_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Int },
            identifier: "foo".to_string(),
            parameters: vec![
                ParameterDefinition {
                    identifier: Some("i".to_string()),
                    type_specifier: TypeSpecifier {
                        is_const: false,
                        indir_is_const: false,
                        is_volatile: false,
                        pointer_indirections: 0,
                        ctype: Type::Int,
                    },
                },
            ],
            block: vec![ParserToken::Return(Box::new(ParserToken::Identifier("i".to_string())))],
        };

        let test_expected = vec![ParserToken::FuncDefinition(Box::new(func_def))];

        let test_out = Parser::new(preprocess(&mut test_in, "").unwrap()).parse().unwrap();

        assert_eq!(test_expected, test_out);
    }
    
    #[test]
    fn test_parser_func_call_no_param_no_return() {
        let mut test_in = "void foo(){} void main(){ foo(); }".as_bytes();
        
        let func_foo_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Void },
            identifier: "foo".to_string(),
            parameters: Vec::new(),
            block: Vec::new(),
        };

        let func_main_def = FuncDefinition {
            return_type_specifier: TypeSpecifier { is_const: false, indir_is_const: false, is_volatile: false, pointer_indirections: 0, ctype: Type::Void },
            identifier: "main".to_string(),
            parameters: Vec::new(),
            block: vec![
                ParserToken::FuncCall(Box::new(FuncCall {
                    call: ParserToken::Identifier("foo".to_string()),
                    params: Vec::new(),
                }))
            ],
        };

        let test_expected = vec![
            ParserToken::FuncDefinition(Box::new(func_foo_def)),
            ParserToken::FuncDefinition(Box::new(func_main_def)),
        ];

        let test_out = Parser::new(preprocess(&mut test_in, "").unwrap()).parse().unwrap();

        assert_eq!(test_expected, test_out);
    }
}
