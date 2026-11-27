//! The OverflowADV2 script machine, reimplemented from `PMAIL.EXE`
//! (interpreter `0x401f30`, expression evaluator `0x4019a0`).
//!
//! It keeps the original's data model so scripts behave the same:
//!
//! * The **accumulator** and every **stack** entry are raw 32-bit
//!   [`Value`]s: `script_id << 20 | kind << 16 | low16`, where kind 0 is a
//!   number, 1 a reference to a variable and 2 a string in the pool of
//!   script `script_id`. Values read from the stack are copied as they
//!   are.
//! * **Variables** are 65 536 slots; bits 12–15 of the id give the type
//!   (bit, `i8`, `u8`, `i16`, `u16`, engine variable). Engine variables
//!   (`0x5000`–`0x5FFF`) belong to the [`Host`].
//! * `usr` pushes a return frame `(pc | 0x40000, script)`; `ret` pops it.
//! * `sys` hands the engine function number and its arguments (the top
//!   `k` stack entries) to the [`Host`]; the script pops them afterwards.
//! * `text` appends a string to the message log, which the message
//!   function consumes.

use std::collections::HashMap;
use std::rc::Rc;

use overflow_adv2::script::{self, Opcode, Operand, Script};

/// A raw machine value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Value(pub u32);

/// What a [`Value`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Number,
    Variable,
    Text,
    Other(u8),
}

impl Value {
    pub fn new(script: u16, kind: u8, low: u16) -> Self {
        Self(u32::from(script) << 20 | u32::from(kind & 0xF) << 16 | u32::from(low))
    }

    pub fn kind(self) -> Kind {
        match (self.0 >> 16) & 0xF {
            0 => Kind::Number,
            1 => Kind::Variable,
            2 => Kind::Text,
            k => Kind::Other(k as u8),
        }
    }

    pub fn script(self) -> u16 {
        (self.0 >> 20) as u16
    }

    pub fn low(self) -> u16 {
        self.0 as u16
    }
}

/// Type of a variable, from bits 12–15 of its id (`0x4014d0`).
fn store_typed(id: u16, value: i32) -> i32 {
    match id >> 12 {
        0 => value & 1,
        1 => i32::from(value as i8),
        2 => value & 0xFF,
        3 => i32::from(value as i16),
        _ => value & 0xFFFF,
    }
}

/// Ids at or above this are engine variables, owned by the host.
pub const ENGINE_VARIABLES: u16 = 0x5000;
const STACK_LIMIT: usize = 0x10000;
const LOG_SIZE: usize = 128;
const FRAME_TAG: u32 = 0x4_0000;

/// How the host wants the machine to go on after an engine function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Keep running.
    Continue,
    /// Stop for now (the host is waiting for the player or for time);
    /// the next [`Machine::run`] resumes after this instruction.
    Yield,
}

/// The engine side of the machine.
pub trait Host {
    /// Engine function `number` with its arguments, oldest first.
    fn sys(&mut self, vm: &mut Machine, number: u8, args: &[Value]) -> Control;

    /// Reads engine variable `id` (`0x000`–`0xFFF`).
    fn engine_variable(&mut self, id: u16) -> i32 {
        let _ = id;
        0
    }

    /// Writes engine variable `id`.
    fn set_engine_variable(&mut self, id: u16, value: i32) {
        let _ = (id, value);
    }
}

/// Why [`Machine::run`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The step budget ran out; call again.
    Running,
    /// The host asked to wait.
    Yielded,
    /// The script executed `halt` or ran off its end.
    Halted,
    /// Something the original would report as an error.
    Error(String),
}

