use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{AccountingPolicy, ConfigError, DeploymentConfig, DeploymentStore};
use crate::runtime::Runtime;

const STATE_SCHEMA_VERSION: u32 = 2;
const PENDING_PERIOD_IDENTITY: &str = "pending-first-reset";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrafficState {
    schema_version: u32,
    pub cycle_key: String,
    pub interface: String,
    pub baseline_rx: u64,
    pub baseline_tx: u64,
    pub accumulated_rx: u64,
    pub accumulated_tx: u64,
    boot_id: String,
    pub corrections: Vec<CorrectionRecord>,
}

/// A persisted manual correction to the current accounting period.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CorrectionRecord {
    /// A total-only adjustment applied to reported VPS traffic without
    /// fabricating RX/TX direction values.
    TotalAdjustment { bytes: u64, at: DateTime<Utc> },
    /// A direction-aware correction setting the reported RX and TX totals.
    SetDirection { rx: u64, tx: u64, at: DateTime<Utc> },
}

impl TrafficState {
    pub fn new(
        cycle_key: impl Into<String>,
        interface: impl Into<String>,
        boot_id: impl Into<String>,
        rx: u64,
        tx: u64,
    ) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            cycle_key: cycle_key.into(),
            interface: interface.into(),
            baseline_rx: rx,
            baseline_tx: tx,
            accumulated_rx: 0,
            accumulated_tx: 0,
            boot_id: boot_id.into(),
            corrections: Vec::new(),
        }
    }

    /// Accumulate counter deltas since the established baseline, preserving
    /// accumulated traffic when the boot ID changes or a counter rolls back.
    pub fn accumulate(mut self, boot_id: &str, rx: u64, tx: u64) -> Self {
        if self.boot_id == boot_id {
            if rx >= self.baseline_rx {
                self.accumulated_rx += rx - self.baseline_rx;
            }
            if tx >= self.baseline_tx {
                self.accumulated_tx += tx - self.baseline_tx;
            }
        }
        self.baseline_rx = rx;
        self.baseline_tx = tx;
        self.boot_id = boot_id.to_owned();
        self
    }

    /// The reported RX accumulated in the current period, including any
    /// direction-aware correction reconciled into the accumulated baseline.
    pub fn reported_rx(&self) -> u64 {
        self.accumulated_rx
    }

    /// The reported TX accumulated in the current period, including any
    /// direction-aware correction reconciled into the accumulated baseline.
    pub fn reported_tx(&self) -> u64 {
        self.accumulated_tx
    }

    /// The live reported direction values for a read: accumulated traffic plus
    /// counter deltas since the persisted baseline. A boot ID change or counter
    /// rollback preserves the accumulated values and keeps the other direction's
    /// valid delta. A direction-aware correction is reconciled into the
    /// accumulated baseline, so later counter deltas continue to accumulate on
    /// top of the corrected value. This is a pure read and never mutates state.
    pub fn live_reported(&self, boot_id: &str, rx: u64, tx: u64) -> (u64, u64) {
        let delta_rx = if self.boot_id == boot_id && rx >= self.baseline_rx {
            rx - self.baseline_rx
        } else {
            0
        };
        let delta_tx = if self.boot_id == boot_id && tx >= self.baseline_tx {
            tx - self.baseline_tx
        } else {
            0
        };
        (
            self.accumulated_rx + delta_rx,
            self.accumulated_tx + delta_tx,
        )
    }

    /// The total-only adjustments that add to reported traffic without
    /// changing the measured RX/TX direction values.
    pub fn total_adjustment(&self) -> u64 {
        self.corrections
            .iter()
            .map(|correction| match correction {
                CorrectionRecord::TotalAdjustment { bytes, .. } => *bytes,
                CorrectionRecord::SetDirection { .. } => 0,
            })
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrafficReport {
    pub interface: String,
    pub received: u64,
    pub transmitted: u64,
    pub total_adjustment: u64,
    pub monthly_traffic_limit: u64,
    pub accounting_period: String,
    pub next_reset: DateTime<Utc>,
}

impl TrafficReport {
    pub fn total(&self) -> u64 {
        self.received + self.transmitted + self.total_adjustment
    }

    pub fn summary(&self) -> String {
        [
            "VPS traffic".to_owned(),
            format!("interface: {}", self.interface),
            format!("received: {} bytes", self.received),
            format!("transmitted: {} bytes", self.transmitted),
            format!("total: {} bytes", self.total()),
            format!(
                "monthly traffic limit: {} bytes",
                self.monthly_traffic_limit
            ),
            format!("accounting period: {}", self.accounting_period),
            format!("next reset: {}", self.next_reset.to_rfc3339()),
        ]
        .join("\n")
    }
}

/// The administrator-authored correction requested by `sbctl traffic set-used`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionTarget {
    /// Set the reported total VPS traffic; stored as a total-only adjustment
    /// that never fabricates RX/TX direction values.
    Total(u64),
    /// Set the reported RX and TX direction values independently, reconciling
    /// the accounting baseline so later counter deltas accumulate on top.
    Directions { rx: u64, tx: u64 },
}

