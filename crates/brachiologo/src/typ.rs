use std::{collections::HashMap, rc::Rc};

use crate::proc::Proc;

pub type EvalResult = Result<Option<Expr>, EvalError>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone)]
pub struct ProcExpr {
    pub inner: Rc<dyn Proc>,
}

impl std::fmt::Debug for ProcExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("proc")
    }
}

impl PartialEq for ProcExpr {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl ProcExpr {
    fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult {
        self.inner.eval(args, env)
    }

    fn num_args(&self) -> usize {
        self.inner.num_args()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

impl Span {
    pub fn union(&self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl From<crate::parse::Span<'_>> for Span {
    fn from(sp: crate::parse::Span) -> Self {
        Span {
            start: sp.location_offset(),
            end: sp.location_offset() + sp.fragment().len(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Lt,
    Gt,
}

impl Op {
    pub fn priority(&self) -> Priority {
        match self {
            Op::Add | Op::Sub => Priority::Add,
            Op::Mul | Op::Div => Priority::Mul,
            Op::Eq | Op::Lt | Op::Gt => Priority::Cmp,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Eq => "=",
            Op::Lt => "<",
            Op::Gt => ">",
        }
    }

    pub fn eval(&self, lhs: &Expr, rhs: &Expr) -> Result<Expr, EvalError> {
        let ExprKind::Num(l) = lhs.e else {
            return Err(EvalError::BadOpArg { op: self.clone(), arg: lhs.clone() });
        };
        let ExprKind::Num(r) = rhs.e else {
            return Err(EvalError::BadOpArg { op: self.clone(), arg: rhs.clone() });
        };
        let e = match self {
            Op::Add => ExprKind::Num(l + r),
            Op::Sub => ExprKind::Num(l - r),
            Op::Mul => ExprKind::Num(l * r),
            Op::Div => ExprKind::Num(l / r), // TODO: check for zero
            Op::Eq => ExprKind::Bool(l == r),
            Op::Lt => ExprKind::Bool(l < r),
            Op::Gt => ExprKind::Bool(l > r),
        };
        let span = lhs.span.union(rhs.span);
        Ok(Expr { e, span })
    }
}

impl TryFrom<char> for Op {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        Ok(match value {
            '+' => Op::Add,
            '-' => Op::Sub,
            '*' => Op::Mul,
            '/' => Op::Div,
            '=' => Op::Eq,
            '<' => Op::Lt,
            '>' => Op::Gt,
            _ => Err(())?,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Num(f64),
    Bool(bool),
    Var(String),
    Word(String),
    Proc(ProcExpr),
    DefProc(ProcExpr),
    Op(Op),
    List(Vec<Expr>),
    Quote(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub e: ExprKind,
    pub span: Span,
}

impl TryFrom<Expr> for f64 {
    type Error = ();

    fn try_from(value: Expr) -> Result<Self, ()> {
        match value.e {
            ExprKind::Num(x) => Ok(x),
            _ => Err(()),
        }
    }
}

impl TryFrom<Expr> for bool {
    type Error = ();

    fn try_from(value: Expr) -> Result<Self, ()> {
        match value.e {
            ExprKind::Bool(x) => Ok(x),
            _ => Err(()),
        }
    }
}

impl TryFrom<Expr> for String {
    type Error = ();

    fn try_from(value: Expr) -> Result<Self, ()> {
        match value.e {
            ExprKind::Word(s) => Ok(s),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurtleCmd {
    Forward(f64),
    Back(f64),
    Right(f64),
    Left(f64),
    PenUp,
    PenDown,
}

pub struct Env {
    // Invariant: this is always non-empty.
    pub stack: Vec<Frame>,
    pub turtle: Vec<TurtleCmd>,
}

impl Default for Env {
    fn default() -> Self {
        let mut ret = Env {
            stack: vec![Frame::default()],
            turtle: Vec::new(),
        };
        crate::proc::add_builtins(&mut ret);
        ret
    }
}

#[derive(Default)]
pub struct Frame {
    vars: HashMap<String, Expr>,
    procs: HashMap<String, ProcExpr>,
}

impl Env {
    pub fn lookup_proc(&self, name: &str) -> Option<ProcExpr> {
        self.stack
            .iter()
            .find_map(|frame| frame.procs.get(name).cloned())
    }

    pub fn lookup_var(&self, name: &str) -> Option<Expr> {
        self.stack
            .iter()
            .find_map(|frame| frame.vars.get(name).cloned())
    }

    pub fn scoped<U>(&mut self, f: impl FnOnce(&mut Env) -> U) -> U {
        self.stack.push(Frame::default());
        let res = f(self);
        self.stack.pop();
        res
    }

    pub fn def_var(&mut self, name: &str, val: Expr) {
        self.stack
            .last_mut()
            .unwrap()
            .vars
            .insert(name.to_owned(), val);
    }

    pub fn def_proc(&mut self, proc: ProcExpr) {
        self.stack
            .last_mut()
            .unwrap()
            .procs
            .insert(proc.name().to_owned(), proc);
    }

    pub fn turtle_do(&mut self, cmd: TurtleCmd) {
        self.turtle.push(cmd);
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.e {
            ExprKind::Num(x) => x.fmt(f),
            ExprKind::Bool(x) => x.fmt(f),
            ExprKind::Var(s) => f.write_fmt(format_args!(":{}", s)),
            ExprKind::Word(w) => w.fmt(f),
            ExprKind::Proc(p) => p.name().fmt(f),
            ExprKind::DefProc(p) => f.write_fmt(format_args!("to {} .. end", p.name())),
            ExprKind::Op(op) => op.name().fmt(f),
            ExprKind::List(list) => {
                f.write_str("(")?;
                for (i, e) in list.iter().enumerate() {
                    e.fmt(f)?;
                    if i < list.len() {
                        f.write_str(" ")?;
                    }
                }
                f.write_str(")")
            }
            ExprKind::Quote(e) => f.write_fmt(format_args!("\"{}", e)),
        }
    }
}

// TODO: improve the context of errors. Maybe have them chain? So for example a BadArg error will
// capture the arg that was bad, but the one level higher we'll also capture the procedure whose evaluation
// complained about the arg...
#[derive(Clone, Debug, thiserror::Error)]
pub enum EvalError {
    #[error("Not enough inputs to {} (got {}, expected {})", .proc.name(), .args.len(), .proc.num_args())]
    NotEnoughInputs { proc: ProcExpr, args: Vec<Expr> },
    #[error("Missing input to {op}")]
    MissingOpInput { op: Expr },
    #[error("You don't say what to do with {val}")]
    UnusedVal { val: Expr },
    #[error("{ident} has no value")]
    UnknownVal { ident: Expr },
    #[error("I don't know how to {ident}")]
    UnknownProc { ident: Expr },
    #[error("didn't output to {proc}")]
    NoOutputTo { proc: String },
    // TODO: location info
    #[error("{proc} doesn't like {arg} as input")]
    BadArg { proc: String, arg: Expr },
    // TODO: op doesn't have location info
    #[error("{} doesn't like {arg} as input", .op.name())]
    BadOpArg { op: Op, arg: Expr },
    // TODO: How does ucblogo handle empty lists?
    #[error("I can't eval an empty list")]
    EmptyList,
}

impl Expr {
    pub fn eval(&self, env: &mut Env) -> Result<Option<Expr>, EvalError> {
        let e = match &self.e {
            ExprKind::Num(_) => Some(self.e.clone()),
            ExprKind::Bool(_) => Some(self.e.clone()),
            ExprKind::Quote(v) => Some(ExprKind::clone(&v.e)),
            ExprKind::Word(w) => Some(ExprKind::Proc(env.lookup_proc(&w).ok_or_else(|| {
                EvalError::UnknownProc {
                    ident: self.clone(),
                }
            })?)),
            ExprKind::Var(w) => Some(
                env.lookup_var(&w)
                    .ok_or_else(|| EvalError::UnknownVal {
                        ident: self.clone(),
                    })?
                    .e,
            ),
            ExprKind::List(list) => eval_list(list.as_slice(), env)?.map(|ex| ex.e),
            ExprKind::Proc(p) => Err(EvalError::NotEnoughInputs {
                proc: p.clone(),
                args: vec![],
            })?,
            ExprKind::DefProc(p) => {
                env.def_proc(p.clone());
                None
            }
            ExprKind::Op(_) => Err(EvalError::MissingOpInput { op: self.clone() })?,
        };
        let span = self.span;
        Ok(e.map(|e| Expr { e, span }))
    }
}

// Operator precedence, with the loosest-binding ones first.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd)]
pub enum Priority {
    Stop,
    Output,
    Maybe,
    Tail,
    Macro,
    Prefix,
    Cmp,
    Add,
    Mul,
}

/// Evaluate a list.
///
/// Logo is a bit weirder than lisp when it comes to evaluating a list. In lisp, the list `(f a b c)` is evaluated by
/// first evaluating `a`, `b`, and `c`, and then applying `f` with the results of those evaluations. In logo, however, `f`
/// is allowed to choose how many arguments it takes. For example, if `f` only wants a single argument then we first
/// evaluate `a`, then apply `f` to the result of that. Then we have `(b c)` left over and we repeat (by evaluating `c`
/// and applying `b` to the result of that).
///
/// If a function doesn't use up the whole list (like `f` in the example above) but it returns a value, that's an error.
fn eval_list(mut list: &[Expr], env: &mut Env) -> EvalResult {
    loop {
        // TODO: break on stop if we're in a procedure
        let (val, rest) = eval_list_once(list, Priority::Stop, env)?;

        match (val, rest.is_empty()) {
            (v, true) => {
                return Ok(v);
            }
            (Some(v), false) => {
                if let Some(
                    op_expr @ Expr {
                        e: ExprKind::Op(op),
                        ..
                    },
                ) = rest.first()
                {
                    let (val, remainder) = eval_list_op(v.clone(), *op, op_expr, &rest[1..], env)?;
                    if dbg!(remainder.is_empty()) {
                        return Ok(Some(val));
                    } else {
                        return Err(EvalError::UnusedVal { val: v });
                    }
                } else {
                    return Err(EvalError::UnusedVal { val: v });
                }
            }
            (None, false) => {}
        }
        list = rest;
    }
}

/// Having already evaluated the left hand side of a binary operator, read the right hand side
/// from a list and evaluate the operator. Returns the result of evaluating the operator, and also
/// the remainder of the list.
fn eval_list_op<'a>(
    mut lhs: Expr,
    mut op: Op,
    op_expr: &Expr,
    mut list: &'a [Expr],
    env: &mut Env,
) -> Result<(Expr, &'a [Expr]), EvalError> {
    loop {
        let (rhs, remainder) = eval_list_once(list, op.priority(), env)?;
        let rhs = rhs.ok_or_else(|| EvalError::MissingOpInput {
            op: op_expr.clone(),
        })?;
        lhs = op.eval(&lhs, &rhs)?;

        if let Some(Expr {
            e: ExprKind::Op(next_op),
            ..
        }) = list.first()
        {
            op = *next_op;
            list = remainder;
        } else {
            return Ok((lhs, remainder));
        }
    }
}

/// Evaluate the first part of a list.
///
/// With the documentation of [`eval_list`] for context, this function just evaluates the first part of a list,
/// by evaluating one function and the arguments it wants. It returns the result of that evaluation and also
/// the part of the list that didn't get evaluated yet.
///
/// `priority` is for handling binary operators: if our evaluation would finish just before a binary operator,
/// and that operator has priority higher than `priority`, we evaluate that boolean operator.
/// For example, if the list is `(2 * 3 a b c)` and our priority is `+`, then we'll evaluate `2 * 3` and return
/// `(a b c)` as the remainder. But if our priority is `*` then we'll just evaluate `2` and return `(* 3 a b c)`
/// as the remainder.
fn eval_list_once<'a>(
    list: &'a [Expr],
    priority: Priority,
    env: &mut Env,
) -> Result<(Option<Expr>, &'a [Expr]), EvalError> {
    let (first, mut list) = list.split_first().ok_or(EvalError::EmptyList)?;
    let first = first.eval(env)?;
    match first {
        None => Ok((None, list)),
        Some(Expr {
            e: ExprKind::Proc(p),
            ..
        }) => {
            let mut args = Vec::with_capacity(p.num_args());
            while args.len() < p.num_args() {
                dbg!(&list);
                if list.is_empty() {
                    return Err(EvalError::NotEnoughInputs {
                        proc: p.clone(),
                        args,
                    });
                }
                let (arg, remainder) = eval_list_once(list, priority, env)?;
                list = remainder;
                let arg = arg.ok_or_else(|| EvalError::NoOutputTo {
                    proc: p.name().to_string(),
                })?;
                args.push(arg);
            }
            Ok((p.eval(&args, env)?, list))
        }
        Some(x) => {
            if let Some(
                op_expr @ Expr {
                    e: ExprKind::Op(op),
                    ..
                },
            ) = list.first()
            {
                if op.priority() > priority {
                    let (val, remainder) = eval_list_op(x, *op, op_expr, &list[1..], env)?;
                    Ok((Some(val), remainder))
                } else {
                    Ok((Some(x), list))
                }
            } else {
                Ok((Some(x), list))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(x: f64) -> Expr {
        Expr {
            e: ExprKind::Num(x),
            span: Span { start: 0, end: 0 },
        }
    }

    fn op(op: Op) -> Expr {
        Expr {
            e: ExprKind::Op(op),
            span: Span { start: 0, end: 0 },
        }
    }

    #[test]
    fn arithmetic() {
        let x = num(42.0);
        let mut env = Env::default();

        assert_eq!(x.eval(&mut env).unwrap().unwrap(), x);

        let y = num(7.0);
        let z = num(2.0);
        let plus = op(Op::Add);
        let times = op(Op::Mul);
        let expr = Expr {
            e: ExprKind::List(vec![x.clone(), plus.clone(), y.clone(), times, z.clone()]),
            span: Span { start: 0, end: 0 },
        };

        assert_eq!(expr.eval(&mut env).unwrap().unwrap(), num(56.0));

        let expr = Expr {
            e: ExprKind::List(vec![x, plus.clone(), y, plus, z]),
            span: Span { start: 0, end: 0 },
        };

        assert_eq!(expr.eval(&mut env).unwrap().unwrap(), num(51.0));
    }
}