/// The machine state.
#[derive(Debug, Clone)]
pub struct Machine {
    scripts: HashMap<u16, Rc<Script>>,
    /// Global function number → (script id, entry point).
    functions: HashMap<u8, (u16, u16)>,
    current: u16,
    pc: usize,
    acc: Value,
    stack: Vec<(u32, u32)>,
    vars: Vec<i32>,
    log: Vec<Value>,
    seed: u32,
    halted: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            functions: HashMap::new(),
            current: 0,
            pc: 0,
            acc: Value::default(),
            stack: Vec::new(),
            vars: vec![0; 0x10000],
            log: Vec::new(),
            seed: 1,
            halted: true,
        }
    }

    fn register(&mut self, script: Script) -> u16 {
        let id = script.id;
        for f in &script.functions {
            self.functions.insert(f.index, (id, f.start));
        }
        self.scripts.insert(id, Rc::new(script));
        id
    }

    /// Loads a script for its functions only (`pmail.obj`).
    pub fn load_library(&mut self, script: Script) {
        self.register(script);
    }

    /// Loads a script and continues execution at its first instruction
    /// (boot, and the `goto_script` engine function). The stack is kept,
    /// as in the original.
    pub fn start(&mut self, script: Script) {
        self.current = self.register(script);
        self.pc = 0;
        self.halted = false;
    }

    pub fn current_script(&self) -> Option<&Script> {
        self.scripts.get(&self.current).map(Rc::as_ref)
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Reads a variable (engine variables go to the host).
    pub fn variable(&self, host: &mut dyn Host, id: u16) -> i32 {
        if id >> 12 == ENGINE_VARIABLES >> 12 {
            host.engine_variable(id & 0xFFF)
        } else {
            self.vars[usize::from(id)]
        }
    }

    /// Writes a variable with the conversion its type implies.
    pub fn set_variable(&mut self, host: &mut dyn Host, id: u16, value: i32) {
        let value = store_typed(id, value);
        if id >> 12 == ENGINE_VARIABLES >> 12 {
            host.set_engine_variable(id & 0xFFF, value);
        }
        self.vars[usize::from(id)] = value;
    }

    /// The string a [`Kind::Text`] value points at.
    pub fn text(&self, value: Value) -> Option<String> {
        if value.kind() != Kind::Text {
            return None;
        }
        self.scripts.get(&value.script())?.text(value.low())
    }

    /// The number a value stands for: numbers are sign-extended 16-bit,
    /// variable references are read.
    pub fn number(&self, host: &mut dyn Host, value: Value) -> i32 {
        match value.kind() {
            Kind::Number => i32::from(value.low() as i16),
            Kind::Variable => self.variable(host, value.low()),
            _ => i32::from(value.low() as i16),
        }
    }

    /// The variable a [`Kind::Variable`] value refers to.
    pub fn variable_ref(&self, value: Value) -> Option<u16> {
        (value.kind() == Kind::Variable).then_some(value.low())
    }

    /// Takes the strings logged with `text` since the last call.
    pub fn take_log(&mut self) -> Vec<String> {
        self.take_log_values()
            .into_iter()
            .filter_map(|v| self.text(v))
            .collect()
    }

    /// Takes the logged values themselves (pool references), for engine
    /// functions that also read the pool entries next to them (the voice
    /// file of a message follows its text).
    pub fn take_log_values(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.log)
    }

    /// Pool entry `index` of script `script`.
    pub fn pool_text(&self, script: u16, index: u16) -> Option<String> {
        self.scripts.get(&script)?.text(index)
    }

    /// The `@` operand of expressions (`0x402740`): MSVC-style `rand()`.
    fn random(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
        ((self.seed >> 16) & 0x7FFF) as i32
    }

    /// Stack entry `n` from the top (1 = top), as `0x4013e0` reads it.
    fn stack_from_top(&self, n: usize) -> Option<(u32, u32)> {
        let len = self.stack.len();
        (n >= 1 && n <= len).then(|| self.stack[len - n])
    }

    fn push(&mut self, entry: (u32, u32)) -> Result<(), String> {
        if self.stack.len() >= STACK_LIMIT {
            return Err("stack overflow".into());
        }
        self.stack.push(entry);
        Ok(())
    }

    /// Value of one expression token (`0x401590`).
    fn token(&mut self, host: &mut dyn Host, token: &[u8]) -> Result<i32, String> {
        match token.first() {
            Some(b'$') => {
                let id = atoi(&token[1..]) as u16;
                Ok(self.variable(host, id))
            }
            Some(0xFF) => {
                let kind = token.get(1).copied().unwrap_or(0);
                let n = atoi(token.get(2..).unwrap_or(&[])).max(0) as usize;
                let (value, _) = self.stack_from_top(n).ok_or_else(|| {
                    format!("expression reads stack slot {n} of {}", self.stack.len())
                })?;
                let low = (value & 0xFFFF) as u16;
                Ok(match kind {
                    b'F' => self.variable(host, low),
                    b'S' => return Err("string operand in an expression".into()),
                    _ => i32::from(low),
                })
            }
            Some(b'@') => Ok(self.random()),
            _ => Ok(atoi(token)),
        }
    }

    /// Evaluates a postfix expression (`0x4019a0`).
    pub fn evaluate(&mut self, host: &mut dyn Host, expr: &[u8]) -> Result<i32, String> {
        let mut values: Vec<i32> = Vec::new();
        for token in expr.split(|&b| b == b' ').filter(|t| !t.is_empty()) {
            let Some(op) = operator(token) else {
                values.push(self.token(host, token)?);
                continue;
            };
            let (Some(b), Some(a)) = (values.pop(), values.pop()) else {
                return Err(format!(
                    "operator {} without two operands",
                    String::from_utf8_lossy(token)
                ));
            };
            values.push(apply(op, a, b)?);
        }
        Ok(values.pop().unwrap_or(0))
    }

    /// The value an operand loads into the accumulator.
    fn load(&mut self, host: &mut dyn Host, operand: Operand) -> Result<Value, String> {
        let id = self.current;
        Ok(match operand {
            Operand::Zero => Value::new(id, 0, 0),
            Operand::One => Value::new(id, 0, 1),
            Operand::High(n) => Value::new(id, 0, u16::from(n)),
            Operand::Number(n) => Value::new(id, 0, n),
            Operand::Pair(high, _) => Value::new(id, 0, u16::from(high)),
            Operand::Expression(n) => {
                let script = self.scripts.get(&id).ok_or("no script")?.clone();
                let expr = script
                    .pool
                    .get(usize::from(n))
                    .ok_or_else(|| format!("expression {n} not in the pool"))?;
                let value = self.evaluate(host, expr)?;
                Value::new(id, 0, value as u16)
            }
            Operand::Variable(n) => Value::new(id, 1, n),
            Operand::Pool(n) => Value::new(id, 2, n),
            Operand::Stack { slot, mode } => {
                let entry = if mode % 2 == 0 {
                    self.stack_from_top(usize::from(slot))
                } else {
                    self.stack.get(usize::from(slot)).copied()
                };
                Value(
                    entry
                        .ok_or_else(|| format!("stack slot {slot} does not exist"))?
                        .0,
                )
            }
            Operand::Forward(n) | Operand::Backward(n) => Value::new(id, 0, n),
            Operand::Unknown { mode, .. } => return Err(format!("operand mode {mode:#x}")),
        })
    }

    /// Runs up to `budget` instructions.
    pub fn run(&mut self, host: &mut dyn Host, budget: usize) -> Status {
        for _ in 0..budget {
            if self.halted {
                return Status::Halted;
            }
            match self.step(host) {
                Ok(Control::Continue) => {}
                Ok(Control::Yield) => return Status::Yielded,
                Err(e) => {
                    self.halted = true;
                    let name = self.current;
                    return Status::Error(format!(
                        "script {name} at {}: {e}",
                        self.pc.saturating_sub(1)
                    ));
                }
            }
        }
        if self.halted {
            Status::Halted
        } else {
            Status::Running
        }
    }

    fn step(&mut self, host: &mut dyn Host) -> Result<Control, String> {
        let script = self
            .scripts
            .get(&self.current)
            .ok_or("no script loaded")?
            .clone();
        let Some(&ins) = script.code.get(self.pc) else {
            self.halted = true;
            return Ok(Control::Continue);
        };
        let pc = self.pc;
        self.pc += 1;
        let op = ins
            .op()
            .ok_or_else(|| format!("unknown opcode {:#04x}", ins.opcode))?;
        let operand = ins.operand();
        match op {
            Opcode::Halt => {
                self.halted = true;
                self.acc = Value::default();
            }
            Opcode::Jump | Opcode::JumpIfTrue | Opcode::JumpIfFalse => {
                let truth = self.acc.low() != 0;
                let take = match op {
                    Opcode::Jump => true,
                    Opcode::JumpIfTrue => truth,
                    _ => !truth,
                };
                if take && let Some(target) = ins.jump_target(pc) {
                    self.pc = target;
                }
            }
            Opcode::Return => {
                let (a, b) = self.stack.pop().ok_or("ret with an empty stack")?;
                if a & 0xF_0000 != FRAME_TAG {
                    return Err("ret without a call frame".into());
                }
                self.current = b as u16;
                self.pc = (a & 0xFFFF) as usize;
            }
            Opcode::Sys => {
                let Operand::Pair(number, argc) = operand else {
                    return Err(format!("sys with operand {operand:?}"));
                };
                let argc = usize::from(argc);
                if argc > self.stack.len() {
                    return Err(format!("sys {number:#04x} wants {argc} arguments"));
                }
                let args: Vec<Value> = self.stack[self.stack.len() - argc..]
                    .iter()
                    .map(|&(a, _)| Value(a))
                    .collect();
                return Ok(host.sys(self, number, &args));
            }
            Opcode::Usr => {
                let Operand::High(n) = operand else {
                    return Err(format!("usr with operand {operand:?}"));
                };
                let (script_id, start) = *self
                    .functions
                    .get(&n)
                    .ok_or_else(|| format!("usr {n} is not defined"))?;
                self.push((
                    FRAME_TAG | (self.pc as u32 & 0xFFFF),
                    u32::from(self.current),
                ))?;
                self.current = script_id;
                self.pc = usize::from(start);
            }
            Opcode::Begin | Opcode::End => {}
            Opcode::Text => {
                if self.acc.kind() == Kind::Text {
                    if self.log.len() == LOG_SIZE {
                        self.log.clear();
                    }
                    self.log.push(self.acc);
                }
            }
            Opcode::ClearText => self.log.clear(),
            Opcode::Store => {
                let Operand::Variable(id) = operand else {
                    return Err(format!("store with operand {operand:?}"));
                };
                let value = i32::from(self.acc.low());
                self.set_variable(host, id, value);
            }
            Opcode::Load => self.acc = self.load(host, operand)?,
            Opcode::Push => self.push((self.acc.0, 0))?,
            Opcode::Pop => {
                let n = usize::from(ins.value);
                if n > self.stack.len() {
                    return Err(format!("pop {n} from a stack of {}", self.stack.len()));
                }
                let keep = self.stack.len() - n;
                if let Some(&(a, _)) = self.stack[keep..].first() {
                    self.acc = Value(a);
                }
                self.stack.truncate(keep);
            }
        }
        Ok(Control::Continue)
    }
}

