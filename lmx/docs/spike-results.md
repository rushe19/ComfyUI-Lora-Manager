# Phase 0 スパイク結果（2026-11-04）

計測マシン: 開発 PC（32 スレッド / RAM 126 GB）、モデル置き場 H: = WD_BLACK SN850X 8TB（NVMe）。
本家 LoRA Manager（`<LoRA Manager のインストール先>`）のファイルは読み取り専用で参照し、変更していない。

## T0-1 Civitai 検索品質 → 問題なし（当初の懸念は誤り）

- 20 クエリ（英語キャラ名・日本語「初音ミク」・NSFW 系タグ・スタイル系・`anima` など）× 3 条件（フィルタ無し / `baseModels=Illustrious` / `sort=Most Downloaded`）を `limit=100` で取得。
- **全クエリで 1 ページ目に 91〜100 件、関連性も妥当、`nextCursor` あり。** 日本語クエリも正しく当たる。
- 計画書作成時に「`query=miku` が 4 件しか返らない」と書いたのは `limit=5` で取ったため。**1 ページの件数が `limit` 未満になることがある（取得後に間引かれる）だけで、続きは cursor で取れる。** UI は件数ではなく `nextCursor` の有無で「続きあり」を判断すること。
- 応答時間はおおむね 0.33〜0.77 s。**1 回だけ 17.6 s の外れ値**（`naruto` フィルタ無し）→ タイムアウト（例 20 s）・読み込み表示・同一クエリの短期キャッシュが必要。
- CORS ヘッダ無し → サーバー経由のプロキシで確定。

## T0-2 ダウンロード認証・再開 → 問題なし

- 対象: NSFW の小さな LoRA（model 8723 / version 10290、約 924 KB）。ファイルは保存せずメモリ上で検証のみ。
- `downloadUrl` は **307 で `b2.civitai.com` の署名付き URL にリダイレクト**（キー無しでも今回のモデルは 307。ログイン必須モデルではキーが要る想定）。
- `Authorization: Bearer <key>` 付きで `Range: bytes=0-99999` → **206**、続きを `Range: bytes=100000-` → **206 / Content-Range: bytes 100000-945775/945776**。
- 結合したデータの SHA256 が API の `hashes.SHA256` と**一致**。→ 一時ファイル + Range 再開 + 完了後ハッシュ検証の方式でいける。
- 注意: リダイレクト先でも `Authorization` を送り続けると拒否するストレージがあるため、実装ではリダイレクト先ホストが変わったら認証ヘッダを外す（reqwest は既定で外す）。

## T0-3 サイドカー全数調査

- models 15,750 件中、サイドカー読み込み成功 15,717 / **欠損 7 / JSON 破損 25〜26**（下記）。平均 5.7 KB。
- **読み込み時間: OS キャッシュ無し（コールド）で 270.8 s、キャッシュ有りで 2.5 s。** → 起動のたびに全サイドカーを読むのは不可。lmx は自前 DB にパース結果とサイドカーの size/mtime を持ち、起動時は stat だけで差分を見る。
- 全件に存在するキー: file_name, model_name, file_path, size, modified, sha256, base_model, preview_url, preview_nsfw_level, notes, from_civitai, civitai, tags, modelDescription, civitai_deleted, favorite, exclude, db_checked, skip_metadata_refresh, metadata_source, last_checked_at, hash_status, autov3, quantization。
- 一部のみ: usage_tips 15,596 / sub_type 348 / auto_tags 8 / license_flags・source_platform・source_url・hf_url・source_model_id・source_version_id 各 1。
- **型の揺れ:** `size` が int 15,672 / float 45、`last_checked_at` が int 10,178 / float 5,539、`metadata_source`・`autov3`・`quantization` は null あり。→ Rust 側は数値を f64 経由で受ける・Option にするなど寛容に読む。
- `metadata_source`: null 10,280 / archive_db 3,383 / civitai_api 1,946 / civarchive 108。`hash_status`: completed 15,703 / pending 14。
- `civitai` オブジェクトは Civitai の model-version API 応答そのもの（id, modelId, name, baseModel, model, files, images, trainedWords, stats, description, creator …）。
- **JSON 破損 25 件の正体:** 正しい JSON の後ろに `}` が余分に 1 つ付いている（`Extra data`）。24 件がこの形、1 件は制御文字混入。古い長い内容の上に短い内容を切り詰めずに書いた、または同時書き込みの痕跡と思われる（**原因は未確定**）。→ lmx は「一時ファイルに書いて rename」の原子的な書き込みにし、読み込み時は先頭の JSON 値だけ救出して「要修復」として表示する（自動修復はユーザー確認後）。
- 代表 45 件を `lmx/tests/fixtures/sidecars-local/` にコピー（個人ライブラリなので .gitignore 済み、コミットしない）。