/// The change summary shown before a traffic correction is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrectionPreview {
    pub accounting_period: String,
    pub next_reset: DateTime<Utc>,
    pub current_received: u64,
    pub current_transmitted: u64,
    pub current_total: u64,
    pub target_received: u64,
    pub target_transmitted: u64,
    pub target_total: u64,
}

impl CorrectionPreview {
    pub fn summary(&self) -> String {
        [
            "VPS traffic correction".to_owned(),
            format!("accounting period: {}", self.accounting_period),
            format!("current received: {} bytes", self.current_received),
            format!("current transmitted: {} bytes", self.current_transmitted),
            format!("current total: {} bytes", self.current_total),
            format!("target received: {} bytes", self.target_received),
            format!("target transmitted: {} bytes", self.target_transmitted),
            format!("target total: {} bytes", self.target_total),
            format!("next reset: {}", self.next_reset.to_rfc3339()),
        ]
        .join("\n")
    }
}

#[derive(Debug, Error)]
pub enum TrafficError {
    #[error("could not read VPS traffic interface counters: {0}")]
    Counters(#[from] io::Error),
    #[error("VPS traffic interface counter is not an unsigned byte count: {0}")]
    InvalidCounter(String),
    #[error("could not read VPS boot ID: {0}")]
    BootId(io::Error),
    #[error("VPS traffic storage failed: {0}")]
    Storage(#[from] ConfigError),
    #[error("invalid accounting schedule: {0}")]
    Schedule(&'static str),
    #[error("accounting state has not been established for the current period")]
    StateMissing,
    #[error(
        "accounting state belongs to a previous period; the reset task has not run for the current period"
    )]
    StateStale,
    #[error("traffic correction cannot apply before the first reset")]
    PendingFirstReset,
    #[error(
        "total correction target {target} bytes is below the currently reported total {current} bytes"
    )]
    TotalTooLow { target: u64, current: u64 },
    #[error("reported traffic would overflow a byte count: {0}")]
    Overflow(&'static str),
}

