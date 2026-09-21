//! The self-contained Chinese overview page served at `/sub/<credential>/index`.
//!
//! The page opens with a per-client quick-pick table (Clash Party, Clash Verge,
//! sing-box, V2rayN, Shadowrocket), then lists every subscription link with its
//! version label, the matching scannable QR code, and per-client import
//! instructions. It carries no external resources, so it renders offline and a
//! credential rotation invalidates the whole page together with every link it
//! shows. A broken accounting state degrades the traffic badges but never the
//! links themselves.

use chrono_tz::Tz;

use crate::config::{DeploymentConfig, DeploymentStore};
use crate::subscription::{
    ClientVersion, SubscriptionRoute, client_subscription_matrix, route_url, subscription_matrix,
};

/// Renders the overview page. Fails only when a QR code or a route URL cannot
/// be produced; the caller turns any failure into a redacted 503. Traffic
/// reporting failures degrade to a "暂不可用" badge instead.
pub fn render(store: &DeploymentStore, config: &DeploymentConfig) -> Result<String, String> {
    let traffic = match crate::traffic::report(store, config) {
        Ok(traffic) => Some(traffic),
        Err(error) => {
            // The page degrades to a "暂不可用" badge, but the reason still has
            // to reach the journal or an administrator cannot diagnose it.
            eprintln!(
                "index page traffic unavailable: {}",
                crate::subscription::redact_secret(
                    &error.to_string(),
                    &config.subscription_credential
                )
            );
            None
        }
    };
    let mut client_rows = String::new();
    for row in client_subscription_matrix() {
        client_rows.push_str("<tr><th>");
        client_rows.push_str(&esc(row.client));
        client_rows.push_str("</th><td><ul>");
        for (index, recommended) in row.formats.iter().enumerate() {
            let url = route_url(config, SubscriptionRoute::Format(recommended.format))
                .map_err(|error| error.to_string())?;
            let primary = index == 0;
            client_rows.push_str(&format!(
                "<li><a href=\"{url_attr}\" rel=\"noreferrer\">{url}</a>\
                 <span class=\"note\">{note}{primary}</span></li>",
                url_attr = esc_attr(&url),
                url = esc(&url),
                note = esc(&recommended.note),
                primary = if primary { "；首选" } else { "" },
            ));
        }
        client_rows.push_str(&format!(
            "</ul><p class=\"note\">{note}</p></td></tr>",
            note = esc(&row.note),
        ));
    }
    let mut rows = String::new();
    for info in subscription_matrix() {
        let url = route_url(config, SubscriptionRoute::Format(info.format))
            .map_err(|error| error.to_string())?;
        let qr = crate::qr::render_svg(&url)?;
        rows.push_str(&format!(
            "<details open><summary><span class=\"label\">{label}</span>\
<span class=\"audience\">{audience}</span></summary>\
<p class=\"note\">{note}</p>\
<p class=\"url\"><a href=\"{url_attr}\" rel=\"noreferrer\">{url}</a></p>\
<div class=\"qr\">{qr}</div></details>",
            label = esc(&info.label),
            audience = esc(&info.audience),
            note = esc(&info.note),
            url = esc(&url),
            url_attr = esc_attr(&url),
            qr = qr,
        ));
    }
    Ok(format!(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<meta name=\"robots\" content=\"noindex, nofollow\">\n\
<title>sbctl 订阅中心</title>\n\
<style>\n\
:root {{ color-scheme: light dark; }}\n\
body {{ font-family: system-ui, sans-serif; max-width: 46rem; margin: 0 auto; padding: 1rem; line-height: 1.5; }}\n\
h1 {{ font-size: 1.3rem; }}\n\
h2 {{ font-size: 1.1rem; margin-top: 1.6rem; }}\n\
.badge {{ display: inline-block; border: 1px solid #8884; border-radius: .4rem; padding: 0 .5rem; margin: .1rem .2rem .6rem 0; font-size: .85rem; }}\n\
details {{ border: 1px solid #8884; border-radius: .5rem; padding: .4rem .8rem; margin: .5rem 0; }}\n\
summary {{ cursor: pointer; font-weight: 600; display: flex; align-items: center; gap: .5rem; flex-wrap: wrap; }}\n\
.label {{ font-size: 1rem; }}\n\
.audience {{ font-size: .8rem; color: #888; font-weight: 400; }}\n\
.note {{ color: #888; font-size: .85rem; margin: .4rem 0 0; }}\n\
.url {{ font-size: .8rem; word-break: break-all; margin: .3rem 0; }}\n\
.qr {{ width: 180px; }}\n\
.qr svg {{ width: 100%; height: auto; }}\n\
table {{ border-collapse: collapse; width: 100%; font-size: .85rem; }}\n\
th, td {{ border: 1px solid #8884; padding: .4rem .6rem; text-align: left; vertical-align: top; }}\n\
th {{ white-space: nowrap; }}\n\
td ul {{ margin: 0; padding-left: 1rem; }}\n\
td li {{ margin: .2rem 0; word-break: break-all; }}\n\
code {{ background: #8882; border-radius: .3rem; padding: 0 .3rem; }}\n\
</style>\n</head>\n<body>\n\
<h1>sbctl 订阅中心</h1>\n\
<p><span class=\"badge\">主机：{host}</span>\
<span class=\"badge\">模式：{mode}</span>\
{traffic_badges}</p>\n\
<p class=\"note\">每个链接都可以直接下载，或用手机扫描旁边的二维码一键导入。订阅凭据泄露时请在服务器上执行 <code>sbctl credential rotate</code> 更换。</p>\n\
<h2>按客户端选择</h2>\n\
<table>\n{client_rows}</table>\n\
<h2>全部订阅链接</h2>\n\
{rows}\n\
<h2>客户端导入</h2>\n\
<ul>\n\
<li><strong>Clash Party / Clash Verge</strong>：粘贴 <code>clash.yaml</code> 链接导入订阅；旧版内核使用 <code>clash-1.18.yaml</code>。</li>\n\
<li><strong>sing-box（SFA / SFI / SFW / 桌面版）</strong>：按已安装内核版本选择对应 <code>sing-box-&lt;版本&gt;.json</code>；不确定就用 <code>sing-box-full.json</code>。1.10/1.11 内核不支持 AnyTLS 节点。</li>\n\
<li><strong>V2rayN</strong>：粘贴 <code>uri.txt</code>（Base64 URI）或复制 <code>uri</code> 明文分享链接；6.6+ 也可导入 <code>sing-box-full.json</code>。</li>\n\
<li><strong>Shadowrocket（iOS）</strong>：扫码或粘贴 <code>shadowrocket.txt</code> 链接 → 首页添加配置 → 自动更新。五协议均受支持，最低版本：VLESS Reality 2.2.16、TUIC 2.2.12、Hysteria2 2.2.35、AnyTLS 2.2.64；低于对应版本时该协议节点不会被识别（其余节点仍可正常导入）。</li>\n\
</ul>\n\
</body>\n</html>\n",
        host = esc(&config.subscription_host),
        mode = esc(&config.subscription_mode.to_string()),
        traffic_badges = traffic_badges(config, traffic.as_ref())?,
        client_rows = client_rows,
        rows = rows,
    ))
}

