//! Alternate timing sources for Rust
//!
//! # Goals
//! * high-performance
//! * graceful fallback for stable Rust
//!
//! # Future work
//! * additional platforms
//!
//! # Usage
//!
//! Note: You must be on nightly Rust to use the rdtsc feature which
//! allows access to the high-speed time stamp counter on `x86_64`
//!
//! # Example
//!
//! Create a clocksource and read from it
//!
//! ```
//! use clocksource::Clocksource;
//!
//! // create a `Clocksource` with rdtsc if on nightly
//! //   falls-back to clock_gettime() otherwise
//! let mut clocksource = Clocksource::new();
//!
//! // we can read the nanoseconds from the clocksource
//! // this adds some conversion overhead
//! let time_0 = clocksource.time();
//!
//! // we can read the time from the reference clock
//! // this should be a zero-cost abstraction
//! let ref_0 = clocksource.reference();
//!
//! // we can access the raw value of the counter that
//! // forms the clocksource (eg, the TSC if on nightly)
//! // this is ideal for tight loops
//! let counter_0 = clocksource.counter();
//!
//! // we can convert the counter value to a time, allowing
//! // separation of timing events and conversion to reference timescale
//! let time_0 = clocksource.convert(counter_0);
//!
//! // we can read the estimated frequency of the counter
//! let frequency = clocksource.frequency();
//!
//! // we can also estimate the phase error between the
//! // source clock and the reference clock
//! let phase_error = clocksource.phase_error();
//! ```

#![cfg_attr(feature = "rdtsc", feature(llvm_asm))]
#![deny(warnings)]

extern crate libc;
#[cfg(target_os = "windows")]
#[macro_use]
extern crate lazy_static;
#[cfg(target_os = "windows")]
extern crate winapi;

#[cfg(any(target_os = "macos", target_os = "ops"))]
extern crate mach;

#[derive(Debug, Clone, Copy)]
pub struct Clocksource {
    ref_id: Clock,
    ref_t0: u64,
    ref_hz: f64,
    src_id: Clock,
    src_t0: u64,
    src_hz: f64,
}

const ONE_GHZ: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Clock {
    Counter,
    Monotonic,
    Realtime,
}

#[inline(always)]
fn read(clock: &Clock) -> u64 {
    match *clock {
        Clock::Counter => rdtsc(),
        Clock::Monotonic => get_precise_ns(),
        Clock::Realtime => get_unix_time(),
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[inline(always)]
fn get_unix_time() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[inline(always)]
fn get_precise_ns() -> u64 {
    use mach::mach_time::{mach_absolute_time, mach_timebase_info};
    unsafe {
        let time = mach_absolute_time();
        let info = {
            static mut INFO: mach_timebase_info = mach_timebase_info { numer: 0, denom: 0 };

            // TODO: Replace with parking_lot::Once
            static ONCE: std::sync::Once = std::sync::Once::new();

            ONCE.call_once(|| {
                mach_timebase_info(&mut INFO);
            });
            &INFO
        };
        time * info.numer as u64 / info.denom as u64
    }
}

#[cfg(target_os = "windows")]
#[inline(always)]
fn get_unix_time() -> u64 {
    use std::mem;
    use winapi::shared::minwindef::FILETIME;
    use winapi::um::sysinfoapi;
    const OFFSET: u64 = 116_444_736_000_000_000; //1jan1601 to 1jan1970
    let mut file_time = unsafe {
        let mut file_time = mem::MaybeUninit::uninit();
        sysinfoapi::GetSystemTimePreciseAsFileTime(file_time.as_mut_ptr());
        (mem::transmute::<FILETIME, i64>(file_time.assume_init())) as u64
    };
    file_time -= OFFSET;
    file_time * 100
}

#[cfg(target_os = "windows")]
#[inline(always)]
fn get_precise_ns() -> u64 {
    use std::mem;
    use winapi::um::profileapi;
    use winapi::um::winnt::LARGE_INTEGER;
    lazy_static! {
        static ref PRF_FREQUENCY: u64 = {
            unsafe {
                let mut frq = mem::MaybeUninit::uninit();
                let res = profileapi::QueryPerformanceFrequency(frq.as_mut_ptr());
                debug_assert_ne!(res, 0, "Failed to query performance frequency, {}", res);
                let frq = *frq.assume_init().QuadPart() as u64;
                frq
            }
        };
    }
    let cnt = unsafe {
        let mut cnt = mem::MaybeUninit::uninit();
        debug_assert_eq!(mem::align_of::<LARGE_INTEGER>(), 8);
        let res = profileapi::QueryPerformanceCounter(cnt.as_mut_ptr());
        debug_assert_ne!(res, 0, "Failed to query performance counter {}", res);
        *cnt.assume_init().QuadPart() as u64
    };

    let cnt = cnt as f64 / (*PRF_FREQUENCY as f64 / 1_000_000_000_f64);
    cnt as u64
}

#[cfg(all(not(target_os = "macos"), not(target_os = "ios"), unix))]
#[inline(always)]
fn get_unix_time() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

#[cfg(all(not(target_os = "macos"), not(target_os = "ios"), unix))]
#[inline(always)]
fn get_precise_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "windows"),
    not(unix)
))]
#[macro_use]
extern crate lazy_static;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "windows"),
    not(unix)
))]
#[inline(always)]
fn get_unix_time() -> u64 {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_nanos() as u64,
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "windows"),
    not(unix)
))]
#[inline(always)]
fn get_precise_ns() -> u64 {
    use std::time::Instant;
    lazy_static! {
        static ref START: Instant = { Instant::now() };
    }
    START.elapsed().as_nanos() as u64
}

