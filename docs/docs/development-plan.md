# Mnema Development Plan (v0.1)

> This document is for **AI-assisted / vibe coding**.
> Use it as a source of prompts and direction when asking Codex / ChatGPT to write code.

---

## 0. Goals of this plan

* 明確な締切やスケジュールは持たない。
* 「次に何を作るか」「どういう単位で AI に投げるか」がわかるようにする。
* **小さい縦のスライス**（"小さめの完成体"）を少しずつ積み上げる。
* 将来の拡張（同期・モバイル・高度なエージェント化）を阻害しない最低限の設計ルールを残す。

AI に渡すときは、このドキュメントから対象フェーズの部分だけをコピーして、
「この方針に沿ってコードを書いて」と指示する想定。

---

## 1. 全体フェーズ構成

1. **Phase 0: リポジトリ / ワークスペースのブートストラップ**
2. **Phase 1: ドメインモデル（core crate）**
3. **Phase 2: 永続化 & Vault 構造（infra-db crate）**
4. **Phase 3: LLM 抽象レイヤ（infra-llm crate）**
5. **Phase 4: 自動化 / ジョブ基盤（automation crate）**
6. **Phase 5: デスクトップ UI のスパイク（desktop crate / Tauri）**
7. **Phase 6: 秘書チャット + 能動通知の MVP**
8. **Phase 7: 週次レビュー / マイルストーン / 体験の磨き込み**

フェーズは厳密に順番通りでなくてもよいが、
「core → infra → automation → UI → AI 秘書 → 体験の磨き込み」という大きな流れは保つ。

---

## 2. Phase 0: リポジトリ / ワークスペースのブートストラップ

### 2.1 目的

* Rust workspace と crates の骨組みを作る。
* docs ディレクトリと README 類を整える（すでにかなり進んでいる）。

### 2.2 やること

* [ ] Cargo workspace の作成

  * ルート `Cargo.toml` に `crates/core`, `crates/infra`, `crates/desktop` などをメンバーとして定義。
* [ ] `crates/core` の空 crate 追加
* [ ] `crates/infra` の空 crate 追加（db / llm は将来サブモジュール分割）
* [ ] `crates/desktop` の空 crate 追加（Tauri / Iced 等は後で決定）
* [ ] `docs/` 以下に `specification-v0.1.md` と `development-plan.md` を置く

### 2.3 AI に投げるときのサンプルプロンプト

> Create a Rust cargo workspace for the Mnema project with the following structure: `crates/core`, `crates/infra`, `crates/desktop`. Each crate can be a simple `lib` or `bin` for now. Use edition 2021 and set up basic `Cargo.toml` files with reasonable names and descriptions.

---

## 3. Phase 1: ドメインモデル（core crate）

### 3.1 目的

* `crates/core` に、Db 依存のない **純粋なドメインモデル / サービス** を定義する。
* ここでは永続化の詳細を一切考えない。

### 3.2 主な型

* `Task`

  * `id`, `title`, `description`
  * `project_id`, `list_id`, `status_id`
  * `due_date`, `start_date`, `estimated_minutes`, `cost_points`
  * `dependencies: Vec<TaskId>`
  * `milestone_id`
  * `created_at`, `updated_at`, `deleted_at`

* `Project`

  * `id`, `title`, `description`
  * `start_date`, `end_date`
  * `archived_at`

* `List`

  * `id`, `project_id`
  * `name`, `kind` (`INBOX`, `PERSONAL`, `PROJECT`)
  * `is_system`, `order`

* `Milestone`

  * `id`, `project_id`
  * `title`, `description`
  * `target_date`
  * `status` (`NOT_DONE`, `DONE`)

* `StatusGroup` / `Status`

  * プロジェクトごとのカスタムステータスをサポートするための枠組み。

* `UserSettings`

  * LLM プロバイダ設定
  * 各自動化項目のレベル（OFF / ASK / AUTO_WITH_REVIEW / AUTO_SILENT）

