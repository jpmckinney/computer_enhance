use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::File;
use std::io::{self, BufReader, Bytes, Read, Write};
use std::iter::Enumerate;

const REG_NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];
const R_M_NAMES: [&str; 8] = ["bx + si", "bx + di", "bp + si", "bp + di", "si", "di", "bp", "bx"];
const SEGMENT_NAMES: [&str; 4] = ["es", "cs", "ss", "ds"];
const UNARY2_NAMES: [&str; 4] = ["inc", "dec", "push", "pop"];
const UNARY3_NAMES: [&str; 8] = ["test", "XXX", "not", "neg", "mul", "imul", "div", "idiv"];
const BINARY_NAMES: [&str; 8] = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];
const LOGIC_NAMES: [&str; 8] = ["rol", "ror", "rcl", "rcr", "shl", "shr", "XXX", "sar"];
const JUMP4_NAMES: [&str; 16] = [
    "jo", "jno", "jb", "jnb", "je", "jne", "jbe", "jnbe", "js", "jns", "jp", "jnp", "jl", "jnl", "jle", "jnle",
];
const JUMP2_NAMES: [&str; 4] = ["loopnz", "loopz", "loop", "jcxz"];

fn next_u8(iterator: &mut Enumerate<Bytes<BufReader<File>>>) -> u8 {
    let byte = iterator.next().unwrap().1.unwrap();
    u8::from_le_bytes([byte])
}

fn next_i16(iterator: &mut Enumerate<Bytes<BufReader<File>>>, w: bool) -> i16 {
    let byte = iterator.next().unwrap().1.unwrap();
    if w {
        i16::from_le_bytes([byte, iterator.next().unwrap().1.unwrap()])
    } else {
        i16::from(i8::from_le_bytes([byte]))
    }
}

