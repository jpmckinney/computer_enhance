use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::{self, BufReader, Bytes, Read, Write};

const REG_NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];

const R_M_NAMES: [&str; 8] = ["bx + si", "bx + di", "bp + si", "bp + di", "si", "di", "bp", "bx"];

const OP_NAMES: [&str; 8] = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];

const JUMP4_NAMES: [&str; 16] = [
    "jo", "jno", "jb", "jnb", "je", "jne", "jbe", "jnbe", "js", "jns", "jp", "jnp", "jl", "jnl", "jle", "jnle",
];

const JUMP2_NAMES: [&str; 4] = ["loopnz", "loopz", "loop", "jcxz"];

fn next_i16(iterator: &mut Bytes<BufReader<File>>, w: bool) -> i16 {
    let byte = iterator.next().unwrap().unwrap();
    if w {
        i16::from_le_bytes([byte, iterator.next().unwrap().unwrap()])
    } else {
        i16::from(i8::from_le_bytes([byte]))
    }
}

const fn operation(byte: u8, mov: bool) -> &'static str {
    if mov {
        "mov"
    } else {
        OP_NAMES[((byte >> 3) & 0b111) as usize]
    }
}

fn disassemble_r_m(iterator: &mut Bytes<BufReader<File>>, w: usize, m0d: u8, r_m: usize) -> String {
    let disp = match m0d {
        // Memory mode. No displacement follows.*
        0b00 => {
            // Direct address. "Except when R/M = 110, then 16-bit displacement follows."
            if r_m == 0b110 {
                return format!("[{}]", next_i16(iterator, true));
            }
            0
        }
        // Memory mode. 8-bit displacement follows.
        0b01 => next_i16(iterator, false),
        // Memory mode. 16-bit displacement follows.
        0b10 => next_i16(iterator, true),
        // Register mode. No displacement follows.
        0b11 => return REG_NAMES[w][r_m].to_string(),
        _ => unreachable!(),
    };

    match disp.cmp(&0) {
        Ordering::Greater => format!("[{} + {}]", R_M_NAMES[r_m], disp),
        Ordering::Less => format!("[{} - {}]", R_M_NAMES[r_m], -disp),
        Ordering::Equal => format!("[{}]", R_M_NAMES[r_m]),
    }
}

fn run<W: Write>(filename: &str, mut stdout: W) {
    let file = BufReader::new(File::open(filename).unwrap());
    let mut iterator = file.bytes();

    writeln!(stdout, "bits 16").unwrap();

    while let Some(Ok(byte1)) = iterator.next() {
        // Register/memory to/from register.
        // 100010 D W MOV
        // 00###0 D W ADD, etc.
        //
        // mov bp, [1024]
        // mov al, [bx + si]
        // mov al, [bx + si + 1024]
        // mov ax, [bx + di - 8]
        // mov dx, [bp]
        // mov si, bx
        if byte1 >> 2 == 0b100010 || (byte1 >> 2) & 0b110001 == 0 {
            let mov = byte1 >> 7 == 1;
            let d = (byte1 >> 1) & 1 == 1;
            let w = (byte1 & 1) as usize;

            // MOD REG R/M
            let byte2 = iterator.next().unwrap().unwrap();
            let m0d = byte2 >> 6; // mod
            let reg = ((byte2 >> 3) & 0b111) as usize;
            let r_m = (byte2 & 0b111) as usize;

            let op_text = operation(byte1, mov);
            let reg_text = REG_NAMES[w][reg];
            let r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);

            // 1 = the REG field identifies the destination operand.
            // 0 = the REG field identifies the source operand.
            if d {
                writeln!(stdout, "{op_text} {reg_text}, {r_m_text}").unwrap();
            } else {
                writeln!(stdout, "{op_text} {r_m_text}, {reg_text}").unwrap();
            }

        // Immediate to register/memory.
        // 110001 1 W MOV
        // 100000 S W ADD, etc.
        // 100000 0 W AND
        // 100000 0 W OR
        //
        // Nothing is 100000 1 X, so it's OK to treat AND and OR as having an S bit.
        //
        // mov [1024], byte 8
        // mov [bx + si], byte 8
        // mov [bx + si + 1024], word 2048
        // mov [bx + di - 8], word 2048
        // mov [bp], byte 8
        // mov bx, word 2048
        } else if byte1 >> 1 == 0b1100011 || byte1 >> 2 == 0b100000 {
            let mov = byte1 >> 6 == 0b11;
            let s = (byte1 >> 1) & 1;
            let w = (byte1 & 1) as usize;

            // MOD ### R/M
            let byte2 = iterator.next().unwrap().unwrap();
            let m0d = byte2 >> 6; // mod
            let r_m = (byte2 & 0b111) as usize;

            let r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);
            let wide = (mov || s == 0) && w == 1;

            // data | data if w = 1 or data | data if sw = 01
            let data = next_i16(&mut iterator, wide);

            let op_text = operation(byte2, mov);
            let unit = if wide { "word" } else { "byte" };

            writeln!(stdout, "{op_text} {r_m_text}, {unit} {data}").unwrap();

        // MOV Immediate to register.
        // 1011 W REG
        //
        // mov cl, 8
        // mov cx, 1024
        } else if byte1 >> 4 == 0b1011 {
            let w = ((byte1 >> 3) & 1) as usize;
            let reg = (byte1 & 0b111) as usize;

            // data | data if w = 1
            let data = next_i16(&mut iterator, w == 1);

            let reg_text = REG_NAMES[w][reg];

            writeln!(stdout, "mov {reg_text}, {data}").unwrap();

        // Memory to accumulator. Accumulator to memory. Immediate to accumulator.
        // 101000 E W MOV
        // 00###1 0 W ADD, etc.
        //
        // mov ax, [8]
        // mov ax, [1024]
        } else if byte1 >> 2 == 0b101000 || (byte1 >> 1) & 0b1100011 == 0b10 {
            let mov = byte1 >> 7 == 1;
            let e = (byte1 >> 1) & 1 == 0; // opposite of d
            let w = byte1 & 1 == 1;

            // addr-lo | addr-hi or data | data if w = 1
            let addr = next_i16(&mut iterator, mov || w);

            let op_text = operation(byte1, mov);
            let reg_text = if w { "ax" } else { "al" };

            if !mov {
                writeln!(stdout, "{op_text} {reg_text}, {addr}").unwrap();
            } else if e {
                writeln!(stdout, "{op_text} {reg_text}, [{addr}]").unwrap();
            } else {
                writeln!(stdout, "{op_text} [{addr}], {reg_text}").unwrap();
            }

        // 0111   JUMP
        // 111000 JUMP
        } else if byte1 >> 4 == 0b111 || byte1 >> 2 == 0b111000 {
            let byte2 = iterator.next().unwrap().unwrap();

            let op_text = if byte1 >> 4 == 0b111 {
                JUMP4_NAMES[(byte1 & 0b1111) as usize]
            } else {
                JUMP2_NAMES[(byte1 & 0b11) as usize]
            };
            let ip_inc8 = i8::from_le_bytes([byte2]);

            writeln!(stdout, "{op_text} {ip_inc8}").unwrap();

        // Debugging.
        } else {
            writeln!(stdout, "{byte1:8b}").unwrap();
        };
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
