# lmx（LoRA Manager 軽量スタンドアロン版）開発計画

**Goal:** ComfyUI-Lora-Manager の中核機能を引き継ぎつつ、Civitai の検索とダウンロードをアプリ内で完結でき、スマホ（Web/PWA）から快適に使える軽量・高速なスタンドアロン版を作る。

**Architecture:** Rust 製の単一バイナリ（axum）が REST API・進捗配信（SSE）・Web UI の静的配信を担当し、UI は Vue 3 + TypeScript の PWA。既存の `<stem>.metadata.json` サイドカーを正として読み書きし、本家 LoRA Manager と同じライブラリを共存・相互切り戻し可能にする。

**Tech Stack:** Rust（axum / tokio / reqwest / rusqlite + FTS5 / serde_json / sha2(asm) / rayon / notify / image / rust-embed）、Vue 3 + Vite + TypeScript（PWA, 仮想スクロール）

**名前（仮）:** `lmx`（未決事項 Q1）

---

## 0. 現状と前提（2026-11-04 実測）

| 項目 | 実測値・事実 | 取得方法 |
|---|---|---|
| フォーク | `rushe19/ComfyUI-Lora-Manager`（PUBLIC、parent=willmiao）作成済み。クローン先 `<開発用クローン>`、remote: origin=フォーク / upstream=本家 | `gh repo fork` → `gh repo view` |
| 稼働中の本家 | `<LoRA Manager のインストール先>`（別ポートで稼働中、ローカルパッチあり）。今回は一切変更していない | `netstat` / `git status` |
| ライセンス | GPL-3.0。移植コードは派生物になるため lmx も GPL-3.0 とする | `gh repo view --json licenseInfo` |
| 本家の規模 | Python 約 21.5 万行（テスト込み）、static JS 約 12 万行、API ルート 143 以上、画面: loras / checkpoints / embeddings / other / recipes / statistics / workflows | `git ls-files \| wc -l` 系 |
| ライブラリ規模 | catalog 15,750 件（LoRA 15,402 / checkpoint 227 / embedding 107 / other 15） | `cache/model/default.sqlite` を読み取り専用で集計・起動ログ |
| 本家の性能 | 一覧 API（50件）0.17〜0.33 s、`/loras` HTML 230 KB、RSS 約 311 MB、起動〜キャッシュ復元 約 1.2 s、初回フルスキャン 約 94 分（0.3〜0.4 s/ファイル） | curl 計測・PowerShell・起動ログ・前回デプロイ記録 |
| ストレージ | モデル置き場は NVMe SSD（8 TB）→ 並列ハッシュで短縮できる見込み（**未計測**、Phase 0 で検証） | `Get-PhysicalDisk` |
| Civitai 検索 API | `GET https://civitai.red/api/v1/models?query=&types=LORA&baseModels=&sort=&nsfw=true&cursor=` はキー無しで 200・約 0.2〜0.3 s。cursor ページングあり。**CORS ヘッダ無し → ブラウザ直叩き不可、サーバー経由のプロキシ必須** | `civ_probe.py` |
| 検索品質 | ~~フィルタ無し `query=miku` が 4 件~~ → **誤り**（`limit=5` で取ったため）。`limit=100` では 20 クエリすべて 91〜100 件・関連性良好・日本語も可。1 ページの件数が limit 未満になることはあるが cursor で続きが取れる。外れ値で 17.6 s の応答が 1 回 | `civ_quality.py`（Phase 0 T0-1） |
| 本家サイドカー | キー: file_name, model_name, file_path, size, modified, sha256, base_model, preview_url, preview_nsfw_level, notes, from_civitai, civitai, tags, modelDescription, civitai_deleted, favorite, exclude, db_checked, skip_metadata_refresh, metadata_source, last_checked_at, hash_status, autov3, quantization, usage_tips, sub_type | 実ファイル 1 件を確認（全件の揺れは Phase 0 で調査） |
| 開発環境 | cargo 1.98.1 / Node 22 あり。Go・Bun は無し | `cargo --version` |
| 不要になるもの | ComfyUI カスタムノード・ウィジェット（スタンドアロンで動けば良い、と利用者確認済み） | 会話 |
| 利用実態 | レシピ 0 件、example images パス未設定 → この2機能は後回しで良い | 起動ログ |
| ローカル archive DB | `<LoRA Manager>\civitai\civitai.sqlite`（4.2 GB、models 526,831 / model_versions 684,956 / model_files 717,362）。**`model_files.sha256` に索引が無く、本家のハッシュ照会は毎回全件走査で 0.34〜0.38 s/件**（15.4k 件なら照会だけで約 92 分） | `archive_bench.py`（EXPLAIN QUERY PLAN = `SCAN model_files`） |
| archive DB の当たり率 | sha256 列だけを一括抽出すると 716,694 行が 0.8 s。手元の LoRA 15,402 件と照合して **8,425 件（54.7%）がヒット、照合自体は 19 ms** | 同上 |
| Tailscale | Serve で公開するポートは、ほかのサービスが使っていない空きを選ぶ（tailnet 限定） | `tailscale serve status` |

