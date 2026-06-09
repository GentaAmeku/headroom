# Usage Window を「ツールごとに可変・パーセント正規化のリスト」でモデル化する

## Context

MVP 対象の 3 Tool は利用枠の形が大きく異なる。Claude は `five_hour` / `seven_day` のパーセント＋ロールング時間窓、Cursor は月次・ドル建ての「included usage」（`$20 / $20`）＋ On-Demand 従量。「5h＋週次の固定スキーマ」では Cursor が乗らない。

## Decision

各 Collector は、その Tool の Usage Window を**可変個のリスト**として返す（Snapshot）。各 Usage Window は `{ label, consumption(0-100%), resets_at }` に正規化する。表示は**常にパーセントで統一**し、通貨・回数ベースのソースは Collector 内で `used ÷ limit × 100` に変換する。UI は Snapshot の Usage Window を上から動的に並べるだけで、Tool 固有の事情を持たない。

## Considered Options

- **5h＋週次の固定2スキーマ** — UI は予測可能だが Cursor が構造的に乗らず、3 Tool 目で作り直しになる。却下。
- **単位（percent / currency）と生値（used/limit）を Usage Window に保持して出し分け** — スクショの `$20 / $20` を厳密再現できるが、MVP では不要と判断（パーセント統一で十分）。post-MVP の選択肢として温存。

## Consequences

- 新しい Tool の追加は「Collector を1つ実装して正規化 Snapshot を返す」だけで UI 改修不要。
- On-Demand（Claude `extra_usage` / Cursor "On-Demand Usage"）は Usage Window と別バケツ。MVP では取得・表示ともに対象外（post-MVP）。
