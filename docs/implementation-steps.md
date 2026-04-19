# Browser Runtime Production Steps

## Goal

`project.zip` を受け取り、`cwd + command + args` でブラウザ内ランタイムを起動し、標準出力・標準エラー・終了コード・公開ポート・preview URL を返しつつ、iframe に描画できる状態まで持っていく。

---

## Phase 0. Product Envelope

### Purpose

実装判断の前提を固定する。

### Done When

- 完全クライアントサイドで完結する
- Node 完全互換ではなく Node-like host API を採用する
- QuickJS-NG を第一候補にしつつ差し替え可能な VM 境界を維持する
- `zip(node_modules 入り)` 前提を守る

### Outputs

- AGENTS.md の設計
- ランタイム責務分離

---

## Phase 1. Host Shell

### Purpose

React ホスト UI から ZIP 読み込み、起動、停止、ログ表示、preview 表示を行えるようにする。

### Done

- ZIP upload UI
- command/cwd 入力
- terminal 表示
- preview iframe
- session / preview router 状態表示

### Main Files

- `src/App.tsx`
- `src/components/PreviewFrame.tsx`
- `src/main.tsx`
- `src/style.css`

---

## Phase 2. Session Mount And ZIP Analysis

### Purpose

ZIP を `/workspace` に mount したものとして扱い、package.json と file index を取れるようにする。

### Done

- ZIP 展開
- top-level folder strip
- `/workspace/...` への path 正規化
- `package.json` 読み取り
- React / Vite 検出
- text/binary を含む file record 保持

### Main Files

- `src/runtime/analyze-archive.ts`
- `src/runtime/analyze-archive.test.ts`

---

## Phase 3. Runtime Worker Protocol

### Purpose

Main Thread と Runtime Worker の責務を固定し、あとから Rust/WASM host に差し替えやすい API にする。

### Done

- `session.create`
- `session.run`
- `session.stop`
- `preview.http`
- `stdout` / `stderr` / `exit` / `preview.ready` / `runtime.error`

### Main Files

- `src/runtime/protocol.ts`
- `src/runtime/controller.ts`
- `src/runtime/runtime.worker.ts`

---

## Phase 4. Mock Runtime Execution

### Purpose

本物の Node 実行の代わりに、session state と process lifecycle を成立させる。

### Done

- `npm run <script>` の解釈
- `node <entry>` の解釈
- unsupported command の fail-fast
- synthetic process / pid
- preview ready event の発火

### Main Files

- `src/runtime/runtime.worker.ts`

---

## Phase 5. Service Worker Preview Bridge

### Purpose

iframe からの `/preview/<session>/<port>/...` を Service Worker 経由で runtime へ橋渡しする。

### Done

- preview Service Worker 登録
- preview registration / unregister
- MessageChannel bridge
- Service Worker fetch intercept
- Runtime Worker への virtual HTTP request 転送

### Main Files

- `public/preview-sw.js`
- `src/runtime/preview-service-worker.ts`
- `src/runtime/controller.ts`

---

## Phase 6. Virtual Preview Server

### Purpose

Runtime Worker の中に preview 用の仮想 HTTP サーバを置く。

### Done

- `/preview/<session>/<port>/`
- `__runtime.json`
- `__workspace.json`
- `__files.json`
- runtime CSS
- `/files/...`
- workspace `index.html` の root 配信
- `dist` / `build` / `public` を含む document root 解決
- root-relative URL rewrite
- binary asset 配信
- `.ts/.tsx/.jsx` の軽量 module transpile
- React / CSS import の preview 向け rewrite
- `index.html` がない場合の source entry からの synthetic app shell
- basic CommonJS (`require` / `module.exports`) の preview 向け rewrite
- 拡張子なし relative import と単純な `node_modules` entry 解決
- `package.json exports` の root / subpath の最小解決
- `browser` field 優先と wildcard exports subpath の最小解決
- `browser` object mapping と nested export condition の最小解決
- `browser` object mapping の `false` を empty module stub に落とす最小互換
- package 内の relative import / require にも `browser` object mapping を適用
- package manifest `name` を使った self-import / self-require の最小解決
- `package.json imports` の direct / wildcard alias を package 内で最小解決
- importer 起点の nested `node_modules` 探索と workspace root fallback
- workspace root package の `imports` / self-import と、`imports` から外部 package への解決
- `imports` / `exports` の `null` を fail-fast な blocked module として可視化

