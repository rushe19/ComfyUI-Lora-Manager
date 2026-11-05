# lmx × Forge Neo 連携 設計書

- 状態: 設計（決定事項を反映済み・未実装）
- 対象: Forge Neo `neo-2.31` 系（gradio 4.40.0 / fastapi 0.127.1 / starlette 0.50.0 / httpx 0.28.1 で確認）
- 関連: `docs/plan.md`, `docs/usage.md`

## 1. 目的

Forge Neo で生成するときの **LoRA・Embedding の選択** と **Checkpoint の切り替え** を、lmx の画面で行えるようにする。

- Forge 標準の LoRA タブは、全カードを HTML にまとめて作り、検索は画面側で全カードの表示を切り替える方式。LoRA が 1 万件を超えると重い。lmx を使えば、一覧・検索・プレビューは lmx（Rust）が受け持ち、Gradio 側の負担はほとんど増えない。
- PC とスマホのブラウザの両方で使えること。
- スマホの lmx PWA（Tailscale の HTTPS）は、今のまま変えない。

### やらないこと（今回の範囲外）

- lmx から Forge の生成を実行すること（生成ボタンは Forge のまま）。
- Forge 標準の LoRA タブを消したり書き換えたりすること。
- Forge 本体のファイルを書き換えること（拡張機能だけで完結させる）。

## 2. 前提と、確認済みの事実

| 項目 | 内容 | 根拠 |
|---|---|---|
| モデルの置き場所 | Forge は `--model-ref <models>` で、lmx と同じフォルダを見ている | `webui-user.bat` の起動オプション、lmx の roots |
| Forge の開き方 | 主に LAN の IP（`http://<LAN IP>:7860`）。外からは Tailscale 経由で同じ LAN の IP | らぐくんの運用 |
| Forge のログイン | `--gradio-auth` で有効。`--api` も有効 | `webui-user.bat` |
| lmx のパスワード | 今は未設定 | 設定画面 |
| lmx のポート | 9100（現在は LAN にも開いているが、環境によっては閉じる） | らぐくんの運用 |
| 拡張から経路を足せるか | `script_callbacks.on_app_started(demo, app)` で FastAPI の `app` を受け取れる | `modules/script_callbacks.py` |
| 足した経路に Forge のログインがかかるか | **かからない**。gradio の `login_check` は gradio 自身の経路にだけ付く。足した経路では自分で確認する必要がある | gradio `routes.py`（`/sdapi` もログインなしで通るのを確認済み） |
| Forge のログイン状態の読み方 | cookie `access-token-{app.cookie_id}`（または `access-token-unsecure-…`）の値を `app.tokens` で引くとユーザー名が返る | gradio `routes.py` の `get_current_user` |
| プロンプトへの追加 | グローバル関数 `updatePromptArea(text, textarea, isNeg)` がある。すでに入っていれば外す（トグル）。区切りは `opts.extra_networks_add_text_separator` | `javascript/extraNetworks.js` |
| プロンプト欄 | `#txt2img_prompt > label > textarea`、`#img2img_prompt …`、ネガティブは `#{tab}_neg_prompt …` | `modules/ui_toprow.py` |
| Checkpoint の切り替え | グローバル関数 `selectCheckpoint(name)` → 隠しボタン `#change_checkpoint` → `get_closet_checkpoint_match`（別名の完全一致、次に title の部分一致） | `javascript/ui.js`, `modules/ui_settings.py`, `modules/sd_models.py` |
| LoRA の名前 | `<lora:ファイル名(拡張子なし):重み>` で必ず通る（ファイル名と別名の両方が登録される） | `networks.py` L219-220 |
| 新しくダウンロードした LoRA | 生成時に見つからない名前があると一覧を読み直すので、Forge 側の更新操作はいらない | `networks.py` L153-155 |
| Forge の通信圧縮 | `GZipMiddleware(minimum_size=1000)` が全体にかかっている | `modules/initialize_util.py` |

## 3. 全体の構成

