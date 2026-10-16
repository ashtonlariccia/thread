//! How busy a remote machine is: its processor, its memory, its graphics
//! card, its network and its disk, and what is running on each of its
//! terminals. For the bottom bar to say while a build runs there, and for
//! the list of tmux windows to say what each one is running.
//!
//! Nothing is installed to find out. A few lines of shell are left running
//! on the remote, reading `/proc` every couple of seconds and printing what
//! they read; this turns each round of it into a [`Sample`]. A machine with
//! no `/proc` prints nothing, and nothing is then said of it.

use std::collections::HashMap;

use serde::Serialize;

/// Marks the line that ends a reading, among whatever else a login shell
/// prints. Before it come the processes: one line of each of the two below
/// for every process that has a terminal.
const MARK: &str = "tide-sys:";
/// `pid ppid terminal-number user-ticks system-ticks`, from `/proc/N/stat`.
const PROCESS: &str = "tide-ps:";
/// `pid terminal command line`, from `ps`.
const ARGS: &str = "tide-arg:";
/// `core user nice system idle ...`, one core's counters from `/proc/stat`.
const CORE: &str = "tide-cpu:";
/// A tmux pane as it stands, tab between each: its number, its window's,
/// when the window last printed, what it is running, and the folder it is in.
const PANE: &str = "tide-pane:";

/// What is left running on the remote, for `sh`.
///
/// The line that ends a reading is `key=value` pairs between bars. The
/// graphics card's is `use %,used MiB,total MiB,temperature,watts`, as much
/// of it as the card says: NVIDIA's is asked of `nvidia-smi`, and AMD's is
/// read from where the kernel keeps it. The processor's temperature is the
/// package's, from whichever sensor the kernel has for it. How full the
/// disk is, is looked at now and then rather than every time.
///
/// The second reading follows the first by a fraction of a second, and
/// only then does it settle to one every two: the rates are the difference
/// between two readings, and no one should wait two seconds to see the first.
///
/// It ends when there is no one left to print to.
pub const SCRIPT: &str = r#"n=$(grep -c ^processor /proc/cpuinfo 2>/dev/null)
hz=$(getconf CLK_TCK 2>/dev/null || echo 100)
temp=
for h in /sys/class/hwmon/hwmon*; do
  case "$(cat "$h/name" 2>/dev/null)" in
    coretemp|k10temp|zenpower|cpu_thermal|cpu-thermal)
      if [ -r "$h/temp1_input" ]; then temp="$h/temp1_input"; break; fi ;;
  esac
done
if [ -z "$temp" ] && [ -r /sys/class/thermal/thermal_zone0/temp ]; then
  temp=/sys/class/thermal/thermal_zone0/temp