### Main Files

- `src/runtime/preview-server.ts`
- `src/runtime/preview-server.test.ts`

---

## Phase 7. Guest Preview Surface

### Purpose

iframe 内に guest React root を起動し、runtime から返ってきた状態を可視化する。

### Done

- preview bootstrap
- runtime / workspace / file index fetch
- selected file fetch
- preview route / raw route の確認 UI

### Main Files

- `src/preview-client.tsx`
- `src/preview/PreviewApp.tsx`

---

## Phase 8. Replace Mock Runtime With WASM Host

### Purpose

ここから先が本実装の中核。TypeScript mock runtime を Rust/WASM host に置き換える。

### Next

1. `EngineAdapter` 境界を Rust 側に置く
2. QuickJS-NG の boot / eval / module load / interrupt を最小 API 化する
3. VFS を Rust 側に移し、ZIP 展開・inode index・path cache を持たせる
4. `fs`, `path`, `process`, `timers`, `buffer` を host API として VM に注入する
5. stdout/stderr/exit/port event を TS Worker へ返す

### Deliverables

- Rust/WASM package
- JS binding layer
- Worker から WASM host を使う session runner

### Started

- Cargo workspace と `rust/runtime-host` crate を追加
- `EngineAdapter` trait / `NullEngineAdapter` を追加
- session store / boot summary / run plan の Rust 側骨格を追加
- POSIX path 正規化と最小 VFS を追加
- TS Worker 側に `RuntimeHostAdapter` / `MockRuntimeHostAdapter` の差し替え境界を追加
- Rust crate に raw wasm export を追加し、Vite build 時に `public/runtime-host.wasm` へ同期
- Worker は wasm host を優先ロードし、失敗時のみ mock adapter へ fallback
- wasm `createSession` で workspace file 本体を Rust VFS へ mount するように修正
- Rust host から workspace file 一覧 / 単一 file 読み出しを取れる export を追加
- preview 配信前に Rust host から workspace file 群を hydrate し、TS 側 preview server の入力を Rust VFS 由来へ切り替え
- preview request ごとに必要 file だけを hydrate する lazy 読み出しへ変更し、全量 preload を回避
- Rust host に複数 file 読み出し export を追加し、preview hydration を batch 化

---

## Phase 9. CommonJS Loader

### Purpose

`node_modules` 同梱 ZIP の主要ケースを動かす。

### Next

1. `require` resolver
2. relative / absolute / package lookup
3. `package.json main`
4. 単純化した `exports`
5. module cache

### Done When

- `node entry.js` が VFS 上の依存込みで実行できる
- `npm run dev` が実ファイル解決を伴って動く

---

## Phase 10. HTTP Server Compatibility Layer

### Purpose

QuickJS 側の `listen(port)` と browser preview を接続する。

### Next

1. lightweight request/response abstraction
2. port registry を Rust/WASM 側に移動
3. runtime generated response を preview bridge に返す
4. static asset ではなく guest app のレスポンスを返す

### Done When

- guest app 自身の HTML / JS / CSS / API response を preview で返せる

---

## Phase 11. ESM Expansion

### Purpose

近年の React/Vite プロジェクトに寄せる。

### Next

1. ESM loader
2. mixed CJS / ESM handling
3. `exports` condition handling の拡張
4. source map / stack trace の改善

---

## Phase 12. Practical Hardening

### Purpose

実用ラインに載せる。

### Next

1. resource limits
2. large ZIP diagnostics
3. unsupported API matrix
4. session cleanup and memory release
5. watch / restart optimization
6. snapshot / reuse strategy

---

## Current Status Summary

### Completed

- Host UI
- ZIP mount
- session/protocol split
- mock runtime process lifecycle
- Service Worker preview bridge
- virtual preview server
- workspace HTML/text/binary static asset serving

### In Progress

- preview を static mock から guest app 実行結果へ寄せる段階

### Next Critical Path

1. Rust/WASM host を導入する
2. QuickJS-NG を抽象境界付きで組み込む
3. CommonJS loader を動かす
4. port registry と HTTP compatibility layer を実サーバ応答に置き換える