pub fn reset(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<TrafficReport, TrafficError> {
    reset_with_runtime(store, config, &Runtime::live(store.root()))
}

pub fn reset_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<TrafficReport, TrafficError> {
    reset_with_runtime(store, config, &Runtime::fixture(store.root(), now))
}

/// The accounting reset task, the authorized writer for accounting state. It
/// establishes a new period baseline when the cycle key or interface changes,
/// and otherwise accumulates measured counter deltas into the persisted state.
/// Production callers run it from `sbctl-accounting-reset.timer`; tests use a
/// fixed clock and fixture root without mocking the accounting algorithm.
pub fn reset_with_runtime<C: crate::runtime::Clock>(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    runtime: &Runtime<C>,
) -> Result<TrafficReport, TrafficError> {
    let period = accounting_period(config, runtime.now_utc())?;
    if period.pending() {
        return Ok(pending_report(config, &period));
    }
    let measurements = read_measurements(runtime, &config.interface)?;
    let mut reconciled = None;
    store.update_state(|prior| {
        let state = match prior {
            Some(contents) => decode_state(&contents)?,
            None => TrafficState::new(
                period.cycle_key(),
                &config.interface,
                &measurements.boot_id,
                measurements.rx,
                measurements.tx,
            ),
        };
        let state = if state.cycle_key != period.cycle_key() || state.interface != config.interface
        {
            TrafficState::new(
                period.cycle_key(),
                &config.interface,
                &measurements.boot_id,
                measurements.rx,
                measurements.tx,
            )
        } else {
            state.accumulate(&measurements.boot_id, measurements.rx, measurements.tx)
        };
        let contents = serde_json::to_vec(&state)
            .map_err(|error| ConfigError::StateContent(error.to_string()))?;
        reconciled = Some(state);
        Ok(contents)
    })?;
    let state = reconciled.expect("active period sets state in the successful state transaction");
    Ok(report_from_state(config, &period, &state, &measurements))
}

pub fn report(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<TrafficReport, TrafficError> {
    report_with_runtime(store, config, &Runtime::live(store.root()))
}

pub fn report_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<TrafficReport, TrafficError> {
    report_with_runtime(store, config, &Runtime::fixture(store.root(), now))
}

/// Read the current accounting period without writing accounting state. Used by
/// `sbctl traffic`, `sbctl status`, and subscription requests; the periodic
/// accounting reset task is the only writer that keeps the state current.
pub fn report_with_runtime<C: crate::runtime::Clock>(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    runtime: &Runtime<C>,
) -> Result<TrafficReport, TrafficError> {
    let period = accounting_period(config, runtime.now_utc())?;
    if period.pending() {
        return Ok(pending_report(config, &period));
    }
    let state = store
        .read_state()?
        .ok_or(TrafficError::StateMissing)
        .and_then(|contents| decode_state(&contents).map_err(TrafficError::Storage))?;
    if state.cycle_key != period.cycle_key() || state.interface != config.interface {
        return Err(TrafficError::StateStale);
    }
    let measurements = read_measurements(runtime, &config.interface)?;
    Ok(report_from_state(config, &period, &state, &measurements))
}

pub fn set_used(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    target: CorrectionTarget,
) -> Result<CorrectionPreview, TrafficError> {
    set_used_with_runtime(store, config, &Runtime::live(store.root()), target)
}

pub fn set_used_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
    target: CorrectionTarget,
) -> Result<CorrectionPreview, TrafficError> {
    set_used_with_runtime(store, config, &Runtime::fixture(store.root(), now), target)
}

/// An explicit administrator traffic correction, one of the two authorized
/// writers for accounting state. It shows the change summary, then commits the
/// correction atomically under the same operation lock as the accounting reset
/// task so a concurrent reset and correction are serialized. A total-only
/// correction appends a total adjustment; a direction-aware correction
/// reconciles the accumulated baseline to the requested RX/TX so later counter
/// deltas accumulate on top. Any validation failure leaves the old state
/// untouched.
pub fn set_used_with_runtime<C: crate::runtime::Clock>(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    runtime: &Runtime<C>,
    target: CorrectionTarget,
) -> Result<CorrectionPreview, TrafficError> {
    let period = accounting_period(config, runtime.now_utc())?;
    if period.pending() {
        return Err(TrafficError::PendingFirstReset);
    }
    let _lock = store
        .acquire_operation_lock()
        .map_err(TrafficError::Storage)?;
    let prior = store.read_state()?.ok_or(TrafficError::StateMissing)?;
    let state = decode_state(&prior).map_err(TrafficError::Storage)?;
    if state.cycle_key != period.cycle_key() || state.interface != config.interface {
        return Err(TrafficError::StateStale);
    }
    let measurements = read_measurements(runtime, &config.interface)?;
    let plan = plan_correction(&state, &period, &measurements, runtime.now_utc(), target)?;
    println!("{}", plan.preview.summary());
    let contents = serde_json::to_vec(&plan.state)
        .map_err(|error| ConfigError::StateContent(error.to_string()))?;
    store.write_relative_locked(crate::config::STATE_RELATIVE_PATH, &contents)?;
    Ok(plan.preview)
}

struct CorrectionPlan {
    state: TrafficState,
    preview: CorrectionPreview,
}

fn plan_correction(
    state: &TrafficState,
    period: &AccountingPeriod,
    measurements: &Measurements,
    now: DateTime<Utc>,
    target: CorrectionTarget,
) -> Result<CorrectionPlan, TrafficError> {
    let (current_received, current_transmitted) =
        state.live_reported(&measurements.boot_id, measurements.rx, measurements.tx);
    let current_total = current_received
        .checked_add(current_transmitted)
        .and_then(|total| total.checked_add(state.total_adjustment()))
        .ok_or(TrafficError::Overflow("current reported total"))?;
    let mut corrected = state.clone();
    let (target_received, target_transmitted, target_total) = match target {
        CorrectionTarget::Total(total) => {
            if total < current_total {
                return Err(TrafficError::TotalTooLow {
                    target: total,
                    current: current_total,
                });
            }
            corrected
                .corrections
                .push(CorrectionRecord::TotalAdjustment {
                    bytes: total - current_total,
                    at: now,
                });
            (current_received, current_transmitted, total)
        }
        CorrectionTarget::Directions { rx, tx } => {
            corrected.accumulated_rx = rx;
            corrected.accumulated_tx = tx;
            corrected.baseline_rx = measurements.rx;
            corrected.baseline_tx = measurements.tx;
            corrected.boot_id = measurements.boot_id.clone();
            corrected
                .corrections
                .push(CorrectionRecord::SetDirection { rx, tx, at: now });
            let target_total = rx
                .checked_add(tx)
                .and_then(|total| total.checked_add(corrected.total_adjustment()))
                .ok_or(TrafficError::Overflow("target reported total"))?;
            (rx, tx, target_total)
        }
    };
    Ok(CorrectionPlan {
        state: corrected,
        preview: CorrectionPreview {
            accounting_period: period.identity().to_owned(),
            next_reset: period.next_reset,
            current_received,
            current_transmitted,
            current_total,
            target_received,
            target_transmitted,
            target_total,
        },
    })
}

fn pending_report(config: &DeploymentConfig, period: &AccountingPeriod) -> TrafficReport {
    TrafficReport {
        interface: config.interface.clone(),
        received: 0,
        transmitted: 0,
        total_adjustment: 0,
        monthly_traffic_limit: config.monthly_traffic_limit,
        accounting_period: period.identity().to_owned(),
        next_reset: period.next_reset,
    }
}

fn report_from_state(
    config: &DeploymentConfig,
    period: &AccountingPeriod,
    state: &TrafficState,
    measurements: &Measurements,
) -> TrafficReport {
    let (received, transmitted) =
        state.live_reported(&measurements.boot_id, measurements.rx, measurements.tx);
    TrafficReport {
        interface: config.interface.clone(),
        received,
        transmitted,
        total_adjustment: state.total_adjustment(),
        monthly_traffic_limit: config.monthly_traffic_limit,
        accounting_period: period.identity().to_owned(),
        next_reset: period.next_reset,
    }
}

struct Measurements {
    rx: u64,
    tx: u64,
    boot_id: String,
}

fn read_measurements<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
    interface: &str,
) -> Result<Measurements, TrafficError> {
    let (rx, tx) = read_interface_counters(runtime, interface)?;
    let boot_id = runtime
        .read_to_string("proc/sys/kernel/random/boot_id")
        .map_err(TrafficError::BootId)?
        .trim()
        .to_owned();
    Ok(Measurements { rx, tx, boot_id })
}