fi
gpu() {
  if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw --format=csv,noheader,nounits 2>/dev/null | head -n 1 | tr -d ' '
    return
  fi
  for d in /sys/class/drm/card*/device; do
    [ -r "$d/gpu_busy_percent" ] || continue
    u=$(cat "$d/gpu_busy_percent" 2>/dev/null)
    mu=$(cat "$d/mem_info_vram_used" 2>/dev/null || echo 0)
    mt=$(cat "$d/mem_info_vram_total" 2>/dev/null || echo 0)
    echo "$u,$((mu / 1048576)),$((mt / 1048576)),,"
    return
  done
}
[ -r /proc/stat ] || exit 0
i=0
disk=
while :; do
  cat /proc/[0-9]*/stat 2>/dev/null | sed -n 's/^\([0-9]*\) (.*) . \([0-9]*\) [0-9]* [0-9]* \([0-9]*\) [-0-9]* [0-9]* [0-9]* [0-9]* [0-9]* [0-9]* \([0-9]*\) \([0-9]*\) .*/tide-ps:\1 \2 \3 \4 \5/p' | grep -v '^tide-ps:[0-9]* [0-9]* 0 '
  ps -eo pid=,tty=,args= 2>/dev/null | awk '$2 != "?" { print "tide-arg:" substr($0, 1, 240) }'
  sed -n 's/^cpu\([0-9][0-9]*\) /tide-cpu:\1 /p' /proc/stat
  tmux list-panes -a -F 'tide-pane:#{pane_id}	#{window_id}	#{window_activity}	#{pane_current_command}	#{pane_current_path}' 2>/dev/null
  read -r _ a b c d e f g h _ < /proc/stat
  m=$(awk '/^MemTotal:/{t=$2} /^MemAvailable:/{a=$2} /^SwapTotal:/{st=$2} /^SwapFree:/{sf=$2} END{print t","a","st","sf}' /proc/meminfo)
  read -r l1 l5 l15 lr _ < /proc/loadavg
  net=$(awk 'NR>2 && $1 !~ /^lo:/ {sub(/^[^:]*:/,""); rx+=$1; tx+=$9} END{printf "%.0f,%.0f", rx, tx}' /proc/net/dev 2>/dev/null)
  if [ $((i % 15)) -eq 0 ]; then disk=$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2{print $3","$2}'); fi
  i=$((i + 1))
  t=
  if [ -n "$temp" ]; then read -r t < "$temp" 2>/dev/null; fi
  read -r up _ < /proc/uptime
  echo "tide-sys:cpu=$a $b $c $d $e $f $g $h|mem=$m|load=$l1 $l5 $l15 $lr|n=$n|hz=$hz|net=$net|disk=$disk|temp=$t|up=$up|gpu=$(gpu)" || exit 0
  if [ "$i" -eq 1 ]; then sleep 0.3; else sleep 2; fi
done"#;

/// One reading of the machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// How much of every core together was in use since the last reading,
    /// in percent.
    pub cpu: f32,
    pub cores: u32,
    /// The processor's temperature, in degrees Celsius, where there is a
    /// sensor for it.
    pub temperature: Option<f32>,
    /// The load average over the last one, five and fifteen minutes.
    pub load: [f32; 3],
    /// Memory in use and in all, in kilobytes. In use is what could not be
    /// had back: the kernel's cache is not counted.
    pub mem_used: u64,
    pub mem_total: u64,
    /// Swap, likewise. Nought in all on a machine with none.
    pub swap_used: u64,
    pub swap_total: u64,
    /// Bytes a second arriving and leaving, over every interface but the
    /// machine's own loopback.
    pub net_down: f64,
    pub net_up: f64,
    /// The disk the home folder is on, in kilobytes; nought in all where it
    /// could not be asked.
    pub disk_used: u64,
    pub disk_total: u64,
    /// Seconds since the machine started.
    pub uptime: u64,
    pub gpu: Option<Gpu>,
    /// Each core's share in use, in percent, in the kernel's order.
    pub per_core: Vec<f32>,
    /// Every process that has a terminal.
    pub processes: Vec<Process>,
    /// Every tmux pane, as it stands now: tmux says when a window is made
    /// or closed, and says nothing when a shell changes folder.
    pub panes: Vec<PaneNow>,
}

/// A tmux pane at the time of a reading.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneNow {
    pub id: u32,
    pub window: u32,
    /// When anything in its window last printed, in seconds since 1970.
    pub activity: u64,
    /// What it is running, as tmux names it.
    pub command: String,
    /// The folder it is in.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    /// Percent.
    pub usage: f32,
    /// Its own memory in use and in all, in mebibytes; nought where unknown.
    pub mem_used: u64,
    pub mem_total: u64,
    /// Degrees Celsius.
    pub temperature: Option<f32>,
    /// Watts being drawn.
    pub power: Option<f32>,
}

/// A process on a terminal: something run in a shell, or the shell.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    /// As `ps` names it: `pts/3`.
    pub tty: String,
    /// Percent of one core since the last reading: a program on four cores
    /// is at 400.
    pub cpu: f32,
    /// Its command line.
    pub command: String,
}