/// C `atoi`: optional sign and leading digits; anything else stops it.
fn atoi(bytes: &[u8]) -> i32 {
    let (negative, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    let value = digits
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0i32, |acc, &b| {
            acc.wrapping_mul(10).wrapping_add(i32::from(b - b'0'))
        });
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Shr,
    Shl,
    RotR,
    RotL,
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
    And,
    Xor,
    Or,
    LogAnd,
    LogOr,
}

fn operator(token: &[u8]) -> Option<Op> {
    Some(match token {
        b"*" => Op::Mul,
        b"/" => Op::Div,
        b"%" => Op::Rem,
        b"+" => Op::Add,
        b"-" => Op::Sub,
        b">>" => Op::Shr,
        b"<<" => Op::Shl,
        b">>>" => Op::RotR,
        b"<<<" => Op::RotL,
        b">" => Op::Gt,
        b"<" => Op::Lt,
        b">=" => Op::Ge,
        b"<=" => Op::Le,
        b"==" => Op::Eq,
        b"!=" => Op::Ne,
        b"&" => Op::And,
        b"^" => Op::Xor,
        b"|" => Op::Or,
        b"&&" => Op::LogAnd,
        b"||" => Op::LogOr,
        _ => return None,
    })
}

/// One operator, as in the switch at `0x401c52`.
fn apply(op: Op, a: i32, b: i32) -> Result<i32, String> {
    Ok(match op {
        Op::Mul => a.wrapping_mul(b),
        // Division by zero gives 0 (the engine also logs an error).
        Op::Div => {
            if b == 0 {
                0
            } else {
                a.wrapping_div(b)
            }
        }
        Op::Rem => {
            if b == 0 {
                0
            } else {
                a.wrapping_rem(b)
            }
        }
        Op::Add => a.wrapping_add(b),
        Op::Sub => a.wrapping_sub(b),
        Op::Shr => a.wrapping_shr(b as u32),
        Op::Shl => a.wrapping_shl(b as u32),
        Op::RotR => (a as u32).rotate_right(b as u32 & 31) as i32,
        Op::RotL => (a as u32).rotate_left(b as u32 & 31) as i32,
        Op::Gt => i32::from(a > b),
        Op::Lt => i32::from(a < b),
        Op::Ge => i32::from(a >= b),
        Op::Le => i32::from(a <= b),
        Op::Eq => i32::from(a == b),
        Op::Ne => i32::from(a != b),
        Op::And => a & b,
        Op::Xor => a ^ b,
        Op::Or => a | b,
        Op::LogAnd => i32::from(a != 0 && b != 0),
        Op::LogOr => i32::from(a != 0 || b != 0),
    })
}

