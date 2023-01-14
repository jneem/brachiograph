// TODO: add spans and decent parser errors.

use std::collections::HashMap;

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, char, multispace0},
    combinator::{map, verify},
    multi::{fold_many0, many0},
    number::complete::double,
    sequence::{delimited, preceded, tuple},
    IResult, Parser,
};

type Span<'a> = nom_locate::LocatedSpan<&'a str>;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Literal(f64);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Ident(String);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Cmp {
    Eq,
    Lt,
    Gt,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NumExpr {
    Lit(Literal),
    Param(Ident),
    Op(Box<NumExpr>, Op, Box<NumExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoolExpr(NumExpr, Cmp, NumExpr);

#[derive(Clone, Debug)]
pub enum Statement {
    Def(ProcedureDef),
    Call(ProcedureCall),
    If(BoolExpr, Block),
    Repeat(NumExpr, Block),
}

#[derive(Clone, Debug)]
pub struct Block {
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug)]
pub struct ProcedureDef {
    pub name: Ident,
    pub params: Vec<Ident>,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct ProcedureCall {
    pub name: Ident,
    // Are params allowed to be booleans?
    pub params: Vec<NumExpr>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("wrong number of parameters (expected {expected}, found {found})")]
    WrongParams { expected: u32, found: u32 },
    #[error("unknown procedure \"{:?}\"", name.0)]
    UnknownProcedure { name: Ident },
    #[error("unknown variable \"{:?}\"", name.0)]
    UnknownVariable { name: Ident },
}

impl ProcedureCall {
    fn check_builtin(&self) -> Result<(), Error> {
        match self.name.0.as_str() {
            "fd" | "forward" | "bk" | "backward" | "lt" | "left" | "rt" | "right" => {
                if self.params.len() == 1 {
                    Ok(())
                } else {
                    Err(Error::WrongParams {
                        expected: 1,
                        found: self.params.len() as u32,
                    })
                }
            }

            "cs" | "clearscreen" | "pu" | "penup" | "pd" | "pendown" => {
                if self.params.len() == 0 {
                    Ok(())
                } else {
                    Err(Error::WrongParams {
                        expected: 0,
                        found: self.params.len() as u32,
                    })
                }
            }
            _ => Err(Error::UnknownProcedure {
                name: self.name.clone(),
            }),
        }
    }

    fn exec_builtin(&self, values: &[f64]) -> Result<BuiltIn, Error> {
        let no_args = || {
            if values.len() > 0 {
                Err(Error::WrongParams {
                    expected: 0,
                    found: values.len() as u32,
                })
            } else {
                Ok(())
            }
        };

        let one_arg = || {
            if values.len() != 1 {
                Err(Error::WrongParams {
                    expected: 1,
                    found: values.len() as u32,
                })
            } else {
                Ok(values[0])
            }
        };

        let two_args = || {
            if values.len() != 2 {
                Err(Error::WrongParams {
                    expected: 2,
                    found: values.len() as u32,
                })
            } else {
                Ok((values[0], values[1]))
            }
        };

        Ok(match self.name.0.as_str() {
            "arc" => {
                let (degrees, radius) = two_args()?;
                BuiltIn::Arc { degrees, radius }
            }
            "fd" | "forward" => BuiltIn::Forward(one_arg()?),
            "bk" | "back" | "backward" => BuiltIn::Back(one_arg()?),
            "lt" | "left" => BuiltIn::Left(one_arg()?),
            "rt" | "right" => BuiltIn::Right(one_arg()?),
            "cs" | "clearscreen" => {
                no_args()?;
                BuiltIn::ClearScreen
            }
            "pu" | "penup" => {
                no_args()?;
                BuiltIn::PenUp
            }
            "pd" | "pendown" => {
                no_args()?;
                BuiltIn::PenDown
            }
            _ => todo!(),
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BuiltIn {
    Forward(f64),
    Back(f64),
    Left(f64),
    Right(f64),
    Arc { degrees: f64, radius: f64 },
    ClearScreen,
    PenUp,
    PenDown,
}

#[derive(Debug, Default)]
pub struct Scope<'a> {
    parent: Option<&'a Scope<'a>>,
    variables: HashMap<Ident, f64>,
    procs: HashMap<Ident, ProcedureDef>,
}

impl<'a> Scope<'a> {
    pub fn lookup(&self, ident: &Ident) -> Result<f64, Error> {
        match self.variables.get(ident) {
            Some(x) => Ok(*x),
            None => self
                .parent
                .ok_or_else(|| Error::UnknownVariable {
                    name: ident.clone(),
                })
                .and_then(|parent| parent.lookup(ident)),
        }
    }

    pub fn lookup_proc(&self, ident: &Ident) -> Option<&ProcedureDef> {
        self.procs
            .get(ident)
            .or_else(|| self.parent.and_then(|parent| parent.lookup_proc(ident)))
    }

    pub fn eval_num_expr(&self, expr: &NumExpr) -> Result<f64, Error> {
        match expr {
            NumExpr::Lit(x) => Ok(x.0),
            NumExpr::Param(p) => self.lookup(p),
            NumExpr::Op(lhs, op, rhs) => {
                let lhs = self.eval_num_expr(&lhs)?;
                let rhs = self.eval_num_expr(&rhs)?;
                Ok(match op {
                    Op::Add => lhs + rhs,
                    Op::Sub => lhs - rhs,
                    Op::Mul => lhs * rhs,
                    Op::Div => lhs / rhs,
                })
            }
        }
    }

    pub fn eval_bool_expr(&self, expr: &BoolExpr) -> Result<bool, Error> {
        let lhs = self.eval_num_expr(&expr.0)?;
        let rhs = self.eval_num_expr(&expr.2)?;
        Ok(match expr.1 {
            Cmp::Eq => lhs == rhs,
            Cmp::Lt => lhs < rhs,
            Cmp::Gt => lhs > rhs,
        })
    }

    pub fn def(&mut self, proc: ProcedureDef) {
        // TODO: check for duplicate definitions?
        self.procs.insert(proc.name.clone(), proc);
    }

    fn sub_scope(&'a self) -> Scope<'a> {
        Scope {
            parent: Some(self),
            variables: HashMap::new(),
            procs: HashMap::new(),
        }
    }

    pub fn exec_block(&mut self, output: &mut Vec<BuiltIn>, block: &Block) -> Result<(), Error> {
        for statement in &block.statements {
            if let Statement::Def(def) = statement {
                self.def(def.clone());
            }
        }

        for statement in &block.statements {
            match statement {
                Statement::Def(_) => {}
                Statement::Call(call) => {
                    self.exec_proc_call(output, call)?;
                }
                Statement::If(cond, block) => {
                    if self.eval_bool_expr(cond)? {
                        self.sub_scope().exec_block(output, block)?;
                    }
                }
                Statement::Repeat(count, block) => {
                    let count = self.eval_num_expr(count)? as u32;
                    for _ in 0..count {
                        self.sub_scope().exec_block(output, block)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn exec_proc_call(
        &self,
        output: &mut Vec<BuiltIn>,
        call: &ProcedureCall,
    ) -> Result<(), Error> {
        let params: Result<Vec<f64>, _> = call
            .params
            .iter()
            .map(|expr| self.eval_num_expr(expr))
            .collect();
        if let Some(proc) = self.lookup_proc(&call.name) {
            if call.params.len() != proc.params.len() {
                return Err(Error::WrongParams {
                    expected: proc.params.len() as u32,
                    found: call.params.len() as u32,
                });
            }
            let variables = proc.params.iter().cloned().zip(params?).collect();
            let mut scope = Scope {
                parent: Some(self),
                variables,
                procs: HashMap::new(),
            };
            scope.exec_block(output, &proc.body)
        } else {
            call.check_builtin()?;
            output.push(call.exec_builtin(&params?)?);
            Ok(())
        }
    }
}

const RESERVED: &'static [&'static str] = &["if", "repeat", "to", "end"];

fn ws<'a, F: 'a, O>(inner: F) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, O>
where
    F: FnMut(Span<'a>) -> IResult<Span<'a>, O>,
{
    delimited(multispace0, inner, multispace0)
}

pub fn ident(input: Span) -> IResult<Span, Ident> {
    verify(
        map(ws(alpha1), |s: Span| Ident(s.to_string())),
        |i: &Ident| !RESERVED.contains(&i.0.as_str()),
    )(input)
}

pub fn param(input: Span) -> IResult<Span, Ident> {
    ws(preceded(char(':'), ident))(input)
}

pub fn literal(input: Span) -> IResult<Span, Literal> {
    map(ws(double), |x| Literal(x))(input)
}

pub fn op<'a, O: Copy + 'a>(ch: char, op: O) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, O> {
    ws(map(char(ch), move |_| op))
}

pub fn atom(input: Span) -> IResult<Span, NumExpr> {
    let paren = delimited(char('('), num_expr, char(')'));
    let lit = map(literal, |lit| NumExpr::Lit(lit));
    let param = map(param, |p| NumExpr::Param(p));
    alt((paren, lit, param))(input)
}

pub fn term(input: Span) -> IResult<Span, NumExpr> {
    let mul = op('*', Op::Mul);
    let div = op('/', Op::Div);
    let (input, init) = atom.parse(input)?;

    fold_many0(
        alt((mul, div)).and(atom),
        move || init.clone(),
        |lhs, (op, rhs)| NumExpr::Op(Box::new(lhs), op, Box::new(rhs)),
    )(input)
}

pub fn num_expr(input: Span) -> IResult<Span, NumExpr> {
    let add = op('+', Op::Add);
    let sub = op('-', Op::Sub);
    let (input, init) = term.parse(input)?;

    fold_many0(
        alt((add, sub)).and(term),
        move || init.clone(),
        |lhs, (op, rhs)| NumExpr::Op(Box::new(lhs), op, Box::new(rhs)),
    )(input)
}

pub fn bool_expr(input: Span) -> IResult<Span, BoolExpr> {
    let cmp = alt((op('=', Cmp::Eq), op('<', Cmp::Lt), op('>', Cmp::Gt)));
    map(tuple((num_expr, cmp, num_expr)), |(a, cmp, b)| {
        BoolExpr(a, cmp, b)
    })(input)
}

pub fn procedure_def(input: Span) -> IResult<Span, ProcedureDef> {
    map(
        delimited(
            tag("to"),
            ws(tuple((ident, many0(param), many0(statement)))),
            tag("end"),
        ),
        |(name, params, statements)| ProcedureDef {
            name,
            params,
            body: Block { statements },
        },
    )(input)
}

pub fn procedure_call(input: Span) -> IResult<Span, ProcedureCall> {
    map(tuple((ident, many0(num_expr))), |(name, params)| {
        ProcedureCall { name, params }
    })(input)
}

pub fn block(input: Span) -> IResult<Span, Block> {
    map(
        delimited(char('['), many0(statement), char(']')),
        |statements| Block { statements },
    )(input)
}

pub fn statement(input: Span) -> IResult<Span, Statement> {
    let if_statement = map(tuple((tag("if"), bool_expr, block)), |(_, e, b)| {
        Statement::If(e, b)
    });
    let repeat_statement = map(tuple((tag("repeat"), num_expr, block)), |(_, n, b)| {
        Statement::Repeat(n, b)
    });
    alt((
        ws(if_statement),
        ws(repeat_statement),
        map(procedure_def, |pd| Statement::Def(pd)),
        map(procedure_call, |pc| Statement::Call(pc)),
    ))(input)
}

pub fn program<'a>(input: impl Into<Span<'a>>) -> IResult<Span<'a>, Block> {
    map(many0(statement), |statements| Block { statements })(input.into())
}