* `LlmMemory`

  * 長期メモリ用のテキスト + 構造化プリファレンス

* `Assistant`

  * 秘書のペルソナ・機能フラグ・アバター情報

* `AutomationLog`

  * AI が行った変更の履歴（before/after の JSON スナップショット）

### 3.3 ルール

* `core` crate は **外部ライブラリ依存を最小限** にする（`serde` などに留める）。
* ID 型（`TaskId`, `ProjectId` など）は newtype で定義しておくと安全。
* ビジネスロジック（例: 進捗率の計算、マイルストーンの `OVERDUE` 判定など）は core に寄せる。

### 3.4 AI に投げるときのサンプルプロンプト

> In the `crates/core` crate of Mnema, define domain structs for `Task`, `Project`, `List`, `Milestone`, `StatusGroup`, `Status`, `UserSettings`, `LlmMemory`, `Assistant`, and `AutomationLog`. Use newtype IDs (e.g. `TaskId`) wrapping a `String` or `Uuid`. Derive `Clone`, `Debug`, `Serialize`, and `Deserialize` where appropriate. Don’t include any database-specific details.

---

## 4. Phase 2: 永続化 & Vault 構造（infra-db）

### 4.1 目的

* `core` のモデルを永続化するための DB レイヤを作る。
* Vault ディレクトリ構造（`tasks.sqlite`, `config.json`, `memory/*.md` など）を決める。

### 4.2 やること

* [ ] `infra` crate 内に `db` モジュールを作成

* [ ] SQLite + `sqlx` または `SeaORM` など、好みの ORM / クエリレイヤを選定

* [ ] マイグレーションファイルの仕組みを整える

* [ ] Repository インターフェイスを core に定義し、infra側で実装:

  * `TaskRepository`
  * `ProjectRepository`
  * `ListRepository`
  * `MilestoneRepository`

* [ ] Vault パスを受け取って DB / ファイル群を初期化する関数を用意

### 4.3 AI に投げるときのサンプルプロンプト

> In the `crates/infra` crate, create a `db` module that uses SQLite (via sqlx or another lightweight crate) to persist the core domain models (`Task`, `Project`, etc.). Define repository traits in `crates/core` (e.g. `TaskRepository`) and provide concrete implementations in `crates/infra`. Also, design a simple `Vault` struct that knows the root directory and can open the database file there.

---

## 5. Phase 3: LLM 抽象レイヤ（infra-llm）

### 5.1 目的

* ローカル（Ollama）とクラウド（OpenAI 互換）を同じインターフェイスで扱えるようにする。
* エージェント構成やチェーンは、まずは「呼び出しをラップする最低限の層」に留める。

### 5.2 やること

* [ ] `infra` crate に `llm` モジュールを追加
* [ ] 共通トrait `LlmClient` を定義

  * `complete(prompt, config) -> String`
  * `chat(messages, config) -> String`
* [ ] 実装:

  * `OllamaClient`
  * `OpenAiCompatibleClient`
* [ ] UserSettings と LlmClient をつなぐ「ファクトリ」ヘルパを用意

### 5.3 AI に投げるときのサンプルプロンプト

> In the `crates/infra` crate, add an `llm` module that defines a trait `LlmClient` with methods like `complete` and `chat`. Implement `LlmClient` for two backends: a local Ollama HTTP API, and a generic OpenAI-compatible HTTP API. Make the code easy to extend to new providers.

---

## 6. Phase 4: 自動化 / ジョブ基盤（automation）

### 6.1 目的

* Inbox 分類や週次レビュー生成などを **バックグラウンドジョブ** として扱う基盤をつくる。

### 6.2 やること

* [ ] `core` に「ジョブ種別」を表現する enum を定義

  * `AutomationJobKind::InboxClassify`, `WeeklyReview`, `DueDateSuggestion`, など