fn decode_state(contents: &[u8]) -> Result<TrafficState, ConfigError> {
    let state = serde_json::from_slice::<TrafficState>(contents)
        .map_err(|error| ConfigError::StateCorrupt(error.to_string()))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(ConfigError::StateSchemaMismatch(state.schema_version));
    }
    Ok(state)
}

pub fn detect_default_route_interface(root: &Path) -> Result<String, TrafficError> {
    let routes = fs::read_to_string(root.join("proc/net/route"))?;
    routes
        .lines()
        .skip(1)
        .find_map(|line| {
            let columns = line.split_whitespace().collect::<Vec<_>>();
            (columns.len() >= 4
                && columns[1] == "00000000"
                && (u32::from_str_radix(columns[3], 16).ok()? & 2) != 0)
                .then(|| columns[0].to_owned())
        })
        .ok_or(TrafficError::Schedule(
            "no default-route interface was found",
        ))
}

/// Whether the named interface is present on this host. The wizard validates
/// the selected traffic interface against the host before committing a
/// configuration, so a typo does not replace a running deployment.
pub fn interface_exists(root: &Path, interface: &str) -> bool {
    root.join("sys/class/net").join(interface).is_dir()
}

fn read_interface_counters<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
    interface: &str,
) -> Result<(u64, u64), TrafficError> {
    let statistics = Path::new("sys/class/net")
        .join(interface)
        .join("statistics");
    Ok((
        read_counter(
            &runtime.read_to_string(statistics.join("rx_bytes"))?,
            "rx_bytes",
        )?,
        read_counter(
            &runtime.read_to_string(statistics.join("tx_bytes"))?,
            "tx_bytes",
        )?,
    ))
}

fn read_counter(contents: &str, name: &str) -> Result<u64, TrafficError> {
    contents
        .trim()
        .parse()
        .map_err(|_| TrafficError::InvalidCounter(name.to_owned()))
}

struct AccountingPeriod {
    cycle_key: String,
    identity: String,
    next_reset: DateTime<Utc>,
    pending: bool,
}

impl AccountingPeriod {
    fn active(identity: String, next_reset: DateTime<Utc>) -> Self {
        Self {
            cycle_key: identity.clone(),
            identity,
            next_reset,
            pending: false,
        }
    }

    fn pending_first_reset(first_reset: DateTime<Utc>) -> Self {
        Self {
            cycle_key: format!("pending:{}", first_reset.to_rfc3339()),
            identity: PENDING_PERIOD_IDENTITY.to_owned(),
            next_reset: first_reset,
            pending: true,
        }
    }

    fn cycle_key(&self) -> &str {
        &self.cycle_key
    }

    fn identity(&self) -> &str {
        &self.identity
    }

    fn pending(&self) -> bool {
        self.pending
    }
}

fn accounting_period(
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<AccountingPeriod, TrafficError> {
    let timezone = config
        .accounting_timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| TrafficError::Schedule("accounting timezone is invalid"))?;
    let local_now = now.with_timezone(&timezone);
    Ok(match config.accounting_policy {
        AccountingPolicy::NaturalMonth => {
            let start = local_datetime(timezone, local_now.year(), local_now.month(), 1, 0, 0)?;
            let (year, month) = next_month(local_now.year(), local_now.month());
            let next = local_datetime(timezone, year, month, 1, 0, 0)?;
            AccountingPeriod::active(start.to_rfc3339(), next.with_timezone(&Utc))
        }
        AccountingPolicy::AnchoredMonth => {
            let reset = config
                .anchored_reset_at
                .as_deref()
                .ok_or(TrafficError::Schedule("anchored reset is missing"))?;
            let reset = chrono::NaiveDateTime::parse_from_str(reset, "%Y-%m-%dT%H:%M")
                .map_err(|_| TrafficError::Schedule("anchored reset time is invalid"))?;
            let first_reset = local_datetime(
                timezone,
                reset.year(),
                reset.month(),
                reset.day(),
                reset.hour(),
                reset.minute(),
            )?;
            if local_now < first_reset {
                return Ok(AccountingPeriod::pending_first_reset(
                    first_reset.with_timezone(&Utc),
                ));
            }
            let candidate = anchored_datetime(
                timezone,
                local_now.year(),
                local_now.month(),
                reset.day(),
                reset.hour(),
                reset.minute(),
            )?;
            let start = if candidate > local_now {
                let (year, month) = previous_month(local_now.year(), local_now.month());
                anchored_datetime(
                    timezone,
                    year,
                    month,
                    reset.day(),
                    reset.hour(),
                    reset.minute(),
                )?
            } else {
                candidate
            };
            let (year, month) = next_month(start.year(), start.month());
            let next = anchored_datetime(
                timezone,
                year,
                month,
                reset.day(),
                reset.hour(),
                reset.minute(),
            )?;
            AccountingPeriod::active(start.to_rfc3339(), next.with_timezone(&Utc))
        }
    })
}

fn local_datetime(
    timezone: chrono_tz::Tz,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Result<DateTime<chrono_tz::Tz>, TrafficError> {
    match timezone.with_ymd_and_hms(year, month, day, hour, minute, 0) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(_, _) => Err(TrafficError::Schedule(
            "reset time is ambiguous in the accounting timezone",
        )),
        LocalResult::None => Err(TrafficError::Schedule(
            "reset time does not exist in the accounting timezone",
        )),
    }
}