```
 ブラウザ（PC / スマホ） ── http://<LAN IP>:7860 ─────────────────┐
                                                                    │
 ┌─ Forge Neo（Python / gradio）──────────────────────────────────┐ │
 │  画面: Forge の UI + 拡張の JS（lmx ボタン・パネル・橋渡し）   │◀┘
 │  経路: /lmx/{path}  ← 拡張が追加する中継                        │
 │        └ Forge のログイン確認 → http://127.0.0.1:9100/{path} へ │
 └────────────────────────────────────────────────────────────────┘
                              │（同じ PC の中だけの通信）
 ┌─ lmx（Rust）── 127.0.0.1:9100 ─────────────────────────────────┐
 │  /api/*  /assets/*  index.html …（中身は今のまま）              │
 └────────────────────────────────────────────────────────────────┘
```

- **同じ出どころ:** ブラウザから見ると、lmx は Forge と同じアドレスの `/lmx/` の下にある。そのため、iframe の中の lmx とのやりとりに、別サイト扱いの制限（cookie の遮断など）がかからない。iPhone の Safari でも同じように動く。
- **lmx のポート:** Forge からの中継は PC の中だけの通信なので、lmx の 9100 番を LAN に開いているかどうかは関係ない。
- **スマホ PWA:** 今まで通り `https://<machine>.<tailnet>.ts.net:10000/` で直接 lmx を開く。中継を通らないので、影響はない。

## 4. 構成要素

### 4.1 Forge 拡張 `sd-forge-lmx`（新規）

```
sd-forge-lmx/
  scripts/lmx_bridge.py      # 中継の経路と設定項目
  lmx_bridge/proxy.py        # 中継の中身（Forge に依存しない。テストしやすくするため）
  javascript/lmx_bridge.js   # lmx ボタン・パネル・橋渡し
  style.css                  # パネルの見た目
  tests/                     # pytest（偽の lmx を相手に動かす）
  conftest.py
```

#### (a) 中継 `/lmx/{path:path}`

- **受け付ける方法:** GET / HEAD / POST / PUT / PATCH / DELETE。
- **転送先:** Forge の設定画面に置く `lmx_upstream`（初期値 `http://127.0.0.1:9100`）だけ。リクエストの中身で転送先を変えられないようにする（どこにでもつながる中継にしない）。
- **ログイン確認:**
  - Forge のログインが有効なとき（`app.auth` が空でない）は、gradio の cookie からユーザーを引けなければ **401** を返し、中継しない。
  - ログインが無効のときは、そのまま通す（LAN の誰でも使える状態になる、と設定の説明に書いておく）。
- **lmx に渡すヘッダー:**
  - Cookie は `lmx_session` だけを残す。gradio のログイン cookie は lmx に渡さない。
  - `Authorization`、`Host`、通信の継ぎ目に関わるもの（`Connection` / `Keep-Alive` / `Transfer-Encoding` / `TE` / `Upgrade` など）は渡さない。
  - `Range` / `If-None-Match` / `If-Modified-Since` / `Accept` / `Content-Type` はそのまま渡す。
  - `X-Forwarded-Prefix: /lmx`（+ Forge の `--subpath`）を付ける（今のところ lmx 側では使わない。将来のため）。
- **返す内容:**
  - httpx の `AsyncClient`（使い回し、接続タイムアウト 5 秒、読み取りタイムアウト 120 秒）で受け取り、`StreamingResponse` でそのまま流す。動画の部分取得（206 / `Content-Range` / `Accept-Ranges`）もそのまま返す。
  - lmx の `Set-Cookie` の `Path=/` は `Path=/lmx/` に書き換える。lmx のパスワードを後で設定しても、cookie が Forge の経路に漏れないようにするため。
  - **圧縮対策:** Forge 全体にかかっている GZip が、動画の部分取得や、すでに圧縮済みの画像・動画を圧縮しないようにする。方法は実装時に実機で決める（候補: 中継の応答に `Content-Encoding: identity` を付ける、または `/lmx/` だけ GZip を外す小さなミドルウェアを先に挟む）。
- **lmx が止まっているとき:** 502 と、日本語の短い説明（「lmx が起動していません」）を返す。

#### (b) 設定（Forge の設定画面 → 「lmx」の欄）