* [ ] `infra` にジョブキュー実装（シンプルなローカルキュー）
* [ ] 各ジョブを処理する `AutomationService` を core に実装

  * LlmClient と Repository を受け取って動く構造にする

### 6.3 AI に投げるときのサンプルプロンプト

> Design a simple background job system for Mnema. In `crates/core`, define an `AutomationJobKind` enum and an `AutomationService` that can handle jobs like Inbox classification, weekly review generation, and due date suggestions. In `crates/infra`, create a minimal in-process job queue that can enqueue and dequeue these jobs.

---

## 7. Phase 5: デスクトップ UI のスパイク（desktop）

### 7.1 目的

* まずは **「今日のタスクリストを表示するだけ」** の超小さい UI を動かす。
* Tauri を本命として、Rust + Web UI の繋ぎ込みを試す。

### 7.2 やること

* [ ] Tauri プロジェクトの初期化（`crates/desktop`）
* [ ] Vault のパスを選ばせて、今日のタスク一覧を表示するだけの画面
* [ ] Inbox にタスクを 1 件追加できるフォーム（タイトルのみ）

### 7.3 AI に投げるときのサンプルプロンプト

> In the `crates/desktop` crate, initialize a Tauri app that connects to the Mnema core and infra layers. Implement a minimal window that: (1) lets the user choose or create a vault directory, (2) shows today's tasks in a list, and (3) allows adding a new inbox task with only a title.

---

## 8. Phase 6: 秘書チャット + 能動通知の MVP

### 8.1 目的

* 右ペインに「秘書」チャットを表示し、

  * タスク入力
  * 簡単な質問（"What should I do next?"）
  * 軽い週次レビュー
    をこなせるようにする。

### 8.2 やること

* [ ] チャット UI コンポーネントを追加（Web 側）
* [ ] バックエンドに「秘書エンドポイント」を用意

  * メッセージ履歴 + LlmClient + core services で応答を生成
* [ ] AutomationLog に秘書経由の変更を書き込む
* [ ] 通知 / 提案のトリガをいくつか実装

  * アプリ起動時の「今日やるべきこと」提案
  * 1 日の終わりの軽いふりかえり提案

### 8.3 AI に投げるときのサンプルプロンプト

> Extend the Mnema desktop app to include a sidebar chat UI for an AI "secretary". The chat backend should use the existing LlmClient and core services to: (1) create tasks from natural language, (2) answer "What should I do next?" by ranking tasks, and (3) write any changes to the AutomationLog. Implement this as a Tauri command that the frontend can call.

---

## 9. Phase 7: 週次レビュー / マイルストーン / 体験の磨き込み

### 9.1 目的

* Mnema の「らしさ」である週次レビュー・マイルストーン・能動的サポートを強化する。

### 9.2 やること（例）

* [ ] 週次レビュー生成ロジックの拡張

  * 「よかったこと / できなかったこと / 来週のフォーカス」などのテンプレート
* [ ] マイルストーンビュー（ガント・一覧）の UI
* [ ] AutomationLog のフィルタ UI（プロジェクト / リストごとに履歴を追えるように）
* [ ] UserSettings UI（自動化レベル・LLM モデル・秘書の口調など）

### 9.3 AI に投げるときのサンプルプロンプト

> Implement a weekly review generator in Mnema. Using tasks, milestones, and AutomationLog entries from the last 7 days, ask the LLM to produce a summary with: what went well, what didn’t go as planned, and 3 focus suggestions for next week. Expose this as a command in the desktop app, and show the result in the secretary chat UI.

---

## 10. 今後の拡張メモ（まだ手をつけないもの）

* クラウド同期サービス（自前サーバ or パーソナルサーバ）
* モバイルクライアント（Tauri モバイル or 別クライアント）
* Live2D / アニメーションする秘書アバター
* 高度な CRDT / イベントソーシングによる同期安全性の向上

これらは v0.x ではなく、**「とりあえずデスクトップで使える Mnema」** ができてから考える。
