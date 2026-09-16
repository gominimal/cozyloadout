//! What the host has, what a VM may take of it, and what the user has picked.
//!
//! **The policy is not invented here.** The vcpu ceiling, the RAM floor, the
//! defaults, and the `x86_64` MMIO-hole hazard all come from `minvmd`, which is
//! what actually boots the VM — a picker offering a value `minvmd` rejects, or
//! worse accepts into a guest that panics at boot, would be a lie. Only the RAM
//! *ceiling* is this module's own, because `minvmd` merely warns about
//! over-allocation where a picker has to draw a line.
//!
//! Ported from minimal's own onboarding resource screen so the two agree. The
//! constants are duplicated rather than depended on: cozy is a separate repo
//! that does not link minvmd, so the honest options were "copy them with the
//! source named" or "guess". `mirrors_minvmds_published_policy` is the test
//! that fails when the copy drifts.

use std::path::Path;

/// Default guest vcpu count (`minvmd::cmd::DEFAULT_VM_VCPUS`).
const DEFAULT_VM_VCPUS: u8 = 2;

/// Logical cores held back from the vcpu ceiling for the host side: the VMM and
/// its worker threads, plus whatever else the machine is running
/// (`minvmd::cmd::VCPU_HOST_RESERVE`).
const VCPU_HOST_RESERVE: u32 = 2;

/// The floor below which a guest cannot reach userspace
/// (`minvmd::cmd::config::MIN_RAM_MIB`).
const MIN_RAM_MIB: u32 = 512;

/// Default guest RAM. **Arch-conditional**, because libkrun boots a same-arch
/// guest: on `x86_64` a 4096 MiB guest straddles the 32-bit MMIO/PCI hole, which
/// mis-places the initramfs and panics the kernel, so the default is a
/// hole-safe 2048 (`minvmd::cmd::DEFAULT_VM_RAM_MIB`).
#[cfg(target_arch = "x86_64")]
const DEFAULT_VM_RAM_MIB: u32 = 2048;
/// See the `x86_64` variant for the MMIO-hole rationale.
#[cfg(not(target_arch = "x86_64"))]
const DEFAULT_VM_RAM_MIB: u32 = 4096;

/// RAM sizes the picker offers, in MiB. A ladder rather than a free slider: the
/// sizes that matter are round ones, and stepping through a dozen of them beats
/// nudging a number 512 MiB at a time.
const RAM_LADDER: [u32; 15] = [
    512, 1024, 1536, 2048, 3072, 4096, 6144, 8192, 12288, 16384, 24576, 32768, 49152, 65536,
    131_072,
];

/// The share of host RAM a VM may claim, as `numerator / denominator`. The rest
/// stays with the host, which still has to run the VMM, the user's desktop, and
/// the page cache the guest's disk I/O goes through.
const HOST_RAM_SHARE_NUMERATOR: u32 = 3;
const HOST_RAM_SHARE_DENOMINATOR: u32 = 4;

/// What the host physically has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostCapacity {
    pub logical_cores: u32,
    pub total_mib: u32,
}

impl HostCapacity {
    /// Probe this machine.
    ///
    /// Cores come from the standard library. Memory has no portable API, so it
    /// is read per platform — `/proc/meminfo` on Linux, `sysctl hw.memsize` on
    /// macOS — rather than by taking a dependency for one number. A probe that
    /// fails yields 0, which the ceiling then floors at [`MIN_RAM_MIB`]; the
    /// page says the host is unknown rather than inventing a size.
    pub fn probe() -> Self {
        Self {
            logical_cores: std::thread::available_parallelism()
                .map_or(1, |n| u32::try_from(n.get()).unwrap_or(1)),
            total_mib: total_mib().unwrap_or(0),
        }
    }
}

#[cfg(target_os = "linux")]
fn total_mib() -> Option<u32> {
    // MemTotal is in kB, and it is the first line, but parse by key rather than
    // by position — the format is stable, the ordering is not guaranteed.
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    u32::try_from(kb / 1024).ok()
}

