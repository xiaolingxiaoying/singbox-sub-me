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
    pub fn reconcile(mut self, boot_id: &str, rx: u64, tx: u64) -> Self {
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

    /// The latest direction-aware correction, if one was applied.
    fn latest_set_direction(&self) -> Option<(u64, u64)> {
        self.corrections
            .iter()
            .rev()
            .find_map(|correction| match correction {
                CorrectionRecord::SetDirection { rx, tx, .. } => Some((*rx, *tx)),
                CorrectionRecord::TotalAdjustment { .. } => None,
            })
    }

    /// The reported RX including the latest direction-aware correction.
    pub fn reported_rx(&self) -> u64 {
        self.latest_set_direction()
            .map(|(rx, _)| rx)
            .unwrap_or(self.accumulated_rx)
    }

    /// The reported TX including the latest direction-aware correction.
    pub fn reported_tx(&self) -> u64 {
        self.latest_set_direction()
            .map(|(_, tx)| tx)
            .unwrap_or(self.accumulated_tx)
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
}

pub fn reconcile(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<TrafficReport, TrafficError> {
    reconcile_with_runtime(store, config, &Runtime::live(store.root()))
}

pub fn reconcile_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<TrafficReport, TrafficError> {
    reconcile_with_runtime(store, config, &Runtime::fixture(store.root(), now))
}

/// Reconcile using an explicit runtime adapter. Production callers use
/// [`reconcile`]; acceptance and boundary tests use a fixed clock and fixture
/// root without mocking the accounting algorithm.
pub fn reconcile_with_runtime<C: crate::runtime::Clock>(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    runtime: &Runtime<C>,
) -> Result<TrafficReport, TrafficError> {
    let (rx, tx) = read_interface_counters(runtime, &config.interface)?;
    let boot_id = runtime
        .read_to_string("proc/sys/kernel/random/boot_id")
        .map_err(TrafficError::BootId)?
        .trim()
        .to_owned();
    let period = accounting_period(config, runtime.now_utc())?;
    if period.pending() {
        return Ok(TrafficReport {
            interface: config.interface.clone(),
            received: 0,
            transmitted: 0,
            total_adjustment: 0,
            monthly_traffic_limit: config.monthly_traffic_limit,
            accounting_period: period.identity().to_owned(),
            next_reset: period.next_reset,
        });
    }
    let mut reconciled = None;
    store.update_state(|prior| {
        let state = match prior {
            Some(contents) => decode_state(&contents)?,
            None => TrafficState::new(period.cycle_key(), &config.interface, &boot_id, rx, tx),
        };
        let state = if state.cycle_key != period.cycle_key() || state.interface != config.interface
        {
            TrafficState::new(period.cycle_key(), &config.interface, &boot_id, rx, tx)
        } else {
            state.reconcile(&boot_id, rx, tx)
        };
        let contents = serde_json::to_vec(&state)
            .map_err(|error| ConfigError::StateContent(error.to_string()))?;
        reconciled = Some(state);
        Ok(contents)
    })?;
    let state = reconciled.expect("active period sets state in the successful state transaction");
    Ok(TrafficReport {
        interface: config.interface.clone(),
        received: state.reported_rx(),
        transmitted: state.reported_tx(),
        total_adjustment: state.total_adjustment(),
        monthly_traffic_limit: config.monthly_traffic_limit,
        accounting_period: period.identity().to_owned(),
        next_reset: period.next_reset,
    })
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
        CorrectionRecord, TrafficState, accounting_period, reconcile_at, reconcile_with_runtime,
    };
    use crate::runtime::Runtime;

    #[test]
    fn reconciles_rx_and_tx_deltas_without_counting_the_first_observation() {
        let initial = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200);
        let reconciled = initial.reconcile("boot-a", 130, 260);
        assert_eq!(reconciled.accumulated_rx, 30);
        assert_eq!(reconciled.accumulated_tx, 60);
    }

    #[test]
    fn boot_id_changes_and_counter_decreases_preserve_prior_accumulation() {
        let mut state = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200)
            .reconcile("boot-a", 140, 250);
        state = state.reconcile("boot-b", 5, 7);
        state = state.reconcile("boot-b", 8, 12);
        assert_eq!((state.accumulated_rx, state.accumulated_tx), (43, 55));
    }

    #[test]
    fn a_counter_decrease_does_not_discard_the_other_direction_delta() {
        let state =
            TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200).reconcile("boot-a", 5, 240);

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

        let report = reconcile_at(&store, &config, now).unwrap();

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
        assert_eq!(reconcile_at(&store, &config, january).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reconcile_at(&store, &config, january).unwrap().total(), 90);

        write_interface_fixture(&fixture, 400, 500, "boot-a");
        let report = reconcile_at(&store, &config, february).unwrap();

        assert_eq!(report.total(), 0);
        assert_eq!(report.accounting_period, "2024-02-01T00:00:00+00:00");
    }

    #[test]
    fn an_interface_change_establishes_a_new_period() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 90);

        config.interface = "eth1".into();
        write_interface_fixture(&fixture, 1000, 2000, "boot-a");
        write_interface_fixture_named(&fixture, "eth1", 5, 7, "boot-a");
        let report = reconcile_at(&store, &config, now).unwrap();

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

        let error = reconcile_at(&store, &config, now).unwrap_err();

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

        let error = reconcile_at(&store, &config, now).unwrap_err();

        assert!(matches!(
            error,
            super::TrafficError::Storage(ConfigError::StateCorrupt(_))
        ));
    }

    #[test]
    fn correction_records_shape_the_reported_traffic() {
        let when = Utc.with_ymd_and_hms(2024, 2, 15, 12, 0, 0).unwrap();
        let mut state = TrafficState::new("2024-02-01", "ens3", "boot-a", 100, 200)
            .reconcile("boot-a", 130, 260);

        state.corrections.push(CorrectionRecord::SetDirection {
            rx: 50,
            tx: 60,
            at: when,
        });
        assert_eq!((state.reported_rx(), state.reported_tx()), (50, 60));
        assert_eq!(state.total_adjustment(), 0);

        state.corrections.push(CorrectionRecord::TotalAdjustment {
            bytes: 700,
            at: when,
        });
        assert_eq!(state.total_adjustment(), 700);
        assert_eq!((state.reported_rx(), state.reported_tx()), (50, 60));
    }

    #[test]
    fn persisted_state_recovers_after_restart_without_losing_accumulated_traffic() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 0);
        write_interface_fixture(&fixture, 130, 260, "boot-a");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 90);
        write_interface_fixture(&fixture, 4, 9, "boot-b");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 90);
        write_interface_fixture(&fixture, 10, 20, "boot-b");
        assert_eq!(reconcile_at(&store, &config, now).unwrap().total(), 107);
    }

    #[test]
    fn reconciliation_uses_the_fixture_clock_and_host_boundary() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = config();
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        write_interface_fixture(&fixture, 100, 200, "boot-a");
        let runtime = Runtime::fixture(fixture.path(), now);

        let report = reconcile_with_runtime(&store, &config, &runtime).unwrap();

        assert_eq!(report.accounting_period, "2024-02-01T00:00:00+00:00");
        assert_eq!(report.total(), 0);
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
}