## 1. 技術選定

### 「Rust は Web に向かない？」への答え

- **バックエンドとしての Rust は Web に非常に向いている。** axum + tokio は高速・省メモリで、単一 exe 配布もでき、HTTP/JSON/WebSocket/SSE の実績も十分。
- 向き不向きが分かれるのは **フロントエンドを Rust（WASM: Leptos/Yew/Dioxus）で書く場合**。ギャラリー型 UI・仮想スクロール・PWA の資産は JS/TS 側が圧倒的に厚いので、UI は TypeScript で書くのが現実的。
- よって「Rust バックエンド + TS の Web UI」構成なら、スマホ対応の必須要件と軽量・高速化の両方を満たせる。

### 言語比較

| 案 | 長所 | 短所 | 判定 |
|---|---|---|---|
| Rust（axum） | 省メモリ・高速、並列ハッシュが安全に書ける、単一バイナリ、型で壊れにくい。ツールチェーン導入済み | 開発速度は Python より遅い。コンパイル時間 | **推奨** |
| Go | 単一バイナリ、学習・開発が速い、並行処理が簡単 | 未導入。型表現が弱く、大規模移植で Rust ほどの安心感がない | 次点 |
| TypeScript（Bun/Node） | UI と言語統一 | ランタイム常駐でメモリが重め。「軽量化」の目的が薄れる | 非推奨 |
| Python のまま改造 | Civitai 検索だけなら最速（数日） | 軽量・高速化はほぼ達成できない。本家の巨大なコードを抱え続ける | 短期の補助案（Q2） |

### 性能目標（Phase 1 完了時に実測で判定）

| 指標 | 本家（実測） | lmx 目標 |
|---|---|---|
| 一覧 API 50 件 | 0.17〜0.33 s | p95 < 30 ms |
| 常駐メモリ | 約 311 MB | < 100 MB |
| 起動（インデックス済み） | 約 1.2 s | < 1 s |
| 初回フルスキャン 15.4k | 約 94 分 | ハッシュ読み出しは 4 並列 3 GB/s 実測 → 約 14 分 + メタデータ作成 |
| スマホの初回表示 | HTML 230 KB + 原寸プレビュー | サムネイル WebP 化で一覧 1 画面 < 1 MB |

## 2. スコープ

### 引き継ぐ（優先順）

1. ライブラリ一覧（LoRA / checkpoint / embedding / other 共通エンジン）・フォルダツリー・フィルタ（base model / タグ / お気に入り / NSFW）・ローカル検索・並べ替え
2. **Civitai 検索 → バージョン選択 → ダウンロード（今回の主目的）**、所持済みバッジ、ダウンロードキュー（再開対応）
3. URL 貼り付けダウンロード（複数行一括）、パステンプレートによる自動振り分け
4. メタデータ取得: ローカル archive DB 優先 → CivitAI（本家の LOCAL PATCH と同じ方針）、トリガーワードコピー、編集（タグ・メモ・お気に入り・base model）
5. ハッシュ計算・重複検出・移動・リネーム・削除（ゴミ箱方式）・一括操作
6. 所持モデルの新バージョン確認、統計

### やらない（当面）

- ComfyUI ノード／ウィジェット／ワークフロー連携
- レシピ・example images（利用 0 件。需要が出たら Phase 5 以降）
- HuggingFace / ModelScope / TensorArt ソース、LLM エージェント機能、10 言語 i18n（日本語 + 英語のみ）