| キー | 初期値 | 説明 |
|---|---|---|
| `lmx_upstream` | `http://127.0.0.1:9100` | lmx の場所（Forge から見たアドレス） |
| `lmx_close_after_add` | `false` | 追加したらパネルを閉じる |
| `lmx_add_trigger_words` | `true` | LoRA と一緒にトリガーワードも入れる |

### 4.2 Forge 側の画面（`javascript/lmx_bridge.js`）

- **lmx ボタン:** txt2img と img2img のプロンプト横（標準の Extra Networks ボタンの並び）に置く。押すとパネルを開く。
- **パネル:**
  - 画面幅が 900px 以上: 右側に幅 440px の引き出し。Forge を見ながら、続けて何個でも追加できる。
  - 900px 未満（スマホ）: 全画面で重ねる。左上に「×」。Android の戻るボタンでも閉じる（`history.pushState` / `popstate`）。
- **iframe:**
  - `src="<root>/lmx/?embed=forge"`。最初に開いたときに作って、閉じるときは消さずに `display:none` にする。そうすると、検索条件やスクロール位置が残る。
  - 隠している間、lmx の中の動画は `LazyVideo` が画面外として扱い、読み込みを解放する（前回入れた仕組みがそのまま効く。実機で確認する）。
- **メッセージの受け取り:**
  - `message` イベントは、`event.origin === location.origin` かつ `event.source === iframe.contentWindow` のときだけ受け付ける。
  - `type` は決まったものだけを受け付けて、知らないものは捨てる。
- **どのタブに入れるか:** 開いたときのタブ（txt2img / img2img）を覚えておき、そのタブのプロンプト欄に入れる。
- **追加の処理:** Forge の `updatePromptArea` を呼ぶ。Forge 標準のカードと同じく、もう一度押すと外れる。Forge の内部が変わって関数が見つからないときは、自分で文字を足して `updateInput` を呼ぶ方法に切り替える。
- **Checkpoint の切り替え:** `selectCheckpoint(name)` を呼ぶ。見つからなかった警告は Forge 自身が出す。

#### メッセージの形（lmx → Forge）

```ts
type LmxToForge =
  | { type: 'lmx:add-network'; kind: 'lora'; name: string; weight: number | null; trigger_words: string[] }
  | { type: 'lmx:add-network'; kind: 'embedding'; name: string; negative: boolean }
  | { type: 'lmx:set-checkpoint'; name: string }
  | { type: 'lmx:close' }
```

- `name` は、拡張子なしのファイル名（LoRA / Embedding）か、ファイル名（Checkpoint。`get_closet_checkpoint_match` の部分一致で見つける）。
- `weight` が `null` のときは、Forge の `opts.extra_networks_default_multiplier` を使う。
- `trigger_words` は、`lmx_add_trigger_words` がオン（初期値）のとき、`<lora:名前:重み>` の後ろに `, ` でつなげて入れる。もう一度押して LoRA を外すときは、タグだけでなくトリガーワードも一緒に外す（ほかの LoRA と同じ単語を使っていて消してしまわないよう、そのとき入れた単語だけを外す）。トリガーワードが空の LoRA は、タグだけを入れる。

#### メッセージの形（Forge → lmx、開いたとき）

```ts
type ForgeToLmx = { type: 'forge:context'; tab: 'txt2img' | 'img2img'; checkpoint: string | null }
```

最初は、lmx の画面に「今のタブ」と「今の checkpoint」を出すことだけに使う。

### 4.3 lmx 側の変更

#### (a) 置き場所が `/` でなくても動くようにする

今は `/api/…`、`/sw.js`、`/manifest.webmanifest`、`/icons/…` など、すべて `/` から始まるアドレスで書いている。`/lmx/` の下でも動くようにする。

1. `vite.config.ts` に `base: './'` を指定する（読み込むファイルを相対の場所にする）。
2. `web/src/base.ts` を新しく作る。

   ```ts
   /** '/' のとき（直接開いたとき）と '/lmx/' のとき（Forge 経由）の両方で動くための土台 */
   export const BASE = new URL('.', document.baseURI).pathname   // 例: '/', '/lmx/'
   export const withBase = (p: string) => (p.startsWith('/') ? BASE + p.slice(1) : p)
   ```

