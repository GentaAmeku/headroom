use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{
    menu::{IconMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager,
};

struct QuitState(AtomicBool);

// ---------- i18n（OS ロケール or ~/.config/headroom/config.json の "language" で ja/en を選択） ----------
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Ja,
    En,
}
fn lang() -> Lang {
    static L: OnceLock<Lang> = OnceLock::new();
    *L.get_or_init(|| {
        if let Some(l) = config_language() {
            return l;
        }
        match sys_locale::get_locale() {
            Some(l) if l.to_lowercase().starts_with("ja") => Lang::Ja,
            _ => Lang::En,
        }
    })
}
fn config_language() -> Option<Lang> {
    let home = std::env::var("HOME").ok()?;
    let data = std::fs::read_to_string(format!("{home}/.config/headroom/config.json")).ok()?;
    #[derive(Deserialize)]
    struct Cfg {
        language: Option<String>,
    }
    match serde_json::from_str::<Cfg>(&data)
        .ok()?
        .language?
        .to_lowercase()
        .as_str()
    {
        "ja" | "japanese" => Some(Lang::Ja),
        "en" | "english" => Some(Lang::En),
        _ => None,
    }
}
/// 言語に応じて文字列を選ぶ
fn tr(ja: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Ja => ja,
        Lang::En => en,
    }
}

// ---------- normalized model (ADR-0003) ----------
enum Status {
    Ok,
    Failed,
    Unconnected,
}

#[derive(Clone)]
struct UsageWindow {
    label: String,
    consumption: f64,          // 0-100（% 表示の枠）
    amount_cents: Option<f64>, // Some なら金額(¢)で表示（On-Demand 用）。その場合 consumption は無視
    resets_at: String,         // ISO8601
}

struct ToolSnapshot {
    display_name: String,
    status: Status,
    windows: Vec<UsageWindow>,
    /// windows が直近の成功値（古い）か。429 等で再取得できないとき true
    stale: bool,
    /// 失敗/未接続/stale 時の原因（短文）
    reason: Option<String>,
    /// 失敗/未接続時の対処（ユーザーが何をすればよいか）
    hint: Option<String>,
}

struct CollectError {
    status: Status,
    reason: String,
    hint: String,
}

// ---------- per-tool キャッシュ：直近成功値の保持＋レート制限クールダウン ----------
#[derive(Default)]
struct ToolCache {
    last_good: Option<Vec<UsageWindow>>,
    cooldown_until: Option<Instant>,
}
fn tool_cache() -> &'static Mutex<HashMap<&'static str, ToolCache>> {
    static C: OnceLock<Mutex<HashMap<&'static str, ToolCache>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// HTTP 失敗を CollectError に変換。429 のときは `retry-after` を読んでクールダウンを記録する
/// （= その間は再取得しない。利用枠ではなく usage 取得 API 側の制限）。
fn http_error(name: &'static str, resp: &reqwest::blocking::Response, auth_hint: &str) -> CollectError {
    let code = resp.status().as_u16();
    if code == 429 {
        let secs = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(900);
        if let Ok(mut c) = tool_cache().lock() {
            c.entry(name).or_default().cooldown_until = Some(Instant::now() + Duration::from_secs(secs));
        }
    }
    let (reason, hint) = match code {
        429 => (
            tr("取得が一時的に制限中", "Fetch rate-limited").to_string(),
            tr("利用枠ではなく取得APIの制限です。後で自動再取得します", "Usage API is throttled (not your quota). Retrying soon.").to_string(),
        ),
        401 | 403 => (
            format!("{} ({code})", tr("認証エラー", "Auth error")),
            auth_hint.to_string(),
        ),
        500..=599 => (
            format!("{} ({code})", tr("サーバーエラー", "Server error")),
            tr("時間をおいて「更新」してください", "Try Refresh again later").to_string(),
        ),
        _ => (
            format!("{} (HTTP {code})", tr("取得に失敗しました", "Fetch failed")),
            tr("「更新」を押して再試行してください", "Press Refresh to retry").to_string(),
        ),
    };
    CollectError {
        status: Status::Failed,
        reason,
        hint,
    }
}

// ---------- Claude collector (ADR-0001 / 0004: 読み取り専用) ----------
#[derive(Deserialize)]
struct ClaudeCreds {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: ClaudeOAuth,
}
#[derive(Deserialize)]
struct ClaudeOAuth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>, // epoch ミリ秒
}
#[derive(Deserialize)]
struct ClaudeUsage {
    five_hour: Option<ClaudeWindow>,
    seven_day: Option<ClaudeWindow>,
}
#[derive(Deserialize)]
struct ClaudeWindow {
    utilization: f64,
    resets_at: String,
}

