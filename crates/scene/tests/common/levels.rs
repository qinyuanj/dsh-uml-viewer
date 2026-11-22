const LEVELS: [usize; 8] = [21, 52, 129, 12, 84, 231, 20, 20];
const SPANS: [usize; 7] = [144, 155, 88, 112, 141, 46, 25];
const FOREIGN: [&str; 9] = [
    "std", "core", "alloc", "libc", "egui", "eframe", "image", "winit", "log",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn weighted(&mut self, weights: &[usize]) -> usize {
        let mut at = self.below(weights.iter().sum());
        for (i, w) in weights.iter().enumerate() {
            if at < *w {
                return i;
            }
            at -= w;
        }
        unreachable!()
    }

    fn word(&mut self) -> String {
        let syllables = ["ka", "ro", "mi", "te", "su", "lan", "vor", "ex", "di", "po"];
        (0..2 + self.below(4))
            .map(|_| syllables[self.below(syllables.len())])
            .collect()
    }
}

pub fn scene(classes: usize) -> String {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ classes as u64);
    let total: usize = LEVELS.iter().sum();
    let mut sizes: Vec<usize> = LEVELS
        .iter()
        .map(|w| (w * classes / total).max(1))
        .collect();
    let largest = (0..sizes.len()).max_by_key(|&i| sizes[i]).unwrap();
    sizes[largest] = classes + sizes[largest] - sizes.iter().sum::<usize>();
    let mut by_level: Vec<Vec<String>> = Vec::new();
    let mut text = format!(
        "{{:title \"Levels {classes}\" :direction :tb :packages [{{:id :p :label \"bench.levels\" :classes [\n"
    );
    for (level, &n) in sizes.iter().enumerate() {
        let mut ids = Vec::new();
        for _ in 0..n {
            let id = format!(
                "c{:05}",
                ids.len() + by_level.iter().map(Vec::len).sum::<usize>()
            );
            let ops = match rng.below(10) {
                0 => 8 + rng.below(12),
                k => k / 3 + rng.below(3),
            };
            let ops: String = (0..ops)
                .map(|_| format!(" {{:text \"M.{}\"}}", rng.word()))
                .collect();
            let name = rng.word();
            text += &format!(
                "  {{:id :{id} :name \"{}{}\" :ns \"bench.levels.{id}\" :level {level} :ops [{ops}]}}\n",
                name[..1].to_uppercase(),
                &name[1..]
            );
            ids.push(id);
        }
        by_level.push(ids);
    }
    text += "]}]\n :foreign [";
    for f in FOREIGN {
        text += &format!("{{:id :{f} :name \"{f}\" :foreign true}} ");
    }
    text += "]\n :edges [\n";
    let edges = classes * 32 / 10;
    let foreign = edges * 8 / 100;
    for i in 0..edges {
        let from_level = rng.weighted(&sizes);
        let level = &by_level[from_level];
        let a = rng.below(level.len());
        let to = if i < foreign {
            FOREIGN[rng.below(FOREIGN.len())].to_string()
        } else if rng.below(100) < 61 || from_level == 0 {
            if level.len() < 2 {
                continue;
            }
            let b = rng.below(level.len() - 1);
            let b = if b >= a { b + 1 } else { b };
            let (a, b) = if (a > b) == (rng.below(20) != 0) {
                (a, b)
            } else {
                (b, a)
            };
            text += &format!(
                "  {{:from :{} :to :{} :kind :dependency}}\n",
                level[a], level[b]
            );
            continue;
        } else {
            let span = 1 + rng.weighted(&SPANS[..from_level.min(SPANS.len())]);
            let target = &by_level[from_level - span];
            target[rng.below(target.len())].clone()
        };
        text += &format!("  {{:from :{} :to :{to} :kind :dependency}}\n", level[a]);
    }
    text + "]}\n"
}
