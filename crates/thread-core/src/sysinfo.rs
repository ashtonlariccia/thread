//! How busy a remote machine is: its processor, its memory and its graphics
//! card, for the bottom bar to say while a build runs there.
//!
//! Nothing is installed to find out. A few lines of shell are left running
//! on the remote, reading `/proc` every couple of seconds and printing what
//! they read as one line; this turns each line into a [`Sample`]. A machine
//! with no `/proc` prints nothing, and the bar then says nothing.

use serde::Serialize;

/// Marks a reading, among whatever else a login shell prints.
const MARK: &str = "thread-sys:";

/// How often the remote is read, in seconds.
const EVERY: u32 = 2;

/// What is left running on the remote, for `sh`.
///
/// A line is `cpu counters|total,available kB|load|cores|gpu`, where the
/// graphics card's part is `use %,used MiB,total MiB,temperature` or empty.
/// NVIDIA's is asked of `nvidia-smi`; AMD's is read from where the kernel
/// keeps it. It ends when there is no one left to print to.
pub fn script() -> String {
    format!(
        r#"n=$(grep -c ^processor /proc/cpuinfo 2>/dev/null)
gpu() {{
  if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu --format=csv,noheader,nounits 2>/dev/null | head -n 1 | tr -d ' '
    return
  fi
  for d in /sys/class/drm/card*/device; do
    [ -r "$d/gpu_busy_percent" ] || continue
    u=$(cat "$d/gpu_busy_percent" 2>/dev/null)
    mu=$(cat "$d/mem_info_vram_used" 2>/dev/null || echo 0)
    mt=$(cat "$d/mem_info_vram_total" 2>/dev/null || echo 0)
    echo "$u,$((mu / 1048576)),$((mt / 1048576)),"
    return
  done
}}
[ -r /proc/stat ] || exit 0
while :; do
  read -r _ a b c d e f g h _ < /proc/stat
  m=$(awk '/^MemTotal:/{{t=$2}} /^MemAvailable:/{{a=$2}} END{{print t","a}}' /proc/meminfo)
  read -r l _ < /proc/loadavg
  echo "{MARK}$a $b $c $d $e $f $g $h|$m|$l|$n|$(gpu)" || exit 0
  sleep {EVERY}
done"#
    )
}

/// One reading of the machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// How much of every core together was in use since the last reading,
    /// in percent.
    pub cpu: f32,
    pub cores: u32,
    /// The load average over the last minute.
    pub load: f32,
    /// Memory in use and in all, in kilobytes. In use is what could not be
    /// had back: the kernel's cache is not counted.
    pub mem_used: u64,
    pub mem_total: u64,
    pub gpu: Option<Gpu>,
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
}

/// Turns the lines the script prints into readings. The processor's figure
/// is the difference between two lines, so the first line gives none.
#[derive(Default)]
pub struct Sampler {
    /// A line not yet ended.
    partial: Vec<u8>,
    /// The processor's counters at the last reading: in all, and idle.
    last: Option<(u64, u64)>,
}

impl Sampler {
    /// Take in what the script printed, and give back the readings in it.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Sample> {
        self.partial.extend_from_slice(bytes);
        let mut samples = Vec::new();
        while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(reading) = line.trim().strip_prefix(MARK) {
                samples.extend(self.read(reading));
            }
        }
        samples
    }

    fn read(&mut self, reading: &str) -> Option<Sample> {
        let mut parts = reading.split('|');
        let counters: Vec<u64> = parts
            .next()?
            .split_whitespace()
            .map(|count| count.parse().unwrap_or(0))
            .collect();
        let (mem_total, mem_available) = parts.next()?.split_once(',')?;
        let load = parts.next()?.trim().parse().unwrap_or(0.0);
        let cores = parts.next()?.trim().parse().unwrap_or(0);
        let gpu = parts.next().and_then(read_gpu);

        // user nice system idle iowait irq softirq steal: idle is the
        // fourth and the fifth, waiting on a disk being no work either.
        let total: u64 = counters.iter().sum();
        let idle = counters.get(3).copied().unwrap_or(0) + counters.get(4).copied().unwrap_or(0);
        let before = self.last.replace((total, idle));
        let (was_total, was_idle) = before?;
        let passed = total.checked_sub(was_total).filter(|passed| *passed > 0)?;
        let idled = idle.saturating_sub(was_idle).min(passed);

        let mem_total: u64 = mem_total.trim().parse().ok()?;
        let mem_available: u64 = mem_available.trim().parse().unwrap_or(0);
        Some(Sample {
            cpu: (passed - idled) as f32 * 100.0 / passed as f32,
            cores,
            load,
            mem_used: mem_total.saturating_sub(mem_available),
            mem_total,
            gpu,
        })
    }
}