3. `fetch('/api/…')`、`href="/api/backup"`、サーバーが返す `preview_url`（`/api/preview/{id}?v=…`）など、`/` から始まるアドレスはすべて `withBase()` を通す。サーバー側は変えない。
4. Service Worker と manifest は、`BASE === '/'` のときだけ登録・読み込みする。Forge 経由では使わない（Forge と同じ出どころに SW を置かないため）。
5. ライブラリやダウンロードのサムネイルなど、JSON で受け取るアドレスは、表示の直前の 1 か所で `withBase()` を通す。

#### (b) 埋め込みモード（`?embed=forge`）

- `web/src/embed.ts`: `isEmbedded`（`?embed=forge` があり、`window.parent !== window`）と `send(msg)`（`parent.postMessage(msg, location.origin)`）。
- 下のタブバーは「ライブラリ」だけにする。Civitai 検索やダウンロード、設定は、PWA のほうで使う。
- **ライブラリ一覧のカード:** 短く押すと、今まで通り詳細シートを開く。カードの右上に「＋」ボタンを足し、LoRA / Embedding はプロンプトに追加、Checkpoint は切り替える。
- **詳細シート:** 「プロンプトに追加」（LoRA）、「プロンプト / ネガティブに追加」（Embedding）、「この Checkpoint に切り替え」の大きなボタンを出す。重みは、詳細シートの「強さ」（`strength`）を使う。
- 追加したら「追加しました: <name>」とトーストで知らせる。`lmx_close_after_add` がオンなら `lmx:close` を送る。
- **PWA への影響:** 埋め込みモードでないときは、画面も動きもまったく変わらない。

## 5. セキュリティ

| 心配なこと | 対策 |
|---|---|
| LAN から Forge 経由で lmx を勝手に操作される | 中継で Forge のログインを確認する（2. の通り、自分で確認しないとかからない）。ログインなしで 401 になることを、テストと実機で確認する |
| Forge のログイン cookie が lmx に渡る | 中継で `lmx_session` 以外の Cookie を外す |
| lmx の cookie が Forge の経路に漏れる | `Set-Cookie` の Path を `/lmx/` に書き換える |
| どこにでもつながる中継になる | 転送先は設定の `lmx_upstream` だけ。経路は `{path}` の部分だけを付け足し、`..` やスキーム付きのものは拒否する |
| 他のサイトから iframe のメッセージを偽装される | `origin` と `source` を両方確認する。決まった `type` だけを受け付ける。送るときも `targetOrigin` に `location.origin` を指定する |
| 他のサイトから Forge 経由で lmx の API を叩かれる | Forge は gradio の CORS をわざと外している（`webui.py`）。ログイン cookie は SameSite のため、他のサイトからの POST には付かない。実装後、別の出どころからの `fetch` が通らないことを確認する |

lmx にパスワードを設定した場合も動く（同じ出どころなので、iframe の中で普通にログインできる）。ただし、Forge のログインと二重になるので、初期の案ではパスワードなしのまま運用する。

## 6. 作業の順番

各段階で、テストとビルドが通ったらコミットする。push の前には、毎回プライバシースキャン（ローカルのパス、LAN の IP、Tailscale のアドレス、API キー、作者のメールアドレス）を行う。

