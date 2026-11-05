# lmx の使い方（起動・スマホ・LoRA Manager との併用と切り替え）

## 起動と停止（Windows）

```powershell
cd <lmx のフォルダ>
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\start-lmx.ps1   # 裏で起動。ログは data\ に出る
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\stop-lmx.ps1    # 停止
```

- `start-lmx.ps1` は `target\release\lmx-server.exe` を `data\bin\` にコピーしてから起動する。動かしたまま `cargo build --release` できる。
- 既定のポートは 9100。変えるときは `-Port 9200` を付ける。環境変数: `LMX_HOST`（0.0.0.0）、`LMX_PORT`、`LMX_WEB_DIR`（web/dist）、`LMX_DATA`（data）、`LMX_LOG`（info）。
- ログオン時に自動で起動したいときは、タスク スケジューラで「ログオン時」に次の操作を登録する: プログラム `powershell.exe`、引数 `-NoProfile -ExecutionPolicy Bypass -File <lmx のフォルダ>\scripts\start-lmx.ps1`、開始フォルダ `<lmx のフォルダ>`。

## スマホから使う（PWA）

Android は https のページしかアプリとしてインストールできない。Tailscale Serve で https にする:

```bash
tailscale serve --bg --https=10000 http://127.0.0.1:9100
```

スマホの Chrome で `https://<machine>.<tailnet>.ts.net:10000/` を開き、メニューの「アプリをインストール」。

- Tailscale のアドレスは tailnet の中からしか開けない。
- LAN の `http://<PC の IP>:9100` はファイアウォールで閉じている（2026-11 時点）。開けるときは先に「設定 → パスワード」を設定しておく。

## 設定画面でできること

| カード | 内容 |
|---|---|
| 新しいバージョン | 手元のモデルに Civitai の新バージョンがあるかを確認。1 日 1 回の自動確認（既定で ON）、無料のみの絞り込み、通知しない設定 |
| 統計 | 件数・容量（種類・ベースモデル・フォルダ・作者別）、Civitai 情報の有無、最近の追加、重複、大きいファイル |
| Civitai 検索のベースモデル | 検索画面に出すベースモデルを選ぶ（サーバー保存、スマホと PC で共通） |
| ダウンロードのファイル名 | Civitai のモデル画面で「ダウンロード」を押すと、保存先の上にファイル名の入力欄が出る（拡張子は元のまま）。同じ名前のファイルがある、または同じ名前をダウンロード待ちのときは、キューに入れる前に知らせて、`名前_2` のような空いている名前を提案する。名前を変えると、プレビュー画像と `.metadata.json` も同じ名前になる。「モデル名_バージョン名にする」ボタンで、`モデル名_バージョン名` を一度に入れられる（使えない文字は `_`、長いときはモデル名側を切る。Embedding は空白を `_` にする。LoRA Manager の `{model_name}_{version_name}` と同じ考え方だが、LoRA Manager の「ダウンロードのファイル名テンプレート」の設定は lmx のダウンロードには効かない）。URL から一括で入れるときは変えられない（Civitai の名前のまま。同名があると、その行だけ入れずにエラーを返す） |
| メタデータ一括取得 | ローカル archive DB 優先などのモードを選んでまとめて取得 |
| パスワード | 任意。設定するとログインしないと使えない（端末ごとに 180 日有効） |
| 設定のバックアップ | lmx 自身の設定を 1 ファイルで保存・読み込み |

### パスワードを忘れたら

サーバーの `data\auth.json` を削除すると解除される（lmx の再起動は不要）。

## data フォルダの中身

| ファイル | 中身 | 消したら |
|---|---|---|
| `lmx.toml` | 設定 | 既定値で起動（モデルの場所が空になる） |
| `index.sqlite` | モデル一覧のキャッシュ | 次のスキャンで作り直す |
| `thumbs/` | サムネイル | 必要になったときに作り直す |
| `archive_hash.sqlite` | archive DB の sha256 索引 | 起動時に作り直す（約 15 秒） |
| `downloads.sqlite` | ダウンロードの履歴と途中のもの | 履歴が消える |
| `updates.json` | 新バージョン確認の結果と「通知しない」一覧 | 次の確認まで新バージョン表示が消える |
| `prefs.json` | 検索のベースモデル、自動確認の ON/OFF | 既定に戻る |
| `civitai_enums.json` | Civitai のベースモデル一覧 | 次に開いたとき取り直す |
| `auth.json` | パスワードのハッシュ | パスワードが解除される |
| `backups/` | lmx がサイドカーを書き換える前の内容 | 元に戻せなくなる |

モデルのファイルとサイドカー（`.metadata.json`）はモデルフォルダ側にあり、`data` には入っていない。

## LoRA Manager との併用

lmx は LoRA Manager と同じサイドカー（`<名前>.metadata.json`）を読み書きする（キーの順序・字下げ・改行コード・小数の書き方まで同じ）。なので両方を同時に動かして、同じライブラリを見られる。

- lmx で編集・移動・削除・ダウンロードしたものは、LoRA Manager の次のスキャンで反映される（逆も同じ。lmx はフォルダ監視ですぐ反映）。
- **メタデータの一括取得は、両方で同時に走らせない。** 同じファイルを書き合うと、片方の結果が消えることがある。lmx は一括取得の前に変更前の内容を `data\backups\` に残す。
- `lm_settings` を設定していると、モデルの場所、Civitai API キー、archive DB の場所を LoRA Manager の settings.json から読む（書き込みはしない）。

### lmx にあって LoRA Manager に無いもの

- キーワード検索の並び替え（Civitai は検索語があると並び順を無視するため、lmx が上位 1,000 件を並べ直す）
- 先行公開・要 Buzz の表示、ダウンロード再開、スマホ向けの画面、軽さ（常駐 80〜100 MB）

### LoRA Manager にあって lmx に無いもの

- ComfyUI のノード連携、レシピ、example images、Hugging Face などからのダウンロード、多言語の画面

## LoRA Manager から切り替える

1. lmx を起動して、いつもの操作（検索・ダウンロード・編集）ができることを確かめる。
2. LoRA Manager を止める（自動起動していればそれも止める）。ファイルの移行は要らない。
3. LoRA Manager を消すときは、先に `lmx.toml` に設定を写しておく:
   - `[roots]` にモデルの場所を書く
   - `civitai_api_key` を書く（公開リポジトリにはコミットしない）
   - archive DB を使い続けるなら、`civitai.sqlite` を別の場所に置いて `archive_db` に書く
   - そのあと `lm_settings` の行を消す

### 元に戻す

LoRA Manager をもう一度起動するだけでいい。サイドカーは共通なので、lmx で行った変更もそのまま見える。lmx を止めるときは `stop-lmx.ps1`。
