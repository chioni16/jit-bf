mod crl;

use std::io::Read;

const MEM_SIZE: usize = 30000;
const PAGE_SIZE: usize = 4096;

fn main() {
    let input = "++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.>++.".to_string();

    // let input = "
    //     +++++++++++++++++++++++++++++++++			c1v33 : ASCII code of !
    //     >++++++++++++++++++++++++++++++
    //     +++++++++++++++++++++++++++++++				c2v61 : ASCII code of =
    //     >++++++++++						c3v10 : ASCII code of EOL
    //     >+++++++						c4v7  : quantity of numbers to be calculated
    //     >							c5v0  : current number (one digit)
    //     >+							c6v1  : current value of factorial (up to three digits)
    //     <<							c4    : loop counter
    //     [							block : loop to print one line and calculate next
    //     >++++++++++++++++++++++++++++++++++++++++++++++++.	c5    : print current number
    //     ------------------------------------------------	c5    : back from ASCII to number
    //     <<<<.-.>.<.+						c1    : print !_=_

    //     >>>>>							block : print c6 (preserve it)
    //     >							c7v0  : service zero
    //     >++++++++++						c8v10 : divizor
    //     <<							c6    : back to dividend
    //     [->+>-[>+>>]>[+[-<+>]>+>>]<<<<<<]			c6v0  : divmod algo borrowed from esolangs; results in 0 n d_n%d n%d n/d
    //     >[<+>-]							c6    : move dividend back to c6 and clear c7
    //     >[-]							c8v0  : clear c8

    //     >>							block : c10 can have two digits; divide it by ten again
    //     >++++++++++						c11v10: divizor
    //     <							c10   : back to dividend
    //     [->-[>+>>]>[+[-<+>]>+>>]<<<<<]				c10v0 : another divmod algo borrowed from esolangs; results in 0 d_n%d n%d n/d
    //     >[-]							c11v0 : clear c11
    //     >>[++++++++++++++++++++++++++++++++++++++++++++++++.[-]]c13v0 : print nonzero n/d (first digit) and clear c13
    //     <[++++++++++++++++++++++++++++++++++++++++++++++++.[-]] c12v0 : print nonzero n%d (second digit) and clear c12
    //     <<<++++++++++++++++++++++++++++++++++++++++++++++++.[-]	c9v0  : print any n%d (last digit) and clear c9

    //     <<<<<<.							c3    : EOL
    //     >>+							c5    : increment current number
    //     							block : multiply c6 by c5 (don't preserve c6)
    //     >[>>+<<-]						c6v0  : move c6 to c8
    //     >>							c8v0  : repeat c8 times
    //     [
    //     <<<[>+>+<<-]						c5v0  : move c5 to c6 and c7
    //     >>[<<+>>-]						c7v0  : move c7 back to c5
    //     >-
    //     ]
    //     <<<<-							c4    : decrement loop counter
    //     ]
    // ".to_string();

    let input = get_op_string(input);
    let mut input = collapse_multiple(input);
    set_jump_targets(&mut input);

    // interpret(&input);

    let data = vec![0u8; MEM_SIZE];
    let a = emit_x86(&input, data.as_ptr() as u64);
    let a = crl::emit_clif(&input);

    unsafe {
        let code = libc::mmap(
            std::ptr::null_mut(),
            PAGE_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if code == usize::MAX as *mut libc::c_void {
            panic!("mmap failed");
        }

        let code_slice = std::slice::from_raw_parts_mut(code as *mut u8, a.len());
        code_slice.copy_from_slice(&a);

        if libc::mprotect(code, PAGE_SIZE, libc::PROT_READ | libc::PROT_EXEC) == -1 {
            panic!("mprotect failed");
        }

        // std::fs::write("./output", code_slice).unwrap();
        println!("hello before function: {}", code as usize);
        // let f: fn() = std::mem::transmute(code);
        // f();
        let f: fn(u64) = std::mem::transmute(code);
        f(data.as_ptr() as u64);
    }


}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
enum Op {
    // >	Increment the data pointer by one (to point to the next cell to the right).
    IncrPointer(usize),
    // <	Decrement the data pointer by one (to point to the next cell to the left).
    DecrPointer(usize),
    // +	Increment the byte at the data pointer by one.
    IncrData(usize),
    // -	Decrement the byte at the data pointer by one.
    DecrData(usize),
    // .	Output the byte at the data pointer.
    Output,
    // ,	Accept one byte of input, storing its value in the byte at the data pointer.
    Input,
    // [	If the byte at the data pointer is zero, then instead of moving the instruction pointer forward to the next command, jump it forward to the command after the matching ] command.
    LoopStart(usize),
    // ]    If the byte at the data pointer is nonzero, then instead of moving the instruction pointer forward to the next command, jump it back to the command after the matching [ command.[a]
    LoopEnd(usize),
}

fn get_op_string(input: String) -> Vec<Op> {
    input
        .chars()
        .filter_map(|c| {
            let c = match c {
                '>' => Op::IncrPointer(1),
                '<' => Op::DecrPointer(1),
                '+' => Op::IncrData(1),
                '-' => Op::DecrData(1),
                '.' => Op::Output,
                ',' => Op::Input,
                '[' => Op::LoopStart(0),
                ']' => Op::LoopEnd(0),
                _ => return None,
            };
            Some(c)
        })
        .collect::<Vec<_>>()
}

fn collapse_multiple(input: Vec<Op>) -> Vec<Op> {
    let mut new_input = Vec::new();

    let mut input = input.into_iter().peekable();
    while let Some(e) = input.next() {
        let ne = match e {
            Op::IncrPointer(_) => {
                let count = get_count(&mut input, Op::IncrPointer(0));
                Op::IncrPointer(count)
            }
            Op::DecrPointer(_) => {
                let count = get_count(&mut input, Op::DecrPointer(0));
                Op::DecrPointer(count)
            }
            Op::IncrData(_) => {
                let count = get_count(&mut input, Op::IncrData(0));
                Op::IncrData(count)
            }
            Op::DecrData(_) => {
                let count = get_count(&mut input, Op::DecrData(0));
                Op::DecrData(count)
            }
            other => other,
        };
        new_input.push(ne);
    }

    new_input
}

fn get_count<I: Iterator<Item = Op>>(iter: &mut std::iter::Peekable<I>, op: Op) -> usize {
    let mut count = 1;
    while let Some(_) = iter.next_if(|e| std::mem::discriminant(e) == std::mem::discriminant(&op)) {
        count += 1;
    }
    count
}

fn set_jump_targets(input: &mut Vec<Op>) {
    let mut loops = vec![];
    let mut i = 0;
    while i < input.len() {
        let e = &mut input[i];
        match e {
            Op::LoopStart(_) => {
                loops.push(i);
            }
            Op::LoopEnd(_) => {
                if let Some(start) = loops.pop() {
                    *e = Op::LoopEnd(start);
                    input[start] = Op::LoopStart(i);
                } else {
                    panic!("found unpaired brackets");
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn interpret(input: &Vec<Op>) {
    let mut data = vec![0u8; MEM_SIZE];
    let mut pc = 0;
    let mut dp = 0;

    while let Some(instr) = input.get(pc) {
        match instr {
            Op::IncrPointer(i) => {
                dp += i;
            }
            Op::DecrPointer(i) => {
                dp -= i;
            }
            Op::IncrData(i) => {
                data[dp] += *i as u8;
            }
            Op::DecrData(i) => {
                data[dp] -= *i as u8;
            }
            Op::Output => {
                print!("{}", data[dp] as char)
            }
            Op::Input => {
                let input = std::io::stdin()
                    .bytes()
                    .next()
                    .and_then(|result| result.ok())
                    .unwrap();
                data[dp] = input;
            }
            Op::LoopStart(end) => {
                if data[dp] == 0 {
                    pc = *end;
                }
            }
            Op::LoopEnd(start) => {
                if data[dp] != 0 {
                    pc = *start;
                }
            }
        }
        pc += 1;
    }
}

fn emit_x86(input: &Vec<Op>, addr: u64) -> Vec<u8> {
    let mut temp: Vec<Vec<u8>> = vec![vec![]; input.len()];
    for (instr, op) in input.iter().enumerate() {
        match op {
            Op::IncrPointer(i) => {
                temp[instr].extend([0x49, 0x81, 0xC5]);
                let i = *i as u32;
                temp[instr].extend(i.to_le_bytes());
            }
            Op::DecrPointer(i) => {
                temp[instr].extend([0x49, 0x81, 0xED]);
                let i = *i as u32;
                temp[instr].extend(i.to_le_bytes());
            }
            Op::IncrData(i) => {
                temp[instr].extend([0x41, 0x80, 0x45, 0x00]);
                temp[instr].push(*i as u8);
            }
            Op::DecrData(i) => {
                temp[instr].extend([0x41, 0x80, 0x6D, 0x00]);
                temp[instr].push(*i as u8);
            }
            Op::Output => {
                temp[instr].extend([0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00]); // mov rax, 1
                temp[instr].extend([0x48, 0xC7, 0xC7, 0x01, 0x00, 0x00, 0x00]); // mov rdi, 1
                temp[instr].extend([0x4C, 0x89, 0xEE]); // mov rsi, r13
                temp[instr].extend([0x48, 0xC7, 0xC2, 0x01, 0x00, 0x00, 0x00]); // mov rdx, 1
                temp[instr].extend([0x0F, 0x05]); // syscall
            }
            Op::Input => {
                temp[instr].extend([0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00]); // mov rax, 0
                temp[instr].extend([0x48, 0xC7, 0xC7, 0x00, 0x00, 0x00, 0x00]); // mov rdi, 0
                temp[instr].extend([0x4C, 0x89, 0xEE]); // mov rsi, r13
                temp[instr].extend([0x48, 0xC7, 0xC2, 0x01, 0x00, 0x00, 0x00]); // mov rdx, 1
                temp[instr].extend([0x0F, 0x05]); // syscall
            }
            Op::LoopStart(_) => {
                temp[instr].extend([0x41, 0x80, 0x7d, 0x00, 0x00]); // cmpb [r13], 0
                temp[instr].extend([0x0F, 0x84]); // jz, rel_addr
                temp[instr].extend([0x00, 0x00, 0x00, 0x00]); // temp 32 bit relative addr
            }
            Op::LoopEnd(_) => {
                temp[instr].extend([0x41, 0x80, 0x7d, 0x00, 0x00]); // cmpb [r13], 0
                temp[instr].extend([0x0F, 0x85]); // jz, rel_addr
                temp[instr].extend([0x00, 0x00, 0x00, 0x00]); // temp 32 bit relative addr
            }
        }
    }

    let lengths: Vec<_> = temp
        .iter()
        .map(|v| v.len())
        .scan(0, |acc, x| {
            *acc = *acc + x;
            Some(*acc)
        })
        .collect();

    for (i, op) in input.iter().enumerate() {
        let jump_rel_dist = match op {
            Op::LoopStart(end) => lengths[*end] as i32 - lengths[i] as i32,
            Op::LoopEnd(start) => lengths[*start] as i32 - lengths[i] as i32,
            _ => continue,
        };
        let jump_rel_dist = jump_rel_dist.to_le_bytes();
        temp[i] = temp[i][..temp[i].len() - 4].to_vec();
        temp[i].extend(jump_rel_dist);
    }

    temp.insert(0, vec![0x49, 0xBD]);
    temp[0].extend(addr.to_le_bytes());
    temp.push(vec![0xc3]);
    temp.into_iter().flatten().collect()
}
