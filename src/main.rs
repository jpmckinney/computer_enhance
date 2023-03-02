use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

const NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];

fn run<W: Write>(filename: &str, mut stdout: W) {
    let mut buffer = [0; 2];
    let mut file = File::open(filename).unwrap();

    writeln!(stdout, "bits 16").unwrap();

    while file.read(&mut buffer).unwrap_or(0) > 0 {
        let byte1 = buffer[0]; // OPCODE D W
        let byte2 = buffer[1]; // MOD REG R/M

        // Assume OPCODE is 100010 and D is 0.
        let w = (byte1 & 1) as usize;

        // Assume MOD is 11.
        writeln!(
            stdout,
            "mov {}, {}",
            NAMES[w][(byte2 & 0b111) as usize],
            NAMES[w][((byte2 >> 3) & 0b111) as usize]
        )
        .unwrap();
    }
}

fn main() {
    let filename = env::args().nth(1).unwrap();
    run(&filename, &mut io::stdout().lock());
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::BufRead;
    use std::io::BufReader;

    fn check(name: &str) {
        let file = File::open(format!("{name}.asm")).unwrap();
        let mut reader = BufReader::new(file);
        let mut line = vec![];
        let mut expected = vec![];

        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            // Skip comments and empty lines.
            if line[0] != b';' && line[0] != b'\n' {
                expected.extend(line.clone());
            }
            line.clear();
        }

        let mut stdout = vec![];
        run(name, &mut stdout);

        // Compare strings for readable output.
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(expected).unwrap()
        );
    }

    include!(concat!(env!("OUT_DIR"), "/main.include"));
}
