//! The Rules tab: what the running configuration listens on, where its rule
//! sets come from, and the routing verdicts in match order. Everything here
//! comes from the engine snapshot, so the page is honest before the core
//! starts too — it shows the cached configuration the next start will use.

use ratatui::Frame;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::style::{panel, short_label};

pub(crate) fn draw_rules(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let lines: Vec<Line> = rules_lines(app).into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(lines).block(panel("入站与分流规则 · r 返回日志")),
        area,
    );
}

pub(crate) fn rules_lines(app: &App) -> Vec<String> {
    let snapshot = &app.snapshot;
    if snapshot.inbounds.is_empty() && snapshot.rules.is_empty() && snapshot.rule_sets.is_empty() {
        return vec!["（尚无激活配置；先启动一次内核）".to_owned()];
    }
    let mut lines = Vec::new();
    if !snapshot.inbounds.is_empty() {
        lines.push("── 入站（内核正在使用的配置）──".to_owned());
        for inbound in &snapshot.inbounds {
            // A tun inbound declares no port, and an unspec'd listen address
            // means every address. Both say something, so neither is hidden.
            let port = if inbound.port == 0 {
                String::new()
            } else {
                format!(":{}", inbound.port)
            };
            let listen = if inbound.listen.is_empty() {
                "全部地址".to_owned()
            } else {
                inbound.listen.clone()
            };
            let tag = if inbound.tag.is_empty() {
                String::new()
            } else {
                format!(" · {}", short_label(&inbound.tag, 24))
            };
            lines.push(format!(
                "• {}{port}（{listen}）{tag}",
                short_label(&inbound.kind, 16)
            ));
        }
        lines.push(String::new());
    }
    if !snapshot.rule_sets.is_empty() {
        lines.push("── 规则集 ──".to_owned());
        for set in &snapshot.rule_sets {
            let source = if set.url.is_empty() {
                "（本地）".to_owned()
            } else {
                short_label(&set.url, 64)
            };
            lines.push(format!("• {}（{}） ← {}", set.tag, set.kind, source));
        }
        lines.push(String::new());
    }
    lines.push("── 分流规则（自上而下匹配）──".to_owned());
    for (index, rule) in snapshot.rules.iter().enumerate() {
        lines.push(format!(
            "{:>2}. {} → {}",
            index + 1,
            rule.matcher_zh(),
            if rule.outbound.is_empty() {
                "（动作）"
            } else {
                &rule.outbound
            }
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientController;

    #[tokio::test]
    async fn rules_lines_render_the_engine_snapshot() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.snapshot.inbounds = vec![
            client_core::state::InboundInfo {
                kind: "mixed".to_owned(),
                tag: "mixed-in".to_owned(),
                listen: "127.0.0.1".to_owned(),
                port: 2080,
            },
            client_core::state::InboundInfo {
                kind: "tun".to_owned(),
                tag: "tun-in".to_owned(),
                listen: String::new(),
                port: 0,
            },
        ];
        app.snapshot.rule_sets = vec![client_core::state::RuleSetSummary {
            tag: "geoip-cn".to_owned(),
            url: "https://example/srs".to_owned(),
            kind: "remote".to_owned(),
        }];
        app.snapshot.rules = vec![
            client_core::state::RouteRuleSnapshot {
                kind: client_core::state::RuleKind::RuleSet,
                value: Some("geoip-cn".to_owned()),
                outbound: "🚀节点选择".to_owned(),
            },
            client_core::state::RouteRuleSnapshot {
                kind: client_core::state::RuleKind::Final,
                value: None,
                outbound: "🚀节点选择".to_owned(),
            },
        ];
        let lines = rules_lines(&app);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("mixed:2080（127.0.0.1） · mixed-in")),
            "the inbound is listed with what it listens on: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("tun（全部地址）")),
            "an inbound with no port and no listen address still says so: {lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("geoip-cn")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1. 规则集 · geoip-cn")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("其他未命中流量")));
    }

    #[tokio::test]
    async fn rules_lines_say_the_configuration_is_absent_instead_of_showing_nothing() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        let lines = rules_lines(&app);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("尚无激活配置"),
            "an empty snapshot has to explain itself: {lines:?}"
        );
    }
}