1. **lmx: 置き場所への対応（4.3 a）**
   - `base.ts` と `withBase` のテスト（`/` と `/lmx/` の両方）。
   - `/` から始まるアドレスの書き換え。`grep` で書き換え漏れがないか確認する（`'/api`、`"/api`、`` `/api ``、`/sw.js`、`/icons`）。
   - 確認: 今の PWA と LAN 直結の両方で、画面・サムネイル・動画・バックアップのダウンロードが今まで通り動く。
2. **拡張: 中継（4.1 a, b）**
   - `proxy.py` を Forge に依存しない形で作る。pytest で次を確認する。
     - ログインあり・なしの判定（401 になるか）
     - Range（206）の受け渡し
     - Set-Cookie の Path の書き換え
     - Cookie・ヘッダーの除去
     - lmx が止まっているときの 502
     - `..` を含む経路の拒否
   - テスト用の環境は `uv venv` で分けて、Forge と同じ版（fastapi 0.127.1 / starlette 0.50.0 / httpx 0.28.1）を入れる。
   - GZip と部分取得の組み合わせを、本物の Forge の上で確認する。
3. **拡張: ボタン・パネル・橋渡し（4.2）**
   - PC 幅（1280px）とスマホ幅（412px）で、開く・閉じる・戻るボタン・タブの判定を確認する。
4. **lmx: 埋め込みモード（4.3 b）**
   - vitest で次を確認する。
     - `isEmbedded` の判定
     - 送るメッセージの形
     - 埋め込みでないときにボタンが出ないこと
5. **全体の確認（実際の Forge で）**
   - txt2img と img2img で、LoRA を追加・もう一度押して外す、トリガーワードあり・なし、Embedding をネガティブに入れる、Checkpoint を切り替える。
   - Forge のログインなしで `/lmx/api/health` を開いたら 401 になる。
   - 新しくダウンロードした LoRA を、Forge を再起動せずに使える。
   - パネルを閉じている間、lmx の動画の読み込みが 0 本になる。
   - スマホの PWA（Tailscale の HTTPS）が今まで通り動く。
6. **インストールと説明書**
   - Forge の `extensions/` に入れる（らぐくんに確認してから）。別リポジトリなので、`extensions/` に clone するか、フォルダのリンクにするかは、そのときに選ぶ。
   - `docs/usage.md` に「Forge から使う」の節を足す。

## 7. 心配なことと、まだ決まっていないこと

### 実装で確かめること

- **Checkpoint と組み合わせのモジュール:** MOSS などは、VAE やテキストエンコーダー（`forge_additional_modules`）を一緒に変える必要がある。`selectCheckpoint` が変えるのは checkpoint だけなので、違う種類の checkpoint に切り替えたときにモジュールが合わなくなるおそれがある。最初は「同じ種類の checkpoint 同士の切り替え」を対象にして、種類が違うときは lmx の画面で注意を出す。Forge の UI プリセット（種類の切り替え）と連動させるかは、実機で動きを見てから決める。
- **GZip:** 4.1 a の圧縮対策が実際に効くか。
- **Forge の今後の変更:** `updatePromptArea`、`selectCheckpoint`、要素の id は、Forge 本体の更新で変わることがある。見つからないときは代わりの方法に切り替え、コンソールに警告を出す。
- **同じ名前の Checkpoint:** 違うフォルダに同じファイル名があると、部分一致で別のほうが選ばれることがある。lmx が持っているフォルダからの相対パスで完全一致を先に試し、だめなときだけファイル名にする。

### 決定事項（らぐくん、確定）

1. **トリガーワード:** LoRA と一緒に入れる（初期値オン。設定で切り替え可）。
2. **追加したあと:** パネルは開いたまま（`lmx_close_after_add` は初期値オフ）。
3. **全体の確認でのログイン:** Forge のログイン情報は Hermes の保管庫（ラベル「forge neo」）に登録済み。ログイン画面への入力は保管庫の機能を通すので、パスワードが会話やファイルに出ることはない。注意点は 2 つ。
   - 保管庫は、ページのアドレスが登録したものと一致しないと入力を拒否する。確認のときは、登録したアドレス（LAN の IP のほう）で Forge を開く。`127.0.0.1` では入力できない。
   - 確認用のブラウザで `--gradio-auth` のログインが通るかは、Forge を起動して実際に試す必要がある。
4. **拡張の置き場所:** lmx とは別のリポジトリにする。名前は `sd-forge-lmx`（仮）。
   - ローカルは `hermes-work/sd-forge-lmx/` に作る。GitHub に作る（公開するか非公開か）のは、作る前にらぐくんに確認する。
   - lmx 側のリポジトリには、このドキュメントと、埋め込みモード対応（4.3）だけを置く。拡張のコードは入れない。
   - 拡張のコミットも、作者は `rushe19`（noreply のメール）にする。push の前に、ローカルのパスや IP、保管庫の情報が入っていないかを、lmx と同じ手順で確認する。

## 8. あとで考えること（今回は作らない）

- 今の checkpoint の種類（ベースモデル）に合わせて、lmx の一覧を自動で絞り込む。
- Forge で最近使った LoRA の履歴を lmx に出す。
- 生成した画像の infotext から、使った LoRA を lmx で開く。
