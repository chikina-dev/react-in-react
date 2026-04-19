<!--VITE PLUS START-->

# Using Vite+, the Unified Toolchain for the Web

This project is using Vite+, a unified toolchain built on top of Vite, Rolldown, Vitest, tsdown, Oxlint, Oxfmt, and Vite Task. Vite+ wraps runtime management, package management, and frontend tooling in a single global CLI called `vp`. Vite+ is distinct from Vite, but it invokes Vite through `vp dev` and `vp build`.

## Vite+ Workflow

`vp` is a global binary that handles the full development lifecycle. Run `vp help` to print a list of commands and `vp <command> --help` for information about a specific command.

### Start

- create - Create a new project from a template
- migrate - Migrate an existing project to Vite+
- config - Configure hooks and agent integration
- staged - Run linters on staged files
- install (`i`) - Install dependencies
- env - Manage Node.js versions

### Develop

- dev - Run the development server
- check - Run format, lint, and TypeScript type checks
- lint - Lint code
- fmt - Format code
- test - Run tests

### Execute

- run - Run monorepo tasks
- exec - Execute a command from local `node_modules/.bin`
- dlx - Execute a package binary without installing it as a dependency
- cache - Manage the task cache

### Build

- build - Build for production
- pack - Build libraries
- preview - Preview production build

### Manage Dependencies

Vite+ automatically detects and wraps the underlying package manager such as pnpm, npm, or Yarn through the `packageManager` field in `package.json` or package manager-specific lockfiles.

- add - Add packages to dependencies
- remove (`rm`, `un`, `uninstall`) - Remove packages from dependencies
- update (`up`) - Update packages to latest versions
- dedupe - Deduplicate dependencies
- outdated - Check for outdated packages
- list (`ls`) - List installed packages
- why (`explain`) - Show why a package is installed
- info (`view`, `show`) - View package information from the registry
- link (`ln`) / unlink - Manage local package links
- pm - Forward a command to the package manager

### Maintain

- upgrade - Update `vp` itself to the latest version

These commands map to their corresponding tools. For example, `vp dev --port 3000` runs Vite's dev server and works the same as Vite. `vp test` runs JavaScript tests through the bundled Vitest. The version of all tools can be checked using `vp --version`. This is useful when researching documentation, features, and bugs.

## Common Pitfalls

