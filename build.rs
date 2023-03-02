use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use glob::glob;

fn main() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("main.include");
    let mut file = File::create(path).unwrap();

    for entry in glob("perfaware/part1/*.asm").expect("Failed to read glob pattern") {
        let path = entry.unwrap();
        let name = path.file_stem().unwrap().to_str().unwrap();

        write!(
            file,
            r#"
#[test]
fn {name}() {{
    check("perfaware/part1/{name}")
}}
"#
        )
        .unwrap();
    }
}
