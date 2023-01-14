use std::path::Path;

use brachiologo::BuiltIn;

#[derive(Default, Clone)]
pub struct TestCase {
    input: String,
    expected: String,
}

fn exec_one(s: &str) -> Vec<BuiltIn> {
    let (remaining, prog) = brachiologo::program(s).unwrap();
    assert!(remaining.is_empty());
    let mut scope = brachiologo::Scope::default();
    let mut builtins = Vec::new();
    scope.exec_block(&mut builtins, &prog).unwrap();
    builtins
}

impl TestCase {
    fn exec(&self) {
        let a = exec_one(&self.input);
        let b = exec_one(&self.expected);
        assert_eq!(a, b);
    }
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
fn text_tests() {
    let tests = read_tests("tests/basic.txt");
    for test in tests {
        test.exec();
    }
}
