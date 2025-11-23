# タスク管理ツール 設計ドキュメント（v0.1）

## 1. プロジェクト概要

### 1.1 コンセプト

- ローカルファーストなタスク管理ツール
- スローガン方向性
  - One inbox. Zero friction.
  - Tasks in, clarity out.
  - Capture everything. Carry nothing in your head.
- ユーザーは「発生したタスクや気づきを全部投げる」だけに集中し、
   プロジェクト整理・優先度の判断・期日の調整・週次レビューなどは LLM＋秘書エージェントに任せる

### 1.2 目標と非目標

- 目標
  - 「面倒くさくない」入力と整理フロー
  - ローカルファースト + クラウド同期オプション
  - プロジェクト単位でのガントチャート・ビュー切り替え
  - LLM による自動分類・期日提案・週次レビュー生成
  - 秘書エージェントによる対話型インターフェース
- 非目標（少なくとも v1 時点）
  - 高度なコラボレーション（複数ユーザー同時編集）
  - 大規模チーム向けの権限管理
  - 完全な Git 並みの履歴管理（v1 では簡易ロールバックのみ）

------

## 2. Obsidian 的思想の取り込み

### 2.1 ローカルファースト / Vault 概念

- データの正本はローカルに置く
- 「Vault」という単位で世界観を管理
- 1 Vault = 1 フォルダ + その中の DB / 設定 / アセット

### 2.2 Vault 構造（案）

- `vault_root/`
  - `tasks.sqlite`（ローカル RDB）
  - `config.json`（UserSettings など）
  - `assistant/`（アバター画像・秘書設定）
  - `memory/`（LLM メモリ関連の永続化）
  - `attachments/`（添付ファイル）
  - `exports/`（JSON / Markdown などのエクスポート）

### 2.3 ロックインしない方針

- DB を使いつつも、定期的に JSON / Markdown にエクスポート
- 最悪アプリが動かなくても、エクスポートからタスク・プロジェクトを復元できる構造を目指す

------

## 3. ドメインモデル設計

### 3.1 Task（タスク）

#### 3.1.1 目的

- 実行単位の作業を表現するエンティティ
- プロジェクトやマイルストーン、ステータスと関連しながらスケジュールされる

#### 3.1.2 フィールド案

- `id`: ULID / UUID（グローバル一意 ID）
- `title`: タスク名
- `description`: 詳細テキスト
- `project_id`: 所属プロジェクト ID（Inbox / Personal の場合は null）
- `list_id`: 所属リスト ID（ボードのカラム的なもの）
- `status_id`: ステータス ID
- `due_date`: 期日（任意）
- `start_date`: 開始日（任意）
- `estimated_minutes`: 見積もり時間（任意）
- `cost_points`: コストの抽象値（時間・労力・お金などをまとめた「重さ」）
- `dependencies`: 依存タスク ID の配列
- `milestone_id`: 関連マイルストーン ID（任意）
- `created_at`
- `updated_at`
- `deleted_at`（soft delete 用）

#### 3.1.3 priority 廃止の理由と扱い

- priority は due_date / estimated_minutes / cost_points / dependencies などから導出できる値
- 永続フィールドとして持つと
  - 情報の重複
  - 人間と LLM の判断不一致
     を生みやすい
- 方針
  - priority は DB に保存しない（またはキャッシュ扱いに留める）
  - ビュー側で「一時的な優先度」を算出し、ソートや表示に利用する
  - 秘書が「なぜこの順番なのか」を説明する UX に寄せる

#### 3.1.4 tags / custom_data を持たない方針

- v1 では Task にタグや任意 JSON を持たせない
- 任意メタデータが必要になった場合
  - Project 側に「カスタムフィールド定義」を置く
  - Task 側には「カスタムフィールド ID + 値」のみを持たせる設計に寄せる（将来）

#### 3.1.5 source フィールドの扱い