/// 有効なら Ok(token)。期限切れなら Err(true)、トークン無し/壊れていれば Err(false)。
fn claude_token_if_valid(o: ClaudeOAuth) -> Result<String, bool> {
    if o.access_token.is_empty() {
        return Err(false);
    }
    if let Some(exp) = o.expires_at {
        // 失効（60秒マージン）していれば使わない。自前リフレッシュはしない（ADR-0004）。
        if exp <= chrono::Utc::now().timestamp_millis() + 60_000 {
            return Err(true);
        }
    }
    Ok(o.access_token)
}

fn read_claude_token() -> Result<String, CollectError> {
    // 現行の Claude Code はリフレッシュ済みトークンを ~/.claude/.credentials.json に保存する。
    // 旧来の Keychain (Claude Code-credentials) は更新されず失効していることがあるため、
    // ファイル → Keychain の順に読み、失効していない方を使う（リフレッシュはしない：ADR-0004）。
    let mut expired = false;
    let mut check = |o: ClaudeOAuth| match claude_token_if_valid(o) {
        Ok(tok) => Some(tok),
        Err(is_expired) => {
            expired |= is_expired;
            None
        }
    };

    // 1) ~/.claude/.credentials.json
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(data) = std::fs::read_to_string(format!("{home}/.claude/.credentials.json")) {
            if let Ok(c) = serde_json::from_str::<ClaudeCreds>(&data) {
                if let Some(tok) = check(c.claude_ai_oauth) {
                    return Ok(tok);
                }
            }
        }
    }
    // 2) Keychain (フォールバック)
    if let Ok(out) = Command::new("security")
        .args(["find-generic-password", "-w", "-s", "Claude Code-credentials"])
        .output()
    {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout);
            if let Ok(c) = serde_json::from_str::<ClaudeCreds>(raw.trim()) {
                if let Some(tok) = check(c.claude_ai_oauth) {
                    return Ok(tok);
                }
            }
        }
    }

    if expired {
        Err(CollectError {
            status: Status::Failed,
            reason: tr("認証の有効期限切れ", "Authentication expired").into(),
            hint: tr("Claude Code を一度使うと自動で更新されます", "Use Claude Code once to refresh").into(),
        })
    } else {
        Err(CollectError {
            status: Status::Unconnected,
            reason: tr("未接続です", "Not connected").into(),
            hint: tr("Claude にログインすると表示されます", "Sign in to Claude to see usage").into(),
        })
    }
}

/// blocking 取得（専用スレッドから呼ぶ。tauri の async runtime 上では呼ばない）
fn collect_claude(name: &'static str) -> Result<Vec<UsageWindow>, CollectError> {
    let token = read_claude_token()?;
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "headroom/0.1")
        .send()
        .map_err(|_| CollectError {
            status: Status::Failed,
            reason: tr("接続できません", "Can't connect").into(),
            hint: tr("ネットワーク接続を確認してください", "Check your network connection").into(),
        })?;

    if !resp.status().is_success() {
        return Err(http_error(name, &resp, tr("Claude を一度起動・使用すると回復します", "Open and use Claude once to recover")));
    }

    let u: ClaudeUsage = resp.json().map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("応答を解析できませんでした", "Couldn't parse the response").into(),
        hint: tr("アプリの更新が必要かもしれません", "The app may need an update").into(),
    })?;

    let mut windows = Vec::new();
    if let Some(w) = u.five_hour {
        windows.push(UsageWindow {
            label: "5-Hour".into(),
            consumption: w.utilization,
            amount_cents: None,
            resets_at: w.resets_at,
        });
    }
    if let Some(w) = u.seven_day {
        windows.push(UsageWindow {
            label: "Weekly".into(),
            consumption: w.utilization,
            amount_cents: None,
            resets_at: w.resets_at,
        });
    }
    Ok(windows)
}