#[cfg(feature = "rdtsc")]
#[allow(unused_mut)]
#[inline(always)]
fn rdtsc() -> u64 {
    let mut l: u32;
    let mut m: u32;
    unsafe {
        llvm_asm!("lfence; rdtsc" : "={eax}" (l), "={edx}" (m) ::: "volatile");
    }
    ((m as u64) << 32) | (l as u64)
}

#[cfg(not(feature = "rdtsc"))]
fn rdtsc() -> u64 {
    panic!("Clock::Counter requires 'rdtsc' feature");
}

impl Default for Clocksource {
    #[inline]
    fn default() -> Clocksource {
        if cfg!(feature = "rdtsc") {
            Clocksource::configured(Clock::Monotonic, Clock::Counter)
        } else {
            Clocksource::configured(Clock::Monotonic, Clock::Monotonic)
        }
    }
}

impl Clocksource {
    /// create a new clocksource
    #[inline]
    pub fn new() -> Clocksource {
        Default::default()
    }

    /// allows manual configuration of the `Clocksource` and performs initial calibration
    pub fn configured(reference: Clock, source: Clock) -> Clocksource {
        let mut cs = Clocksource {
            ref_id: reference,
            ref_t0: 0,
            ref_hz: ONE_GHZ,
            src_id: source,
            src_t0: 0,
            src_hz: ONE_GHZ,
        };
        cs.calibrate();
        cs
    }

    /// get the approximate frequency of the source clock in Hz
    #[inline(always)]
    pub fn frequency(&self) -> f64 {
        self.src_hz
    }

    /// get the raw counter reading of the source clock
    #[inline(always)]
    pub fn counter(&self) -> u64 {
        read(&self.src_id)
    }

    /// get nanoseconds from the reference clock
    #[inline(always)]
    pub fn reference(&self) -> u64 {
        read(&self.ref_id)
    }

    /// get the nanoseconds from the source clock
    #[inline(always)]
    pub fn time(&self) -> u64 {
        let raw = self.counter();
        if self.src_id != self.ref_id {
            self.convert(raw) as u64
        } else {
            raw
        }
    }

    /// calibrate the source frequency against the reference
    pub fn calibrate(&mut self) {
        self.ref_t0 = self.reference();
        self.src_t0 = self.counter();

        let ref_t1 = self.ref_t0 + self.ref_hz as u64;

        loop {
            let t = self.reference();
            if t >= ref_t1 {
                break;
            }
        }

        let src_t1 = self.counter();

        let ref_d = ref_t1 - self.ref_t0;
        let src_d = src_t1 - self.src_t0;

        self.src_hz = src_d as f64 * self.ref_hz / ref_d as f64;
    }

    /// recalculate the frequency, without changing the reference time
    pub fn recalibrate(&mut self) {
        let ref_t1 = self.reference();
        let src_t1 = self.counter();

        let ref_d = ref_t1.wrapping_sub(self.ref_t0);
        let src_d = src_t1.wrapping_sub(self.src_t0);

        self.src_hz = src_d as f64 * self.ref_hz / ref_d as f64;
    }

    /// estimate of the phase error between the source and reference clocksource
    pub fn phase_error(&self) -> f64 {
        if self.src_id == self.ref_id {
            return 0.0;
        }

        let ref_t1 = self.reference();
        let src_t1 = self.time();

        (src_t1 as i64 - ref_t1 as i64) as f64 / self.ref_hz
    }

    /// converts a raw reading to approximation of reference in nanoseconds
    #[inline(always)]
    pub fn convert(&self, src_t1: u64) -> f64 {
        if self.src_id != self.ref_id {
            (self.ref_hz * ((src_t1 - self.src_t0) as f64 / self.src_hz)) + self.ref_t0 as f64
        } else {
            src_t1 as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw() {
        let cs = Clocksource::default();
        let a = cs.counter();
        let b = cs.counter();
        assert!(b >= a);
    }

    #[test]
    fn test_reference() {
        let cs = Clocksource::default();
        let a = cs.reference();
        let b = cs.reference();
        assert!(b >= a);
    }

    #[test]
    fn test_source() {
        let cs = Clocksource::default();
        let ref_0 = cs.reference();
        let src_0 = cs.time();

        let dt = src_0 as f64 - ref_0 as f64;
        let pe = cs.phase_error();

        // assert that we're within 1 microsecond
        assert!(dt < 1000.0);
        assert!(dt > -1000.0);
        assert!(pe < 1000.0);
        assert!(pe > -1000.0);
    }
}
