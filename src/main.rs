use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::File;
use std::io::{self, BufReader, Bytes, Read, Write};
use std::iter::Enumerate;
use std::time::Instant;

const REG_NAMES: [[&str; 8]; 2] = [
    ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"],
    ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
];
const R_M_NAMES: [&str; 8] = ["bx + si", "bx + di", "bp + si", "bp + di", "si", "di", "bp", "bx"];
const SEGMENT_NAMES: [&str; 4] = ["es", "cs", "ss", "ds"];

// "N/A" indices are "(not used)" according to the manual.
const ASCII_ADJUST_NAMES: [&str; 2] = ["aam", "aad"];
const BINARY_NAMES: [&str; 8] = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];
const CALL_NAMES: [&str; 2] = ["call", "jmp"];
const LOGIC_NAMES: [&str; 8] = ["rol", "ror", "rcl", "rcr", "shl", "shr", "N/A", "sar"];
const STACK_NAMES: [&str; 2] = ["push", "pop"];
const UNARY_NAMES: [&str; 4] = ["inc", "dec", "push", "pop"];
// I don't know what unifies these two groups of instructions, other than their first byte.
const NAMES_1111011W: [&str; 8] = ["test", "N/A", "not", "neg", "mul", "imul", "div", "idiv"];
const NAMES_11111111: [&str; 8] = ["inc", "dec", "call", "call", "jmp", "jmp", "push", "N/A"];
const JUMP2_NAMES: [&str; 4] = ["loopnz", "loopz", "loop", "jcxz"];
const JUMP4_NAMES: [&str; 16] = [
    "jo", "jno", "jb", "jnb", "je", "jne", "jbe", "jnbe", "js", "jns", "jp", "jnp", "jl", "jnl", "jle", "jnle",
];

fn next_u8(iterator: &mut Enumerate<Bytes<BufReader<File>>>) -> u8 {
    let byte = iterator.next().unwrap().1.unwrap();
    u8::from_le_bytes([byte])
}