// ---------- Codex collector (ChatGPT OAuth, ~/.codex/auth.json, 読み取り専用) ----------
#[derive(Deserialize)]
struct CodexAuth {
    tokens: CodexTokens,
}
#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    account_id: String,
}
#[derive(Deserialize)]
struct CodexUsageResp {
    rate_limit: Option<CodexRateLimit>,
}
#[derive(Deserialize)]
struct CodexRateLimit {
    primary_window: Option<CodexWindow>,
    secondary_window: Option<CodexWindow>,
}
#[derive(Deserialize)]
struct CodexWindow {
    used_percent: f64,
    reset_at: Option<i64>, // epoch 秒
}

fn epoch_to_rfc3339(epoch: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(epoch, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

fn read_codex_auth() -> Result<(String, String), CollectError> {
    let home = std::env::var("HOME").map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("ホームディレクトリが不明です", "Home directory not found").into(),
        hint: tr("再度お試しください", "Please try again").into(),
    })?;
    let data = std::fs::read_to_string(format!("{home}/.codex/auth.json")).map_err(|_| {
        CollectError {
            status: Status::Unconnected,
            reason: tr("未接続です", "Not connected").into(),
            hint: tr("Codex にログインすると表示されます", "Sign in to Codex to see usage").into(),
        }
    })?;
    let auth: CodexAuth = serde_json::from_str(&data).map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("資格情報を解析できません", "Couldn't parse credentials").into(),
        hint: tr("Codex に再ログインしてください", "Sign in to Codex again").into(),
    })?;
    Ok((auth.tokens.access_token, auth.tokens.account_id))
}

fn collect_codex(name: &'static str) -> Result<Vec<UsageWindow>, CollectError> {
    let (token, account) = read_codex_auth()?;
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get("https://chatgpt.com/backend-api/codex/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("ChatGPT-Account-ID", account)
        .header("originator", "codex_cli_rs")
        .header("User-Agent", "headroom/0.1")
        .send()
        .map_err(|_| CollectError {
            status: Status::Failed,
            reason: tr("接続できません", "Can't connect").into(),
            hint: tr("ネットワーク接続を確認してください", "Check your network connection").into(),
        })?;

    if !resp.status().is_success() {
        return Err(http_error(name, &resp, tr("Codex を一度起動・使用すると回復します", "Open and use Codex once to recover")));
    }

    let u: CodexUsageResp = resp.json().map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("応答を解析できませんでした", "Couldn't parse the response").into(),
        hint: tr("アプリの更新が必要かもしれません", "The app may need an update").into(),
    })?;
    let rl = u.rate_limit.ok_or(CollectError {
        status: Status::Failed,
        reason: tr("利用枠情報がありません", "No usage data").into(),
        hint: tr("Codex を一度使ってみてください", "Try using Codex once").into(),
    })?;

    let mut windows = Vec::new();
    if let Some(w) = rl.primary_window {
        windows.push(UsageWindow {
            label: "5-Hour".into(),
            consumption: w.used_percent,
            amount_cents: None,
            resets_at: w.reset_at.map(epoch_to_rfc3339).unwrap_or_default(),
        });
    }
    if let Some(w) = rl.secondary_window {
        windows.push(UsageWindow {
            label: "Weekly".into(),
            consumption: w.used_percent,
            amount_cents: None,
            resets_at: w.reset_at.map(epoch_to_rfc3339).unwrap_or_default(),
        });
    }
    Ok(windows)
}