- v1 の Task からは `source` を外す
- 「どこから生まれたタスクか」は `AutomationLog` 側で管理
- 外部連携（メールなど）を入れる段階で Task に `origin` を追加する余地を残す

------

### 3.2 StatusGroup & Status（ステータス）

#### 3.2.1 目的

- ClickUp 的な「プロジェクトごとのステータスカスタマイズ」を実現しつつ
- 大枠の状態（Not started / In progress / Pending / Done）は共通の枠組みで扱う

#### 3.2.2 StatusGroup

- フィールド案
  - `id`
  - `name`: 表示名（"Not started", "In progress", "Pending", "Done"）
  - `kind`: `NOT_STARTED` / `IN_PROGRESS` / `PENDING` / `DONE` の固定値

#### 3.2.3 Status

- フィールド案
  - `id`
  - `project_id`: このステータスが属するプロジェクト（null ならグローバル）
  - `name`: 表示名（"To do", "In review" など）
  - `group_id`: 所属する `StatusGroup`
  - `order`: 並び順

#### 3.2.4 Task との関係

- Task は `status_id` のみを持つ
- プロジェクトごとにステータスのセットを変えられる

------

### 3.3 Project（プロジェクト）

#### 3.3.1 目的

- 複数タスクを束ねる単位
- ガントチャート表示やマイルストーン管理の単位

#### 3.3.2 フィールド案

- `id`
- `title`
- `description`
- `start_date`: プロジェクト期間の開始（任意）
- `end_date`: プロジェクト期間の終了（任意）
- `default_status_set_id`: 使うステータスセット（必要なら）
- `archived_at`: アーカイブ日時

#### 3.3.3 進捗率の扱い

- DB カラムとしては持たない
- 算出方法（ビュー側で計算）
  - 例1: Done グループのタスク数 / 全タスク数
  - 例2: Cost や estimated_minutes を重みとして加味した比率

------

### 3.4 List（リスト）

#### 3.4.1 目的

- プロジェクト内のビューやカラムを表現する単位
- Inbox / Personal も List として扱う

#### 3.4.2 フィールド案

- `id`
- `project_id`: 所属プロジェクト ID（Inbox / Personal の場合は null）
- `name`
- `is_system`: システムリストかどうか（Inbox / Personal は true）
- `kind`: `INBOX` / `PERSONAL` / `PROJECT`
- `view_type`: List / Board / Calendar / etc…（将来の拡張用）
- `order`

#### 3.4.3 システムリスト

- Inbox
  - `project_id = null`
  - `kind = INBOX`
- Personal
  - `project_id = null`
  - `kind = PERSONAL`
- UI 上ではサイドバー上部にピン留めして特別扱い

------

### 3.5 Milestone（マイルストーン）

#### 3.5.1 目的

- プロジェクトにおける重要なチェックポイント
- ガントや週次レビューで「山場」を視覚化するための単位

#### 3.5.2 フィールド案

- `id`
- `project_id`
- `title`
- `description`
- `target_date`: 目標日
- `status`: `NOT_DONE` / `OVERDUE` / `DONE`
- `dependency_task_ids`: 関連タスク ID の配列
- `created_at`
- `updated_at`

#### 3.5.3 ステータスの自動更新

- `target_date < today` かつ `status != DONE` の場合、自動的に `OVERDUE` 扱い
- 秘書が
  - 「このマイルストーンのために今週やるべきタスク」
     を引き出してくれる前提で利用

------

### 3.6 UserSettings（ユーザー設定）

#### 3.6.1 LLM 設定

- `provider`: local / openai-compatible / other
- `model_for_planning`: 高次思考・計画用モデル名
- `model_for_routine`: 雑務・分類用モデル名

#### 3.6.2 自動化項目ごとの設定

- 自動化項目の例
  - `auto_inbox_classify`（Inbox の自動分類）
  - `auto_due_suggestion`（期日提案）
  - `auto_schedule_generation`（日・週のスケジュール自動生成）
  - `auto_weekly_review`（週次レビュー自動生成）