/// Turns what the script prints into readings. The rates in one -- the
/// processor's, each process's, the network's -- are the difference between
/// two rounds, so the first round gives no reading.
#[derive(Default)]
pub struct Sampler {
    /// A line not yet ended.
    partial: Vec<u8>,
    /// The processor's counters at the last reading: in all, and idle.
    last: Option<(u64, u64)>,
    /// The network's counters then: bytes in, and out.
    last_net: Option<(u64, u64)>,
    /// Each process's ticks then.
    last_ticks: HashMap<u32, u64>,
    /// This round's processes so far: its parent and its ticks, by pid.
    stats: HashMap<u32, (u32, u64)>,
    /// And its terminal and command line.
    args: Vec<(u32, String, String)>,
    /// This round's counters for each core, in all and idle; and last round's.
    cores: Vec<(u64, u64)>,
    last_cores: Vec<(u64, u64)>,
    /// This round's tmux panes.
    panes: Vec<PaneNow>,
}

impl Sampler {
    /// Take in what the script printed, and give back the readings in it.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Sample> {
        self.partial.extend_from_slice(bytes);
        let mut samples = Vec::new();
        while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(process) = line.strip_prefix(PROCESS) {
                let mut fields = process
                    .split(' ')
                    .map(|field| field.parse::<u64>().unwrap_or(0));
                let mut next = || fields.next().unwrap_or(0);
                let (pid, ppid, _tty, user, system) = (next(), next(), next(), next(), next());
                self.stats.insert(pid as u32, (ppid as u32, user + system));
            } else if let Some(args) = line.strip_prefix(ARGS) {
                let mut fields = args.trim_start().splitn(2, char::is_whitespace);
                let pid = fields.next().and_then(|pid| pid.parse().ok());
                let rest = fields.next().unwrap_or_default().trim_start();
                let (tty, command) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                if let Some(pid) = pid {
                    self.args
                        .push((pid, tty.to_owned(), command.trim().to_owned()));
                }
            } else if let Some(core) = line.strip_prefix(CORE) {
                // The core's number, then its counters as the whole machine's are.
                let counters: Vec<u64> = core
                    .split(' ')
                    .skip(1)
                    .map(|count| count.parse().unwrap_or(0))
                    .collect();
                let idle =
                    counters.get(3).copied().unwrap_or(0) + counters.get(4).copied().unwrap_or(0);
                self.cores.push((counters.iter().take(8).sum(), idle));
            } else if let Some(pane) = line.strip_prefix(PANE) {
                let mut fields = pane.splitn(5, '\t');
                let parsed = (|| {
                    Some(PaneNow {
                        id: fields.next()?.strip_prefix('%')?.parse().ok()?,
                        window: fields.next()?.strip_prefix('@')?.parse().ok()?,
                        activity: fields.next()?.parse().unwrap_or(0),
                        command: fields.next()?.to_owned(),
                        path: fields.next()?.to_owned(),
                    })
                })();
                self.panes.extend(parsed);
            } else if let Some(reading) = line.trim().strip_prefix(MARK) {
                samples.extend(self.read(reading));
            }
        }
        samples
    }

    fn read(&mut self, reading: &str) -> Option<Sample> {
        let fields: HashMap<&str, &str> = reading
            .split('|')
            .filter_map(|field| field.split_once('='))
            .collect();
        let field = |key: &str| fields.get(key).copied().unwrap_or_default();
        let numbers = |key: &str, between: &[char]| -> Vec<u64> {
            field(key)
                .split(between)
                .map(|number| number.trim().parse().unwrap_or(0))
                .collect()
        };
        let at = |list: &[u64], index: usize| list.get(index).copied().unwrap_or(0);

        // The processes of this round are this reading's, whether or not
        // there turns out to be a reading.
        let stats = std::mem::take(&mut self.stats);
        let args = std::mem::take(&mut self.args);
        let panes = std::mem::take(&mut self.panes);
        let cores_now = std::mem::take(&mut self.cores);
        let cores_before = std::mem::replace(&mut self.last_cores, cores_now.clone());
        let per_core = match cores_before.len() == cores_now.len() {
            true => cores_now
                .iter()
                .zip(&cores_before)
                .map(|((total, idle), (was_total, was_idle))| {
                    let passed = total.saturating_sub(*was_total);
                    let idled = idle.saturating_sub(*was_idle).min(passed);
                    match passed {
                        0 => 0.0,
                        _ => (passed - idled) as f32 * 100.0 / passed as f32,
                    }
                })
                .collect(),
            false => Vec::new(),
        };

        // user nice system idle iowait irq softirq steal: idle is the
        // fourth and the fifth, waiting on a disk being no work either.
        let counters = numbers("cpu", &[' ']);
        let total: u64 = counters.iter().sum();
        let idle = at(&counters, 3) + at(&counters, 4);
        let before = self.last.replace((total, idle));

        let net = numbers("net", &[',']);
        let net = (at(&net, 0), at(&net, 1));
        let net_before = self.last_net.replace(net);

        let ticks_before = std::mem::replace(
            &mut self.last_ticks,
            stats
                .iter()
                .map(|(pid, (_, ticks))| (*pid, *ticks))
                .collect(),
        );

        let (was_total, was_idle) = before?;
        let passed = total.checked_sub(was_total).filter(|passed| *passed > 0)?;
        let idled = idle.saturating_sub(was_idle).min(passed);

        let cores: u32 = field("n").trim().parse().unwrap_or(0);
        // The counters are every core's together, in the same ticks a
        // process's are: one core's worth of them is the time that passed.
        let one_core = passed as f32 / cores.max(1) as f32;
        let hz: f64 = field("hz").trim().parse().unwrap_or(100.0);
        let seconds = f64::from(one_core) / hz.max(1.0);
        let rate = |now: u64, then: u64| match seconds > 0.0 {
            true => now.saturating_sub(then) as f64 / seconds,
            false => 0.0,
        };

        let mut processes: Vec<Process> = args
            .into_iter()
            .filter_map(|(pid, tty, command)| {
                let (ppid, ticks) = stats.get(&pid).copied()?;
                // One not seen before has been running for less than a
                // round, and what it has used it used in that.
                let used = ticks.saturating_sub(ticks_before.get(&pid).copied().unwrap_or(0));
                Some(Process {
                    pid,
                    ppid,
                    tty,
                    cpu: used as f32 * 100.0 / one_core,
                    command,
                })
            })
            .collect();
        processes.sort_unstable_by_key(|process| process.pid);

        let mem = numbers("mem", &[',']);
        let disk = numbers("disk", &[',']);
        let mut load = field("load")
            .split(' ')
            .map(|each| each.parse().unwrap_or(0.0));
        let mut next_load = || load.next().unwrap_or(0.0);
        Some(Sample {
            cpu: (passed - idled) as f32 * 100.0 / passed as f32,
            cores,
            // The kernel says it in thousandths of a degree.
            temperature: field("temp")
                .trim()
                .parse::<f32>()
                .ok()
                .map(|milli| milli / 1000.0)
                .filter(|degrees| *degrees > 0.0),
            load: [next_load(), next_load(), next_load()],
            mem_used: at(&mem, 0).saturating_sub(at(&mem, 1)),
            mem_total: at(&mem, 0),
            swap_used: at(&mem, 2).saturating_sub(at(&mem, 3)),
            swap_total: at(&mem, 2),
            net_down: net_before.map_or(0.0, |then| rate(net.0, then.0)),
            net_up: net_before.map_or(0.0, |then| rate(net.1, then.1)),
            disk_used: at(&disk, 0),
            disk_total: at(&disk, 1),
            uptime: field("up").trim().parse::<f64>().unwrap_or(0.0) as u64,
            gpu: read_gpu(field("gpu")),
            per_core,
            processes,
            panes,
        })
    }
}

