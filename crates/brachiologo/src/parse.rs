use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, anychar, char, line_ending, multispace0, multispace1, space0},
    combinator::{consumed, map, map_opt, verify},
    multi::{many0, separated_list1},
    number::complete::double,
    sequence::{delimited, preceded, tuple},
    IResult,
};

use crate::{
    proc::UserProc,
    typ::{Expr, ExprKind, Op, Val},
};

pub type Span<'a> = nom_locate::LocatedSpan<&'a str>;
pub type ParseError<'a> = nom::error::Error<Span<'a>>;

fn ws<'a, F: 'a, O>(inner: F) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, O>
where
    F: FnMut(Span<'a>) -> IResult<Span<'a>, O>,
{
    delimited(multispace0, inner, multispace0)
}

fn ws_no_newline<'a, F: 'a, O>(inner: F) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, O>
where
    F: FnMut(Span<'a>) -> IResult<Span<'a>, O>,
{
    delimited(space0, inner, space0)
}

fn with_span<'a, F: 'a>(inner: F) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, Expr>
where
    F: FnMut(Span<'a>) -> IResult<Span<'a>, ExprKind>,
{
    map(consumed(inner), |(input, kind)| Expr {
        e: kind,
        span: input.into(),
    })
}

const RESERVED: &'static [&'static str] = &["to", "end"];

fn ident(input: Span) -> IResult<Span, String> {
    verify(map(alpha1, |s: Span| s.to_string()), |s| {
        !RESERVED.contains(&s)
    })(input)
}

pub fn word(input: Span) -> IResult<Span, Expr> {
    with_span(map(ident, |w| ExprKind::Word(w)))(input)
}

pub fn param(input: Span) -> IResult<Span, Expr> {
    with_span(map(preceded(char(':'), ident), |v| ExprKind::Var(v)))(input)
}

pub fn num(input: Span) -> IResult<Span, Expr> {
    with_span(map(double, |x| ExprKind::Val(Val::Num(x))))(input)
}

pub fn op(input: Span) -> IResult<Span, Expr> {
    with_span(map(map_opt(anychar, |ch| Op::try_from(ch).ok()), |op| {
        ExprKind::Op(op)
    }))(input)
}

pub fn bare_list(input: Span) -> IResult<Span, Expr> {
    with_span(map(separated_list1(multispace1, expr), |exprs| {
        ExprKind::List(exprs)
    }))(input)
}

pub fn list(input: Span) -> IResult<Span, Expr> {
    with_span(map(
        delimited(char('('), ws(bare_list), char(')')),
        |expr| expr.e,
    ))(input)
}

pub fn quoted_list(input: Span) -> IResult<Span, Expr> {
    with_span(map(
        delimited(char('['), ws(bare_list), char(']')),
        |expr| ExprKind::Quote(Box::new(expr)),
    ))(input)
}

pub fn quote(input: Span) -> IResult<Span, Expr> {
    with_span(map(preceded(char('"'), expr), |expr| {
        ExprKind::Quote(Box::new(expr))
    }))(input)
}

pub fn proc_def(input: Span) -> IResult<Span, Expr> {
    with_span(map(
        tuple((
            tag("to"),
            ws(word),
            many0(ws_no_newline(param)),
            line_ending,
            ws(bare_list),
            tag("end"),
        )),
        |(_to, name, args, _newline, body, _end)| {
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

pub fn expr(input: Span) -> IResult<Span, Expr> {
    alt((proc_def, num, op, list, quoted_list, quote, word, param))(input)
}

pub fn program(input: Span) -> IResult<Span, Expr> {
    ws(bare_list)(input)
}