## 3. アーキテクチャ

```
lmx/                         ← フォーク内の新ディレクトリ（Q3）
  Cargo.toml                 ← workspace
  crates/
    lmx-core/      ドメイン型、サイドカー I/O（未知キー保持）、設定読み込み
    lmx-scan/      フォルダ走査、safetensors ヘッダ解析、並列 SHA256、差分検出、ファイル監視
    lmx-index/     自前 SQLite（FTS5）インデックス。サイドカーから再構築可能なキャッシュ
    lmx-civitai/   Civitai クライアント（検索・ハッシュ照会・バージョン取得・ダウンロード）、ホスト別レート制御
    lmx-archive/   CivArchive メタデータ archive DB の読み取り専用アクセス
    lmx-server/    axum ルーター、REST、SSE 進捗、静的配信（rust-embed）、ダウンロードキュー
  web/             Vue 3 + Vite + TS（PWA）。build 成果物は rust-embed で exe に同梱
  tests/fixtures/  実データから採取したサイドカー・Civitai 応答 JSON
```

### 重要な設計判断

1. **サイドカーが正、SQLite はキャッシュ。** 本家と同じ `<stem>.metadata.json` を読み書きする。書き込みは `serde_json::Value` 往復＋既知フィールドだけ差し替え、**未知キー・順序を保持**して本家が読めなくなる事故を防ぐ。centralized モードのパス導出は本家 `py/utils/sidecar_paths.py` と同じ規則を移植する。
2. **初回起動は再ハッシュしない。** サイドカーの `sha256` とファイルの size/mtime が一致すればそのまま採用 → 15k 件のインデックス構築を数十秒以内に収める（目標、要実測）。
3. **本家と共存。** lmx は自前の DB（`lmx/data/index.sqlite`）を持ち、本家の `cache/model/default.sqlite` には書かない。ポートも別（例: 9100）。移行期間は両方起動でき、いつでも本家に戻れる。
4. **Civitai はすべてサーバー経由。** CORS 無しのためブラウザ直叩き不可（実測）。API キーはサーバー側だけが保持し、UI には出さない。ホスト別レート制御は「待つ前に枠を予約する」方式（読む→寝る→書くだと並列時に一斉発射する）。
5. **スマホ前提の UI。** モバイルファースト（下部ナビ、2 列カード、無限スクロール + 仮想化、長押しメニュー）、PC では多列グリッド + サイドバー。プレビューはサーバー側で WebP サムネイル（例: 長辺 384px）を生成・キャッシュし、詳細画面だけ原寸を読む。動画プレビューはポスター画像 + タップ再生。
6. **公開範囲。** 既定は LAN バインド + Tailscale Serve で HTTPS 化（PWA インストールと Service Worker に必須）。削除などの破壊的操作が LAN に露出するため、簡易パスワード（Cookie セッション）をオプションで用意（Q5）。
7. **メタデータ取得元の優先順位を設定で選べる（利用者要望）。** 設定 `metadata_source` を 3 モードで用意する。

   | モード | 動作 | 向いている場面 |
   |---|---|---|
   | `archive_first`（既定） | ローカル archive DB で見つかればそれで `metadata.json` を作り、無いものだけ CivitAI に問い合わせる | 通常運用 |
   | `archive_only` | ローカル archive DB だけを使い、CivitAI には一切問い合わせない。見つからないものは「未解決」として残す | 15k 件の一括処理、オフライン、API 制限を避けたいとき |
   | `civitai_first` | 本家既定と同じく CivitAI を先に照会し、失敗時に archive DB | 最新情報（新バージョン・削除状態）を優先したいとき |

   - 一括メタデータ取得の画面でもジョブ単位でモードを上書きできる（例: まず `archive_only` で 8.4k 件を数秒で埋め、残りだけ後で `civitai_first` で回す）。
   - archive DB は**読み取り専用で開き、ファイルには一切書かない**。起動時（または DB 更新検知時）に sha256 列だけを抽出して lmx 自前の DB に `archive_hash(sha256 PRIMARY KEY, model_id, version_id)` を作る（実測 0.8 s / 71.7 万行）。これで照会は 0.36 s/件 → ほぼ 0 になる。
   - 作成した `metadata.json` には `metadata_source: "archive_db"` を記録し、後から CivitAI で上書き更新できるようにする（本家と同じ値なので互換）。
   - archive の data 列は Civitai API と同形の JSON。本家 `_get_version_with_model_data` と同じ組み立て方（version 行 + model 行 + files）を移植し、出力 JSON を本家と突き合わせるテストを置く。