// ---------- Cursor collector (state.vscdb の accessToken, 読み取り専用) ----------
// Cursor API は利用枠の「上限」を返さない（Enterprise は INCLUDED_IN_BUSINESS のみ／
// usage-based は $0、上限値の提供なし）。そこで「当月の実利用額(¢)」を取得し、プランの
// 「含まれる月枠」の上限で 2 バケツに分ける（CONTEXT.md: Consumption / On-Demand）：
//   - Monthly（含まれる枠）: min(実利用額, 上限) ÷ 上限 × 100（% 正規化, ADR-0003）
//   - On-Demand: 上限を超えた分を金額(¢)で表示（超過があるときのみ）
// 上限は各自のプランの included 額に合わせて変更すること。
const CURSOR_INCLUDED_LIMIT_CENTS: f64 = 2000.0; // 既定 $20/月（含まれる枠の上限）

/// Cursor の「含まれる月枠」の上限(¢)。プランは人により異なるため上書き可能：
/// 環境変数 `HEADROOM_CURSOR_BUDGET`（ドル）→ `~/.config/headroom/config.json` の
/// `cursorMonthlyBudgetUsd`（ドル）→ 既定 $20、の順で解決する。
fn cursor_budget_cents() -> f64 {
    if let Ok(usd) = std::env::var("HEADROOM_CURSOR_BUDGET").map(|v| v.trim().to_string()) {
        if let Ok(usd) = usd.parse::<f64>() {
            if usd > 0.0 {
                return usd * 100.0;
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(data) = std::fs::read_to_string(format!("{home}/.config/headroom/config.json")) {
            #[derive(Deserialize)]
            struct Cfg {
                #[serde(rename = "cursorMonthlyBudgetUsd")]
                usd: Option<f64>,
            }
            if let Ok(Some(usd)) = serde_json::from_str::<Cfg>(&data).map(|c| c.usd) {
                if usd > 0.0 {
                    return usd * 100.0;
                }
            }
        }
    }
    CURSOR_INCLUDED_LIMIT_CENTS
}

#[derive(Deserialize)]
struct CursorAggUsage {
    #[serde(rename = "totalCostCents")]
    total_cost_cents: Option<f64>,
}
#[derive(Deserialize)]
struct CursorReqUsage {
    #[serde(rename = "startOfMonth")]
    start_of_month: Option<String>,
}

fn read_cursor_token() -> Result<String, CollectError> {
    let home = std::env::var("HOME").map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("ホームディレクトリが不明です", "Home directory not found").into(),
        hint: tr("再度お試しください", "Please try again").into(),
    })?;
    let db = format!("{home}/Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    // macOS 同梱の sqlite3 で平文値を読み取る（追加依存なし・読み取り専用）
    let out = Command::new("sqlite3")
        .arg(&db)
        .arg("SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken';")
        .output()
        .map_err(|_| CollectError {
            status: Status::Unconnected,
            reason: tr("未接続です", "Not connected").into(),
            hint: tr("Cursor にログインすると表示されます", "Sign in to Cursor to see usage").into(),
        })?;
    let token = String::from_utf8_lossy(&out.stdout)
        .trim()
        .trim_matches('"')
        .to_string();
    if !out.status.success() || token.is_empty() {
        return Err(CollectError {
            status: Status::Unconnected,
            reason: tr("未接続です", "Not connected").into(),
            hint: tr("Cursor にログインすると表示されます", "Sign in to Cursor to see usage").into(),
        });
    }
    Ok(token)
}

/// best-effort: 請求サイクル開始(startOfMonth)＋30日 をリセット時刻とする
fn cursor_reset_at(client: &reqwest::blocking::Client, token: &str) -> String {
    let start = client
        .get("https://api2.cursor.sh/auth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "headroom/0.1")
        .send()
        .ok()
        .and_then(|r| r.json::<CursorReqUsage>().ok())
        .and_then(|u| u.start_of_month);
    match start.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok()) {
        Some(t) => (t + chrono::Duration::days(30)).to_rfc3339(),
        None => String::new(),
    }
}

fn collect_cursor(name: &'static str) -> Result<Vec<UsageWindow>, CollectError> {
    let token = read_cursor_token()?;
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("https://api2.cursor.sh/aiserver.v1.DashboardService/GetAggregatedUsageEvents")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "headroom/0.1")
        .body("{}")
        .send()
        .map_err(|_| CollectError {
            status: Status::Failed,
            reason: tr("接続できません", "Can't connect").into(),
            hint: tr("ネットワーク接続を確認してください", "Check your network connection").into(),
        })?;

    if !resp.status().is_success() {
        return Err(http_error(name, &resp, tr("Cursor を一度起動・ログインすると回復します", "Open and sign in to Cursor to recover")));
    }

    let agg: CursorAggUsage = resp.json().map_err(|_| CollectError {
        status: Status::Failed,
        reason: tr("応答を解析できませんでした", "Couldn't parse the response").into(),
        hint: tr("アプリの更新が必要かもしれません", "The app may need an update").into(),
    })?;

    let spent = agg.total_cost_cents.unwrap_or(0.0).max(0.0);
    let limit = cursor_budget_cents();
    let reset = cursor_reset_at(&client, &token);

    // 含まれる枠（% 表示、100% 頭打ち）
    let mut windows = vec![UsageWindow {
        label: "Monthly".into(),
        consumption: (spent / limit * 100.0).clamp(0.0, 100.0),
        amount_cents: None,
        resets_at: reset.clone(),
    }];
    // 含まれる枠を超えた分は On-Demand として金額表示（超過時のみ）
    let on_demand = spent - limit;
    if on_demand > 0.0 {
        windows.push(UsageWindow {
            label: "On-Demand".into(),
            consumption: 0.0,
            amount_cents: Some(on_demand),
            resets_at: reset,
        });
    }
    Ok(windows)
}

