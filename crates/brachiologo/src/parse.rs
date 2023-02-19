use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, anychar, char, line_ending, multispace0, multispace1, space0},
    combinator::{all_consuming, consumed, cut, map, map_opt, verify},
    multi::{many0, separated_list1},
    number::complete::double,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

use crate::{
    proc::UserProc,
    typ::{Expr, ExprKind, Op},
};

pub type Span<'a> = nom_locate::LocatedSpan<&'a str>;
pub type PResult<'a, O> = IResult<Span<'a>, O, ParseError<'a>>;

#[derive(Clone, Debug)]
pub struct ParseError<'a> {
    pub input: Span<'a>,
    pub kind: ErrorKind,
    pub cause: Option<Box<ParseError<'a>>>,
}

impl<'a> ParseError<'a> {
    pub fn new(input: Span<'a>, kind: ErrorKind) -> Self {
        Self {
            input,
            kind,
            cause: None,
        }
    }

    pub fn with_cause(input: Span<'a>, kind: ErrorKind, cause: ParseError<'a>) -> Self {
        Self {
            input,
            kind,
            cause: Some(Box::new(cause)),
        }
    }
}

impl<'a> nom::error::ParseError<Span<'a>> for ParseError<'a> {
    fn from_error_kind(input: Span<'a>, kind: nom::error::ErrorKind) -> Self {
        ParseError::new(input, ErrorKind::Nom(kind))
    }

    fn append(input: Span<'a>, kind: nom::error::ErrorKind, other: Self) -> Self {
        ParseError::with_cause(input, ErrorKind::Nom(kind), other)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ErrorKind {
    QuoteList,
    List,
    Proc,
    UnendedProc,
    UnclosedList,
    UnclosedQuoteList,
    Nom(nom::error::ErrorKind),
}

fn err_ctx<'a, F, O>(kind: ErrorKind, mut f: F) -> impl FnMut(Span<'a>) -> PResult<'a, O>
where
    F: nom::Parser<Span<'a>, O, ParseError<'a>>,
{
    move |input: Span<'a>| match f.parse(input.clone()) {
        Ok(o) => Ok(o),
        Err(nom::Err::Incomplete(i)) => Err(nom::Err::Incomplete(i)),
        Err(nom::Err::Error(e)) => Err(nom::Err::Error(ParseError::with_cause(input, kind, e))),
        Err(nom::Err::Failure(e)) => Err(nom::Err::Failure(ParseError::with_cause(input, kind, e))),
    }
}

fn ws<'a, F: 'a, O>(inner: F) -> impl FnMut(Span<'a>) -> PResult<O>
where
    F: FnMut(Span<'a>) -> PResult<O>,
{
    delimited(multispace0, inner, multispace0)
}

fn ws_no_newline<'a, F: 'a, O>(inner: F) -> impl FnMut(Span<'a>) -> PResult<O>
where
    F: FnMut(Span<'a>) -> PResult<O>,
{
    delimited(space0, inner, space0)
}

fn with_span<'a, F: 'a>(inner: F) -> impl FnMut(Span<'a>) -> PResult<Expr>
where
    F: FnMut(Span<'a>) -> PResult<'a, ExprKind>,
{
    map(consumed(inner), |(input, kind)| Expr {
        e: kind,
        span: input.into(),
    })
}

const RESERVED: &'static [&'static str] = &["to", "end"];

fn ident(input: Span) -> PResult<String> {
    verify(map(alpha1, |s: Span| s.to_string()), |s| {
        !RESERVED.contains(&s)
    })(input)
}

pub fn word(input: Span) -> PResult<Expr> {
    with_span(map(ident, |w| ExprKind::Word(w)))(input)
}

pub fn param(input: Span) -> PResult<Expr> {
    with_span(map(preceded(char(':'), ident), |v| ExprKind::Var(v)))(input)
}

pub fn num(input: Span) -> PResult<Expr> {
    with_span(map(double, |x| ExprKind::Num(x)))(input)
}

pub fn op(input: Span) -> PResult<Expr> {
    with_span(map(map_opt(anychar, |ch| Op::try_from(ch).ok()), |op| {
        ExprKind::Op(op)
    }))(input)
}

pub fn bare_list(input: Span) -> PResult<Expr> {
    with_span(map(separated_list1(multispace1, expr), |exprs| {
        ExprKind::List(exprs)
    }))(input)
}

pub fn list(input: Span) -> PResult<Expr> {
    let rest = terminated(ws(bare_list), err_ctx(ErrorKind::UnclosedList, char(')')));

    err_ctx(
        ErrorKind::List,
        with_span(map(preceded(char('('), cut(rest)), |expr| expr.e)),
    )(input)
}

pub fn quoted_list(input: Span) -> PResult<Expr> {
    let rest = terminated(
        ws(bare_list),
        err_ctx(ErrorKind::UnclosedQuoteList, char(']')),
    );

    err_ctx(
        ErrorKind::QuoteList,
        with_span(map(preceded(char('['), cut(rest)), |expr| expr.e)),
    )(input)
}

pub fn quote(input: Span) -> PResult<Expr> {
    with_span(map(preceded(char('"'), expr), |expr| {
        ExprKind::Quote(Box::new(expr))
    }))(input)
}

pub fn proc_def(input: Span) -> PResult<Expr> {
    let rest = tuple((
        ws(word),
        many0(ws_no_newline(param)),
        line_ending,
        ws(bare_list),
        err_ctx(ErrorKind::UnendedProc, tag("end")),
    ));

    err_ctx(
        ErrorKind::Proc,
        with_span(map(
            preceded(tag("to"), cut(rest)),
            |(name, args, _newline, body, _end)| {
                let ExprKind::Word(name) = name.e else {
                panic!("name should be a word");
            };
                let args: Vec<String> = args
                    .into_iter()
                    .map(|p| {
                        let ExprKind::Var(p) = p.e else { panic!("param should be a var") };
                        p
                    })
                    .collect();
                ExprKind::DefProc(UserProc { name, args, body }.into())
            },
        )),
    )(input)
}

pub fn expr(input: Span) -> PResult<Expr> {
    alt((proc_def, num, op, list, quoted_list, quote, word, param))(input)
}

pub fn program(input: Span) -> PResult<Expr> {
    all_consuming(ws(bare_list))(input)
}