8. **Android に PWA としてインストールできること（利用者要望）。** Chrome/Samsung Internet で WebAPK（ランチャーに本物のアプリとして出る形）になる条件をすべて満たす。
   - **安全なオリジン:** `http://<LAN IP>` では WebAPK にならない（Web の仕様）。Tailscale Serve の空いているポートで HTTPS 化する: `tailscale serve --bg --https=10000 http://127.0.0.1:9100` → `https://<machine>.<tailnet>.ts.net:10000/`（ほかのサービスが使っているポートは避ける）。
   - **manifest.webmanifest:** `name` / `short_name` / `start_url: "/"` / `scope: "/"` / `display: "standalone"` / `theme_color` / `background_color` / `icons` に 192×192 と 512×512（`purpose: "any"` と `"maskable"` を別エントリで）。
   - **Service Worker:** アプリシェル（HTML/JS/CSS/アイコン）をキャッシュし、`/api/*` は常にネットワーク優先、サムネイルは容量上限付きキャッシュ。オフライン時は「サーバーに接続できません」画面を出す。
   - **スマホらしさ:** 戻るボタンでモーダルが閉じる（History API）、セーフエリア対応（`viewport-fit=cover`）、プル更新、長押しメニュー。
   - 検証: Chrome DevTools の Application → Manifest でエラー 0、Android 実機の Chrome で「アプリをインストール」が出てランチャーに追加されることを利用者に確認してもらう（実機はこちらから操作できないため）。

### API（初版）

| メソッド・パス | 用途 |
|---|---|
| `GET /api/models?type=&q=&base=&tag=&folder=&fav=&sort=&cursor=&limit=` | 一覧（カーソルページング） |
| `GET /api/models/{id}` / `PATCH /api/models/{id}` | 詳細・編集（サイドカー更新） |
| `GET /api/folders?type=` | フォルダツリー |
| `GET /api/thumb/{id}?w=384` | サムネイル（WebP、ETag） |
| `GET /api/civitai/search?q=&types=&base=&sort=&period=&nsfw=&cursor=` | Civitai 検索プロキシ + 所持済み判定 |
| `GET /api/civitai/models/{modelId}` | バージョン一覧（所持済み・DL 済みバッジ付き） |
| `POST /api/downloads` / `GET /api/downloads` / `DELETE /api/downloads/{id}` | キュー投入・一覧・取消 |
| `GET /api/events` | SSE（DL 進捗・スキャン進捗） |
| `POST /api/scan` / `POST /api/metadata/refresh` | スキャン・メタデータ取得（非同期ジョブ） |

## 4. フェーズ計画

各フェーズの「完了条件」を実測で満たしたら次へ進む。目安日数は 1 日数時間の作業を想定した**概算**（Phase 0 の結果で見直す）。

### Phase 0: 検証スパイク（目安 1〜2 日）

計画の前提を実測で固める。成果は `lmx/docs/spike-results.md` に記録。

- **T0-1 Civitai 検索品質:** 同じクエリ 20 個（日本語キャラ名・英語・作者名）で v1 API の件数・順位を、サイトの検索結果と比較。弱ければ代替（サイトが使う検索バックエンドの利用可否、規約上の扱い）を調査し採否を決める。
- **T0-2 ダウンロード認証:** `civitai.red` で API キー付きダウンロード（`Authorization: Bearer`）がリダイレクト先まで通るか、Range 再開が効くか、NSFW モデルで可否を確認（小さいファイルで実施）。
- **T0-3 サイドカー全数調査:** 15.7k 件のキー出現頻度・型の揺れ・`civitai` オブジェクトの形を集計し、`tests/fixtures/sidecars/` に代表 30 件を匿名化せず保存（ローカル限定、コミット対象外に）。
- **T0-4 ハッシュ速度:** モデル置き場上の 50 ファイルで、Rust の sha2(asm) 単スレッド / rayon 2・4・8 並列の MB/s を計測し、既定並列数を決める。
- **T0-5 雛形:** `cargo new --lib` で workspace、`lmx-server` に `/api/health` だけ返す axum、`web/` に Vite + Vue 3 + TS。`cargo test` と `npm run build` が通ること。