/// The traffic badges: a short summary when accounting works, an explicit
/// degraded badge when it does not, so the links always stay available.
fn traffic_badges(
    config: &DeploymentConfig,
    traffic: Option<&crate::traffic::TrafficReport>,
) -> Result<String, String> {
    let Some(traffic) = traffic else {
        return Ok(
            "<span class=\"badge\">流量：暂不可用（账期状态恢复后自动显示）</span>".to_owned(),
        );
    };
    let tz: Tz = config
        .client_display_timezone
        .parse()
        .map_err(|_| format!("unknown IANA timezone {}", config.client_display_timezone))?;
    let reset = traffic
        .next_reset
        .with_timezone(&tz)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    Ok(format!(
        "<span class=\"badge\">已用流量：{used}</span>\
<span class=\"badge\">总量：{quota}</span>\
<span class=\"badge\">下次重置：{reset}（{tz}）</span>",
        used = format_gib(traffic.received + traffic.transmitted),
        quota = if traffic.monthly_traffic_limit > 0 {
            format_gib(traffic.monthly_traffic_limit)
        } else {
            "不限".to_owned()
        },
        reset = esc(&reset),
        tz = esc(&config.client_display_timezone),
    ))
}

fn format_gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn esc_attr(text: &str) -> String {
    esc(text)
}

/// Re-exported for tests and callers that want to label version rows.
pub fn version_label(version: ClientVersion) -> String {
    format!("sing-box {version}")
}
