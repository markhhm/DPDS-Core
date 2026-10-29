//! Normalized sample-and-hold delay/loss and explicit outage actions.
use crate::hng::Probability;
use serde::Deserialize;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutageAction {
    Drop,
    Hold,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPolicy {
    Error,
    HoldLastSample,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceProfile {
    pub end_ns: u64,
    pub rows: Vec<TraceRow>,
    pub outage_action: OutageAction,
    pub terminal: TerminalPolicy,
    pub loss_seed: u64,
    pub terminal_horizon_ns: Option<u64>,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceRow {
    pub at_ns: u64,
    pub t_ns: u64,
    pub loss: Probability,
    pub outage: bool,
}

impl TraceProfile {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.rows.is_empty()
            || self.rows[0].at_ns != 0
            || self.rows.last().unwrap().at_ns >= self.end_ns
            || self.rows.windows(2).any(|p| p[0].at_ns >= p[1].at_ns)
            || self
                .rows
                .iter()
                .any(|r| r.t_ns.checked_add(self.end_ns).is_none())
        {
            return Err("trace needs increasing rows from zero, a finite end and safe delays");
        }
        if self.rows.last().unwrap().outage
            && (self.terminal == TerminalPolicy::HoldLastSample
                || self.outage_action == OutageAction::Hold)
        {
            return Err("outage hold needs a finite up recovery; terminal HOLD must be up");
        }
        if self.terminal == TerminalPolicy::HoldLastSample
            && self.terminal_horizon_ns.is_none_or(|h| h <= self.end_ns)
        {
            return Err("hold_last_sample needs a finite terminal_horizon_ns after end_ns");
        }
        Ok(())
    }
    pub fn max_delay_ns(&self) -> u64 {
        self.rows.iter().map(|r| r.t_ns).max().unwrap_or(0)
    }
    pub fn max_hold_ns(&self) -> u64 {
        let mut start = None;
        let mut longest = 0;
        for r in &self.rows {
            if r.outage {
                start.get_or_insert(r.at_ns);
            } else if let Some(s) = start.take() {
                longest = longest.max(r.at_ns - s);
            }
        }
        longest
    }
}

#[derive(Clone)]
pub(crate) struct TracePolicy {
    pub profile: TraceProfile,
    next_up: Vec<Option<u64>>,
}

impl TracePolicy {
    pub fn new(profile: TraceProfile) -> Result<Self, &'static str> {
        profile.validate()?;
        let mut next_up = vec![None; profile.rows.len()];
        let mut up = None;
        for (i, row) in profile.rows.iter().enumerate().rev() {
            if !row.outage {
                up = Some(row.at_ns);
            }
            next_up[i] = up;
        }
        Ok(Self { profile, next_up })
    }

    pub fn index(&self, ns: u64) -> Result<usize, &'static str> {
        if (ns >= self.profile.end_ns && self.profile.terminal == TerminalPolicy::Error)
            || self.profile.terminal_horizon_ns.is_some_and(|h| ns >= h)
        {
            return Err("trace coverage exhausted without terminal HOLD");
        }
        Ok(self
            .profile
            .rows
            .partition_point(|r| r.at_ns <= ns)
            .saturating_sub(1))
    }

    pub fn release(&self, deadline_ns: u64) -> Result<Option<u64>, &'static str> {
        let i = self.index(deadline_ns)?;
        if !self.profile.rows[i].outage {
            return Ok(Some(deadline_ns));
        }
        match self.profile.outage_action {
            OutageAction::Drop => Ok(None),
            OutageAction::Hold => self.next_up[i]
                .map(Some)
                .ok_or("outage has no finite recovery"),
        }
    }

    pub fn gate(&self, now_ns: u64) -> u32 {
        match self.index(now_ns) {
            Err(_) => 3,
            Ok(i) if !self.profile.rows[i].outage => 0,
            Ok(_) if self.profile.outage_action == OutageAction::Hold => 1,
            Ok(_) => 2,
        }
    }
}

/// Independent immutable copy for the TX owner; no concurrent Engine borrow.
#[derive(Clone)]
pub struct TxPolicy {
    pub(crate) trace: Option<TracePolicy>,
}
impl TxPolicy {
    pub fn gate(&self, now_ns: u64) -> u32 {
        self.trace.as_ref().map_or(0, |t| t.gate(now_ns))
    }
}
