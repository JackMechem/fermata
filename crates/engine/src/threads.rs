//! Settings › Audio › Processing Threads: how many threads play tracks, and what Automatic means on
//! this machine. See "Multi-core processing" in docs/ARCHITECTURE.md for the measurements behind it.

/// The setting's value for Automatic.
pub const AUTOMATIC: u32 = 0;
/// The most Automatic ever picks. Past this a project of the sizes measured gained nothing, and
/// every thread is another that can be late.
pub const AUTOMATIC_MAX: usize = 6;
/// The most the setting offers.
pub const MANUAL_MAX: usize = 16;

/// A fresh install's setting.
pub fn default_setting() -> u32 {
    AUTOMATIC
}

/// The CPUs of this machine, as far as they matter here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    /// Logical CPUs we may use.
    pub logical: usize,
    /// Physical cores of the fastest kind (on a machine with performance and efficiency cores, the
    /// performance ones).
    pub fast_cores: usize,
    /// The logical CPUs of those cores, on a machine with two kinds of core. Empty on a uniform
    /// machine or where it can't be told.
    pub fast: Vec<usize>,
}

impl Topology {
    pub fn detect() -> Topology {
        let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
        #[cfg(target_os = "linux")]
        {
            let read = |p: &str| std::fs::read_to_string(p).ok();
            let sibling = |c: usize| read(&format!("/sys/devices/system/cpu/cpu{c}/topology/thread_siblings_list"));
            detect_linux(logical, read("/sys/devices/cpu_core/cpus"), read("/sys/devices/system/cpu/online"), &sibling)
        }
        // Workers only get real-time priority on Linux so far, and at normal priority beside a
        // real-time callback they'd cost more than they bring: a number can be chosen, but Automatic
        // stays on one thread.
        #[cfg(not(target_os = "linux"))]
        Topology { logical, fast_cores: 1, fast: Vec::new() }
    }
}

/// Parses a kernel CPU list such as "0-15,18".
fn parse_cpus(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.parse::<usize>().ok()),
        }
    }
    out
}

#[allow(dead_code)]
fn detect_linux(logical: usize, p_cores: Option<String>, online: Option<String>, sibling: &dyn Fn(usize) -> Option<String>) -> Topology {
    let fast = p_cores.map(|s| parse_cpus(&s)).unwrap_or_default();
    let all = online.map(|s| parse_cpus(&s)).unwrap_or_else(|| (0..logical).collect());
    let among = if fast.is_empty() { &all } else { &fast };
    // A core counts once, however many hardware threads it has: its first sibling stands for it.
    let cores = among.iter().filter(|&&c| sibling(c).and_then(|l| parse_cpus(&l).first().copied()).is_none_or(|first| first == c)).count();
    Topology { logical, fast_cores: cores.max(1), fast }
}

/// What Automatic picks: leave a fast core for everything else the machine and the plug-ins' own
/// threads do, and stop at [`AUTOMATIC_MAX`]. A machine with a core or two plays serially.
pub fn automatic(t: &Topology) -> usize {
    t.fast_cores.min(t.logical).saturating_sub(1).clamp(1, AUTOMATIC_MAX)
}

/// The threads a setting means here. A number is taken at its word, up to what the setting offers
/// (more threads than cores works, just not well).
pub fn resolve(setting: u32, t: &Topology) -> usize {
    match setting {
        AUTOMATIC => automatic(t),
        n => (n as usize).min(MANUAL_MAX),
    }
}

/// What Settings › Audio offers, for the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThreadChoices {
    /// What Automatic is on this machine.
    pub automatic: u32,
    /// The highest number on offer.
    pub max: u32,
}

pub fn choices(t: &Topology) -> ThreadChoices {
    ThreadChoices { automatic: automatic(t) as u32, max: t.logical.clamp(1, MANUAL_MAX) as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_parse() {
        assert_eq!(parse_cpus("0-3,8,10-11\n"), [0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cpus(""), Vec::<usize>::new());
        assert_eq!(parse_cpus("x,4"), [4]);
    }

    fn hybrid_siblings(c: usize) -> Option<String> {
        // Eight performance cores with two hardware threads each, then eight efficiency cores.
        Some(if c < 16 { format!("{},{}", c & !1, c | 1) } else { c.to_string() })
    }

    #[test]
    fn performance_cores_count_once_and_efficiency_cores_not_at_all() {
        let t = detect_linux(24, Some("0-15\n".into()), Some("0-23\n".into()), &hybrid_siblings);
        assert_eq!(t.fast_cores, 8);
        assert_eq!(t.fast, (0..16).collect::<Vec<_>>());
        assert_eq!(automatic(&t), 6);
    }

    #[test]
    fn a_uniform_machine_counts_its_physical_cores() {
        let t = detect_linux(8, None, Some("0-7".into()), &|c| Some(format!("{},{}", c & !1, c | 1)));
        assert_eq!(t.fast_cores, 4);
        assert_eq!(automatic(&t), 3);
    }

    #[test]
    fn small_machines_stay_serial() {
        for (logical, cores) in [(1, 1), (2, 1), (2, 2)] {
            let t = Topology { logical, fast_cores: cores, fast: Vec::new() };
            assert_eq!(automatic(&t), 1, "{logical} logical, {cores} cores");
        }
    }

    #[test]
    fn automatic_is_bounded_on_a_big_machine() {
        let t = Topology { logical: 128, fast_cores: 64, fast: Vec::new() };
        assert_eq!(automatic(&t), AUTOMATIC_MAX);
        assert_eq!(resolve(1, &t), 1);
        assert_eq!(resolve(AUTOMATIC, &t), AUTOMATIC_MAX);
        assert_eq!(resolve(4, &t), 4);
        assert_eq!(resolve(1000, &t), MANUAL_MAX);
    }

    #[test]
    fn the_setting_offers_what_the_machine_has() {
        let c = choices(&Topology { logical: 24, fast_cores: 8, fast: Vec::new() });
        assert_eq!((c.automatic, c.max), (6, 16));
        let c = choices(&Topology { logical: 2, fast_cores: 1, fast: Vec::new() });
        assert_eq!((c.automatic, c.max), (1, 2));
    }
}