## T0-4 ハッシュ速度 → 既定 4 並列

各条件で**別々のファイル集合**（約 6 GB、32〜44 ファイル）を使い、OS キャッシュの影響を避けた。全ファイルのハッシュが本家のカタログ値と一致（不一致 0・エラー 0）。

| 実装 | 並列数 | 速度 | 1 ファイルあたり |
|---|---|---|---|
| Python hashlib（本家相当） | 1 | 929 MB/s | 0.147 s |
| Rust sha2 | 1 | 1,498 MB/s | 0.104 s |
| Rust sha2 | 2 | 2,429 MB/s | 0.063 s |
| **Rust sha2** | **4** | **3,057 MB/s** | **0.048 s** |
| Rust sha2 | 8 | 2,922 MB/s | 0.052 s |
| Rust sha2 | 16 | 2,678 MB/s | 0.072 s |
| （参考）Rust 1 並列・キャッシュ済み | 1 | 1,730 MB/s | CPU 単体の上限 |

- 4 並列で NVMe の読み出しが頭打ち。それ以上は逆に遅くなる → **既定 4 並列**（設定で変更可）。
- LoRA 全体 2.58 TB を 3 GB/s で読むと**約 14 分**（本家の初回スキャン約 94 分）。
- 本家の 0.3〜0.4 s/ファイルのうち、ハッシュ計算そのものは 0.147 s。残りはサイドカー作成などの処理。

## T0-5 雛形 → 完了

- `lmx/` に Cargo workspace（`lmx-server`, `lmx-scan`）と `web/`（Vite + Vue 3 + TS）。
- `cargo test`: 6 件成功（health JSON / 未知 API は JSON の 404 で SPA の HTML にならない / 深いリンクは index.html / sw.js は no-cache / SHA256 既知ベクタ / バッファ境界をまたぐファイル）。
- `npm test`: 3 件成功。`npm run build`: JS 65.7 KB（gzip 26.3 KB）。
- `lmx-server` リリース版の常駐メモリ: **8.4 MB**（雛形なので参考値）。

## T0-6 archive DB → 自前のハッシュ索引で解決

- `civitai\civitai.sqlite` 4.2 GB。`model_files.sha256` に索引が無く、`EXPLAIN QUERY PLAN` = `SCAN model_files`。照会 0.34〜0.38 s/件。
- sha256 列の一括抽出は 716,694 行で 0.8 s、手元 LoRA 15,402 件との照合は 19 ms で **8,425 件（54.7%）ヒット**。
- → lmx は archive DB を読み取り専用で開き、自前 DB に `archive_hash` 表を作って照会する。`archive_only` モードなら 8.4k 件を CivitAI 通信 0 で処理できる見込み。

## T0-7 PWA インストール可否 → 条件クリア（localhost）

- Chrome（CDP）で `http://127.0.0.1:9100/` を開き、`Page.getInstallabilityErrors` = **[]**、`Page.getAppManifest` のエラー = **[]**、Service Worker = **active**。
- manifest は `application/manifest+json`、アイコンは any 192/512・maskable 192/512・apple-touch 180。
- Android 実機では `http://<LAN IP>` だとインストールできないため、Tailscale Serve の **10000 番**（443/8443 は使用中）で HTTPS 化して確認する。実機での確認は利用者にお願いする。
