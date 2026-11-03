# Civitai 検索の仕様（2026-11-04）

## キーワード＋並び順

- Civitai v1 `/models` は `query` があると `sort` を無視する（評価順・DL 数順・新着で結果が順番まで同一、実測）。キーワードなしなら `sort` は効く。`period` はキーワードありでも効く。
- キーワード検索は約 1,000 件で打ち切られる（`nextCursor` が無くなる）。
- そこで lmx は「キーワード＋並び順」のときだけ、関連度順の結果を最大 1,000 件集めてサーバー側で並べ直し、`r<offset>` カーソルで少しずつ返す。
  - 集めた結果はカード表示に必要な項目だけに絞って 90 秒キャッシュ。並び順を変えても取り直さない。
  - 並びのキー: DL 数順 = `stats.downloadCount`、評価順 = `stats.thumbsUpCount`、新着 = 各バージョンの `publishedAt` の最大値。同点は関連度順のまま。
  - 応答の `resorted: { pool, capped }` で画面に「上位 N 件を並べています」と出す。1,000 件を超えたときは期間で絞るよう案内する。
- 並び順に「関連度（標準）」を追加。こちらは Civitai の結果をそのまま返す。

実測（frieren、809 件）: 初回 3.3 s、並び順の切り替え 0.03〜0.04 s、2 ページ目 47 ms。

## ベースモデルの候補

- 一覧は Civitai 公式の `/api/v1/enums` の `ActiveBaseModel`（82 件）。`data/civitai_enums.json` に保存し、1 日 1 回取り直す。2 回目以降の取得で初めて出てきた名前は 14 日間「新着」表示。
- 検索画面に出すものは設定画面で選ぶ。選択は `data/prefs.json`（サーバー）に保存し、スマホと PC で共通。全種類共通のリスト。
- 未設定のときは、手元にあるベースモデル（件数の多い順）を選んだ状態から始める。
- 検索画面の「＋ほか」から、選んでいないものもその場だけ選べる。
- 設定と検索画面は同じリアクティブなストアを見ているので、リロードは不要。

API: `GET /api/civitai/base-models` → `{ all, selected, new, local, fetched_at, error }`、`PUT /api/civitai/base-models { selected }`。