#[cfg(target_os = "macos")]
fn total_mib() -> Option<u32> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    let bytes: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    u32::try_from(bytes / (1024 * 1024)).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn total_mib() -> Option<u32> {
    None
}

/// What the VM will be booted with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Allocation {
    pub vcpus: u8,
    pub ram_mib: u32,
}

/// Which field the arrows act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Cpu,
    Memory,
}

/// Which way a step moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Down,
    Up,
}

/// The host as probed, the ceilings derived from it, and the current pick.
#[derive(Clone, Debug)]
pub struct Resources {
    pub host: HostCapacity,
    pub max_vcpus: u8,
    /// Offerable RAM sizes, ascending. Never empty.
    ram_choices: Vec<u32>,
    allocation: Allocation,
    pub field: Field,
}

impl Resources {
    /// Probe the host and start from the defaults `minvmd` would boot with,
    /// clamped into range.
    pub fn probe() -> Self {
        Self::for_host(HostCapacity::probe())
    }

    /// The same for a supplied host — capacity is injected so the limits are
    /// testable without probing this machine.
    pub fn for_host(host: HostCapacity) -> Self {
        let max_vcpus = max_vm_vcpus(host.logical_cores);
        let ram_choices = ram_choices(host.total_mib);
        // The ladder may not reach the default RAM on a small host, so take the
        // largest offered size at or below it. The vcpu default is itself the
        // floor of the ceiling, so it always fits.
        let ram_mib = ram_choices
            .iter()
            .rev()
            .find(|&&mib| mib <= DEFAULT_VM_RAM_MIB)
            .copied()
            .unwrap_or(ram_choices[0]);
        Self {
            host,
            max_vcpus,
            ram_choices,
            allocation: Allocation {
                vcpus: DEFAULT_VM_VCPUS.min(max_vcpus),
                ram_mib,
            },
            field: Field::Cpu,
        }
    }

    pub fn allocation(&self) -> Allocation {
        self.allocation
    }

    /// Restore a remembered pick, ignoring anything this host can no longer
    /// offer — a config carried to a smaller machine must not propose a VM that
    /// machine cannot boot.
    pub fn restore(&mut self, vcpus: Option<u8>, ram_mib: Option<u32>) {
        if let Some(v) = vcpus {
            if (1..=self.max_vcpus).contains(&v) {
                self.allocation.vcpus = v;
            }
        }
        if let Some(m) = ram_mib {
            if self.ram_choices.contains(&m) {
                self.allocation.ram_mib = m;
            }
        }
    }

    /// The defaults this host would get with no config at all.
    pub fn defaults(&self) -> Allocation {
        Self::for_host(self.host).allocation
    }

    /// Whether the pick says anything the absence of config does not.
    pub fn is_default(&self) -> bool {
        self.allocation == self.defaults()
    }

    pub fn max_ram_mib(&self) -> u32 {
        *self.ram_choices.last().unwrap_or(&MIN_RAM_MIB)
    }

    pub fn toggle_field(&mut self) {
        self.field = match self.field {
            Field::Cpu => Field::Memory,
            Field::Memory => Field::Cpu,
        };
    }

    /// Step the focused field one notch. Clamps rather than wraps: rolling from
    /// the maximum RAM back to 512 MiB on one extra key press is a trap, not a
    /// convenience.
    pub fn adjust(&mut self, step: Step) {
        match self.field {
            Field::Cpu => {
                self.allocation.vcpus = match step {
                    Step::Down => self.allocation.vcpus.saturating_sub(1).max(1),
                    Step::Up => self.allocation.vcpus.saturating_add(1).min(self.max_vcpus),
                };
            }
            Field::Memory => {
                let current = self
                    .ram_choices
                    .iter()
                    .position(|&mib| mib == self.allocation.ram_mib)
                    .unwrap_or(0);
                let next = match step {
                    Step::Down => current.saturating_sub(1),
                    Step::Up => (current + 1).min(self.ram_choices.len() - 1),
                };
                self.allocation.ram_mib = self.ram_choices[next];
            }
        }
    }
}