fn next_i8(iterator: &mut Enumerate<Bytes<BufReader<File>>>) -> i8 {
    let byte = iterator.next().unwrap().1.unwrap();
    i8::from_le_bytes([byte])
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

fn run<W: Write>(filename: &str, mut stdout: W) {
    let file = BufReader::new(File::open(filename).unwrap());
    let mut iterator = file.bytes().enumerate();
    // Insert assembly instructions at byte indices.
    let mut instructions = BTreeMap::new();
    // Track the byte index of each label.
    let mut labels = HashMap::new();
    // Segment override.
    let mut segment = "";
    // Override the order of operands to avoid "instruction is not lockable".
    let mut locked = false;
    let mut release_lock = false;

    while let Some((position, Ok(byte1))) = iterator.next() {
        // Uncomment to just print bytes.
        // println!("{byte1:8b}");
        // continue;

        if locked {
            release_lock = true;
        }

        // Next bytes are: MOD REG R/M | (DISP-LO) | (DISP-HI)
        match byte1 {
              0b00_000_0_00..=0b00_000_0_11 // 00 ADD 0 D W
            | 0b00_001_0_00..=0b00_001_0_11 // 00 OR  0 D W
            | 0b00_010_0_00..=0b00_010_0_11 // 00 ADC 0 D W
            | 0b00_011_0_00..=0b00_011_0_11 // 00 SBB 0 D W
            | 0b00_100_0_00..=0b00_100_0_11 // 00 AND 0 D W
            | 0b00_101_0_00..=0b00_101_0_11 // 00 SUB 0 D W
            | 0b00_110_0_00..=0b00_110_0_11 // 00 XOR 0 D W
            | 0b00_111_0_00..=0b00_111_0_11 // 00 CMP 0 D W
            | 0b1000010_0..=0b1000010_1     // 10 000 1 0 W TEST
            | 0b1000011_0..=0b1000011_1     // 10 000 1 1 W XCHG
            | 0b100010_00..=0b100010_11     // 10 001 0 D W MOV
            | 0b10001101                    // LEA
            | 0b11000100                    // LES
            | 0b11000101                    // LDS
            => {
                // LEA, LES and LDS use REG for the destination and are wide.
                let (d, w) = match byte1 {
                    0b10001101 | 0b11000100 | 0b11000101 => (1, 1),
                    _ => ((byte1 >> 1) & 1, (byte1 & 1) as usize),
                };

                // MOD REG R/M
                let byte2 = iterator.next().unwrap().1.unwrap();
                let m0d = byte2 >> 6; // mod
                let reg = ((byte2 >> 3) & 0b111) as usize;
                let r_m = (byte2 & 0b111) as usize;

                let op_text = match byte1 {
                    0b10001101 => "lea",
                    0b11000100 => "les",
                    0b11000101 => "lds",
                    0b1000010_0..=0b1000010_1 => "test",
                    0b1000011_0..=0b1000011_1 => "xchg",
                    0b100010_00..=0b100010_11 => "mov",
                    _ => BINARY_NAMES[((byte1 >> 3) & 0b111) as usize],
                };
                let reg_text = REG_NAMES[w][reg];
                let mut r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);
                if !segment.is_empty() {
                    r_m_text = format!("{segment}:{r_m_text}");
                    segment = "";
                }

                // 1 = the REG field identifies the destination operand.
                // 0 = the REG field identifies the source operand.
                if d == 1 && !locked {
                    instructions.insert(position, format!("{op_text} {reg_text}, {r_m_text}\n"));
                } else {
                    instructions.insert(position, format!("{op_text} {r_m_text}, {reg_text}\n"));
                }
            },

            // Next bytes are: MOD OP R/M | (DISP-LO) | (DISP-HI) | (DATA) | (DATA if cond = 1)
            //
            // TEST "Immediate data and register/memory" has the same first byte as NEG, etc.
            // so we need to put them all in this branch - but the latter do not have DATA bytes.
            //
            // It's OK to treat AND, OR, XOR as having an S bit (of 0). 1 is "(not used)" according to the manual.
              0b100000_00..=0b100000_11 // 100000 S W ADC, ADD, AND, CMP, OR, SBB, SUB, XOR
            | 0b1100011_0..=0b1100011_1 // 110001 1 W MOV
            // The following do not have DATA bytes, except TEST.
            | 0b10001111                // 100011 1 1 POP
            | 0b110100_00..=0b110100_11 // 110100 V W SHL SHR SAR ROL ROR RCL RCR
            | 0b1111011_0..=0b1111011_1 // 111101 1 W NEG, MUL, IMUL, DIV, IDIV, NOT, TEST
            | 0b1111111_0..=0b1111111_1 // 111111 1 W INC, DEC
                                        // 111111 1 1 PUSH, CALL, JMP
            => {
                let group = byte1 >> 2;
                let s_v = (byte1 >> 1) & 1; // s or v
                let w = (byte1 & 1) as usize;

                // MOD OP R/M
                let byte2 = iterator.next().unwrap().1.unwrap();
                let m0d = byte2 >> 6; // mod
                let op = ((byte2 >> 3) & 0b111) as usize;
                let r_m = (byte2 & 0b111) as usize;

                let mov_test = group == 0b110001 || group == 0b111101 && (byte2 >> 3).trailing_zeros() >= 3;

                let op_text = match group {
                    0b100011 => "pop",
                    0b110001 => "mov",
                    0b100000 => BINARY_NAMES[op],
                    0b110100 => LOGIC_NAMES[op],
                    0b111101 => NAMES_1111011W[op],
                    0b111111 => NAMES_11111111[op],
                    _ => unreachable!(),
                };
                let unit_text = if w == 1 { "word" } else { "byte" };
                let mut r_m_text = disassemble_r_m(&mut iterator, w, m0d, r_m);
                if !segment.is_empty() {
                    r_m_text = format!("{segment}:{r_m_text}");
                    segment = "";
                }

                // Binary instructions (MOV, TEST, ADD, etc.) have DATA bytes.
                if mov_test || group == 0b100000 {
                    // data | data if w = 1 for MOV and TEST. data | data if sw = 01 for ADD, etc.
                    let data = next_i16(&mut iterator, (mov_test || s_v == 0) && w == 1);
                    instructions.insert(position, format!("{op_text} {r_m_text}, {unit_text} {data}\n"));
                // Logic instructions.
                } else if group == 0b110100 {
                    // 0 = Shift/rotate count is one. 1 = Shift/rotate count is specified in CL register.
                    let count = if s_v == 0 { "1" } else { "cl" };
                    instructions.insert(position, format!("{op_text} {unit_text} {r_m_text}, {count}\n"));
                // Jump instructions. "Indirect intersegment."
                } else if byte1 == 0b11111111 && (op == 0b011 || op == 0b101) {
                    instructions.insert(position, format!("{op_text} {unit_text} far {r_m_text}\n"));
                } else {
                    instructions.insert(position, format!("{op_text} {unit_text} {r_m_text}\n"));
                }
            },

            // MOV Immediate to register. First byte: 1011 W REG
            0b1011_0_000..=0b1011_1_111 => {
                let w = ((byte1 >> 3) & 1) as usize;
                let reg = (byte1 & 0b111) as usize;

                // data | data if w = 1
                let data = next_i16(&mut iterator, w == 1);

                let reg_text = REG_NAMES[w][reg];

                instructions.insert(position, format!("mov {reg_text}, {data}\n"));
            },

            // Accumulator. Next bytes are either: DATA | DATA if W = 1, ADDR-LO | ADDR-HI, DATA-8.
              0b00_000_10_0..=0b00_000_10_1 // 00 ADD 1 0 W
            | 0b00_001_10_0..=0b00_001_10_1 // 00 OR  1 0 W
            | 0b00_010_10_0..=0b00_010_10_1 // 00 ADC 1 0 W
            | 0b00_011_10_0..=0b00_011_10_1 // 00 SBB 1 0 W
            | 0b00_100_10_0..=0b00_100_10_1 // 00 AND 1 0 W
            | 0b00_101_10_0..=0b00_101_10_1 // 00 SUB 1 0 W
            | 0b00_110_10_0..=0b00_110_10_1 // 00 XOR 1 0 W
            | 0b00_111_10_0..=0b00_111_10_1 // 00 CMP 1 0 W
            | 0b101000_00..=0b101000_11     // 101000 E W MOV
            | 0b1010100_0..=0b1010100_1     // 101010 0 W TEST
            | 0b111001_00..=0b111001_11     // 111001 E W IN, OUT
            => {
                let mov = byte1 >> 2 == 0b101000;
                let in_out = byte1 >> 2 == 0b111001;
                let e = (byte1 >> 1) & 1 == 0; // opposite of d
                let w = byte1 & 1 == 1;

                let data = if in_out {
                    // data-8
                    i16::from(next_u8(&mut iterator))
                } else {
                    // addr-lo | addr-hi or data | data if w = 1
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
                // MOV does "memory to accumulator", others do "immediate to accumulator".
                let data_text = if mov { format!("[{data}]") } else { data.to_string() };

                if e {
                    instructions.insert(position, format!("{op_text} {acc_text}, {data_text}\n"));
                } else {
                    instructions.insert(position, format!("{op_text} {data_text}, {acc_text}\n"));
                }
            },

            // PUSH POP INC DEC Register. One byte: 010 OP REG
            0b010_00_000..=0b010_11_111 => {
                let op = ((byte1 >> 3) & 0b11) as usize;
                let reg = (byte1 & 0b111) as usize;

                let op_text = UNARY_NAMES[op];
                let reg_text = REG_NAMES[1][reg];

                instructions.insert(position, format!("{op_text} {reg_text}\n"));
            },

            // PUSH POP Segment register. One byte.
              0b000_00_11_0..=0b000_00_11_1 // 000 ES 11 OP
            | 0b000_01_11_0..=0b000_01_11_1 // 000 CS 11 OP
            | 0b000_10_11_0..=0b000_10_11_1 // 000 SS 11 OP
            | 0b000_11_11_0..=0b000_11_11_1 // 000 DS 11 OP
            => {
                let sg = ((byte1 >> 3) & 0b11) as usize;
                let op = (byte1 & 1) as usize;

                let sg_text = SEGMENT_NAMES[sg];
                let op_text = STACK_NAMES[op];

                instructions.insert(position, format!("{op_text} {sg_text}\n"));
            },

            // SEGMENT. One byte.
              0b001_00_110 // 001 ES 110
            | 0b001_01_110 // 001 CS 110
            | 0b001_10_110 // 001 SS 110
            | 0b001_11_110 // 001 DS 110
            => {
                let sg = ((byte1 >> 3) & 0b11) as usize;

                segment = SEGMENT_NAMES[sg];
            },

            // XCHG Accumulator. One byte: 10010 REG
            0b10010_000..=0b10010_111 => {
                let reg = (byte1 & 0b111) as usize;

                let reg_text = REG_NAMES[1][reg];

                instructions.insert(position, format!("xchg ax, {reg_text}\n"));
            },

            // IN OUT Accumulator. One byte: 111011 OUT W
            0b111011_00..=0b111011_11 => {
                let out = (byte1 >> 1) & 1 == 1;
                let w = byte1 & 1 == 1;

                let acc_text = if w { "ax" } else { "al" };

                if out {
                    instructions.insert(position, format!("out dx, {acc_text}\n"));
                } else {
                    instructions.insert(position, format!("in {acc_text}, dx\n"));
                }
            },

            // RET RETF. Fixed byte plus i16 data.
            0b11000010 | 0b11001010 => {
                let retf = (byte1 >> 3) & 1 == 1;
                let data = next_i16(&mut iterator, true);

                let op_text = if retf { "retf" } else { "ret" };

                instructions.insert(position, format!("{op_text} {data}\n"));
            },

            // INT. Fixed byte plus u8 data.
            0b11001101 => {
                let data = next_u8(&mut iterator);

                instructions.insert(position, format!("int {data}\n"));
            },

            // REP. Fixed byte plus lookup table.
            0b11110011 => {
                // 1010 OP W
                let byte2 = iterator.next().unwrap().1.unwrap();
                let op = (byte2 >> 1) & 0b111;
                let w = byte2 & 1 == 1;

                let op_text = match op {
                    0b010 => "movs",
                    0b011 => "cmps",
                    0b101 => "stos",
                    0b110 => "lods",
                    0b111 => "scas",
                    _ => unreachable!(),
                };
                let unit_text = if w { "w" } else { "b" };

                instructions.insert(position, format!("rep {op_text}{unit_text}\n"));
            },

              0b11101011                // JMP Direct within segment-short
            | 0b111000_00..=0b111000_11 // 111000 OP JUMP
            | 0b0111_0000..=0b0111_1111 // 0111   OP JUMP
            => {
                let group = byte1 >> 2;

                let ip_inc8 = next_i8(&mut iterator);

                let op_text = match group {
                    0b111010 => "jmp",
                    0b111000 => JUMP2_NAMES[(byte1 & 0b11) as usize],
                    _ => JUMP4_NAMES[(byte1 & 0b1111) as usize],
                };

                // This instruction is 2 bytes.
                let target = position.checked_add_signed(2 + ip_inc8 as isize).unwrap();
                let length = labels.len();
                let label = labels.entry(target).or_insert_with(|| format!("label{length}"));

                instructions.insert(position, format!("{op_text} {label} ; {ip_inc8} short\n"));
            },

            // CALL JMP Direct within segment. 1110100 OP
            0b1110100_0 | 0b1110100_1 => {
                let op = (byte1 & 1) as usize;

                let ip_inc = next_i16(&mut iterator, true);

                let op_text = CALL_NAMES[op];

                // This instruction is 3 bytes.
                let target = position.checked_add_signed(3 + ip_inc as isize).unwrap();
                let length = labels.len();
                let label = labels.entry(target).or_insert_with(|| format!("label{length}"));

                instructions.insert(position, format!("{op_text} {label} ; {ip_inc}\n"));
            },

            // CALL JMP Direct intersegment.
            0b1_001_1010 | 0b1_110_1010 => {
                // LSB bit 5 also works to map 0 to CALL and 1 to JMP.
                let op = ((byte1 >> 6) & 1) as usize;

                let ip = next_i16(&mut iterator, true);
                let cs = next_i16(&mut iterator, true);

                let op_text = CALL_NAMES[op];

                instructions.insert(position, format!("{op_text} {cs}:{ip}\n"));
            },

            // Two fixed bytes.
            0b1101010_0 | 0b1101010_1 => {
                let op = (byte1 & 1) as usize;

                let byte2 = iterator.next().unwrap().1.unwrap();

                let op_text = ASCII_ADJUST_NAMES[op];

                if byte2 == 0b00001010 {
                    instructions.insert(position, format!("{op_text}\n"));
                } else {
                    unreachable!();
                }
            },

            // One fixed byte.
            _ => {
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
                    0b11000011 => "ret\n",
                    0b11001011 => "retf\n",
                    0b11001100 => "int3\n",
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
                    0b11110000 => {
                        locked = true;
                        "lock "
                    }
                    _ => "",
                };
                if op_text.is_empty() {
                    instructions.insert(position, format!("; {byte1:8b}\n")); // debugging
                } else {
                    instructions.insert(position, op_text.to_string());
                }
            }
        };

        if release_lock {
            locked = false;
            release_lock = false;
        }
    }

    let mut unlabeled = HashMap::new();
    for (target, label) in &labels {
        if !instructions.contains_key(target) {
            unlabeled.insert(format!("{label} "), target);
        }
    }

    writeln!(stdout, "bits 16").unwrap();
    for (position, string) in &mut instructions {
        if let Some(label) = labels.get(position) {
            writeln!(stdout, "{label}:").unwrap();
        }
        for (label, target) in &unlabeled {
            if string.contains(label) {
                *string = string.replacen(label, &format!("{target}"), 1);
            }
        }
        write!(stdout, "{string}").unwrap();
    }
}

fn main() {
    let now = Instant::now();
    let filename = env::args().nth(1).unwrap();
    run(&filename, &mut io::stdout().lock());
    eprintln!("{}ms", now.elapsed().as_micros());
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