/// `use,used,total,temperature`, any of which a card may not say.
fn read_gpu(text: &str) -> Option<Gpu> {
    let mut fields = text.trim().split(',').map(str::trim);
    let usage = fields.next()?.parse().ok()?;
    let mut number = || fields.next().and_then(|field| field.parse::<f32>().ok());
    let (mem_used, mem_total, temperature) = (number(), number(), number());
    Some(Gpu {
        usage,
        mem_used: mem_used.unwrap_or(0.0) as u64,
        mem_total: mem_total.unwrap_or(0.0) as u64,
        temperature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_processor_is_the_difference_between_two_readings() {
        let mut sampler = Sampler::default();
        let first =
            "Last login: today\nthread-sys:100 0 100 800 0 0 0 0|16000000,4000000|1.50|8|\n";
        assert_eq!(
            sampler.feed(first.as_bytes()),
            [],
            "nothing to compare with yet"
        );

        // 200 more in all, 50 of them idle or waiting on a disk.
        let next = "thread-sys:200 0 150 840 10 0 0 0|16000000,6000000|2.25|8|\n";
        assert_eq!(
            sampler.feed(next.as_bytes()),
            [Sample {
                cpu: 75.0,
                cores: 8,
                load: 2.25,
                mem_used: 10_000_000,
                mem_total: 16_000_000,
                gpu: None,
            }]
        );
    }

    #[test]
    fn a_graphics_card_is_read_whichever_way_it_was_asked() {
        let mut sampler = Sampler::default();
        sampler.feed(b"thread-sys:1 0 0 1 0 0 0 0|8,4|0|4|\n");
        let nvidia = sampler.feed(b"thread-sys:2 0 0 2 0 0 0 0|8,4|0|4|87,6144,8192,71\n");
        assert_eq!(
            nvidia[0].gpu,
            Some(Gpu {
                usage: 87.0,
                mem_used: 6144,
                mem_total: 8192,
                temperature: Some(71.0)
            })
        );
        // AMD's, which says no temperature; and a card that says nothing usable.
        let amd = sampler.feed(b"thread-sys:3 0 0 3 0 0 0 0|8,4|0|4|12,512,16384,\n");
        assert_eq!(amd[0].gpu.as_ref().unwrap().temperature, None);
        let none = sampler.feed(b"thread-sys:4 0 0 4 0 0 0 0|8,4|0|4|[N/A],1,2,3\n");
        assert_eq!(none[0].gpu, None);
    }

    #[test]
    fn a_line_in_pieces_is_one_line_and_a_stalled_clock_is_no_reading() {
        let mut sampler = Sampler::default();
        sampler.feed(b"thread-sys:10 0 0 10 0 0 0 0|8,4|0|4|\n");
        assert_eq!(sampler.feed(b"thread-sys:20 0 0 20 0"), []);
        assert_eq!(sampler.feed(b" 0 0 0|8,4|0|4|\n").len(), 1);
        // The same counters again: no time passed, and nothing to divide by.
        assert_eq!(sampler.feed(b"thread-sys:20 0 0 20 0 0 0 0|8,4|0|4|\n"), []);
    }

    #[test]
    fn the_script_is_for_any_sh() {
        let script = script();
        assert!(script.contains("echo \"thread-sys:$a"));
        assert!(script.contains("sleep 2"));
        // Braces that are the shell's own came through the formatting whole.
        assert!(script.contains("gpu() {\n") && script.contains("END{print t\",\"a}"));
    }
}