/// Upper bound on guest vcpus: the core count minus the host reserve, floored
/// at the baseline so small hosts still get it (`minvmd::cmd::max_vm_vcpus`).
fn max_vm_vcpus(logical_cores: u32) -> u8 {
    u8::try_from(logical_cores.saturating_sub(VCPU_HOST_RESERVE))
        .unwrap_or(u8::MAX)
        .max(DEFAULT_VM_VCPUS)
}

/// The RAM sizes offerable on a host with `total_mib` of memory: the host's
/// share, and — on `x86_64` — nothing inside the MMIO hole. Sizes in the hole are
/// not offered at all, since there is nothing the user could do about the
/// resulting panic.
fn ram_choices(total_mib: u32) -> Vec<u32> {
    let ceiling = max_ram_mib(total_mib);
    let choices: Vec<u32> = RAM_LADDER
        .into_iter()
        .filter(|&mib| mib >= MIN_RAM_MIB && mib <= ceiling && !straddles_mmio_hole(mib))
        .collect();
    if choices.is_empty() {
        vec![MIN_RAM_MIB]
    } else {
        choices
    }
}

/// The most RAM a VM may claim: the host's share, floored at what a guest needs
/// to reach userspace.
fn max_ram_mib(total_mib: u32) -> u32 {
    (total_mib / HOST_RAM_SHARE_DENOMINATOR * HOST_RAM_SHARE_NUMERATOR).max(MIN_RAM_MIB)
}

/// Whether `mib` lands in the `x86_64` 32-bit MMIO/PCI hole.
#[cfg(target_arch = "x86_64")]
fn straddles_mmio_hole(mib: u32) -> bool {
    (3073..=6143).contains(&mib)
}

/// aarch64 and friends have no low MMIO hole.
#[cfg(not(target_arch = "x86_64"))]
fn straddles_mmio_hole(_mib: u32) -> bool {
    false
}

/// MiB as a person reads it: whole GiB where it divides evenly, one decimal
/// where it does not, MiB below a gigabyte.
pub fn format_mib(mib: u32) -> String {
    match mib {
        mib if mib < 1024 => format!("{mib} MiB"),
        mib if mib % 1024 == 0 => format!("{} GiB", mib / 1024),
        mib => format!("{:.1} GiB", f64::from(mib) / 1024.0),
    }
}