/// 取得を実行してスナップショット化する。クールダウン中は取得せず直近値(stale)を表示し、
/// 失敗時も直近の成功値があればそれを stale として保持する（AGENTS.md ガードレール1）。
fn collect_tool(
    name: &'static str,
    collector: fn(&'static str) -> Result<Vec<UsageWindow>, CollectError>,
) -> ToolSnapshot {
    let now = Instant::now();
    // クールダウン中：取得せず直近値(stale)＋残り時間を表示
    if let Ok(cache) = tool_cache().lock() {
        if let Some(tc) = cache.get(name) {
            if let Some(until) = tc.cooldown_until {
                if until > now {
                    let mins = (until - now).as_secs() / 60 + 1;
                    let has = tc.last_good.is_some();
                    return ToolSnapshot {
                        display_name: name.into(),
                        status: if has { Status::Ok } else { Status::Failed },
                        windows: tc.last_good.clone().unwrap_or_default(),
                        stale: true,
                        reason: Some(match lang() {
                            Lang::Ja => format!("取得が一時制限中（約{mins}分後に再取得）"),
                            Lang::En => format!("Fetch limited (retry in ~{mins}m)"),
                        }),
                        hint: if has {
                            None
                        } else {
                            Some(tr("利用枠ではなく取得APIの制限です", "Usage API is throttled, not your quota").into())
                        },
                    };
                }
            }
        }
    }
    match collector(name) {
        Ok(windows) => {
            if let Ok(mut cache) = tool_cache().lock() {
                let tc = cache.entry(name).or_default();
                tc.last_good = Some(windows.clone());
                tc.cooldown_until = None;
            }
            ToolSnapshot {
                display_name: name.into(),
                status: Status::Ok,
                windows,
                stale: false,
                reason: None,
                hint: None,
            }
        }
        Err(e) => {
            // 失敗：直近の成功値があれば stale として表示（値＋理由）、無ければエラー表示
            let last = tool_cache()
                .lock()
                .ok()
                .and_then(|c| c.get(name).and_then(|tc| tc.last_good.clone()));
            let has = last.is_some();
            ToolSnapshot {
                display_name: name.into(),
                status: if has { Status::Ok } else { e.status },
                windows: last.unwrap_or_default(),
                stale: has,
                reason: Some(e.reason),
                hint: if has { None } else { Some(e.hint) },
            }
        }
    }
}

fn collect_all() -> Vec<ToolSnapshot> {
    // MVP 対象（Claude → Cursor → Codex）
    vec![
        collect_tool("Claude", collect_claude),
        collect_tool("Cursor", collect_cursor),
        collect_tool("Codex", collect_codex),
    ]
}

/// resets_at(ISO) → 「2時間54分後」/ 2日以上先は「6月11日」
fn fmt_reset(iso: &str) -> String {
    use chrono::{DateTime, Local, Utc};
    match DateTime::parse_from_rfc3339(iso) {
        Ok(t) => {
            let secs = (t.with_timezone(&Utc) - Utc::now()).num_seconds();
            if secs <= 0 {
                tr("まもなくリセット", "resets soon").into()
            } else if secs >= 2 * 86400 {
                t.with_timezone(&Local)
                    .format(tr("%-m月%-d日", "%b %-d"))
                    .to_string()
            } else {
                let h = secs / 3600;
                let m = (secs % 3600) / 60;
                match (lang(), h > 0) {
                    (Lang::Ja, true) => format!("{h}時間{m}分後"),
                    (Lang::Ja, false) => format!("{m}分後"),
                    (Lang::En, true) => format!("in {h}h {m}m"),
                    (Lang::En, false) => format!("in {m}m"),
                }
            }
        }
        Err(_) => iso.to_string(),
    }
}

/// メニュー見出し用の小さなブランド色アイコン（角の取れた円。アンチエイリアス付き）
fn brand_dot(rgb: (u8, u8, u8)) -> tauri::image::Image<'static> {
    let s: i32 = 18;
    let c = (s as f32 - 1.0) / 2.0;
    // 18px キャンバスは維持しつつ円を一回り小さく（文字との位置揃えはそのまま）
    let rad = s as f32 / 2.0 - 3.0;
    let mut px = vec![0u8; (s * s * 4) as usize];
    for y in 0..s {
        for x in 0..s {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let d = (dx * dx + dy * dy).sqrt();
            let a = if d <= rad - 0.5 {
                1.0
            } else if d >= rad + 0.5 {
                0.0
            } else {
                rad + 0.5 - d
            };
            let i = ((y * s + x) * 4) as usize;
            px[i] = rgb.0;
            px[i + 1] = rgb.1;
            px[i + 2] = rgb.2;
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    tauri::image::Image::new_owned(px, s as u32, s as u32)
}

/// メニューバー（トレイ）用グリフ：ゲージ（メーター）＋下部に小さく「AI」。
/// テンプレート画像なので RGB は無視され、アルファのみが使われる
/// （色はライト/ダークに合わせて macOS が自動反転。`icon_as_template(true)`）。
fn menubar_icon() -> tauri::image::Image<'static> {
    let s: i32 = 36; // 18pt @2x（Retina で潰れないよう高解像度で生成）
    let c = (s as f32 - 1.0) / 2.0; // 水平中心 17.5
    let gy = 19.0_f32; // ゲージ中心（やや下げて全体を縦中央に寄せる）

    // 線分（カプセル）の符号付き距離。針の描画に使う。
    fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, th: f32) -> f32 {
        let (pax, pay) = (px - ax, py - ay);
        let (bax, bay) = (bx - ax, by - ay);
        let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
        ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt() - th
    }
    // 距離 → 被覆率（1px のアンチエイリアス帯）
    let cov = |d: f32| (0.5 - d).clamp(0.0, 1.0);
    let ang = 55.0_f32.to_radians();
    let (nx, ny) = (c + ang.cos() * 10.5, gy - ang.sin() * 10.5); // 針先端（上向き右）

    // 下部の開いた部分に小さく「AI」（A=2本脚＋横棒 / I=縦棒）
    let ai_h = 6.0_f32; // 文字高
    let ai_th = 1.15_f32; // 線の太さ
    let ai_cy = 26.3_f32; // 文字の縦中心
    let acx = c - ai_h * 0.44; // A の中心x
    let icx = c + ai_h * 0.42; // I の中心x
    let aw = ai_h * 0.74; // A の幅
    let (a_top, a_bot, a_cross) = (ai_cy - ai_h / 2.0, ai_cy + ai_h / 2.0, ai_cy + ai_h * 0.12);

    let mut px = vec![0u8; (s * s * 4) as usize];
    for y in 0..s {
        for x in 0..s {
            let (fx, fy) = (x as f32, y as f32);
            let (dx, dy) = (fx - c, fy - gy);
            let d = (dx * dx + dy * dy).sqrt();

            // 270° スピードメーターの目盛弧（下 90° を開ける）
            let arc = cov((d - 12.0).abs() - 2.0) * cov(dy - dx.abs());
            // 針（中心 → 上向き右）と中心ハブ
            let needle = cov(seg(fx, fy, c, gy, nx, ny, 1.7));
            let hub = cov(d - 2.4);
            // 「AI」の文字
            let a_letter = cov(seg(fx, fy, acx, a_top, acx - aw / 2.0, a_bot, ai_th))
                .max(cov(seg(fx, fy, acx, a_top, acx + aw / 2.0, a_bot, ai_th)))
                .max(cov(seg(fx, fy, acx - aw * 0.30, a_cross, acx + aw * 0.30, a_cross, ai_th)));
            let i_letter = cov(seg(fx, fy, icx, a_top, icx, a_bot, ai_th));
            let ai = a_letter.max(i_letter);

            // テンプレート画像：黒（RGB=0、初期化済み）＋アルファのみ
            let i = ((y * s + x) * 4) as usize;
            px[i + 3] = (arc.max(needle).max(hub).max(ai) * 255.0).round() as u8;
        }
    }
    tauri::image::Image::new_owned(px, s as u32, s as u32)
}

