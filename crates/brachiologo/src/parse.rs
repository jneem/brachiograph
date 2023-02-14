use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, anychar, char, line_ending, multispace0, multispace1, space0},
    combinator::{all_consuming, consumed, cut, map, map_opt, verify},
    multi::{many0, separated_list1},
    number::complete::double,
    sequence::{delimited, preceded, tuple},
    IResult,
};

use crate::{
    proc::UserProc,
    typ::{Expr, ExprKind, Op},
};

pub type Span<'a> = nom_locate::LocatedSpan<&'a str>;
pub type ParseError<'a> = nom::error::VerboseError<Span<'a>>;
pub type PResult<'a, O> = IResult<Span<'a>, O, ParseError<'a>>;

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
    with_span(map(
        delimited(char('('), ws(bare_list), char(')')),
        |expr| expr.e,
    ))(input)
}

pub fn quoted_list(input: Span) -> PResult<Expr> {
    with_span(map(
        delimited(char('['), ws(bare_list), char(']')),
        |expr| ExprKind::Quote(Box::new(expr)),
    ))(input)
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
        tag("end"),
    ));
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
    ))(input)
}

pub fn expr(input: Span) -> PResult<Expr> {
    alt((proc_def, num, op, list, quoted_list, quote, word, param))(input)
}

pub fn program(input: Span) -> PResult<Expr> {
    all_consuming(ws(bare_list))(input)
}