- **T0-6 archive DB:** 索引の有無・照会速度・一括抽出速度・手元ライブラリとの当たり率を計測（**実施済み**: 0.36 s/件の全件走査、一括抽出 0.8 s、当たり率 54.7%）。archive の JSON から作る `metadata.json` を本家の出力と 5 件で突き合わせる。
- **T0-7 PWA の前提:** Tailscale Serve の空きポート確認（**実施済み**: 空きポート 10000 を使う）、雛形に manifest・アイコン・Service Worker を入れて Lighthouse／DevTools でインストール可能判定を確認。

完了条件: 7 項目すべての結果が spike-results.md にある。（未決事項 Q1〜Q6 は 2026-11-04 に「すべておすすめ通り」で決定済み）

### Phase 1: 読み取り専用ライブラリ（目安 5〜7 日）

- サイドカー読み込み（未知キー保持のラウンドトリップテスト必須）
- フォルダ走査 + size/mtime による差分検出、インデックス構築（FTS5: model_name / file_name / tags / trained words / creator）
- 一覧・詳細・フォルダ API、サムネイル生成キャッシュ
- PWA シェル: 一覧（仮想スクロール）、詳細シート、フィルタ、ローカル検索、トリガーワードコピー、ダーク/ライト
- Windows 常駐起動スクリプト（PowerShell Start-Process、ログ追記）

完了条件: 15.7k 件が表示でき、一覧 API p95 < 30 ms・RSS < 100 MB を実測、スマホ（Tailscale HTTPS）で一覧→詳細→コピーが動き、**Android の Chrome で PWA としてインストールしてランチャーから起動できる**。サイドカーは 1 バイトも変更されていない（読み取りのみ）ことをハッシュ比較で確認。

### Phase 2: Civitai 検索とダウンロード（主目的、目安 5〜7 日）

- 検索プロキシ（クエリ・種別・base model・並び替え・期間・NSFW）、カーソル無限スクロール
- 所持済み判定: 各バージョンの files[].hashes.SHA256 をローカルの hash 索引と照合し「所持済み」「別バージョン所持」を表示
- バージョン選択・ファイル選択（fp16/pruned 等）・保存先（パステンプレート + base model 別ルート）
- ダウンロードキュー: 同時数制限、Range 再開、一時ファイル → 完了後 SHA256 検証 → リネーム、プレビュー取得、サイドカーを API 応答から生成（再照会不要）
- URL 貼り付け（複数行一括）
- SSE で進捗表示、スマホでバックグラウンドに回っても再表示で復元

完了条件: スマホから検索 → DL → 一覧に即反映、を実機で確認。DL 途中でサーバーを kill → 再起動で再開。ハッシュ不一致は破棄して失敗表示。テストは記録済み Civitai 応答フィクスチャで決定的に通す（実通信テストは `live` 指定時のみ）。

### Phase 3: メタデータ管理と書き込み操作（目安 7〜10 日）

- 新規ファイルの並列ハッシュ、メタデータ取得（設定 `metadata_source` の 3 モード、既定 `archive_first`。ジョブ単位で上書き可）、一括取得ジョブ（キャンセル可・進捗・モード別の件数内訳: archive で解決 / CivitAI で解決 / 未解決）
- 完了条件に追加: 手元 15.4k 件を `archive_only` で一括処理し、CivitAI へのリクエスト数 0・所要時間を実測（目標: ハッシュ済みなら数分以内）
- 編集（タグ・メモ・お気に入り・base model・除外）→ サイドカーへ書き込み
- 移動・リネーム（サイドカー・プレビューも追従）、削除はゴミ箱方式 + 取り消し
- 重複検出（sha256 一致）、一括操作（複数選択）
- ファイル監視（notify）で外部からの追加・削除を反映

完了条件: 本家で同じライブラリを開いて、lmx が書いたサイドカーが正しく読めること（相互運用テスト）。

### Phase 4: 仕上げ（目安 3〜5 日）