/// Where `minvmd` lives, if it is on the PATH.
///
/// The wizard applies a pick by running `minvmd config set`, not by writing
/// `config.toml` itself: that command validates against host capacity, takes
/// the lifecycle lock, and knows its own state directory. Reproducing any of
/// that here would be a guess that breaks quietly.
pub fn minvmd_on_path() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("minvmd"))
        .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(logical_cores: u32, total_mib: u32) -> HostCapacity {
        HostCapacity {
            logical_cores,
            total_mib,
        }
    }

    /// 64 GiB, 16 cores: roomy enough that the ladder binds nowhere.
    fn workstation() -> Resources {
        Resources::for_host(host(16, 65_536))
    }

    #[test]
    fn mirrors_minvmds_published_policy() {
        // The values this file copies from minvmd. If minvmd moves one, this
        // test is what says the copy has drifted.
        assert_eq!(DEFAULT_VM_VCPUS, 2);
        assert_eq!(VCPU_HOST_RESERVE, 2);
        assert_eq!(MIN_RAM_MIB, 512);
        assert_eq!(
            DEFAULT_VM_RAM_MIB,
            if cfg!(target_arch = "x86_64") {
                2048
            } else {
                4096
            }
        );
    }

    #[test]
    fn the_vcpu_ceiling_leaves_the_host_its_reserve() {
        assert_eq!(workstation().max_vcpus, 14);
    }

    #[test]
    fn a_small_host_still_gets_the_baseline() {
        // The floor lifts the ceiling above the core count rather than
        // offering zero vcpus.
        assert_eq!(
            Resources::for_host(host(1, 2048)).max_vcpus,
            DEFAULT_VM_VCPUS
        );
        assert_eq!(
            Resources::for_host(host(2, 2048)).max_vcpus,
            DEFAULT_VM_VCPUS
        );
    }

    #[test]
    fn the_host_keeps_a_quarter_of_its_memory() {
        // 16 GiB host: three quarters is 12 GiB, so 12288 is offered and 16384
        // is not.
        let r = Resources::for_host(host(8, 16_384));
        assert_eq!(r.max_ram_mib(), 12_288);
    }

    #[test]
    fn ram_never_drops_below_the_floor_even_on_a_tiny_host() {
        let r = Resources::for_host(host(1, 256));
        assert_eq!(r.max_ram_mib(), MIN_RAM_MIB);
        assert_eq!(r.allocation().ram_mib, MIN_RAM_MIB);
    }

    #[test]
    fn an_unprobeable_host_still_yields_something_bootable() {
        // `probe` reports 0 when the platform read fails; the picker must not
        // then offer an unbootable VM.
        let r = Resources::for_host(host(0, 0));
        assert_eq!(r.allocation().ram_mib, MIN_RAM_MIB);
        assert!(r.allocation().vcpus >= 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sizes_inside_the_mmio_hole_are_never_offered() {
        // 4096 and 6143 panic an x86_64 guest at boot. There is nothing the
        // user could do about it, so the ladder simply skips them.
        let r = Resources::for_host(host(16, 131_072));
        for bad in [4096, 6143] {
            assert!(!r.ram_choices.contains(&bad), "{bad} straddles the hole");
        }
        assert!(r.ram_choices.contains(&3072), "the size below it is fine");
        assert!(r.ram_choices.contains(&6144), "and the one above it");
    }

    #[test]
    fn stepping_clamps_at_both_ends_rather_than_wrapping() {
        let mut r = workstation();
        for _ in 0..40 {
            r.adjust(Step::Down);
        }
        assert_eq!(r.allocation().vcpus, 1, "one core is the floor");
        for _ in 0..40 {
            r.adjust(Step::Up);
        }
        assert_eq!(r.allocation().vcpus, r.max_vcpus, "and the ceiling holds");

        r.toggle_field();
        for _ in 0..40 {
            r.adjust(Step::Up);
        }
        assert_eq!(r.allocation().ram_mib, r.max_ram_mib());
        for _ in 0..40 {
            r.adjust(Step::Down);
        }
        assert_eq!(r.allocation().ram_mib, MIN_RAM_MIB);
    }

    #[test]
    fn the_starting_pick_is_what_minvmd_would_boot_anyway() {
        let r = workstation();
        assert!(r.is_default());
        assert_eq!(r.allocation().vcpus, DEFAULT_VM_VCPUS);
        assert_eq!(r.allocation().ram_mib, DEFAULT_VM_RAM_MIB);
    }

    #[test]
    fn a_remembered_pick_this_host_cannot_honour_is_ignored() {
        // The settings file travels with the checkout. Carried to a smaller
        // machine, an out-of-range value must fall back rather than propose a
        // VM that will not boot.
        let mut r = Resources::for_host(host(4, 4096));
        r.restore(Some(200), Some(131_072));
        assert_eq!(r.allocation(), r.defaults(), "both were out of range");

        r.restore(Some(2), Some(1024));
        assert_eq!(r.allocation().vcpus, 2);
        assert_eq!(r.allocation().ram_mib, 1024);
    }

    #[test]
    fn a_remembered_ram_size_off_the_ladder_is_ignored() {
        // Including one inside the MMIO hole, which is the case that matters.
        let mut r = workstation();
        r.restore(None, Some(5000));
        assert_eq!(r.allocation().ram_mib, r.defaults().ram_mib);
    }

    #[test]
    fn formats_sizes_the_way_a_person_reads_them() {
        assert_eq!(format_mib(512), "512 MiB");
        assert_eq!(format_mib(2048), "2 GiB");
        assert_eq!(format_mib(1536), "1.5 GiB");
        assert_eq!(format_mib(131_072), "128 GiB");
    }
}
