use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};

const NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];

fn run<W: Write>(filename: &str, mut stdout: W) {
    let file = BufReader::new(File::open(filename).unwrap());
    let mut iterator = file.bytes();

    writeln!(stdout, "bits 16").unwrap();

    // OPCODE D W
    while let Some(Ok(byte1)) = iterator.next() {
        // Immediate to register.
        if (byte1 >> 4) & 1 == 1 {
            let w = ((byte1 >> 3) & 1) as usize;
            let reg = byte1 & 0b111;

            let mut data = [0; 2];
            if w == 1 {
                data[0] = iterator.next().unwrap().unwrap();
            }
            data[1] = iterator.next().unwrap().unwrap();

            writeln!(stdout, "mov {}, {}", NAMES[w][reg as usize], u16::from_ne_bytes(data)).unwrap();
        } else {
            // MOD REG R/M
            let byte2 = iterator.next().unwrap().unwrap();
            let w = (byte1 & 1) as usize;

            writeln!(
                stdout,
                "mov {}, {}",
                NAMES[w][(byte2 & 0b111) as usize],
                NAMES[w][((byte2 >> 3) & 0b111) as usize]
            )
            .unwrap();
        }
    }
}

fn main() {
    let filename = env::args().nth(1).unwrap();
    run(&filename, &mut io::stdout().lock());
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;
    use std::process::Command;

    use tempfile::tempdir;

    fn check(test_path: &str) {
        let dir = tempdir().unwrap();
        let assembly_path = dir.path().join("test.asm");
        let binary_path = dir.path().join("test");
        let mut assembly_file = File::create(assembly_path.clone()).unwrap();

        run(test_path, &mut assembly_file);

        Command::new("nasm")
            .args(["-o", binary_path.to_str().unwrap(), assembly_path.to_str().unwrap()])
            .output()
            .unwrap();

        let mut actual = vec![];
        File::open(binary_path).unwrap().read_to_end(&mut actual).unwrap();
        let mut expected = vec![];
        File::open(test_path).unwrap().read_to_end(&mut expected).unwrap();
        assert_eq!(actual, expected);
    }

    include!(concat!(env!("OUT_DIR"), "/main.include"));
}
