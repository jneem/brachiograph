use std::rc::Rc;

use crate::{
    typ::{EvalResult, ExprKind, ProcExpr, Span, TurtleCmd, Val},
    Env, EvalError, Expr,
};

#[derive(Clone, Debug, PartialEq)]
pub struct UserProc {
    pub args: Vec<String>,
    pub body: Expr,
    pub name: String,
}

impl From<UserProc> for ProcExpr {
    fn from(p: UserProc) -> Self {
        ProcExpr { inner: Rc::new(p) }
    }
}

impl Proc for UserProc {
    fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult {
        assert_eq!(args.len(), self.args.len());
        env.scoped(|env| {
            for (name, e) in self.args.iter().zip(args) {
                env.def_var(name, e.clone());
            }
            self.body.eval(env)
        })
    }

    fn num_args(&self) -> usize {
        self.args.len()
    }

    fn name(&self) -> &str {
        &self.name
    }
}

pub trait Proc {
    fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult;
    fn num_args(&self) -> usize;
    fn name(&self) -> &str;
}

struct FnZero<F: Fn(&mut Env) -> EvalResult> {
    f: F,
    name: &'static str,
}

struct FnOne<T, F: Fn(T, &mut Env) -> EvalResult> {
    f: F,
    marker: std::marker::PhantomData<T>,
    name: &'static str,
}

struct FnTwo<S, T, F: Fn(S, T, &mut Env) -> EvalResult> {
    f: F,
    marker1: std::marker::PhantomData<S>,
    marker2: std::marker::PhantomData<T>,
    name: &'static str,
}

impl<F: Fn(&mut Env) -> EvalResult> Proc for FnZero<F> {
    fn eval(&self, _args: &[Expr], env: &mut Env) -> EvalResult {
        (self.f)(env)
    }

    fn num_args(&self) -> usize {
        0
    }

    fn name(&self) -> &str {
        self.name
    }
}
impl<T: TryFrom<Expr>, F: Fn(T, &mut Env) -> EvalResult> Proc for FnOne<T, F> {
    fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult {
        match args[0].clone().try_into() {
            Ok(x) => (self.f)(x, env),
            Err(_) => Err(EvalError::BadArg {
                proc: self.name.to_owned(),
                arg: args[0].clone(),
            }),
        }
    }

    fn num_args(&self) -> usize {
        1
    }

    fn name(&self) -> &str {
        self.name
    }
}

impl<S, T, F> Proc for FnTwo<S, T, F>
where
    S: TryFrom<Expr>,
    T: TryFrom<Expr>,
    F: Fn(S, T, &mut Env) -> EvalResult,
{
    fn eval(&self, args: &[Expr], env: &mut Env) -> EvalResult {
        match (args[0].clone().try_into(), args[1].clone().try_into()) {
            (Ok(x), Ok(y)) => (self.f)(x, y, env),
            (Err(_), _) => Err(EvalError::BadArg {
                proc: self.name.to_owned(),
                arg: args[0].clone(),
            }),
            (_, Err(_)) => Err(EvalError::BadArg {
                proc: self.name.to_owned(),
                arg: args[1].clone(),
            }),
        }
    }

    fn num_args(&self) -> usize {
        2
    }

    fn name(&self) -> &str {
        self.name
    }
}

trait IntoEvalResult {
    fn into_eval_result(self) -> EvalResult;
}

impl IntoEvalResult for () {
    fn into_eval_result(self) -> EvalResult {
        Ok(None)
    }
}

impl IntoEvalResult for EvalResult {
    fn into_eval_result(self) -> EvalResult {
        self
    }
}

impl IntoEvalResult for f64 {
    fn into_eval_result(self) -> EvalResult {
        Ok(Some(Expr {
            e: ExprKind::Val(Val::Num(self)),
            // TODO: how to handle missing spans in a principled way?
            span: Span { start: 0, end: 0 },
        }))
    }
}

fn fn_zero<U, F>(name: &'static str, f: F) -> ProcExpr
where
    U: IntoEvalResult + 'static,
    F: Fn(&mut Env) -> U + 'static,
{
    ProcExpr {
        inner: Rc::new(FnZero {
            f: move |env| f(env).into_eval_result(),
            name,
        }),
    }
}
fn fn_one<
    T: TryFrom<Expr> + 'static,
    U: IntoEvalResult + 'static,
    F: Fn(T, &mut Env) -> U + 'static,
>(
    name: &'static str,
    f: F,
) -> ProcExpr {
    ProcExpr {
        inner: Rc::new(FnOne {
            f: move |x, env| f(x, env).into_eval_result(),
            marker: std::marker::PhantomData,
            name,
        }),
    }
}

fn fn_two<S, T, U, F>(name: &'static str, f: F) -> ProcExpr
where
    S: TryFrom<Expr> + 'static,
    T: TryFrom<Expr> + 'static,
    U: IntoEvalResult + 'static,
    F: Fn(S, T, &mut Env) -> U + 'static,
{
    ProcExpr {
        inner: Rc::new(FnTwo {
            f: move |x, y, env| f(x, y, env).into_eval_result(),
            marker1: std::marker::PhantomData,
            marker2: std::marker::PhantomData,
            name,
        }),
    }
}

pub fn add_builtins(env: &mut Env) {
    env.def_proc(fn_one("forward", |x, env| {
        env.turtle_do(TurtleCmd::Forward(x))
    }));
    env.def_proc(fn_one("fd", |x, env| env.turtle_do(TurtleCmd::Forward(x))));
    env.def_proc(fn_one("back", |x, env| env.turtle_do(TurtleCmd::Back(x))));
    env.def_proc(fn_one("bk", |x, env| env.turtle_do(TurtleCmd::Back(x))));

    env.def_proc(fn_zero("penup", |env| env.turtle_do(TurtleCmd::PenUp)));
    env.def_proc(fn_zero("pendown", |env| env.turtle_do(TurtleCmd::PenDown)));

    env.def_proc(fn_two("make", |sym: String, val, env| {
        env.def_var(&sym, val)
    }));
    env.def_proc(fn_two("sum", |x: f64, y: f64, _env| x + y));
    env.def_proc(fn_two("prod", |x: f64, y: f64, _env| x * y));

    env.def_proc(fn_two("if", |cond: bool, body: Expr, env| {
        if dbg!(cond) {
            dbg!(dbg!(body).eval(env))
        } else {
            Ok(None)
        }
    }));
    env.def_proc(fn_two("repeat", |count: Expr, body: Expr, env| {
        let ExprKind::Val(Val::Num(count_num)) = count.e.clone() else {
                return Err(EvalError::BadArg { proc: "repeat".to_owned(), arg: count });
            };
        if count_num < 0.0 || count_num.trunc() != count_num {
            return Err(EvalError::BadArg {
                proc: "repeat".to_owned(),
                arg: count,
            });
        }
        for _ in 0..(count_num as u64) {
            if let Some(res) = body.eval(env)? {
                return Err(EvalError::UnusedVal { val: res });
            }
        }
        Ok(None)
    }));
}
