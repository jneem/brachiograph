pub type EvalResult = Result<Option<Expr>, EvalError>;

#[derive(Clone, Debug)]
pub struct Token {
    // TODO: logo does a very lazy kind of evaluation. You can write [+ 1 2] or [1 if 2] and it won't throw an error until evaluation time.
    // So elements of a list are stored as raw "tokens" until evaluation time.
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OpKind {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Lt,
    Gt,
}

impl OpKind {
    pub fn priority(&self) -> Priority {
        match self {
            OpKind::Add | OpKind::Sub => Priority::Add,
            OpKind::Mul | OpKind::Div => Priority::Mul,
            OpKind::Eq | OpKind::Lt | OpKind::Gt => Priority::Cmp,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            OpKind::Add => "+",
            OpKind::Sub => "-",
            OpKind::Mul => "*",
            OpKind::Div => "/",
            OpKind::Eq => "=",
            OpKind::Lt => "<",
            OpKind::Gt => ">",
        }
    }

    pub fn eval(&self, lhs: &Expr, rhs: &Expr) -> Result<Expr, EvalError> {
        let Expr::Val(Val::Num(lhs)) = lhs else {
            return Err(EvalError::BadArg { proc: self.name().to_owned(), arg: lhs.clone() });
        };
        let Expr::Val(Val::Num(rhs)) = rhs else {
            return Err(EvalError::BadArg { proc: self.name().to_owned(), arg: rhs.clone() });
        };
        Ok(Expr::Val(match self {
            OpKind::Add => Val::Num(rhs + lhs),
            OpKind::Sub => Val::Num(rhs - lhs),
            OpKind::Mul => Val::Num(rhs * lhs),
            OpKind::Div => Val::Num(rhs / lhs), // TODO: check for zero
            OpKind::Eq => Val::Bool(rhs == lhs),
            OpKind::Lt => Val::Bool(rhs < lhs),
            OpKind::Gt => Val::Bool(rhs > lhs),
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Bool(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Val(Val),
    Var(String),
    Word(String),
    Proc(Proc), // TODO: reference equality?
    Op(OpKind),
    List(Vec<Expr>),
    Quote(Box<Expr>),
}

pub struct ProgState {
    // TODO
}

pub enum ProcBody {
    User(Vec<Token>),
    BuiltIn(/* TODO */),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    name: String,
    num_args: usize,
    priority: Priority,
    // TODO: body?
}

impl Proc {
    pub fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult {
        todo!()
    }
}

#[derive(Default)]
pub struct Env {
    pub stack: Vec<Frame>,
}

pub struct Frame {
    // TODO: local variables, etc
}

impl Env {
    pub fn lookup_proc(&self, name: &str) -> Option<Proc> {
        todo!()
    }

    pub fn lookup_var(&self, name: &str) -> Option<Val> {
        todo!()
    }
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum EvalError {
    #[error("Not enough inputs to {proc}")]
    NotEnoughInputs { proc: String },
    #[error("You don't say what to do with {val}")]
    UnusedVal { val: Expr },
    #[error("{ident} has no value")]
    UnknownVal { ident: String },
    #[error("I don't know how to {ident}")]
    UnknownProc { ident: String },
    #[error("didn't output to {proc}")]
    NoOutputTo { proc: String },
    #[error("{proc} doesn't like {arg} as input")]
    BadArg { proc: String, arg: Expr },
    // TODO: How does ucblogo handle empty lists?
    #[error("I can't eval an empty list")]
    EmptyList,
}

impl Expr {
    pub fn eval(&self, env: &mut Env) -> Result<Option<Expr>, EvalError> {
        match self {
            // TODO: what does a word evaluate to?
            Expr::Val(_) => Ok(Some(self.clone())),
            Expr::Quote(v) => Ok(Some(Expr::clone(v))),
            Expr::Word(w) => {
                Ok(Some(Expr::Proc(env.lookup_proc(&w).ok_or_else(|| {
                    EvalError::UnknownProc { ident: w.clone() }
                })?)))
            }
            Expr::Var(w) => {
                Ok(Some(Expr::Val(env.lookup_var(&w).ok_or_else(|| {
                    EvalError::UnknownVal { ident: w.clone() }
                })?)))
            }
            Expr::List(list) => eval_list(list.as_slice(), env),
            Expr::Proc(p) => Err(EvalError::NotEnoughInputs {
                proc: p.name.clone(),
            }),
            Expr::Op(op) => Err(EvalError::NotEnoughInputs {
                proc: op.name().to_owned(),
            }),
        }
    }
}

// Operator precedence, with the loosest-binding ones first.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd)]
enum Priority {
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

fn eval_list(mut list: &[Expr], env: &mut Env) -> EvalResult {
    loop {
        // TODO: break on stop if we're in a procedure
        let (val, rest) = eval_list_once(list, Priority::Stop, env)?;

        match (val, rest.is_empty()) {
            (v, true) => {
                return Ok(v);
            }
            (Some(v), false) => {
                if let Some(Expr::Op(op)) = rest.first() {
                    let (val, remainder) = eval_list_op(v.clone(), *op, &rest[1..], env)?;
                    if dbg!(remainder.is_empty()) {
                        return Ok(Some(val));
                    } else {
                        return Err(EvalError::UnusedVal { val: v });
                    }
                } else {
                    // TODO: this is the right place to handle operators, I think
                    return Err(EvalError::UnusedVal { val: v });
                }
            }
            (None, false) => {}
        }
        list = rest;
    }
}

fn eval_list_op<'a>(
    mut lhs: Expr,
    mut op: OpKind,
    mut list: &'a [Expr],
    env: &mut Env,
) -> Result<(Expr, &'a [Expr]), EvalError> {
    loop {
        let (rhs, remainder) = eval_list_once(list, op.priority(), env)?;
        let rhs = rhs.ok_or_else(|| EvalError::NotEnoughInputs {
            proc: op.name().to_owned(),
        })?;
        lhs = op.eval(&lhs, &rhs)?;

        if let Some(Expr::Op(next_op)) = list.first() {
            op = *next_op;
            list = remainder;
        } else {
            return Ok((lhs, remainder));
        }
    }
}

fn eval_list_once<'a>(
    mut list: &'a [Expr],
    priority: Priority,
    env: &mut Env,
) -> Result<(Option<Expr>, &'a [Expr]), EvalError> {
    let (first, mut list) = list.split_first().ok_or(EvalError::EmptyList)?;
    let first = first.eval(env)?;
    match first {
        None => Ok((None, list)),
        Some(Expr::Proc(p)) => {
            let mut args = Vec::with_capacity(p.num_args);
            while args.len() < p.num_args {
                if list.is_empty() {
                    return Err(EvalError::NotEnoughInputs {
                        proc: p.name.clone(),
                    });
                }
                let (arg, remainder) = eval_list_once(list, priority, env)?;
                list = remainder;
                let arg = arg.ok_or_else(|| EvalError::NoOutputTo {
                    proc: p.name.clone(),
                })?;
                args.push(arg);
            }
            Ok((p.eval(&args, env)?, list))
        }
        Some(x) => {
            if let Some(Expr::Op(op)) = list.first() {
                if op.priority() > priority {
                    let (val, remainder) = eval_list_op(x, *op, &list[1..], env)?;
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

    #[test]
    fn arithmetic() {
        let x = Expr::Val(Val::Num(42.0));
        let mut env = Env::default();

        assert_eq!(x.eval(&mut env).unwrap().unwrap(), x);

        let y = Expr::Val(Val::Num(7.0));
        let z = Expr::Val(Val::Num(2.0));
        let plus = Expr::Op(OpKind::Add);
        let times = Expr::Op(OpKind::Mul);
        let expr = Expr::List(vec![x.clone(), plus.clone(), y.clone(), times, z.clone()]);

        assert_eq!(
            expr.eval(&mut env).unwrap().unwrap(),
            Expr::Val(Val::Num(56.0))
        );

        let expr = Expr::List(vec![x, plus.clone(), y, plus, z]);
        assert_eq!(
            expr.eval(&mut env).unwrap().unwrap(),
            Expr::Val(Val::Num(51.0))
        );
    }
}
