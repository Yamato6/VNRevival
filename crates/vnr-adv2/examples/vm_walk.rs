//! Runs the real boot sequence with a host that only follows script
//! changes and answers the title menu, to exercise the machine.

use std::collections::BTreeMap;

use overflow_adv2::Pack;
use overflow_adv2::script::Script;
use vnr_adv2::vm::{Control, Host, Machine, Status, Value};

struct Walker {
    pack: Pack<std::io::BufReader<std::fs::File>>,
    calls: BTreeMap<u8, usize>,
    examples: BTreeMap<u8, Vec<String>>,
    scripts: Vec<String>,
}

impl Walker {
    fn script(&mut self, name: &str) -> Script {
        let file = if name.to_ascii_lowercase().ends_with(".obj") {
            name.to_string()
        } else {
            format!("{name}.obj")
        };
        let entry = self
            .pack
            .find(&file)
            .unwrap_or_else(|| panic!("{file}"))
            .clone();
        Script::parse(&self.pack.read(&entry).unwrap()).unwrap()
    }
}

impl Host for Walker {
    fn sys(&mut self, vm: &mut Machine, number: u8, args: &[Value]) -> Control {
        *self.calls.entry(number).or_default() += 1;
        let shown: Vec<String> = args
            .iter()
            .map(|&v| match (vm.text(v), vm.variable_ref(v)) {
                (Some(t), _) => format!("{t:?}"),
                (_, Some(id)) => format!("${id}={}", vm.variable(self, id)),
                _ => format!("{}", vm.number(self, v)),
            })
            .collect();
        let ex = self.examples.entry(number).or_default();
        let line = shown.join(", ");
        if ex.len() < 5 && !ex.contains(&line) {
            ex.push(line);
        }
        match number {
            0xd5 => {
                if let Some(var) = args.get(1).and_then(|v| vm.variable_ref(*v)) {
                    vm.set_variable(self, var, 1);
                }
            }
            0x10 => {
                let name = vm.text(args[0]).unwrap();
                self.scripts.push(name.clone());
                let s = self.script(&name);
                vm.start(s);
            }
            _ => {}
        }
        Control::Continue
    }
}

fn main() {
    let dir = std::env::args().nth(1).expect("game dir");
    let steps: usize = std::env::args()
        .nth(2)
        .map_or(5_000_000, |s| s.parse().unwrap());
    let pack = Pack::open(format!("{dir}/PACK/SCRIPT.DAT")).unwrap();
    let mut host = Walker {
        pack,
        calls: BTreeMap::new(),
        examples: BTreeMap::new(),
        scripts: vec![],
    };
    let mut vm = Machine::new();
    let lib = host.script("pmail.obj");
    vm.load_library(lib);
    let start = host.script("start.obj");
    vm.start(start);
    let status = vm.run(&mut host, steps);
    println!("status: {status:?}");
    println!(
        "pc {} in script {:?}",
        vm.pc(),
        vm.current_script().map(|s| s.id)
    );
    println!(
        "scripts visited ({}): {:?}",
        host.scripts.len(),
        &host.scripts[..host.scripts.len().min(30)]
    );
    for (n, c) in &host.calls {
        let ex: Vec<String> = host.examples[n]
            .iter()
            .map(|e| e.chars().take(70).collect())
            .collect();
        println!("{n:02x} x{c}: {}", ex.join(" | "));
    }
    let _ = Status::Running;
}
