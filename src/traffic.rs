use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{AccountingPolicy, ConfigError, DeploymentConfig, DeploymentStore};

const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TrafficState {
    schema_version: u32,
    accounting_policy: AccountingPolicy,
    pub period_identity: String,
    pub accumulated_rx: u64,
    pub accumulated_tx: u64,
    latest_rx: u64,
    latest_tx: u64,
    boot_id: String,
}

impl TrafficState {
    pub fn new(boot_id: String, rx: u64, tx: u64) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            accounting_policy: AccountingPolicy::NaturalMonth,
            period_identity: String::new(),
            accumulated_rx: 0,
            accumulated_tx: 0,
            latest_rx: rx,
            latest_tx: tx,
            boot_id,
        }
    }

    pub fn reconcile(mut self, boot_id: &str, rx: u64, tx: u64) -> Self {
        if self.boot_id == boot_id {
            if rx >= self.latest_rx {
                self.accumulated_rx += rx - self.latest_rx;
            }
            if tx >= self.latest_tx {
                self.accumulated_tx += tx - self.latest_tx;
            }
        }
        self.latest_rx = rx;
        self.latest_tx = tx;
        self.boot_id = boot_id.to_owned();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrafficReport {
    pub interface: String,
    pub received: u64,
    pub transmitted: u64,
    pub monthly_traffic_limit: u64,
    pub accounting_period: String,
    pub next_reset: DateTime<Utc>,
}

impl TrafficReport {
    pub fn total(&self) -> u64 {
        self.received + self.transmitted
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
    reconcile_at(store, config, Utc::now())
}

pub fn reconcile_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<TrafficReport, TrafficError> {
    let (rx, tx) = read_interface_counters(store.root(), &config.interface)?;
    let boot_id = fs::read_to_string(store.root().join("proc/sys/kernel/random/boot_id"))
        .map_err(TrafficError::BootId)?
        .trim()
        .to_owned();
    let period = accounting_period(config, now)?;
    let mut reconciled = None;
    store.update_state(|prior| {
        let mut state = match prior {
            Some(contents) => serde_json::from_slice::<TrafficState>(&contents)
                .map_err(|error| ConfigError::StateContent(error.to_string()))?,
            None => TrafficState::new(boot_id.clone(), rx, tx),
        };
        if state.schema_version != STATE_SCHEMA_VERSION
            || state.accounting_policy != config.accounting_policy
            || state.period_identity != period.identity
        {
            state = TrafficState::new(boot_id.clone(), rx, tx);
            state.period_identity = period.identity.clone();
            state.accounting_policy = config.accounting_policy.clone();
        } else {
            state = state.reconcile(&boot_id, rx, tx);
        }
        let contents = serde_json::to_vec(&state)
            .map_err(|error| ConfigError::StateContent(error.to_string()))?;
        reconciled = Some(state);
        Ok(contents)
    })?;
    let state = reconciled.expect("state is set by the successful state transaction");
    Ok(TrafficReport {
        interface: config.interface.clone(),
        received: state.accumulated_rx,
        transmitted: state.accumulated_tx,
        monthly_traffic_limit: config.monthly_traffic_limit,
        accounting_period: period.identity,
        next_reset: period.next_reset,
    })
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

fn read_interface_counters(root: &Path, interface: &str) -> Result<(u64, u64), TrafficError> {
    let statistics = root
        .join("sys/class/net")
        .join(interface)
        .join("statistics");
    Ok((
        read_counter(&statistics.join("rx_bytes"))?,
        read_counter(&statistics.join("tx_bytes"))?,
    ))
}

fn read_counter(path: &Path) -> Result<u64, TrafficError> {
    let contents = fs::read_to_string(path)?;
    contents
        .trim()
        .parse()
        .map_err(|_| TrafficError::InvalidCounter(path.display().to_string()))
}

struct AccountingPeriod {
    identity: String,
    next_reset: DateTime<Utc>,
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
    let (start, next) = match config.accounting_policy {
        AccountingPolicy::NaturalMonth => {
            let start = local_datetime(timezone, local_now.year(), local_now.month(), 1, 0, 0)?;
            let (year, month) = next_month(local_now.year(), local_now.month());
            (start, local_datetime(timezone, year, month, 1, 0, 0)?)
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
                return Err(TrafficError::Schedule(
                    "the first anchored reset has not occurred yet",
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
            (start, next)
        }
    };
    Ok(AccountingPeriod {
        identity: start.to_rfc3339(),
        next_reset: next.with_timezone(&Utc),
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
        LocalResult::Ambiguous(first, _) => Ok(first),
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

    use crate::config::{AccountingPolicy, DeploymentConfig, ManagedProtocol, SubscriptionMode};

    use super::{TrafficState, accounting_period, reconcile_at};

    #[test]
    fn reconciles_rx_and_tx_deltas_without_counting_the_first_observation() {
        let initial = TrafficState::new("boot-a".into(), 100, 200);
        let reconciled = initial.reconcile("boot-a", 130, 260);
        assert_eq!(reconciled.accumulated_rx, 30);
        assert_eq!(reconciled.accumulated_tx, 60);
    }

    #[test]
    fn boot_id_changes_and_counter_decreases_preserve_prior_accumulation() {
        let mut state = TrafficState::new("boot-a".into(), 100, 200).reconcile("boot-a", 140, 250);
        state = state.reconcile("boot-b", 5, 7);
        state = state.reconcile("boot-b", 8, 12);
        assert_eq!((state.accumulated_rx, state.accumulated_tx), (43, 55));
    }

    #[test]
    fn a_counter_decrease_does_not_discard_the_other_direction_delta() {
        let state = TrafficState::new("boot-a".into(), 100, 200).reconcile("boot-a", 5, 240);

        assert_eq!((state.accumulated_rx, state.accumulated_tx), (0, 40));
    }

    #[test]
    fn anchored_month_uses_the_last_day_in_short_months() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-01-31T09:30".into());
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();
        let period = accounting_period(&config, now).unwrap();
        assert_eq!(period.identity, "2024-01-31T09:30:00+00:00");
        assert_eq!(period.next_reset.to_rfc3339(), "2024-02-29T09:30:00+00:00");
    }

    #[test]
    fn anchored_month_rejects_a_period_before_its_first_reset() {
        let mut config = config();
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.anchored_reset_at = Some("2024-03-31T09:30".into());
        let now = Utc.with_ymd_and_hms(2024, 2, 15, 0, 0, 0).unwrap();

        assert!(accounting_period(&config, now).is_err());
    }

    #[test]
    fn persisted_state_recovers_after_restart_without_losing_accumulated_traffic() {
        let fixture = TempDir::new().unwrap();
        let store = crate::config::DeploymentStore::new(fixture.path());
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

    fn config() -> DeploymentConfig {
        let mut config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::Hysteria2],
            None,
        )
        .unwrap();
        config.accounting_timezone = "UTC".into();
        config
    }

    fn write_interface_fixture(fixture: &TempDir, rx: u64, tx: u64, boot_id: &str) {
        let statistics = fixture.path().join("sys/class/net/ens3/statistics");
        fs::create_dir_all(&statistics).unwrap();
        fs::write(statistics.join("rx_bytes"), rx.to_string()).unwrap();
        fs::write(statistics.join("tx_bytes"), tx.to_string()).unwrap();
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().unwrap()).unwrap();
        fs::write(boot_path, boot_id).unwrap();
    }
}