fn tool_icon(name: &str) -> tauri::image::Image<'static> {
    // 生成したブランド色の円（design.md §3 の暫定色）
    if name.contains("Claude") {
        brand_dot((217, 119, 87))
    } else if name.contains("Cursor") {
        brand_dot((90, 90, 95))
    } else if name.contains("Codex") {
        brand_dot((88, 108, 240))
    } else {
        brand_dot((130, 130, 135))
    }
}

/// 取得結果からネイティブメニューを構築する（NSMenu 操作のためメインスレッドで呼ぶこと）
fn build_menu(app: &tauri::AppHandle, tools: &[ToolSnapshot]) -> tauri::Result<Menu<tauri::Wry>> {
    let mut b = MenuBuilder::new(app);
    for t in tools {
        // Tool 見出し（アイコン付き・クリックで対応アプリを起動）
        b = b.item(
            &IconMenuItemBuilder::with_id(format!("open:{}", t.display_name), &t.display_name)
                .icon(tool_icon(&t.display_name))
                .build(app)?,
        );
        match t.status {
            // 利用枠は情報表示（クリック不可＝グレー）
            Status::Ok => {
                for w in &t.windows {
                    // 金額枠（On-Demand）は実額、それ以外は「残り %」
                    let value = match w.amount_cents {
                        Some(cents) => format!("${:.2}", cents / 100.0),
                        None => {
                            let n = (100.0 - w.consumption).round().max(0.0) as i64;
                            match lang() {
                                Lang::Ja => format!("残り {n}%"),
                                Lang::En => format!("{n}% left"),
                            }
                        }
                    };
                    let line = format!(
                        "    {}  ·  {}  ·  {}{}",
                        w.label,
                        value,
                        fmt_reset(&w.resets_at),
                        if t.stale { tr("  ·  前回値", "  ·  cached") } else { "" }
                    );
                    b = b.item(&MenuItemBuilder::new(line).enabled(false).build(app)?);
                }
                // stale（再取得できず直近値を表示中）の理由を併記
                if t.stale {
                    if let Some(reason) = &t.reason {
                        b = b.item(
                            &MenuItemBuilder::new(format!("    ⚠ {reason}"))
                                .enabled(false)
                                .build(app)?,
                        );
                    }
                    if let Some(hint) = &t.hint {
                        b = b.item(
                            &MenuItemBuilder::new(format!("       {hint}"))
                                .enabled(false)
                                .build(app)?,
                        );
                    }
                }
            }
            // 失敗/未接続は理由＋対処をグレーで
            _ => {
                if let Some(reason) = &t.reason {
                    b = b.item(
                        &MenuItemBuilder::new(format!("    ⚠ {reason}"))
                            .enabled(false)
                            .build(app)?,
                    );
                }
                if let Some(hint) = &t.hint {
                    b = b.item(
                        &MenuItemBuilder::new(format!("       {hint}"))
                            .enabled(false)
                            .build(app)?,
                    );
                }
            }
        }
    }
    b = b.separator();
    b = b.item(&MenuItemBuilder::with_id("refresh", tr("更新", "Refresh")).build(app)?);
    b = b.item(&MenuItemBuilder::with_id("quit", tr("Headroom を終了", "Quit Headroom")).build(app)?);
    b.build()
}

