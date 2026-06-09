# Headroom

サブスクリプション契約している各 AI コーディングツールの「利用枠の残量」を一目で把握するための macOS メニューバー常駐ツール。メニューバーにはアイコンのみを表示し、クリックすると**ネイティブメニュー（NSMenu）**が開いて各 Tool の Usage Window が項目として並ぶ（UI は OS 標準に委ねる。ADR-0005）。コストの可視化は従的な情報で、核ではない。

## Language

**Tool**:
追跡対象の AI コーディングサービス1つ。MVP 対象は Claude・Cursor・Codex の3つ（この順で着手）。ネイティブメニューに全 Tool を並べ、各 Tool の見出し（アイコン付き）はクリックで対応アプリを起動する。Copilot は個人残量の到達可能なエンドポイントが不明なため MVP 対象外（post-MVP）。Grok（Grok Build）も、利用枠取得が WebSocket JSON-RPC（`check_subscription` @ `wss://code.grok.com/ws/code-agent`、CLI バージョン検証あり）専用で安定した REST/ローカルキャッシュが無いため MVP 対象外（post-MVP。xAI が REST 利用枠 API を公開するか、CLI が利用枠をローカルキャッシュしたら再検討）。
_Avoid_: プロバイダ, サービス, ベンダー（混在させない）

**Usage Window**:
一定期間ごとにリセットされる利用上限の単位。**Tool ごとに個数も性質も異なる**（固定スキーマではない）：Claude=「5時間」「週次」、Cursor=「月次の含まれる利用枠」など。各 Usage Window は `label` ＋ Consumption(0-100%) ＋ Reset を持ち、UI はこれを動的に並べる。
_Avoid_: クォータ, リミット（単独では使わない）, プラン

**Consumption**:
ある Usage Window 内でこれまでに消費した割合（0〜100%）。**表示は常にパーセントで統一**する。サーバーが直接パーセントを返す場合（Claude の `utilization`）はそのまま、`used / limit` の金額・回数で返す場合（Cursor の `$20 / $20`）は Collector が `used ÷ limit × 100` に正規化する。
_Avoid_: utilization（生フィールド名。ドメイン語としては使わない）, 使用量（曖昧）, リミット

**Collector**:
1つの Tool 専用のアダプタ。認証情報の取得 → その Tool の usage エンドポイント呼び出し → 正規化された Snapshot 返却までを担う。Tool ごとの差異（認証方式・エンドポイント・単位・Usage Window の構成）を吸収する境界。

**Snapshot**:
ある時点で1つの Collector が返す、その Tool の Usage Window 集合。アプリはこれをネイティブメニューに描画するだけで、Tool 固有の事情を知らずに済む。状態で表示が変わる：**成功**（Usage Window をグレーの情報行で表示）／**失敗**（取得エラー・トークン失効。グレーの無効項目で「理由＋対処（どうすれば直るか）」を併記）／**未接続**（資格情報が見つからない。同じくグレーで案内）。

**On-Demand**:
プランに含まれる Usage Window を超えた分の従量課金枠（Claude の `extra_usage`、Cursor の "On-Demand Usage"）。サブスクの「含まれる枠」とは別バケツ。**Cursor では別の Usage Window として表示する**：含まれる枠（$20）を超えた分を**金額（$）**で出す（超過があるときのみ）。青天井で上限が無いため % ではなく実額で示す。Claude の `extra_usage` は post-MVP。
_Avoid_: 従量, 超過（単独では使わない）

**Reset**:
Usage Window の Consumption がゼロに戻る時刻。サーバーは絶対時刻で返す（Claude では `resets_at`、ISO8601）。「Resets in 2h 46m」のような残り時間表示は `now → resets_at` でクライアント側計算する。

## Example dialogue

> **Dev**: 「Claude の Consumption が 42% って、これ残り42%ってこと？」
> **Domain**: 「いや、消費した方が42%。残りは58%。Consumption は常に『使った割合』。」
> **Dev**: 「Reset は？」
> **Domain**: 「その Usage Window が 42% から 0% に戻る時刻。5時間枠なら最初の利用から5時間後。残り時間で『2h46m』のように見せる。」