fn disassemble_r_m(iterator: &mut Enumerate<Bytes<BufReader<File>>>, w: usize, m0d: u8, r_m: usize) -> String {
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

fn run(filename: &str) -> Vec<String> {
    let file = BufReader::new(File::open(filename).unwrap());
    let mut iterator = file.bytes().enumerate();
    // Insert assembly instructions at byte indices.
    let mut instructions = BTreeMap::new();
    // Track the byte index of each label.
    let mut labels = HashMap::new();

    while let Some((position, Ok(byte1))) = iterator.next() {
        // Uncomment to just print bytes.
        // println!("{byte1:8b}");
        // continue;

        // "Register/memory to/from register." "Reg/memory with register to either."
        // "Register/memory with register." "Register/memory and register."
        // "Load EA to register." "Load pointer to DS/ES."
        //
        // 100010 D W MOV
        if byte1 >> 2 == 0b100010
            // 00...0 D W ADD, etc.
            || (byte1 >> 2) & 0b110001 == 0
            // 100001 0 W TEST
            || byte1 >> 1 == 0b1000010
            // 100001 1 W XCHG
            || byte1 >> 1 == 0b1000011
            // LEA
            || byte1 == 0b10001101
            // LDS
            || byte1 == 0b11000101
            // LES
            || byte1 == 0b11000100
        {
            // LEA, LDS and LES use REG for the destination and are wide.
            let (d, w) = if byte1 == 0b10001101 || byte1 == 0b11000101 || byte1 == 0b11000100 {
                (1, 1)
            } else {
                ((byte1 >> 1) & 1, (byte1 & 1) as usize)
            };

            // MOD REG R/M
            let byte2 = iterator.next().unwrap().1.unwrap();
            let m0d = byte2 >> 6; // mod
            let reg = ((byte2 >> 3) & 0b111) as usize;
            let r_m = (byte2 & 0b111) as usize;

            let op_text = match byte1 {
                0b10001101 => "lea",
                0b11000101 => "lds",
                0b11000100 => "les",
                _ => match byte1 >> 1 {
                    0b1000010 => "test",
                    0b1000011 => "xchg",
                    0b1000100 | 0b1000101 => "mov",
                    _ => BINARY_NAMES[((byte1 >> 3) & 0b111) as usize],
                },
            };

            let reg_text = REG_NAMES[w][reg];
            let r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);

            // 1 = the REG field identifies the destination operand.
            // 0 = the REG field identifies the source operand.
            if d == 1 {
                instructions.insert(position, format!("{op_text} {reg_text}, {r_m_text}\n"));
            } else {
                instructions.insert(position, format!("{op_text} {r_m_text}, {reg_text}\n"));
            }

        // "Immediate to register/memory." "Register/memory." "Immediate data and register/memory."
        //
        // TEST "Immediate data and register/memory" has the same first byte as NEG, etc.
        // so we need to put them all in this branch - but the latter do not have DATA bytes.
        //
        // 110001 1 W MOV
        } else if byte1 >> 1 == 0b1100011
            // 100000 S W ADD, etc.
            // 100000 0 W AND
            // 100000 0 W OR
            // It's OK to treat AND and OR as having an S bit.
            || byte1 >> 2 == 0b100000
            // The following do not have DATA bytes.
            // 100011 1 1 POP
            || byte1 == 0b10001111
            // 111111 1 1 PUSH
            || byte1 == 0b11111111
            // 111111 1 W INC, DEC
            || byte1 >> 1 == 0b1111111
            // 111101 1 W NEG, MUL, IMUL, DIV, IDIV, NOT, TEST
            || byte1 >> 1 == 0b1111011
            // 110100 V W SHL SHR SAR ROL ROR RCL RCR
            || byte1 >> 2 == 0b110100
        {
            let op = byte1 >> 2;
            let mov = op == 0b110001;
            let s_v = (byte1 >> 1) & 1;
            let w = (byte1 & 1) as usize;

            // MOD ... R/M
            let byte2 = iterator.next().unwrap().1.unwrap();
            let m0d = byte2 >> 6; // mod
            let r_m = (byte2 & 0b111) as usize;

            let r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);

            let op_text = match op {
                0b110001 => "mov",
                0b100011 => "pop",
                0b100000 => BINARY_NAMES[((byte2 >> 3) & 0b111) as usize],
                0b111111 => UNARY2_NAMES[((byte2 >> 3) & 0b11) as usize],
                0b111101 => UNARY3_NAMES[((byte2 >> 3) & 0b111) as usize],
                0b110100 => LOGIC_NAMES[((byte2 >> 3) & 0b111) as usize],
                _ => unreachable!(),
            };
            let unit = if w == 1 { "word" } else { "byte" };

            // Binary operators (MOV, TEST, ADD, etc.) have DATA bytes.
            if op == 0b110001 || op == 0b100000 || op == 0b111101 && (byte2 >> 3).trailing_zeros() >= 3 {
                // data | data if w = 1 or data | data if sw = 01
                let data = next_i16(&mut iterator, (mov || s_v == 0) && w == 1);
                instructions.insert(position, format!("{op_text} {r_m_text}, {unit} {data}\n"));
            } else if op == 0b110100 {
                // 0 = Shift/rotate count is one. 1 = Shift/rotate count is specified in CL register.
                let count = if s_v == 0 { "1" } else { "cl" };
                instructions.insert(position, format!("{op_text} {unit} {r_m_text}, {count}\n"));
            } else {
                instructions.insert(position, format!("{op_text} {unit} {r_m_text}\n"));
            }

        // MOV Immediate to register.
        //
        // 1011 W REG
        } else if byte1 >> 4 == 0b1011 {
            let w = ((byte1 >> 3) & 1) as usize;
            let reg = (byte1 & 0b111) as usize;

            // data | data if w = 1
            let data = next_i16(&mut iterator, w == 1);

            let reg_text = REG_NAMES[w][reg];

            instructions.insert(position, format!("mov {reg_text}, {data}\n"));

        // Accumulator.
        //
        // 101000 E W MOV
        } else if byte1 >> 2 == 0b101000
            // 00...1 0 W ADD, etc.
            || (byte1 >> 1) & 0b1100011 == 0b10
            // 101010 0 W TEST
            || (byte1 >> 1) == 0b1010100
            // 111001 E W IN, OUT
            || (byte1 >> 2) == 0b111001
        {
            let mov = byte1 >> 2 == 0b101000;
            let port = (byte1 >> 2) == 0b111001;
            let e = (byte1 >> 1) & 1 == 0; // opposite of d
            let w = byte1 & 1 == 1;

            // addr-lo | addr-hi or data | data if w = 1 or data-8
            let addr = if port {
                i16::from(next_u8(&mut iterator))
            } else {
                next_i16(&mut iterator, mov || w)
            };

            let op_text = match byte1 >> 1 {
                0b1010000 | 0b1010001 => "mov",
                0b1010100 => "test",
                0b1110010 => "in",
                0b1110011 => "out",
                _ => BINARY_NAMES[((byte1 >> 3) & 0b111) as usize],
            };
            let acc_text = if w { "ax" } else { "al" };
            let addr_text = if mov { format!("[{addr}]") } else { addr.to_string() };

            if e {
                instructions.insert(position, format!("{op_text} {acc_text}, {addr_text}\n"));
            } else {
                instructions.insert(position, format!("{op_text} {addr_text}, {acc_text}\n"));
            }

        // PUSH POP INC DEC Register. One byte: 010 .. REG
        } else if (byte1 >> 5) == 0b010 {
            let reg = (byte1 & 0b111) as usize;

            let reg_text = REG_NAMES[1][reg];
            let op_text = UNARY2_NAMES[((byte1 >> 3) & 0b11) as usize];

            instructions.insert(position, format!("{op_text} {reg_text}\n"));

        // PUSH POP Segment register. One byte: 000 SG 11 .
        } else if (byte1 & 0b11100111) == 0b110 || (byte1 & 0b11100111) == 0b111 {
            let sg = ((byte1 >> 3) & 0b11) as usize;

            let sg_text = SEGMENT_NAMES[sg];
            let op_text = if (byte1 & 1) == 0 { "push" } else { "pop" };

            instructions.insert(position, format!("{op_text} {sg_text}\n"));

        // XCHG Accumulator. One byte: 10010 REG
        } else if (byte1 >> 3) == 0b10010 {
            let reg = (byte1 & 0b111) as usize;

            let reg_text = REG_NAMES[1][reg];

            instructions.insert(position, format!("xchg ax, {reg_text}\n"));

        // IN OUT Accumulator. One byte: 111011 . W
        } else if (byte1 >> 2) == 0b111011 {
            let out = (byte1 >> 1) & 1 == 1;
            let w = byte1 & 1 == 1;

            let acc_text = if w { "ax" } else { "al" };

            if out {
                instructions.insert(position, format!("out dx, {acc_text}\n"));
            } else {
                instructions.insert(position, format!("in {acc_text}, dx\n"));
            }

        // RET. Fixed byte plus i16 data.
        } else if byte1 == 0b11000010 || byte1 == 0b11001010 {
            let data = next_i16(&mut iterator, true);

            instructions.insert(position, format!("ret {data}\n"));

        // INT. Fixed byte plus u8 data.
        } else if byte1 == 0b11001101 {
            let data = next_u8(&mut iterator);

            instructions.insert(position, format!("int {data}\n"));

        // REP. Fixed byte plus lookup table.
        } else if byte1 == 0b11110011 {
            let byte2 = iterator.next().unwrap().1.unwrap();
            let w = byte2 & 1 == 1;

            let op_text = match byte2 >> 1 {
                0b1010010 => "movs",
                0b1010011 => "cmps",
                0b1010111 => "scas",
                0b1010110 => "lods",
                0b1010101 => "stos",
                _ => unreachable!(),
            };
            let unit = if w { "w" } else { "b" };

            instructions.insert(position, format!("rep {op_text}{unit}\n"));

        // 0111   JUMP
        // 111000 JUMP
        } else if byte1 >> 4 == 0b111 || byte1 >> 2 == 0b111000 {
            let byte2 = iterator.next().unwrap().1.unwrap();

            let op_text = if byte1 >> 4 == 0b111 {
                JUMP4_NAMES[(byte1 & 0b1111) as usize]
            } else {
                JUMP2_NAMES[(byte1 & 0b11) as usize]
            };
            let ip_inc8 = i8::from_le_bytes([byte2]);

            // This instruction is two bytes.
            let target = position.checked_add_signed(2 + ip_inc8 as isize).unwrap();
            let length = labels.len();
            let label = labels.entry(target).or_insert_with(|| format!("label{length}"));

            instructions.insert(position, format!("{op_text} {label} ; {ip_inc8}\n"));

        // Two fixed bytes.
        } else if byte1 == 0b11010100 || byte1 == 0b11010101 {
            let byte2 = iterator.next().unwrap().1.unwrap();

            let op_text = if byte1 & 1 == 0 { "aam" } else { "aad" };

            if byte2 == 0b00001010 {
                instructions.insert(position, format!("{op_text}\n"));
            } else {
                unreachable!();
            }

        // One fixed byte.
        } else {
            let op_text = match byte1 {
                0b11010111 => "xlat\n",
                0b10011111 => "lahf\n",
                0b10011110 => "sahf\n",
                0b10011100 => "pushf\n",
                0b10011101 => "popf\n",
                0b00110111 => "aaa\n",
                0b00100111 => "daa\n",
                0b00111111 => "aas\n",
                0b00101111 => "das\n",
                0b10011000 => "cbw\n",
                0b10011001 => "cwd\n",
                0b11000011 | 0b11001011 => "ret\n",
                0b11001100 => "int\n",
                0b11001110 => "into\n",
                0b11001111 => "iret\n",
                0b11111000 => "clc\n",
                0b11110101 => "cmc\n",
                0b11111001 => "stc\n",
                0b11111100 => "cld\n",
                0b11111101 => "std\n",
                0b11111010 => "cli\n",
                0b11111011 => "sti\n",
                0b11110100 => "hlt\n",
                0b10011011 => "wait\n",
                0b11110000 => "lock ",
                _ => "",
            };
            if op_text.is_empty() {
                // Debugging.
                instructions.insert(position, format!("{byte1:8b}\n"));
            } else {
                instructions.insert(position, op_text.to_string());
            }
        };
    }

    for (position, label) in labels {
        let prefix = format!("{label}:\n");
        instructions.get_mut(&position).unwrap().insert_str(0, &prefix);
    }
    if !instructions.is_empty() {
        instructions.get_mut(&0).unwrap().insert_str(0, "bits 16\n");
    }

    instructions.into_values().collect()
}

fn main() {
    let filename = env::args().nth(1).unwrap();
    let mut stdout = io::stdout().lock();
    for line in run(&filename) {
        write!(stdout, "{line}").unwrap();
    }
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

        for line in run(test_path) {
            write!(assembly_file, "{line}").unwrap();
        }

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