- 所持モデルの新バージョン確認・通知バッジ
- 統計画面（件数・容量・base model 別）
- 簡易認証（Q5 で採用した場合）、バックアップ（インデックスは再構築可能なので設定ファイルのみ）
- README・起動手順・本家との切り替え手順

### Phase 5 以降（需要が出たら）

レシピ、example images、HF/ModelScope、ComfyUI への「LoRA 文字列コピー」以上の連携。

## 5. テストと検証

- Rust: 各クレートに単体テスト（`cargo test`）。サイドカーは「読む→書く→元と JSON 意味比較で一致」、未知キー保持、centralized パス導出を必須ケースにする。
- Civitai: 実応答を `tests/fixtures/civitai/` に保存してパーサテスト。実通信は `cargo test -- --ignored` 相当の明示実行のみ。
- レート制御: 偽クライアントで並列 4 発の発射時刻を記録し、間隔が守られることをテスト。
- UI: Vitest（ロジック）+ 実機確認。スマホ由来の不具合は CDP のモバイルエミュレーション + タッチ入力で再現する。
- 性能: Phase 1・2 の終わりに計測スクリプト（一覧 API の p50/p95、RSS、起動時間、スキャン時間）を回し、本家の実測値と並べて記録。
- **本番データ保護:** 開発中は 本物ではなく、サイドカー・ダミー safetensors を複製したテスト用ルートで動かす。本物のライブラリへの書き込み操作の初回は、利用者の確認を取ってから。

## 6. リスク

| リスク | 影響 | 対策 |
|---|---|---|
| Civitai 応答の外れ値（実測 17.6 s が 1 回） | 検索が固まって見える | タイムアウト 20 s、読み込み表示、同一クエリの短期キャッシュ。件数ではなく `nextCursor` で続きを判断 |
| サイドカー破損（実測 25 件、末尾に余分な `}`） | 読めないモデルが出る | 原子的書き込み（一時ファイル → rename）、先頭 JSON 値の救出と「要修復」表示 |
| Civitai の API 仕様変更・Cloudflare チャレンジ | 検索/DL 停止 | クライアントを 1 クレートに隔離、フィクスチャで早期検知、エラーを UI に明示 |
| サイドカー互換を壊す | 本家側でメタデータ喪失 | 未知キー保持の往復テスト、Phase 3 の相互運用テスト、書き込み前バックアップ |
| 全面移植の作業量 | 長期化 | スコープを絞り、Phase 2 終了時点で「日常利用できる」状態を最優先 |
| 本家の進化に追随できない | 新機能を取り込めない | upstream を定期取得し、必要な機能だけ選んで移植 |
| GPL-3.0 | 公開時の義務 | lmx も GPL-3.0、フォークは公開済みなので整合 |

## 7. 決定事項（2026-11-04、利用者「おすすめ通りでいいよ」）

以下の Q1〜Q6 はすべて推奨案で確定。追加要望として「Android への PWA インストール」と「メタデータ作成時のローカル DB 優先オプション」を設計判断 7・8 に反映済み。

## 7-旧. 未決事項（推奨の既定値つき・記録用）

1. **Q1 名前:** `lmx`（推奨）でよいか。
2. **Q2 Python 版への暫定 Civitai 検索追加:** 推奨は「やらない」（二重実装になる。Phase 2 まで約 2〜3 週間の見込み）。待てない場合は本家フォークに検索モーダルだけ足す（1〜3 日）案もある。
3. **Q3 コードの置き場所:** 推奨は「フォーク内 `lmx/` ディレクトリ、フォークの main は upstream 同期用に保ち、開発は `feature/lmx-*` ブランチ」。別リポジトリ（`rushe19/lmx`）に分ける案もある。
4. **Q4 UI 部品:** 推奨は Vue 3 + 自前の軽量 CSS（PC とスマホの両立重視）。makiwarp と同じ Framework7 は iOS 風で手慣れているが、PC の多列グリッドとは相性が弱い。
5. **Q5 認証:** 推奨は「Tailscale Serve + 任意の簡易パスワード」。
6. **Q6 移行のしかた:** 推奨は「Phase 2 完了まで本家と並行運用、Phase 3 の相互運用テスト後に lmx を主にする」。
