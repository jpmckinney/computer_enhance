use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

fn convert(byte: u8, wide: bool) -> &'static str {
    if wide {
        match byte {
            0b0000_0000 => "ax",
            0b0000_0001 => "cx",
            0b0000_0010 => "dx",
            0b0000_0011 => "bx",
            0b0000_0100 => "sp",
            0b0000_0101 => "bp",
            0b0000_0110 => "si",
            0b0000_0111 => "di",
            _ => panic!(),
        }
    } else {
        match byte {
            0b0000_0000 => "al",
            0b0000_0001 => "cl",
            0b0000_0010 => "dl",
            0b0000_0011 => "bl",
            0b0000_0100 => "ah",
            0b0000_0101 => "ch",
            0b0000_0110 => "dh",
            0b0000_0111 => "bh",
            _ => panic!(),
        }
    }
}

fn run<W: Write>(filename: &str, mut stdout: W) {
    let mut buffer = [0; 2];
    let mut file = File::open(filename).unwrap();

    writeln!(stdout, "bits 16").unwrap();

    while file.read(&mut buffer).unwrap_or(0) > 0 {
        let byte1 = buffer[0]; // OPCODE D W
        let byte2 = buffer[1]; // MOD REG R/M

        // Assume OPCODE is 100010 and D is 0.
        let w = byte1 & 1 == 1;

        // Assume MOD is 11.
        writeln!(
            stdout,
            "mov {}, {}",
            // I don't know bit operations. ¯\_(ツ)_/¯
            convert(byte2 & 0b0000_0111, w),
            convert((byte2 & 0b0011_1000).rotate_right(3), w)
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