/// 取得（blocking）→ メインスレッドでメニュー差し替え。専用スレッドから呼ぶこと。
fn refresh_menu(app: &tauri::AppHandle) {
    let tools = collect_all();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(menu) = build_menu(&app2, &tools) {
            if let Some(tray) = app2.tray_by_id("main-tray") {
                let _ = tray.set_menu(Some(menu));
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(QuitState(AtomicBool::new(false)))
        .setup(|app| {
            // Dock 非表示のメニューバー常駐アプリ（ADR-0002）
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // ログイン時に自動起動（リリースビルドのみ。System 設定 > ログイン項目 で解除可）
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }

            // 初期メニュー（取得前）
            let loading = MenuItemBuilder::new(tr("読み込み中…", "Loading…")).enabled(false).build(app)?;
            let quit = MenuItemBuilder::with_id("quit", tr("Headroom を終了", "Quit Headroom")).build(app)?;
            let init_menu = MenuBuilder::new(app)
                .item(&loading)
                .separator()
                .item(&quit)
                .build()?;

            TrayIconBuilder::with_id("main-tray")
                .icon(menubar_icon())
                .icon_as_template(true)
                .tooltip("Headroom")
                .menu(&init_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    let id = event.id.as_ref();
                    if id == "quit" {
                        app.state::<QuitState>().0.store(true, Ordering::SeqCst);
                        app.exit(0);
                    } else if id == "refresh" {
                        let h = app.clone();
                        std::thread::spawn(move || refresh_menu(&h));
                    } else if let Some(app_name) = id.strip_prefix("open:") {
                        // id に埋め込んだ Tool 名で対応アプリを起動（インストールされていれば）
                        let _ = Command::new("open").args(["-a", app_name]).spawn();
                    }
                })
                .build(app)?;

            // 起動時に取得 → 以後 5 分ごとに更新（429 回避のため低頻度）
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                refresh_menu(&handle);
                std::thread::sleep(std::time::Duration::from_secs(300));
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // メニューバー常駐：ウィンドウが無くてもアプリは終了しない（Quit のみ終了）
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !app.state::<QuitState>().0.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