- **Using the package manager directly:** Do not use pnpm, npm, or Yarn directly. Vite+ can handle all package manager operations.
- **Always use Vite commands to run tools:** Don't attempt to run `vp vitest` or `vp oxlint`. They do not exist. Use `vp test` and `vp lint` instead.
- **Running scripts:** Vite+ built-in commands (`vp dev`, `vp build`, `vp test`, etc.) always run the Vite+ built-in tool, not any `package.json` script of the same name. To run a custom script that shares a name with a built-in command, use `vp run <script>`. For example, if you have a custom `dev` script that runs multiple services concurrently, run it with `vp run dev`, not `vp dev` (which always starts Vite's dev server).
- **Do not install Vitest, Oxlint, Oxfmt, or tsdown directly:** Vite+ wraps these tools. They must not be installed directly. You cannot upgrade these tools by installing their latest versions. Always use Vite+ commands.
- **Use Vite+ wrappers for one-off binaries:** Use `vp dlx` instead of package-manager-specific `dlx`/`npx` commands.
- **Import JavaScript modules from `vite-plus`:** Instead of importing from `vite` or `vitest`, all modules should be imported from the project's `vite-plus` dependency. For example, `import { defineConfig } from 'vite-plus';` or `import { expect, test, vi } from 'vite-plus/test';`. You must not install `vitest` to import test utilities.
- **Type-Aware Linting:** There is no need to install `oxlint-tsgolint`, `vp lint --type-aware` works out of the box.

## CI Integration

For GitHub Actions, consider using [`voidzero-dev/setup-vp`](https://github.com/voidzero-dev/setup-vp) to replace separate `actions/setup-node`, package-manager setup, cache, and install steps with a single action.

```yaml
- uses: voidzero-dev/setup-vp@v1
  with:
    cache: true
- run: vp check
- run: vp test
```

## Review Checklist for Agents

- [ ] Run `vp install` after pulling remote changes and before getting started.
- [ ] Run `vp check` and `vp test` to validate changes.
<!--VITE PLUS END-->

<!-- 作りたいもの --start-- -->

# ブラウザ内 Node 互換実行基盤 設計書

## 1. 目的

本システムは、ブラウザ上で受け取った Node 系プロジェクト一式を、ローカルサーバや外部Node環境に依存せず実行し、起動したHTTPサーバを iframe 等で描画可能にすることを目的とする。

入力は以下とする。

- `project.zip`
- `cwd`
- `command`
- `args[]`

出力は以下とする。

- 標準出力
- 標準エラー
- 終了コード
- 公開ポート一覧
- プレビューURL

またgithub pagesで完結させるため、**完全クライアントサイド実装**とする。(Github Actionsでビルドを行なって流し込む形式)

---

## 2. スコープ

### 2.1 対応範囲

初版では以下を対応対象とする。

- 純JS / 純TS ベースの Node 系プロジェクト
- `node_modules` 同梱済み ZIP
- `package.json` の `scripts` 実行
- `node <entry>` 形式の実行
- 開発サーバ起動後の HTTP プレビュー
- ESM / CommonJS の基本解決
- 仮想ファイルシステム上での読み書き
- 単一ワークスペース実行

### 2.2 非対応

初版では以下を明確に非対応とする。

- native addon (`.node`)
- 任意のOSコマンド
- 任意のシェル互換
- Docker / bash / sh 前提
- 生TCP / UDP / Unix Socket
- 完全な Node.js API 互換
- `npm install` / `pnpm install` / `yarn install`
- 実ホストOSへのアクセス
- マルチプロセス完全互換
- すべてのnpm package互換保証

---

## 3. 設計方針

### 3.1 基本方針

中核ランタイムは **QuickJS系 VM** を採用し、これを **Rust/WASM ホスト** 上で動作させる。
ブラウザとの統合、UI、Service Worker、プレビュー制御は TypeScript 側で実装する。

### 3.2 採用理由

- JS 実行コアは QuickJS に委譲し、自前のJSエンジン実装を避ける
- 高頻度なFS操作、パス解決、ZIP展開、モジュール解決を Rust/WASM に寄せる
- ブラウザ固有要素は TS に寄せ、DOM / Worker / SW / iframe 制御を簡潔に保つ
- Node 完全互換ではなく、**Node ライクホストAPI** を定義して段階的に拡張する

---

## 4. 全体アーキテクチャ

```text
+--------------------------------------------------------+
| Main Thread (TypeScript UI)                            |
|--------------------------------------------------------|
| Editor / FileTree / Terminal / Preview iframe          |
| Session lifecycle / User actions                       |
+------------------------+-------------------------------+
                         |
                         | postMessage / MessageChannel
                         v
+--------------------------------------------------------+
| Runtime Worker (TypeScript)                            |
|--------------------------------------------------------|
| WASM boot / session control / script resolution        |
| stdout/stderr bridge / process lifecycle               |
| preview event relay / SW coordination                  |
+------------------------+-------------------------------+
                         |
                         | wasm-bindgen / JS bridge
                         v
+--------------------------------------------------------+
| Rust/WASM Host                                          |
|--------------------------------------------------------|
| VFS / ZIP mount / module resolution / process table    |
| timers / host API / port registry / stream buffers     |
| QuickJS embedding                                       |
+------------------------+-------------------------------+
                         |
                         | embedded host API
                         v
+--------------------------------------------------------+
| QuickJS VM                                             |
|--------------------------------------------------------|
| JS execution / require / import / package entry load   |
| process / fs / path / Buffer-like injected objects     |
+--------------------------------------------------------+

+--------------------------------------------------------+
| Service Worker (TypeScript)                            |
|--------------------------------------------------------|
| Preview URL routing / asset proxy / port mapping       |
| request dispatch to runtime session                    |
+--------------------------------------------------------+
```

---

## 5. コンポーネント設計

## 5.1 Main Thread

責務はUIおよびセッション操作の入口とする。

### 責務

- ZIPアップロード
- 実行開始・停止
- ターミナル表示
- ファイルツリー表示
- プレビュー iframe 管理
- エラー表示
- セッション状態表示

### 保持しない責務

- モジュール解決
- ZIP展開
- 実行本体
- プロセススケジューリング

---

## 5.2 Runtime Worker

TypeScript 製の制御層。
WASM と UI の間のオーケストレーターとして振る舞う。

### 責務

- WASM初期化
- セッション生成 / 破棄
- ZIP投入
- `command + args` の起動要求
- `package.json scripts` の解釈
- stdout/stderr の購読
- preview URL イベント中継
- Service Worker との連携

### Workerを置く理由

- Main Thread をブロックしない
- ログやイベントの集約点を単一化できる
- SW と UI の責務を分離できる

---

## 5.3 Rust/WASM Host

システムの中核。
QuickJS を埋め込み、仮想OS的な責務を持つ。

### 責務

- 仮想ファイルシステム
- ZIP展開
- パス正規化
- モジュール解決
- `package.json` / `exports` / `main` 解釈の一部
- プロセス表管理
- stdout/stderr バッファ
- タイマー管理
- ポート登録
- QuickJSホストAPI提供

### 非責務

- DOM操作
- iframe制御
- Service Worker API 直接操作
- UI state 管理

---

## 5.4 QuickJS VM

JSコード実行専用層。
Node 本体ではなく、Node ライク API を注入された埋め込みVMとして扱う。

### 責務

- JS評価
- CommonJS ローダ実行
- ESM の基本実行
- グローバルオブジェクト管理
- ホスト注入API呼び出し

### VM に与える組み込み

- `console`
- `setTimeout`, `clearTimeout`
- `process`
- `Buffer`
- `fs`
- `path`
- `URL`
- `TextEncoder`, `TextDecoder`
- `fetch`（必要なら限定対応）
- `__runtime`（内部用非公開API）

---

## 5.5 Service Worker

プレビュー配信用。

### 責務

- 仮想ポートと URL の対応付け
- iframe からの HTTP リクエスト受信
- 対応セッションへリクエスト転送
- レスポンス返却
- キャッシュ戦略管理

### 設計意図

QuickJS / Rust 側は「HTTPサーバがポートをlistenした」という抽象だけを扱い、実際のブラウザHTTP入口は SW が担当する。

---

# 6. セッションモデル

## 6.1 Session

1回の ZIP 起動単位を Session と呼ぶ。

```ts
type SessionId = string;

type SessionState = "booting" | "mounted" | "running" | "stopped" | "errored";
```

### セッションが持つ情報

- sessionId
- workspace root
- process table
- mounted files
- allocated virtual ports
- preview mappings
- stdout/stderr stream
- diagnostics

---

## 6.2 Process

本物のOSプロセスではなく、仮想プロセスとする。

```ts
type ProcessId = number;

type ProcessStatus = "created" | "running" | "exited" | "failed";
```

### 属性

- pid
- parentPid
- cwd
- argv
- env
- entrypoint
- stdio handles
- startTime
- exitCode

---

# 7. 仮想ファイルシステム

## 7.1 要件

- ZIP を `/workspace` に展開
- 読み取り・書き込み可能
- `stat`, `readFile`, `writeFile`, `readdir`, `exists`, `mkdirp` を持つ
- パス正規化 (`.` `..` 二重スラッシュ除去)
- symlink は初版では未対応または限定対応

## 7.2 データ構造

Rust 側では inode ライクな木構造またはパスインデックスを保持する。

```rust
struct VfsNode {
    kind: NodeKind,
    name: String,
    parent: NodeId,
    metadata: Metadata,
    content: Option<Vec<u8>>,
    children: Vec<NodeId>,
}
```

### 補助インデックス

- 絶対パス -> NodeId
- NodeId -> メタデータ
- 更新世代番号
- watcher購読者一覧

## 7.3 マウント手順

1. ZIP受領
2. Rust側で展開
3. `/workspace` 以下へ書き込み
4. インデックス生成
5. `package.json` 検出
6. エントリ候補探索

---

# 8. モジュール解決

## 8.1 対象

- CommonJS: `require(...)`
- ESM: `import ...`
- `package.json main`
- `package.json exports` の基本解釈
- 相対パス
- 絶対パス
- `node_modules` 探索

## 8.2 初版の優先順位

CommonJS を先に安定化し、その後 ESM を広げる。

### CommonJS 解決アルゴリズム

1. builtin 判定
2. 相対 / 絶対パス判定
3. ファイル探索
   - exact
   - `.js`
   - `.mjs`
   - `.cjs`
   - `.json`
   - `/index.js`

4. package 判定
   - `node_modules/<pkg>/package.json`
   - `main`
   - `exports` の単純条件

5. キャッシュ利用

## 8.3 builtin module 戦略

Nodeの builtin を完全再現せず、サポート対象のみ仮想実装する。

### 初版 builtin

- `fs`
- `path`
- `process`
- `events`
- `buffer`
- `util`
- `stream`（限定）
- `timers`
- `url`

### 後続候補

- `http`
- `https`
- `crypto`（限定）
- `zlib`（用途次第）
- `os`（ダミー寄り）

---

# 9. Node ライクホストAPI

## 9.1 process

```ts
interface RuntimeProcess {
  cwd(): string;
  chdir(path: string): void;
  env: Record<string, string>;
  argv: string[];
  stdout: WritableStreamLike;
  stderr: WritableStreamLike;
  exit(code?: number): never;
  nextTick(cb: () => void): void;
}
```

### 方針

- `process.platform` などは固定値で返す
- `process.exit` は仮想プロセス終了に変換
- `process.spawn` は公開せず、`child_process` は後回し

---

## 9.2 fs

同期APIを中心に最小実装する。

```ts
interface RuntimeFs {
  readFileSync(path: string, encoding?: string): string | Uint8Array;
  writeFileSync(path: string, data: string | Uint8Array): void;
  existsSync(path: string): boolean;
  statSync(path: string): StatLike;
  readdirSync(path: string): string[];
  mkdirSync(path: string, opts?: { recursive?: boolean }): void;
}
```

### 理由

Node系ツールチェーンは同期FS前提が多い。
初版では非同期APIより同期API優先でよい。

---

## 9.3 path

POSIX固定で実装する。

```ts
interface RuntimePath {
  resolve(...segments: string[]): string;
  join(...segments: string[]): string;
  dirname(path: string): string;
  basename(path: string): string;
  extname(path: string): string;
  normalize(path: string): string;
}
```

---

## 9.4 Buffer

QuickJS に対して `Uint8Array` ベースで実装し、NodeライクなAPIを最小提供する。

---

# 10. スクリプト実行モデル

## 10.1 起動要求

入力例:

```json
{
  "cwd": "/workspace",
  "command": "npm",
  "args": ["run", "dev"]
}
```

## 10.2 起動フロー

1. Worker が起動要求受領
2. `command` 種別判定
3. `npm run <script>` の場合:
   - `/workspace/package.json` を読む
   - `scripts[scriptName]` を取り出す
   - コマンド文字列を内部表現に変換

4. Rust/WASM に仮想プロセス作成要求
5. QuickJS VM で CLI エントリを評価
6. stdout/stderr を Worker 経由で UI に流す

## 10.3 コマンド対応方針

初版で許可する `command` は限定する。

### 許可コマンド

- `node`
- `npm` の `run`
- `npx` 相当の限定解決
- 将来 `pnpm dlx` 的な抽象化

### 非対応

- 任意シェル
- パイプ
- リダイレクト
- バックグラウンドジョブ

---

# 11. HTTPサーバ / プレビュー

## 11.1 目的

起動した開発サーバを iframe に表示する。

## 11.2 仮想ポート

Rust/WASM 側で `listen(port)` を検知したら、ポートレジストリへ登録する。

```ts
type PortRegistration = {
  sessionId: string;
  pid: number;
  port: number;
  protocol: "http";
};
```

## 11.3 プレビューURL

Worker / SW 側で以下のような URL を発行する。

```text
/preview/<sessionId>/<port>/
```

iframe はこの URL を `src` に設定する。

## 11.4 リクエスト経路

1. iframe が `/preview/<session>/<port>/...` にアクセス
2. Service Worker が intercept
3. 対応 session / port を解決
4. Runtime Worker に HTTP request を転送
5. Rust/WASM -> QuickJS 側でレスポンス生成
6. SW が `Response` を返却

## 11.5 サーバ実装方針

初版では Node の `http` API 完全互換を避け、最低限の request/response 抽象を作る。

---

# 12. 通信設計

## 12.1 Main Thread <-> Worker

`postMessage` + `MessageChannel`

### メッセージ種別

```ts
type UiToWorker =
  | { type: "session.create"; zip: ArrayBuffer }
  | { type: "session.run"; sessionId: string; cwd: string; command: string; args: string[] }
  | { type: "session.stop"; sessionId: string }
  | { type: "preview.attach"; sessionId: string; port: number };

type WorkerToUi =
  | { type: "session.created"; sessionId: string }
  | { type: "process.stdout"; sessionId: string; pid: number; chunk: string }
  | { type: "process.stderr"; sessionId: string; pid: number; chunk: string }
  | { type: "process.exit"; sessionId: string; pid: number; code: number }
  | { type: "preview.ready"; sessionId: string; port: number; url: string }
  | { type: "runtime.error"; sessionId: string; error: string };
```

## 12.2 Worker <-> WASM

- direct binding
- shared memory は将来検討
- 初版はシリアライズされたコマンド送信でよい

## 12.3 SW <-> Worker

- `postMessage`
- session / port routing
- request body は ArrayBuffer

---

# 13. エラーハンドリング

## 13.1 分類

- ZIP解析エラー
- FSマウントエラー
- モジュール解決エラー
- スクリプト解釈エラー
- 実行時例外
- ポート公開失敗
- プレビュー配信失敗

## 13.2 エラーオブジェクト

```ts
type RuntimeError = {
  code: string;
  message: string;
  detail?: string;
  path?: string;
  pid?: number;
};
```

## 13.3 方針

- UI にそのまま見せる文言と内部ログを分ける
- QuickJS stack trace は保持する
- モジュール解決失敗は探索した候補パスを含める
- SW 失敗時は preview 層の問題として切り分ける

---

# 14. セキュリティ

## 14.1 原則

- ホストOSにはアクセスさせない
- ネットワークは既定で閉じる
- 仮想FSに閉じる
- iframe は sandbox 化する

## 14.2 実施項目

- 外部 `fetch` はデフォルト無効または allowlist 制
- `eval` は VM 内では許可されてもホスト拡張は制限
- DOM 直接アクセス不可
- Service Worker は preview ルートのみに責務限定
- セッション破棄時にメモリと仮想ポートを明示的解放

## 14.3 資源制限

- 最大ZIPサイズ
- 最大ファイル数
- 最大メモリ使用量
- 実行時間制限
- 同時プロセス数制限

---

# 15. パフォーマンス方針

## 15.1 初版ボトルネック

- ZIP展開
- `node_modules` インデックス化
- モジュール解決
- stdout 大量出力
- HMR相当の再起動

## 15.2 最適化候補

- パスキャッシュ
- package.json キャッシュ
- module resolution cache
- inode インデックス
- 差分マウント
- セッションスナップショット
- 依存グラフ単位の無効化

---

# 16. 実装フェーズ

## Phase 1: コア起動

目標:

- ZIPを展開できる
- `/workspace` に mount できる
- `node entry.js` を走らせられる
- stdout/stderr を見られる

成果物:

- VFS
- QuickJS 起動
- `fs`, `path`, `process` の最小API

## Phase 2: package.json scripts

目標:

- `npm run dev` を内部解釈で動かす
- `node_modules` 解決を入れる

成果物:

- script resolver
- CommonJS ローダ
- package main 解釈

## Phase 3: HTTP preview

目標:

- 開発サーバのポートを検知する
- iframe で描画できる

成果物:

- port registry
- SW routing
- preview URL

## Phase 4: ESM拡張

目標:

- ESM対応を拡張
- `exports` 解釈強化

## Phase 5: 実用化

目標:

- watch系最適化
- スナップショット
- セッション再利用
- 診断改善

---

# 17. 最小API仕様

## 17.1 Worker公開API

```ts
interface RuntimeController {
  createSession(zip: ArrayBuffer): Promise<{ sessionId: string }>;
  run(sessionId: string, req: RunRequest): Promise<{ pid: number }>;
  stop(sessionId: string): Promise<void>;
  destroy(sessionId: string): Promise<void>;
  subscribe(sessionId: string, cb: (ev: RuntimeEvent) => void): Unsubscribe;
}
```

### 17.2 RunRequest

```ts
type RunRequest = {
  cwd: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
};
```

### 17.3 RuntimeEvent

```ts
type RuntimeEvent =
  | { type: "stdout"; pid: number; chunk: string }
  | { type: "stderr"; pid: number; chunk: string }
  | { type: "exit"; pid: number; code: number }
  | { type: "preview"; pid: number; port: number; url: string }
  | { type: "error"; error: RuntimeError };
```

---

## 18. 技術選定

### 18.1 採用

- JS VM: QuickJS系
- Host: Rust/WASM
- UI / Worker / SW: TypeScript
- ZIP展開: Rust側
- 通信: MessageChannel / postMessage

### 18.2 不採用

- TypeScript Compiler API を中核に据える構成
- TS only runtime
- wasmer-js を主軸にした設計
- native addon 対応
- Node 完全互換を初版で狙う設計

---

## 19. リスク

### 19.1 互換性リスク

- npm package により必要APIがまちまち
- ESM/CJS混在で解決規則が複雑
- 開発サーバが想定外の Node API に依存する可能性

### 19.2 ブラウザ制約

- Service Worker 制約
- cross-origin isolation 要件
- メモリ上限
- 長時間実行時の安定性

### 19.3 対策

- 対応プロジェクト種別を明記
- unsupported API を明示的に fail-fast
- capability matrix を用意
- トレースログを厚くする

---

## 20. 成功条件

初版の成功条件を以下とする。

1. `zip(node_modules入り)` をアップロードできる
2. `/workspace/package.json` を認識できる
3. `npm run dev` 相当を起動できる
4. ログがターミナルに流れる
5. HTTPサーバ起動を検知できる
6. iframe にプレビューを表示できる
7. セッション停止で資源を解放できる

---

## 21. 補足: 実装優先順位

最優先は次です。

- VFS
- CommonJS ローダ
- `package.json scripts`
- `fs`, `path`, `process`
- preview port registry

後回しでよいものは次です。

- ESM 完全化
- `child_process`
- watch最適化
- HMR最適化
- `fetch` / `crypto` 拡張
- スナップショット高速化

## QuickJS vs QuickJS-NG

結論として、**この用途なら QuickJS-NG を推奨**です。
理由は、**保守性・機能追加・周辺エコシステムとの相性**がよく、あなたが作ろうとしている **埋め込みランタイム + 独自ホストAPI** という用途に合うからです。QuickJS-NG は公式に、コミュニティ開発、継続的なリリース、テスト、クロスプラットフォーム対応、性能改善、新しい ECMAScript API の追加を差分として挙げています。

ただし、**性能最優先なら upstream QuickJS も捨てがたい**です。QuickJS 本家は 2025-09-13 に新リリースがあり、現行版でも小さく埋め込みやすく、低起動コストを強みとして継続しています。さらに一部の実利用側では、QuickJS-NG より upstream QuickJS の方を性能面で推奨する記述もあります。

設計観点での判断を一言で言うと、こうです。
**製品基盤なら QuickJS-NG、最小で速いVMコアだけ欲しいなら QuickJS** です。QuickJS-NG は upstream との差分として、Opcode fusion、polymorphic inline caching、メモリ割り当て改善、パース速度改善に加えて、Resizable ArrayBuffer、WeakRef、FinalizationRegistry、Iterator Helpers、V8互換の stack trace API などを明示しています。あなたのように Node ライクなホスト層を上に積むなら、こうした周辺互換の厚みが効きます。

保守面では、QuickJS-NG 側のメンテナが「両者は今後も別系統で進み、統合するなら実質 QuickJS-NG を正史にする形になる」と述べており、少なくともコミュニティの温度感は QuickJS-NG 優勢です。実際、バインディング側でも「長期保守性、リリース頻度、クロスプラットフォーム対応、CI/テスト」を理由に QuickJS-NG へ移行している例があります。

一方で、注意点もあります。
QuickJS-NG には 2026年1月公開の GitHub Advisory があり、少なくとも 0.11.0 までに TypedArray 周りの heap-based buffer overflow が報告されています。使うなら最新版固定と追従体制が前提です。本家 QuickJS も当然ゼロリスクではありませんが、QuickJS-NG は開発速度が高いぶん、**更新追従を前提に採用する製品**として見るのがよいです。

あなたのプロジェクト向けの実務的な推奨は次です。
**初期採用は QuickJS-NG** にして、Rust 側に **VM abstraction layer** を1枚置いてください。つまり `EngineAdapter` を切って、`eval`, `load_module`, `set_host_fn`, `interrupt`, `memory_limit`, `dump_exception` だけ共通化します。そうしておけば、後から upstream QuickJS に差し替えてベンチ比較できます。QuickJS 本家も 2025年に複数回リリースされており、今後性能や安定性で逆転余地はあります。

なので最終判断はこれです。
**いま設計を進めるなら QuickJS-NG。**
**ただし実装は差し替え可能な抽象境界で切る。**
これが最も堅いです。

必要なら次に、**QuickJS-NG 前提の Rust 側 `EngineAdapter` trait 設計** まで落とします。

<!-- 作りたいもの --end-- -->