- 各項目のレベル
  - `OFF`: 自動化しない
  - `ASK`: 実行前にユーザーに確認
  - `AUTO_WITH_REVIEW`: 自動実行しつつ、後で差分を提示
  - `AUTO_SILENT`: 完全自動（慎重に扱うレベル）

#### 3.6.3 週次レビュー設定

- 週次レビュー生成のタイミング（例: 日曜 22:00）
- レビュー内容のテンプレート（秘書の口調も含めてカスタマイズ可能）

------

### 3.7 LlmMemory（LLM メモリ）

#### 3.7.1 目的

- ユーザーの好み・作業スタイル・過去の調整結果などを記録し
- 将来の判断に活かすための長期メモリ

#### 3.7.2 フィールド案

- `id`
- `user_id`
- `content`: 自由テキスト（Markdown 等）
- `structured_preferences`: JSON（作業パターンや好みを構造化したもの）
- `updated_at`

#### 3.7.3 ユーザー編集

- 秘書の「性格調整シート」として UI から閲覧・編集できるようにする

------

### 3.8 Assistant（秘書エージェント）

#### 3.8.1 目的

- ユーザーとのインターフェイスとなる「秘書」
- 内部では複数の LLM 呼び出しを束ねながら、表面上は 1 人のキャラクターとして振る舞う

#### 3.8.2 フィールド案

- `id`
- `name`
- `persona_prompt`: 口調・性格・振る舞いのプロンプト
- `avatar_kind`: `STATIC` / `GIF` / `WEBM` / `LIVE2D`
- `avatar_path`: 画像やモデルファイルのパス
- `enabled_features`: 担当する自動化機能のセット
- `memory_id`: 関連する `LlmMemory` の ID

#### 3.8.3 表示

- ホーム画面右側に立ち絵とチャットビュー
- 表示 / 非表示はユーザー設定で切り替え可能

------

### 3.9 AutomationLog（自動化ログ）

#### 3.9.1 目的

- LLM / 秘書が行った自動処理や提案を記録し、可視化・ロールバックを可能にする

#### 3.9.2 フィールド案

- `id`
- `task_id`（任意）
- `project_id`（任意）
- `list_id`（任意）
- `assistant_id`
- `action_type`: `MOVE`, `UPDATE_DUE`, `CLASSIFY`, `CREATE_TASK` など
- `before_state`: 対象エンティティの JSON スナップショット
- `after_state`
- `created_at`
- `explanation`: なぜその処理を行ったかの説明（LLM 生成）

#### 3.9.3 ロールバック方針

- v1
  - AutomationLog の 1 件単位で「before_state に戻す」操作をサポート
  - Project / List / Task ごとにフィルタして閲覧できるようにする
- v2 以降
  - イベントソーシング的な履歴管理（差分形式）を検討

------

## 4. アーキテクチャ

### 4.1 レイヤ構造

- domain 層
  - Task / Project / List / Milestone / Assistant などのビジネスロジック
- infrastructure 層
  - DB（SQLite 予定）、ファイルストレージ、同期クライアント、LLM クライアント
- application / service 層
  - ユースケース単位のサービス
    - Inbox への追加
    - 自動分類ジョブの実行
    - 週次レビュー生成
    - スケジュール提案
- presentation 層
  - デスクトップ UI（Tauri + Web フロント想定）

------

## 5. 技術選定メモ

### 5.1 GUI フレームワーク

- 第一候補: **Tauri 2.x**
  - Rust バックエンド + Web フロント（Tailwind / React / Svelte など）
  - UI リッチ化が比較的容易（チャット UI、Gantt、Kanban など）
  - Android / iOS サポートが公式に視野に入っている
- 代替案: Iced
  - バックエンド・フロントとも Rust で完結
  - デスクトップ特化で安定
  - 将来的に「全部 Rust に寄せたい」と感じたときの候補

