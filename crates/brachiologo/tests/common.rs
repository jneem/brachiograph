use std::path::Path;

use brachiologo::{Env, EvalError, Expr};

#[derive(Default, Clone)]
pub struct TestCase {
    input: String,
    expected: String,
}

fn exec_expr(s: &str) -> Result<Option<Expr>, EvalError> {
    let (remaining, expr) = brachiologo::parse::expr(dbg!(s.trim().into())).unwrap();
    assert!(remaining.is_empty());
    let mut env = Env::default();
    dbg!(expr).eval(&mut env)
}

/*
fn exec_one(s: &str) -> Result<(), brachiologo::Error> {
    let (remaining, prog) = brachiologo::parse::program(dbg!(s)).unwrap();
    assert!(remaining.is_empty());
    let mut scope = brachiologo::Scope::default();
    let mut builtins = Vec::new();
    scope.exec_block(&mut builtins, &prog)?;
    Ok(())
    //Ok(builtins)
}
*/

fn parse_loc(s: &str) -> (usize, u32, &str) {
    let mut split = s.trim().splitn(3, ' ');
    let offset = split.next().unwrap().parse().unwrap();
    let line = split.next().unwrap().parse().unwrap();
    let rest = split.next().unwrap();
    (offset, line, rest)
}

impl TestCase {
    fn exec_expr(&self) {
        let a = exec_expr(&self.input).unwrap();
        let b = exec_expr(&self.expected).unwrap();
        assert_eq!(a.map(|e| e.e), b.map(|e| e.e));
    }

    /*
    fn exec(&self) {
        let a = exec_one(&self.input).unwrap();
        let b = exec_one(&self.expected).unwrap();
        assert_eq!(a, b);
    }

    fn exec_failure(&self) {
        let a = exec_one(&self.input).unwrap_err();
        let spn = a.span();
        let (offset, line, frag) = parse_loc(&self.expected);
        assert_eq!(
            (offset, line, frag),
            (spn.location_offset(), spn.location_line(), *spn.fragment())
        );
    }
    */
}

pub fn read_tests(path: impl AsRef<Path>) -> Vec<TestCase> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut ret = Vec::new();
    let mut in_input = true;
    let mut cur = TestCase::default();

    fn separator_line(line: &str, ch: u8) -> bool {
        line.trim().len() >= 2 && line.trim().bytes().all(|c| c == ch)
    }

    for line in text.split_inclusive('\n') {
        if in_input {
            if separator_line(line, b'-') {
                in_input = false;
            } else {
                cur.input += line;
            }
        } else {
            if separator_line(line, b'=') {
                in_input = true;
                ret.push(std::mem::take(&mut cur));
            } else {
                cur.expected += line;
            }
        }
    }
    ret
}

#[test]
fn test_exprs() {
    let tests = read_tests("tests/expr.txt");
    for test in tests {
        test.exec_expr();
    }
}

/*
#[test]
fn text_tests() {
    let tests = read_tests("tests/basic.txt");
    for test in tests {
        test.exec();
    }
}

#[test]
fn exec_failures() {
    let tests = read_tests("tests/exec-failures.txt");
    for test in tests {
        test.exec_failure();
    }
}
*/