/// `use,used,total,temperature,watts`, any of which a card may not say.
fn read_gpu(text: &str) -> Option<Gpu> {
    let mut fields = text.trim().split(',').map(str::trim);
    let usage = fields.next()?.parse().ok()?;
    let mut number = || fields.next().and_then(|field| field.parse::<f32>().ok());
    let (mem_used, mem_total, temperature, power) = (number(), number(), number(), number());
    Some(Gpu {
        usage,
        mem_used: mem_used.unwrap_or(0.0) as u64,
        mem_total: mem_total.unwrap_or(0.0) as u64,
        temperature,
        power,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line that ends a reading: eight cores, a hundred ticks a second.
    fn line(cpu: &str, rest: &str) -> String {
        format!("tide-sys:cpu={cpu}|mem=16000000,6000000,8000000,7000000|n=8|hz=100|{rest}\n")
    }

    #[test]
    fn the_rates_are_the_difference_between_two_readings() {
        let mut sampler = Sampler::default();
        let first = format!(
            "Last login: today\n{}",
            line(
                "100 0 100 800 0 0 0 0",
                "load=1.50 1.00 0.50 2/300|net=1000,500|up=10.5"
            )
        );
        assert_eq!(
            sampler.feed(first.as_bytes()),
            [],
            "nothing to compare with yet"
        );

        // 1600 more ticks in all: two seconds on eight cores. 400 of them
        // idle or waiting on a disk.
        let next = line(
            "1000 0 400 1100 100 0 0 0",
            "load=2.25 1.50 0.75 3/301|net=2049000,10500|disk=120000000,450000000|temp=62500|up=12.5|gpu=",
        );
        let sample = sampler.feed(next.as_bytes()).remove(0);
        assert_eq!(sample.cpu, 75.0);
        assert_eq!(sample.cores, 8);
        assert_eq!(sample.temperature, Some(62.5));
        assert_eq!(sample.load, [2.25, 1.5, 0.75]);
        assert_eq!(
            (sample.mem_used, sample.mem_total),
            (10_000_000, 16_000_000)
        );
        assert_eq!(
            (sample.swap_used, sample.swap_total),
            (1_000_000, 8_000_000)
        );
        assert_eq!((sample.net_down, sample.net_up), (1_024_000.0, 5_000.0));
        assert_eq!(
            (sample.disk_used, sample.disk_total),
            (120_000_000, 450_000_000)
        );
        assert_eq!(sample.uptime, 12);
        assert_eq!(sample.gpu, None);
    }

    #[test]
    fn a_process_is_what_it_used_of_one_core_since_the_last_reading() {
        let mut sampler = Sampler::default();
        let first = format!(
            "tide-ps:50 1 34819 100 0\ntide-arg:   50 pts/3    -bash\n{}",
            line("0 0 0 800 0 0 0 0", "")
        );
        sampler.feed(first.as_bytes());

        // Two seconds on, as above: the shell has done nothing, and a make
        // that was not there before has used three seconds of processor.
        let next = format!(
            "tide-ps:50 1 34819 100 0\ntide-ps:77 50 34819 250 50\ntide-ps:9 1 0 5 5\n\
             tide-arg:   50 pts/3    -bash\ntide-arg:   77 pts/3    make -j8 all\n{}",
            line("1200 0 0 1200 0 0 0 0", "")
        );
        let sample = sampler.feed(next.as_bytes()).remove(0);
        assert_eq!(
            sample.processes,
            [
                Process {
                    pid: 50,
                    ppid: 1,
                    tty: "pts/3".into(),
                    cpu: 0.0,
                    command: "-bash".into()
                },
                Process {
                    pid: 77,
                    ppid: 50,
                    tty: "pts/3".into(),
                    cpu: 150.0,
                    command: "make -j8 all".into()
                },
            ]
        );
    }

    #[test]
    fn each_core_and_each_tmux_pane_is_read_with_the_rest() {
        let mut sampler = Sampler::default();
        let first = format!(
            "tide-cpu:0 100 0 0 100 0 0 0 0 0 0\ntide-cpu:1 0 0 0 200 0 0 0 0 0 0\n{}",
            line("100 0 0 300 0 0 0 0", "")
        );
        sampler.feed(first.as_bytes());
        let next = format!(
            "tide-cpu:0 300 0 0 100 0 0 0 0 0 0\ntide-cpu:1 50 0 0 350 0 0 0 0 0 0\n\
             tide-pane:%3\t@1\t1791850000\tmake\t/home/ash/my project\n\
             tide-pane:nonsense\n{}",
            line("350 0 0 450 0 0 0 0", "")
        );
        let sample = sampler.feed(next.as_bytes()).remove(0);
        // The first did nothing but work; the second worked a quarter of the time.
        assert_eq!(sample.per_core, [100.0, 25.0]);
        assert_eq!(
            sample.panes,
            [PaneNow {
                id: 3,
                window: 1,
                activity: 1_791_850_000,
                command: "make".into(),
                path: "/home/ash/my project".into(),
            }]
        );
    }

    #[test]
    fn a_graphics_card_is_read_whichever_way_it_was_asked() {
        let mut sampler = Sampler::default();
        sampler.feed(line("1 0 0 1 0 0 0 0", "gpu=").as_bytes());
        let nvidia = sampler.feed(line("2 0 0 2 0 0 0 0", "gpu=87,6144,8192,71,182.40").as_bytes());
        assert_eq!(
            nvidia[0].gpu,
            Some(Gpu {
                usage: 87.0,
                mem_used: 6144,
                mem_total: 8192,
                temperature: Some(71.0),
                power: Some(182.4),
            })
        );
        // AMD's, which says neither temperature nor power; and a card that
        // says nothing usable.
        let amd = sampler.feed(line("3 0 0 3 0 0 0 0", "gpu=12,512,16384,,").as_bytes());
        let amd = amd[0].gpu.as_ref().unwrap();
        assert_eq!((amd.temperature, amd.power), (None, None));
        let none = sampler.feed(line("4 0 0 4 0 0 0 0", "gpu=[N/A],1,2,3,4").as_bytes());
        assert_eq!(none[0].gpu, None);
    }

    #[test]
    fn a_line_in_pieces_is_one_line_and_a_stalled_clock_is_no_reading() {
        let mut sampler = Sampler::default();
        sampler.feed(line("10 0 0 10 0 0 0 0", "").as_bytes());
        let whole = line("20 0 0 20 0 0 0 0", "");
        let (head, tail) = whole.split_at(30);
        assert_eq!(sampler.feed(head.as_bytes()), []);
        assert_eq!(sampler.feed(tail.as_bytes()).len(), 1);
        // The same counters again: no time passed, and nothing to divide by.
        assert_eq!(sampler.feed(whole.as_bytes()), []);
    }

    #[test]
    fn the_script_prints_what_is_read_here() {
        assert!(SCRIPT.contains(&format!("echo \"{MARK}cpu=$a")));
        assert!(SCRIPT.contains(&format!("/{PROCESS}\\1 \\2 \\3 \\4 \\5/p")));
        assert!(SCRIPT.contains(&format!("print \"{ARGS}\"")));
        assert!(SCRIPT.contains(&format!("/{CORE}\\1 /p")));
        assert!(SCRIPT.contains(&format!("-F '{PANE}#{{pane_id}}\t#{{window_id}}")));
        assert!(SCRIPT.contains("sleep 0.3; else sleep 2"));
    }
}