### 5.2 データ永続化

- v1: SQLite + `sqlx` or `SeaORM`
  - 理由
    - ローカルファーストアプリと相性が良い
    - Rust エコシステムが成熟
- 拡張の余地
  - 分析用途で DuckDB をサブ DB として利用
  - サーバーサイド（クラウド同期サービス）では PostgreSQL / libSQL 系を検討

### 5.3 ID 設計

- ULID / UUID を各エンティティの主キーに採用
- 同期やマルチデバイスを考慮し、整数自増 ID のみにはしない

### 5.4 同期設計の基本方針

- ローカルファースト + クラウド同期オプション
- v1
  - Last-Write-Wins + AutomationLog による変更履歴
- v2 以降
  - CRDT や event sourcing によるより安全なマージ戦略を検討

### 5.5 LLM 抽象化

- ライブラリベースで実装
  - マルチプロバイダ対応クライアント（例: rsllm 系）
  - ローカル LLM（Ollama）とクラウド LLM を同じ抽象で扱う
- Agent フレームワーク（LangChain 系）は必要になった段階で導入

### 5.6 バックグラウンド処理

- ジョブキューを用意し非同期で処理
  - Inbox 自動分類
  - 期日接近タスクの検出
  - 週次レビューの準備
- AFK 時間の検出や「しばらく入力が止まったタイミング」で処理を走らせる設計も検討

------

## 6. LLM 活用ポリシー（サマリ）

### 6.1 自然言語入力 → 構造化タスク

- タイトル・期間・プロジェクト推定
- 必要ならユーザーに質問しながら補完

### 6.2 自動分類

- Inbox のタスクを personal / プロジェクトに振り分け
- 信頼度が低いときはユーザーに確認
- 自動分類の結果は AutomationLog に記録し、後からレビュー可能にする

### 6.3 依存関係・サブタスク抽出

- 文章から必要なステップを分解し、サブタスク案を生成
- Gantt / Kanban / カレンダーに映しやすい構造を作る

### 6.4 期日・スケジュール提案

- 期日が未設定の場合、必要ならユーザーに質問
- 一旦 LLM が案を決めて due_date / start_date を設定し
  - ユーザーが後からレビューする前提
  - レビュー内容は LlmMemory に記録して次回以降に活かす

### 6.5 週次レビュー

- 完了・未完了・延期タスク、マイルストーン状況を集約
- 秘書の口調で「よかった点」「改善点」「来週のフォーカス」を生成
- レビュー内容やテンプレートはユーザーがカスタマイズ可能

------

## 7. UI / UX のラフ

### 7.1 ホーム画面

- 中央
  - 今日やること（Agenda）
  - プロジェクト単位の Gantt またはカレンダー表示
- 左サイドバー
  - Inbox / Personal
  - プロジェクト一覧
  - ビュー切り替え（List / Board / Calendar / Gantt）
- 右ペイン
  - 秘書の立ち絵（表示 ON/OFF 可）
  - チャットビュー（タスク相談・週次レビュー・計画相談）

### 7.2 入力体験

- グローバルショートカットでどこからでも入力モーダルを開く
- 入力内容を
  - スラッシュコマンド（/due, /p など）
  - 自然言語
     で解析し、タスク or チャットを自動判定

------

## 8. スローガン候補（英語）

- Carry no tasks in your head.
- Think in projects, not in to-do lists.
- Less task juggling. More actual work.

------

## 9. 未決定事項・TODO メモ

- RDBMS ライブラリの最終決定（`sqlx` / `SeaORM` など）
- LLM クライアントライブラリの選定
- AutomationLog の内部フォーマット（丸ごと JSON か diff か）
- Vault 単位の切り替え UX（マルチプロファイル的な扱い）
- Live2D 対応の具体的な技術選定（当面は静止画 / GIF / WebM のみ）
- 自動化項目の粒度と設定 UI（項目数とユーザー負担のバランス）