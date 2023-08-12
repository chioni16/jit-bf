use std::io::{Write, Read};

use cranelift::codegen::{
    ir::{AbiParam, Function, Signature, UserFuncName},
    isa::{self, CallConv},
    settings, Context, verify_function,
};
use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift::prelude::{InstBuilder, MemFlags, EntityRef, Variable, types::I8};
use target_lexicon::Triple;

use crate::Op;


// Similar to interpreter in that you need figure out what target code to generate for a given source instruction
// you need to find the target code for all source instructions
// unlike interpreter, you don't follow the program counter, instead you travserse the list of instructions 'once', translating each of them
// You use static environment (variable names and things that represent values) instead of dynamic environment (actual values generated)

pub(crate) fn emit_clif(input: &Vec<Op>) -> Vec<u8> {
    // target-independent compilation flags 
    let settings_builder = settings::builder();
    let flags = settings::Flags::new(settings_builder);
    
    // target ISA
    let isa_builder = isa::lookup(Triple::host()).unwrap();
    let isa = isa_builder.finish(flags).unwrap();

    ////////////////////////////////////////////////////////////////////////////////
    // declare a new function in IR
    ////////////////////////////////////////////////////////////////////////////////

    // function signature
    let pointer_type = isa.pointer_type();
    let mut sig = Signature::new(CallConv::SystemV);
    sig.params.push(AbiParam::new(pointer_type));
    sig.returns.push(AbiParam::new(pointer_type));

    // function builder
    let mut func = Function::with_name_signature(UserFuncName::default(), sig);   
    let mut func_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut func, &mut func_ctx);

    // entry block
    let block = builder.create_block();
    builder.seal_block(block);
    builder.append_block_params_for_function_params(block);
    builder.switch_to_block(block);

    // write block
    let (write_sig, write_addr) = {
        let mut write_sig = Signature::new(CallConv::SystemV);
        write_sig.params.push(AbiParam::new(I8));
        let write_sig = builder.import_signature(write_sig);
    
        let write_addr= builder.ins().iconst(pointer_type, write as *const fn(u8) as i64);
        (write_sig, write_addr)
    };

    // read block
    let (read_sig, read_addr) = {
        let mut read_sig = Signature::new(CallConv::SystemV);
        read_sig.params.push(AbiParam::new(pointer_type));
        let read_sig = builder.import_signature(read_sig);
    
        let read_addr= builder.ins().iconst(pointer_type, read as *const fn(*mut u8) as i64);
        (read_sig, read_addr)
    };

    ////////////////////////////////////////////////////////////////////////////////
    // build IR
    ////////////////////////////////////////////////////////////////////////////////

    // passed argument 
    let start_addr = builder.block_params(block)[0];

    // local variable initialisation
    let dp = Variable::new(0);
    builder.declare_var(dp, pointer_type);
    let zero = builder.ins().iconst(pointer_type, 0);
    builder.def_var(dp, zero);

    // stack to hold jump target locations
    let mut stack = Vec::new();

    // meat of IRgen
    for instr in input.iter() {
        match instr {
            Op::IncrPointer(i) => {
                let dp_val = builder.use_var(dp);
                let updated_dp_val= builder.ins().iadd_imm(dp_val, *i as i64);
                builder.def_var(dp, updated_dp_val);
            }
            Op::DecrPointer(i) => {
                let dp_val = builder.use_var(dp);
                let updated_dp_val= builder.ins().iadd_imm(dp_val, -(*i as i64));
                builder.def_var(dp, updated_dp_val);
            }
            Op::IncrData(i) => {
                // read cell value
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);
                let cell_val = builder.ins().load(I8, MemFlags::new(), cell_addr, 0);

                // update cell value
                let updated_cell_val = builder.ins().iadd_imm(cell_val, *i as i64);

                // write back updated cell value
                builder.ins().store(MemFlags::new(), updated_cell_val, cell_addr, 0);
            }
            Op::DecrData(i) => {
                // read cell value
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);
                let cell_val = builder.ins().load(I8, MemFlags::new(), cell_addr, 0);

                // update cell value
                let updated_cell_val = builder.ins().iadd_imm(cell_val, -(*i as i64));

                // write back updated cell value
                builder.ins().store(MemFlags::new(), updated_cell_val, cell_addr, 0);
            }
            Op::Output => {
                // read cell value
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);
                let cell_val = builder.ins().load(I8, MemFlags::new(), cell_addr, 0);

                builder.ins().call_indirect(write_sig, write_addr, &[cell_val]);
            }
            Op::Input => {
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);

                let inst = builder.ins().call_indirect(read_sig, read_addr, &[cell_addr]);
            }
            Op::LoopStart(_) => {
                // create new blocks
                let inner_block = builder.create_block();
                let after_block = builder.create_block();
                stack.push((inner_block, after_block));

                // read cell value
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);
                let cell_val = builder.ins().load(I8, MemFlags::new(), cell_addr, 0);

                // branching
                builder.ins().brif(cell_val, inner_block, &[], after_block, &[]);

                // switch to the inner block
                builder.switch_to_block(inner_block);
            }
            Op::LoopEnd(_) => {
                // get the blocks created when the inner scope was entered
                let (inner_block, after_block) = stack.pop().unwrap();

                // read cell value
                let dp_val = builder.use_var(dp);
                let cell_addr = builder.ins().iadd(start_addr, dp_val);
                let cell_val = builder.ins().load(I8, MemFlags::new(), cell_addr, 0);

                // branching
                builder.ins().brif(cell_val, inner_block, &[], after_block, &[]);

                // seal blocks as all jumps to blocks have been determined
                builder.seal_block(inner_block);
                builder.seal_block(after_block);

                // switch to the after block
                builder.switch_to_block(after_block);
            }
        }
    }

    // code check
    if !stack.is_empty() {
        panic!("Unbalanced brackets");
    }

    // return value
    builder.ins().return_(&[zero]);

    // done building the function IR
    builder.finalize();
    println!("{}", func.display());

    ////////////////////////////////////////////////////////////////////////////////
    // IR verification
    ////////////////////////////////////////////////////////////////////////////////
    
    verify_function(&func, &*isa).unwrap();

    ////////////////////////////////////////////////////////////////////////////////
    // IR -> assembly conversion
    ////////////////////////////////////////////////////////////////////////////////

    // compile function
    let mut ctx = Context::for_function(func);
    let code = ctx.compile(&*isa, &mut Default::default()).unwrap();

    // dump compiled binary
    // std::fs::write("dump.bin", code.code_buffer()).unwrap();

    code.code_buffer().to_vec()

}

extern fn write(val: u8) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&[val]).unwrap();
    stdout.flush().unwrap();
}

extern fn read(addr: *mut u8) {
    let mut stdin = std::io::stdin().lock();
    let mut value = 0u8;
    stdin.read_exact(std::slice::from_mut(&mut value)).unwrap();
    unsafe {
        *addr = value;
    }
}