/// Name of an engine function, for logs.
pub fn sys_label(number: u8) -> String {
    match script::sys_name(number) {
        Some(name) => format!("sys {number:#04x} {name}"),
        None => format!("sys {number:#04x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records engine calls; answers nothing.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<(u8, Vec<Value>)>,
        engine: HashMap<u16, i32>,
        yield_on: Option<u8>,
    }

    impl Host for Recorder {
        fn sys(&mut self, _vm: &mut Machine, number: u8, args: &[Value]) -> Control {
            self.calls.push((number, args.to_vec()));
            if self.yield_on == Some(number) {
                Control::Yield
            } else {
                Control::Continue
            }
        }

        fn engine_variable(&mut self, id: u16) -> i32 {
            self.engine.get(&id).copied().unwrap_or(0)
        }

        fn set_engine_variable(&mut self, id: u16, value: i32) {
            self.engine.insert(id, value);
        }
    }

    /// A script with the given pool, functions and code (see
    /// `overflow_adv2::script` for the layout).
    fn script(id: u16, pool: &[&[u8]], functions: &[[u8; 8]], code: &[u32]) -> Script {
        let header = 4 + pool.len() * 4;
        let mut data = vec![0u8; header];
        let mut offset = header;
        let mut strings = Vec::new();
        for (i, s) in pool.iter().enumerate() {
            let len = s.len() + 1;
            data[4 + i * 4..6 + i * 4].copy_from_slice(&(offset as u16).to_le_bytes());
            data[6 + i * 4..8 + i * 4].copy_from_slice(&(len as u16).to_le_bytes());
            strings.extend_from_slice(s);
            strings.push(0);
            offset += len;
        }
        data.extend_from_slice(&strings);
        let pool_end = data.len() as u16;
        data[0..2].copy_from_slice(&pool_end.to_le_bytes());
        data[2..4].copy_from_slice(&(pool.len() as u16).to_le_bytes());
        data.extend_from_slice(&(4 + 8 * functions.len() as u32).to_le_bytes());
        for f in functions {
            data.extend_from_slice(f);
        }
        for w in code {
            data.extend_from_slice(&w.to_le_bytes());
        }
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        Script::parse(&data).unwrap()
    }

    #[test]
    fn evaluates_postfix_expressions() {
        let mut vm = Machine::new();
        let mut host = Recorder::default();
        let eval = |vm: &mut Machine, host: &mut Recorder, e: &str| {
            vm.evaluate(host, e.as_bytes()).unwrap()
        };
        assert_eq!(eval(&mut vm, &mut host, "16 48 3 * +"), 160);
        assert_eq!(eval(&mut vm, &mut host, "7 2 - 3 %"), 2);
        assert_eq!(eval(&mut vm, &mut host, "5 0 /"), 0);
        assert_eq!(eval(&mut vm, &mut host, "-8 1 >>"), -4);
        assert_eq!(eval(&mut vm, &mut host, "1 1 >>>"), i32::MIN);
        assert_eq!(eval(&mut vm, &mut host, "1 2 < 3 3 == &&"), 1);
        assert_eq!(eval(&mut vm, &mut host, "0 0 ||"), 0);
        vm.set_variable(&mut host, 0x3064, 7);
        assert_eq!(eval(&mut vm, &mut host, "$12388 7 == "), 1);
        vm.set_variable(&mut host, 0x5001, 9);
        assert_eq!(host.engine[&1], 9);
        assert_eq!(eval(&mut vm, &mut host, "$20481"), 9);
        // Stack operands: I = value, F = variable named by the value.
        vm.stack = vec![(0x3064, 0), (Value::new(1, 0, 42).0, 0)];
        assert_eq!(eval(&mut vm, &mut host, "\u{0}"), 0);
        assert_eq!(vm.evaluate(&mut host, b"\xFFI1").unwrap(), 42);
        assert_eq!(vm.evaluate(&mut host, b"\xFFF2").unwrap(), 7);
        assert!(vm.evaluate(&mut host, b"\xFFI3").is_err());
        assert!(vm.evaluate(&mut host, b"1 +").is_err());
    }

    #[test]
    fn variables_are_typed() {
        let mut vm = Machine::new();
        let mut host = Recorder::default();
        for (id, input, stored) in [
            (0x0001u16, 3, 1),
            (0x1001, 200, -56),
            (0x2001, 300, 44),
            (0x3001, 40000, -25536),
            (0x4001, -1, 65535),
        ] {
            vm.set_variable(&mut host, id, input);
            assert_eq!(vm.variable(&mut host, id), stored, "{id:#x}");
        }
    }

    #[test]
    fn random_matches_the_engine() {
        let mut vm = Machine::new();
        // seed = 1 * 0x41C64E6D + 12345 = 0x41C67EA6 -> 0x41C6 & 0x7FFF.
        assert_eq!(vm.random(), 0x41C6);
    }

    #[test]
    fn runs_calls_jumps_and_engine_functions() {
        // Library: usr 7 stores 5 + its argument into $12388 and returns.
        let lib = script(
            1,
            &[b"\xFFI2 5 +"],
            &[[7, 0, 1, 0, 2, 0, 6, 0]],
            &[
                0x0110_0004, // jmp over the function
                0x0702_0700, // begin 7
                0x1105_0000, // load expr
                0x1020_3064, // store $12388
                0x0400_0000, // ret
                0x0800_0000, // end
            ],
        );
        let main = script(
            2,
            &[b"Hello", b"$12388 8 =="],
            &[],
            &[
                0x1103_0003, // load #3
                0x2001_0000, // push
                0x0602_0700, // usr 7
                0x2103_0001, // pop 1
                0x1105_0001, // load expr ($12388 == 8)
                0x0310_0003, // jf +3 (to the halt)
                0x1180_0000, // load "Hello"
                0x0900_0000, // text
                0x0504_0100, // sys 0x01, 0
                0x0000_0000, // halt
            ],
        );
        let mut vm = Machine::new();
        let mut host = Recorder {
            yield_on: Some(0x01),
            ..Default::default()
        };
        vm.load_library(lib);
        vm.start(main);
        assert_eq!(vm.run(&mut host, 1000), Status::Yielded);
        assert_eq!(vm.variable(&mut host, 0x3064), 8);
        assert_eq!(host.calls.len(), 1);
        assert_eq!(vm.take_log(), ["Hello"]);
        assert!(vm.take_log().is_empty());
        assert_eq!(vm.run(&mut host, 1000), Status::Halted);
        assert!(vm.stack.is_empty());
    }

    #[test]
    fn engine_function_arguments_are_raw_values() {
        let main = script(
            3,
            &[b"PRO1.OBJ"],
            &[],
            &[
                0x1180_0000, // load "PRO1.OBJ"
                0x2001_0000, // push
                0x1120_3064, // load $12388 (a reference)
                0x2001_0000, // push
                0x0504_1002, // sys 0x10, 2
                0x2103_0002, // pop 2
            ],
        );
        let mut vm = Machine::new();
        let mut host = Recorder::default();
        vm.start(main);
        vm.set_variable(&mut host, 0x3064, -3);
        assert_eq!(vm.run(&mut host, 100), Status::Halted);
        let (number, args) = host.calls[0].clone();
        assert_eq!(number, 0x10);
        assert_eq!(vm.text(args[0]).as_deref(), Some("PRO1.OBJ"));
        assert_eq!(vm.variable_ref(args[1]), Some(0x3064));
        assert_eq!(vm.number(&mut host, args[1]), -3);
    }

    #[test]
    fn reports_errors() {
        let main = script(4, &[], &[], &[0x0602_0900]); // usr 9: undefined
        let mut vm = Machine::new();
        vm.start(main);
        let status = vm.run(&mut Recorder::default(), 10);
        assert!(
            matches!(status, Status::Error(ref e) if e.contains("usr 9")),
            "{status:?}"
        );
        assert!(vm.is_halted());
    }
}
