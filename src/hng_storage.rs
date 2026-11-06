// SPDX-License-Identifier: Apache-2.0
//! Explicit M7 registration. Absent registrations use the original envelope.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageClass {
    pub l2_bytes: u32,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M7StorageRegistration {
    pub mode: String,
    #[serde(default = "default_threshold")]
    pub threshold_l2: u32,
    #[serde(default = "default_hold")]
    pub hold_ns: u64,
    #[serde(default = "default_hold")]
    pub window_ns: u64,
    #[serde(default = "default_rate")]
    pub offered_bps: u64,
    #[serde(default = "default_numerator")]
    pub margin_numerator: u64,
    #[serde(default = "default_denominator")]
    pub margin_denominator: u64,
    pub composition: Vec<StorageClass>,
}
fn default_threshold() -> u32 {
    1024
}
fn default_hold() -> u64 {
    10_000_000
}
fn default_rate() -> u64 {
    100_000_000_000
}
fn default_numerator() -> u64 {
    5
}
fn default_denominator() -> u64 {
    4
}
impl M7StorageRegistration {
    pub fn validate(&self) -> Result<(), String> {
        if self.mode != "m7-hybrid-v1"
            || !(64..=1522).contains(&self.threshold_l2)
            || self.hold_ns == 0
            || self.window_ns != 10_000_000
            || self.offered_bps == 0
            || self.offered_bps > 100_000_000_000
            || self.margin_numerator != 5
            || self.margin_denominator != 4
            || self.composition.is_empty()
            || self.composition.len() > 16
        {
            return Err(
                "invalid M7 mode, threshold, hold, rate, window or margin registration".into(),
            );
        }
        let mut seen = Vec::new();
        let mut weights = 0u64;
        for class in &self.composition {
            if !(64..=1522).contains(&class.l2_bytes)
                || class.weight == 0
                || seen.contains(&class.l2_bytes)
            {
                return Err("invalid or duplicate M7 frame class".into());
            }
            seen.push(class.l2_bytes);
            weights += u64::from(class.weight);
        }
        if weights > 65535 {
            return Err("M7 class weight sum exceeds 65535".into());
        }
        Ok(())
    }
}

/// Physical reserves and additional W-derived live limits. No packet allocation.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct M7StorageInfo {
    pub threshold_l2: u32,
    pub class_count: u32,
    pub lengths: [u32; 16],
    pub hold_ns: u64,
    pub window_ns: u64,
    pub slots: u64,
    pub records: u64,
    pub cells: u64,
    pub normal_mbufs: u64,
    pub slot_limit: u64,
    pub record_limit: u64,
    pub cell_limit: u64,
    pub normal_limit: u64,
}

