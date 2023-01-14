use brachiologo::{program, Scope};

fn main() {
    let prog = program("to square :n repeat 4 [fd :n rt 90] end square 90")
        .unwrap()
        .1;
    let mut scope = Scope::default();
    let mut output = Vec::new();
    scope.exec_block(&mut output, &prog).unwrap();
    dbg!(output);
}
