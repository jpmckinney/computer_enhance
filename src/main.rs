use std::env;
use std::fs::File;
use std::io::{self, BufReader, Bytes, Read, Write};

const REG_NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];

const RM_NAMES: [&str; 8] = [
    "[bx + si]",
    "[bx + di]",
    "[bp + si]",
    "[bp + di]",
    "si",
    "di",
    "[bp]",
    "bx",
];

const RM_SUBSTRINGS: [&str; 8] = ["bx + si", "bx + di", "bp + si", "bp + di", "si", "di", "bp", "bx"];

macro_rules! out {
    ( $stdout:ident, $left:expr, $right:expr) => {
        writeln!($stdout, "mov {}, {}", $left, $right).unwrap();
    };
}

fn disp(iterator: &mut Bytes<BufReader<File>>, w: bool) -> u16 {
    if w {
        u16::from_ne_bytes([iterator.next().unwrap().unwrap(), iterator.next().unwrap().unwrap()])
    } else {
        u16::from(iterator.next().unwrap().unwrap())
    }
}

fn run<W: Write>(filename: &str, mut stdout: W) {
    let file = BufReader::new(File::open(filename).unwrap());
    let mut iterator = file.bytes();

    writeln!(stdout, "bits 16").unwrap();

    // OPCODE D W
    while let Some(Ok(byte1)) = iterator.next() {
        // Immediate to register.
        if (byte1 >> 4) & 0b1011 == 0b1011 {
            let w = ((byte1 >> 3) & 1) as usize;
            let reg = (byte1 & 0b111) as usize;
            let data = disp(&mut iterator, w == 1);

            out!(stdout, REG_NAMES[w][reg], data);
        // Assume MOV opcode.
        } else {
            // MOD REG R/M
            let byte2 = iterator.next().unwrap().unwrap();

            let d = (byte1 >> 1) & 1 == 1;
            let w = (byte1 & 1) as usize;
            let m0d = byte2 >> 6; // mod
            let reg = ((byte2 >> 3) & 0b111) as usize;
            let rm = (byte2 & 0b111) as usize;

            let reg_text = REG_NAMES[w][reg].to_string();
            let rm_text = match m0d {
                // Memory mode. No displacement follows.*
                0b00 => RM_NAMES[rm].to_string(),
                // Memory mode. Displacement follows.
                0b01 | 0b10 => {
                    let data = disp(&mut iterator, m0d == 0b10);
                    if data == 0 {
                        RM_NAMES[rm].to_string()
                    } else {
                        format!("[{} + {}]", RM_SUBSTRINGS[rm], data)
                    }
                }
                // Register mode. No displacement follows.
                0b11 => REG_NAMES[w][rm].to_string(),
                _ => panic!(),
            };

            // 1 = the REG field identifies the destination operand.
            // 0 = the REG field identifies the source operand.
            let (dst, src) = if d { (reg_text, rm_text) } else { (rm_text, reg_text) };
            out!(stdout, dst, src);
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

        let mut text = vec![];
        File::open(assembly_path.clone())
            .unwrap()
            .read_to_end(&mut text)
            .unwrap();
        println!("{}", String::from_utf8(text).unwrap());

        let status = Command::new("nasm")
            .args(["-o", binary_path.to_str().unwrap(), assembly_path.to_str().unwrap()])
            .status()
            .expect("failed to execute process");

        assert!(status.success());

        let mut actual = vec![];
        File::open(binary_path).unwrap().read_to_end(&mut actual).unwrap();
        let mut expected = vec![];
        File::open(test_path).unwrap().read_to_end(&mut expected).unwrap();
        assert_eq!(actual, expected);
    }

    include!(concat!(env!("OUT_DIR"), "/main.include"));
}