// Checked exact fractions; malformed or unrepresentable registrations fail
// before prefaulting memory. No floating-point sizing or new dependency.
#[derive(Clone, Copy)]
struct Ratio {
    n: u128,
    d: u128,
}
fn overflow() -> String {
    "M7 quota arithmetic overflow".into()
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
impl Ratio {
    fn new(n: u128, d: u128) -> Self {
        assert_ne!(d, 0);
        let g = gcd(n, d);
        Self { n: n / g, d: d / g }
    }
    fn mul(self, other: Self) -> Result<Self, String> {
        let a = gcd(self.n, other.d);
        let b = gcd(other.n, self.d);
        Ok(Self::new(
            (self.n / a).checked_mul(other.n / b).ok_or_else(overflow)?,
            (self.d / b).checked_mul(other.d / a).ok_or_else(overflow)?,
        ))
    }
    fn add(self, other: Self) -> Result<Self, String> {
        let g = gcd(self.d, other.d);
        let n = self
            .n
            .checked_mul(other.d / g)
            .and_then(|a| {
                other
                    .n
                    .checked_mul(self.d / g)
                    .and_then(|b| a.checked_add(b))
            })
            .ok_or_else(overflow)?;
        Ok(Self::new(
            n,
            self.d.checked_mul(other.d / g).ok_or_else(overflow)?,
        ))
    }
    fn sub(self, other: Self) -> Result<Self, String> {
        let g = gcd(self.d, other.d);
        let n = self
            .n
            .checked_mul(other.d / g)
            .and_then(|a| {
                other
                    .n
                    .checked_mul(self.d / g)
                    .and_then(|b| a.checked_sub(b))
            })
            .ok_or_else(overflow)?;
        Ok(Self::new(
            n,
            self.d.checked_mul(other.d / g).ok_or_else(overflow)?,
        ))
    }
    fn min(self, other: Self) -> Result<Self, String> {
        let a = self.n.checked_mul(other.d).ok_or_else(overflow)?;
        let b = other.n.checked_mul(self.d).ok_or_else(overflow)?;
        Ok(if a <= b { self } else { other })
    }
    fn ceil(self) -> Result<u64, String> {
        u64::try_from(self.n.div_ceil(self.d)).map_err(|_| overflow())
    }
}
impl M7StorageRegistration {
    /// C is bit/s, Q charged L2 bytes, and all times are ns.
    pub fn plan(
        &self,
        c_bps: u64,
        q_bytes: u64,
        delay_ns: u64,
        burst: u64,
    ) -> Result<M7StorageInfo, String> {
        self.validate()?;
        if c_bps == 0 || c_bps > self.offered_bps || burst == 0 {
            return Err("invalid M7 service or burst".into());
        }
        let residence = delay_ns.checked_add(self.hold_ns).ok_or_else(overflow)?;
        let wire = Ratio::new(u128::from(c_bps) * u128::from(residence), 8_000_000_000);
        let q = Ratio::new(q_bytes.into(), 1);
        let episode = Ratio::new(
            u128::from(self.offered_bps) * u128::from(self.window_ns),
            8_000_000_000,
        );
        let bd = wire.min(episode)?;
        let bq = q.min(episode)?;
        let sum_l: u128 = self
            .composition
            .iter()
            .map(|a| u128::from(a.weight) * u128::from(a.l2_bytes))
            .sum();
        let sum_w: u128 = self
            .composition
            .iter()
            .map(|a| u128::from(a.weight) * u128::from(a.l2_bytes + 20))
            .sum();
        let mut physical = [0; 4];
        let mut limits = [0; 4];
        for resource in 0..4 {
            let costs: Vec<u128> = self
                .composition
                .iter()
                .map(|a| {
                    let compact = a.l2_bytes <= self.threshold_l2;
                    match resource {
                        0 => 1,
                        1 => u128::from(compact),
                        2 => {
                            if compact {
                                u128::from((a.l2_bytes - 4).div_ceil(64))
                            } else {
                                0
                            }
                        }
                        _ => u128::from(!compact),
                    }
                })
                .collect();
            let sr: u128 = costs
                .iter()
                .zip(&self.composition)
                .map(|(r, c)| r * u128::from(c.weight))
                .sum();
            let aw = Ratio::new(sr, sum_w);
            let al = Ratio::new(sr, sum_l);
            let nominal = wire.mul(aw)?.add(q.mul(al)?)?;
            let burst_cost =
                Ratio::new(u128::from(burst) * costs.iter().copied().max().unwrap(), 1);
            physical[resource] = nominal
                .mul(Ratio::new(
                    self.margin_numerator.into(),
                    self.margin_denominator.into(),
                ))?
                .add(burst_cost)?
                .ceil()?;
            let mut maxw = Ratio::new(0, 1);
            let mut maxl = maxw;
            for (r, c) in costs.iter().zip(&self.composition) {
                let rw = Ratio::new(*r, u128::from(c.l2_bytes + 20));
                let rl = Ratio::new(*r, u128::from(c.l2_bytes));
                if rw.n * maxw.d > maxw.n * rw.d {
                    maxw = rw;
                }
                if rl.n * maxl.d > maxl.n * rl.d {
                    maxl = rl;
                }
            }
            let skew = wire
                .sub(bd)?
                .mul(aw)?
                .add(bd.mul(maxw)?)?
                .add(q.sub(bq)?.mul(al)?)?
                .add(bq.mul(maxl)?)?
                .add(burst_cost)?
                .ceil()?;
            limits[resource] = physical[resource].min(skew);
        }
        if physical.iter().any(|n| *n >= u64::from(u32::MAX)) {
            return Err("M7 resource count exceeds bounded index range".into());
        }
        let mut out = M7StorageInfo {
            threshold_l2: self.threshold_l2,
            class_count: self.composition.len() as u32,
            hold_ns: self.hold_ns,
            window_ns: self.window_ns,
            slots: physical[0],
            records: physical[1],
            cells: physical[2],
            normal_mbufs: physical[3],
            slot_limit: limits[0],
            record_limit: limits[1],
            cell_limit: limits[2],
            normal_limit: limits[3],
            ..Default::default()
        };
        for (dst, class) in out.lengths.iter_mut().zip(&self.composition) {
            *dst = class.l2_bytes;
        }
        Ok(out)
    }
}

/// Calculate a registration without constructing or prefaulting an engine.
/// Failure leaves `out` unchanged.
/// # Safety
/// `registration` is NULL or a live NUL-terminated UTF-8 string. `out` is
/// NULL or points to writable M7StorageInfo for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn hng_m7_storage_plan(
    registration: *const std::ffi::c_char,
    c_bps: u64,
    q_bytes: u64,
    delay_ns: u64,
    burst: u64,
    out: *mut M7StorageInfo,
) -> bool {
    if registration.is_null() || out.is_null() {
        return false;
    }
    let result = (|| {
        let json = std::ffi::CStr::from_ptr(registration)
            .to_str()
            .map_err(|e| e.to_string())?;
        let config: M7StorageRegistration =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        config.plan(c_bps, q_bytes, delay_ns, burst)
    })();
    match result {
        Ok(info) => {
            *out = info;
            true
        }
        Err(_) => false,
    }
}

/// New storage takes payload custody only by returning a non-NULL pointer.
/// Called after all model rejection gates, before service/Q/payload commit.
pub type HngStorageCommit = Option<
    unsafe extern "C" fn(
        record: *const crate::hng::HngPacket,
        context: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void,
>;