fn anchored_datetime(
    timezone: chrono_tz::Tz,
    year: i32,
    month: u32,
    requested_day: u32,
    hour: u32,
    minute: u32,
) -> Result<DateTime<chrono_tz::Tz>, TrafficError> {
    let last_day = (NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or(TrafficError::Schedule("invalid reset month"))?
        + Duration::days(32))
    .with_day(1)
    .expect("first day exists")
        - Duration::days(1);
    local_datetime(
        timezone,
        year,
        month,
        requested_day.min(last_day.day()),
        hour,
        minute,
    )
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}
fn previous_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use std::fs;
    use tempfile::TempDir;

    use crate::config::{
        AccountingPolicy, ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol,
        SubscriptionMode,
    };

    use super::{
        CorrectionRecord, CorrectionTarget, TrafficState, accounting_period, report_at, reset_at,
        reset_with_runtime, set_used_at,
    };
    use crate::runtime::Runtime;

    #[test]
    fn accumulates_rx_and_tx_deltas_without_counting_the_first_observation() {
        let initial = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200);
        let reconciled = initial.accumulate("boot-a", 130, 260);
        assert_eq!(reconciled.accumulated_rx, 30);
        assert_eq!(reconciled.accumulated_tx, 60);
    }

    #[test]
    fn boot_id_changes_and_counter_decreases_preserve_prior_accumulation() {
        let mut state = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200)
            .accumulate("boot-a", 140, 250);
        state = state.accumulate("boot-b", 5, 7);
        state = state.accumulate("boot-b", 8, 12);
        assert_eq!((state.accumulated_rx, state.accumulated_tx), (43, 55));
    }

    #[test]
    fn a_counter_decrease_does_not_discard_the_other_direction_delta() {
        let state = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200)
            .accumulate("boot-a", 5, 240);

        assert_eq!((state.accumulated_rx, state.accumulated_tx), (0, 40));
    }

    #[test]
    fn anchored_month_uses_the_last_day_in_short_months() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-01-31T09:30".into());
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        let period = accounting_period(&config, now).unwrap();
        assert_eq!(period.identity(), "2024-01-31T09:30:00+00:00");
        assert_eq!(period.next_reset.to_rfc3339(), "2024-02-29T09:30:00+00:00");
    }

    #[test]
    fn anchored_month_before_the_first_reset_is_a_valid_pending_state() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-03-31T09:30".into());
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        let period = accounting_period(&config, now).unwrap();

        assert!(period.pending());
        assert_eq!(period.identity(), "pending-first-reset");
        assert_eq!(period.next_reset.to_rfc3339(), "2024-03-31T09:30:00+00:00");
    }

    #[test]
    fn natural_month_boundaries_are_correct_in_utc_and_non_utc_timezones() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::NaturalMonth;
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 12, 0, 0).unwrap();

        config.accounting_timezone = "UTC".into();
        let utc_period = accounting_period(&config, now).unwrap();
        assert_eq!(utc_period.identity(), "2024-02-01T00:00:00+00:00");
        assert_eq!(
            utc_period.next_reset.to_rfc3339(),
            "2024-03-01T00:00:00+00:00"
        );

        config.accounting_timezone = "Asia/Tokyo".into();
        let tokyo_period = accounting_period(&config, now).unwrap();
        assert_eq!(tokyo_period.identity(), "2024-02-01T00:00:00+09:00");
        assert_eq!(
            tokyo_period.next_reset.to_rfc3339(),
            "2024-02-29T15:00:00+00:00"
        );
    }

    #[test]
    fn a_nonexistent_dst_local_time_is_rejected() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.accounting_timezone = "America/New_York".into();
        config.anchored_reset_at = Some("2024-03-10T02:30".into());

        assert!(
            accounting_period(&config, Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap()).is_err()
        );
    }

    #[test]
    fn an_ambiguous_dst_local_time_is_rejected() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.accounting_timezone = "America/New_York".into();
        config.anchored_reset_at = Some("2024-11-03T01:30".into());

        assert!(
            accounting_period(&config, Utc.with_ymd_and_hms(2024, 10, 1, 0, 0, 0).unwrap())
                .is_err()
        );
    }

    #[test]
    fn pending_first_reset_reports_zero_usage_and_the_first_reset_instant() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-06-15T12:00".into());
        let now = Utc.with_ymd_and_hms(2024, 5, 1, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");

        let report = reset_at(&store, &config, now).unwrap();

        assert_eq!(report.received, 0);
        assert_eq!(report.transmitted, 0);
        assert_eq!(report.total(), 0);
        assert_eq!(report.accounting_period, "pending-first-reset");
        assert_eq!(report.next_reset.to_rfc3339(), "2024-06-15T12:00:00+00:00");
        assert!(!fixture.path().join("var/lib/sbctl/state.json").exists());
    }

    #[test]
    fn a_period_switch_establishes_a_new_baseline() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let january = Utc.with_ymd_and_hms(2024, 1, 20, 0, 0, 0).unwrap();
        let february = Utc.with_ymd_and_hms(2024, 2, 5, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reset_at(&store, &config, january).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reset_at(&store, &config, january).unwrap().total(), 90);

        write_interface_fixture(&fixture, 400, 500, "boot-a");
        let report = reset_at(&store, &config, february).unwrap();

        assert_eq!(report.total(), 0);
        assert_eq!(report.accounting_period, "2024-02-01T00:00:00+00:00");
        let state: TrafficState = serde_json::from_str(
            &fs::read_to_string(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state.cycle_key, "2024-02-01T00:00:00+00:00");
        assert_eq!((state.baseline_rx, state.baseline_tx), (400, 500));
        assert_eq!((state.accumulated_rx, state.accumulated_tx), (0, 0));
    }

    #[test]
    fn an_interface_change_establishes_a_new_period() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);

        config.interface = "eth1".into();
        write_interface_fixture(&fixture, 1000, 2000, "boot-a");
        write_interface_fixture_named(&fixture, "eth1", 5, 7, "boot-a");
        let report = reset_at(&store, &config, now).unwrap();

        assert_eq!(report.total(), 0);
        assert_eq!(report.interface, "eth1");
    }

    #[test]
    fn a_schema_mismatch_is_a_diagnosable_error() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        store
            .write_state(br#"{"schema_version":1,"cycle_key":"2024-02-01T00:00:00+00:00","interface":"ens3","baseline_rx":0,"baseline_tx":0,"accumulated_rx":0,"accumulated_tx":0,"boot_id":"boot-a","corrections":[]}"#)
            .unwrap();

        let error = reset_at(&store, &config, now).unwrap_err();

        assert!(matches!(
            error,
            super::TrafficError::Storage(ConfigError::StateSchemaMismatch(1))
        ));
    }

    #[test]
    fn corrupted_state_is_a_diagnosable_error() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        store.write_state(b"not json").unwrap();

        let error = reset_at(&store, &config, now).unwrap_err();

        assert!(matches!(
            error,
            super::TrafficError::Storage(ConfigError::StateCorrupt(_))
        ));
    }

    #[test]
    fn correction_records_shape_the_reported_traffic() {
        let when = Utc.with_ymd_and_hms(2024, 2, 15, 12, 0, 0).unwrap();
        let mut state = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200)
            .accumulate("boot-a", 130, 260);

        state.corrections.push(CorrectionRecord::TotalAdjustment {
            bytes: 700,
            at: when,
        });
        assert_eq!(state.total_adjustment(), 700);
        assert_eq!((state.reported_rx(), state.reported_tx()), (30, 60));

        state.corrections.push(CorrectionRecord::SetDirection {
            rx: 50,
            tx: 60,
            at: when,
        });
        assert_eq!(state.total_adjustment(), 700);
        assert_eq!(
            (state.reported_rx(), state.reported_tx()),
            (30, 60),
            "a direction record is audit history; the correction itself reconciles the accumulated baseline"
        );
    }

    #[test]
    fn persisted_state_recovers_after_restart_without_losing_accumulated_traffic() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);
        write_interface_fixture(&fixture, 4, 9, "boot-b");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);
        write_interface_fixture(&fixture, 10, 20, "boot-b");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 107);
    }

    #[test]
    fn reset_uses_the_fixture_clock_and_host_boundary() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        let runtime = Runtime::fixture(fixture.path(), now);

        let report = reset_with_runtime(&store, &config, &runtime).unwrap();

        assert_eq!(report.accounting_period, "2024-02-01T00:00:00+00:00");
        assert_eq!(report.total(), 0);
    }

    #[test]
    fn report_without_established_state_is_a_diagnosable_error() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");

        let error = report_at(&store, &config, now).unwrap_err();

        assert!(matches!(error, super::TrafficError::StateMissing));
    }

    #[test]
    fn report_reads_live_deltas_without_writing_state() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();

        write_interface_fixture(&fixture, 130, 260, "boot-a");
        let report = report_at(&store, &config, now).unwrap();

        assert_eq!(report.total(), 90);
        let persisted =
            fs::read_to_string(fixture.path().join("var/lib/sbctl/state.json")).unwrap();
        let state: TrafficState = serde_json::from_str(&persisted).unwrap();
        assert_eq!(state.accumulated_rx, 0);
        assert_eq!(state.accumulated_tx, 0);
        assert_eq!((state.baseline_rx, state.baseline_tx), (100, 200));
    }

    #[test]
    fn report_preserves_accumulated_across_boot_change_and_adds_the_valid_direction() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);
        write_interface_fixture(&fixture, 4, 9, "boot-b");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);

        write_interface_fixture(&fixture, 10, 20, "boot-b");
        let before = fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap();
        let report = report_at(&store, &config, now).unwrap();
        let after = fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap();

        assert_eq!((report.received, report.transmitted), (36, 71));
        assert_eq!(after, before, "a read must not change the persisted state");
    }

    #[test]
    fn report_errors_when_state_belongs_to_a_previous_period() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let january = Utc.with_ymd_and_hms(2024, 1, 20, 0, 0, 0).unwrap();
        let february = Utc.with_ymd_and_hms(2024, 2, 5, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, january).unwrap();

        let error = report_at(&store, &config, february).unwrap_err();

        assert!(matches!(error, super::TrafficError::StateStale));
    }

    #[test]
    fn a_repeated_reset_for_the_same_cycle_key_does_not_reestablish_the_baseline() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);
        assert_eq!(reset_at(&store, &config, now).unwrap().total(), 90);

        let state: TrafficState = serde_json::from_str(
            &fs::read_to_string(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state.cycle_key, "2024-02-01T00:00:00+00:00");
        assert_eq!((state.accumulated_rx, state.accumulated_tx), (30, 60));
    }

    fn config() -> DeploymentConfig {
        let mut config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .unwrap();
        config.accounting_timezone = "UTC".into();
        config
    }

    fn write_interface_fixture(fixture: &TempDir, rx: u64, tx: u64, boot_id: &str) {
        write_interface_fixture_named(fixture, "ens3", rx, tx, boot_id);
    }

    fn write_interface_fixture_named(
        fixture: &TempDir,
        interface: &str,
        rx: u64,
        tx: u64,
        boot_id: &str,
    ) {
        let statistics = fixture
            .path()
            .join("sys/class/net")
            .join(interface)
            .join("statistics");
        fs::create_dir_all(&statistics).unwrap();
        fs::write(statistics.join("rx_bytes"), rx.to_string()).unwrap();
        fs::write(statistics.join("tx_bytes"), tx.to_string()).unwrap();
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().unwrap()).unwrap();
        fs::write(boot_path, boot_id).unwrap();
    }

    #[test]
    fn total_correction_changes_the_total_without_direction_values() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        reset_at(&store, &config, now).unwrap();
        assert_eq!(report_at(&store, &config, now).unwrap().total(), 90);

        let preview = set_used_at(&store, &config, now, CorrectionTarget::Total(1000)).unwrap();

        assert_eq!(
            (preview.current_received, preview.current_transmitted),
            (30, 60)
        );
        assert_eq!(preview.current_total, 90);
        assert_eq!(preview.target_total, 1000);
        assert_eq!(
            (preview.target_received, preview.target_transmitted),
            (30, 60)
        );
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!(report.total(), 1000);
        assert_eq!((report.received, report.transmitted), (30, 60));
    }

    #[test]
    fn total_correction_keeps_accumulating_future_counter_deltas_on_top_of_the_target() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        set_used_at(&store, &config, now, CorrectionTarget::Total(500)).unwrap();
        assert_eq!(report_at(&store, &config, now).unwrap().total(), 500);

        write_interface_fixture(&fixture, 104, 205, "boot-a");
        let report = report_at(&store, &config, now).unwrap();

        assert_eq!(report.total(), 509);
        assert_eq!((report.received, report.transmitted), (4, 5));
    }

    #[test]
    fn repeated_total_corrections_compute_each_delta_against_the_current_total() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        set_used_at(&store, &config, now, CorrectionTarget::Total(500)).unwrap();
        assert_eq!(report_at(&store, &config, now).unwrap().total(), 500);

        set_used_at(&store, &config, now, CorrectionTarget::Total(600)).unwrap();
        assert_eq!(report_at(&store, &config, now).unwrap().total(), 600);

        let persisted: TrafficState = serde_json::from_str(
            &fs::read_to_string(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.total_adjustment(), 600);
    }

    #[test]
    fn total_correction_below_the_current_total_is_rejected_without_writing() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        reset_at(&store, &config, now).unwrap();
        let before = fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap();

        let error = set_used_at(&store, &config, now, CorrectionTarget::Total(50)).unwrap_err();

        assert!(matches!(error, super::TrafficError::TotalTooLow { .. }));
        assert_eq!(
            fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
            before
        );
    }

    #[test]
    fn direction_correction_sets_values_and_accumulates_future_deltas_without_touching_counters() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        let counters_before = (
            fs::read_to_string(
                fixture
                    .path()
                    .join("sys/class/net/ens3/statistics/rx_bytes"),
            )
            .unwrap(),
            fs::read_to_string(
                fixture
                    .path()
                    .join("sys/class/net/ens3/statistics/tx_bytes"),
            )
            .unwrap(),
        );

        let preview = set_used_at(
            &store,
            &config,
            now,
            CorrectionTarget::Directions { rx: 500, tx: 300 },
        )
        .unwrap();

        assert_eq!(
            (preview.target_received, preview.target_transmitted),
            (500, 300)
        );
        assert_eq!(preview.target_total, 800);
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!((report.received, report.transmitted), (500, 300));
        assert_eq!(report.total(), 800);

        assert_eq!(
            fs::read_to_string(
                fixture
                    .path()
                    .join("sys/class/net/ens3/statistics/rx_bytes"),
            )
            .unwrap(),
            counters_before.0,
            "a correction must not modify the real sysfs counter"
        );
        assert_eq!(
            fs::read_to_string(
                fixture
                    .path()
                    .join("sys/class/net/ens3/statistics/tx_bytes"),
            )
            .unwrap(),
            counters_before.1
        );

        write_interface_fixture(&fixture, 104, 205, "boot-a");
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!((report.received, report.transmitted), (504, 305));
        assert_eq!(report.total(), 809);
    }

    #[test]
    fn direction_correction_can_reduce_reported_values() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        reset_at(&store, &config, now).unwrap();
        assert_eq!(report_at(&store, &config, now).unwrap().total(), 90);

        set_used_at(
            &store,
            &config,
            now,
            CorrectionTarget::Directions { rx: 40, tx: 20 },
        )
        .unwrap();

        let report = report_at(&store, &config, now).unwrap();
        assert_eq!((report.received, report.transmitted), (40, 20));
        assert_eq!(report.total(), 60);
    }

    #[test]
    fn direction_correction_after_a_reboot_accumulates_deltas_before_the_next_reset() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();

        write_interface_fixture(&fixture, 0, 0, "boot-b");
        set_used_at(
            &store,
            &config,
            now,
            CorrectionTarget::Directions { rx: 600, tx: 400 },
        )
        .unwrap();

        write_interface_fixture(&fixture, 5, 7, "boot-b");
        let report = report_at(&store, &config, now).unwrap();

        assert_eq!(
            (report.received, report.transmitted),
            (605, 407),
            "deltas since the correction-time counter accumulate even before the reset task runs"
        );
    }

    #[test]
    fn direction_correction_preserves_the_set_values_across_a_counter_rollback() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        set_used_at(
            &store,
            &config,
            now,
            CorrectionTarget::Directions { rx: 500, tx: 300 },
        )
        .unwrap();

        write_interface_fixture(&fixture, 4, 9, "boot-b");
        reset_at(&store, &config, now).unwrap();
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!((report.received, report.transmitted), (500, 300));

        write_interface_fixture(&fixture, 10, 20, "boot-b");
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!((report.received, report.transmitted), (506, 311));
    }

    #[test]
    fn direction_correction_rejects_an_overflowing_rx_tx_total() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();
        let before = fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap();

        let error = set_used_at(
            &store,
            &config,
            now,
            CorrectionTarget::Directions {
                rx: u64::MAX,
                tx: 1,
            },
        )
        .unwrap_err();

        assert!(matches!(error, super::TrafficError::Overflow(_)));
        assert_eq!(
            fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
            before
        );
    }

    #[test]
    fn correction_rejects_missing_state_without_creating_it() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");

        let error = set_used_at(&store, &config, now, CorrectionTarget::Total(500)).unwrap_err();

        assert!(matches!(error, super::TrafficError::StateMissing));
        assert!(!fixture.path().join("var/lib/sbctl/state.json").exists());
    }

    #[test]
    fn correction_rejects_corrupted_state_without_overwriting_it() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        store.write_state(b"not json").unwrap();

        let error = set_used_at(&store, &config, now, CorrectionTarget::Total(500)).unwrap_err();

        assert!(matches!(
            error,
            super::TrafficError::Storage(ConfigError::StateCorrupt(_))
        ));
        assert_eq!(
            fs::read(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
            b"not json"
        );
    }

    #[test]
    fn correction_rejects_state_belonging_to_a_previous_period() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let january = Utc.with_ymd_and_hms(2024, 1, 20, 0, 0, 0).unwrap();
        let february = Utc.with_ymd_and_hms(2024, 2, 5, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, january).unwrap();

        let error =
            set_used_at(&store, &config, february, CorrectionTarget::Total(500)).unwrap_err();

        assert!(matches!(error, super::TrafficError::StateStale));
    }

    #[test]
    fn correction_is_rejected_before_the_first_reset() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-06-15T12:00".into());
        let now = Utc.with_ymd_and_hms(2024, 5, 1, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");

        let error = set_used_at(&store, &config, now, CorrectionTarget::Total(500)).unwrap_err();

        assert!(matches!(error, super::TrafficError::PendingFirstReset));
        assert!(!fixture.path().join("var/lib/sbctl/state.json").exists());
    }

    #[test]
    fn concurrent_corrections_serialize_on_the_operation_lock() {
        use std::sync::Arc;

        let fixture = TempDir::new().unwrap();
        let store = Arc::new(DeploymentStore::new(fixture.path()));
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..10 {
                let store = Arc::clone(&store);
                let thread_config = config.clone();
                scope.spawn(move || {
                    set_used_at(&store, &thread_config, now, CorrectionTarget::Total(1000))
                        .unwrap();
                });
            }
        });

        let report = report_at(&store, &config, now).unwrap();
        assert_eq!(
            report.total(),
            1000,
            "each correction recomputes its delta against the latest committed total under the lock"
        );
    }

    #[test]
    fn correction_applies_under_the_same_lock_as_the_reset_task() {
        use std::sync::Arc;

        let fixture = TempDir::new().unwrap();
        let store = Arc::new(DeploymentStore::new(fixture.path()));
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        reset_at(&store, &config, now).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..5 {
                let thread_store = Arc::clone(&store);
                let thread_config = config.clone();
                scope.spawn(move || {
                    reset_at(&thread_store, &thread_config, now).unwrap();
                });
                let thread_store = Arc::clone(&store);
                let thread_config = config.clone();
                scope.spawn(move || {
                    set_used_at(
                        &thread_store,
                        &thread_config,
                        now,
                        CorrectionTarget::Total(500),
                    )
                    .unwrap();
                });
            }
        });

        let persisted: TrafficState = serde_json::from_str(
            &fs::read_to_string(fixture.path().join("var/lib/sbctl/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.total_adjustment(), 500);
        let report = report_at(&store, &config, now).unwrap();
        assert_eq!(report.total(), 500);
    }
}
