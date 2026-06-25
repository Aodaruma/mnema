# Mnema

> Carry no tasks in your head.  
> 頭の中にタスクを残さない。

Mnema は、ローカルファーストで動く **AI agent付きタスク管理ツール** です。

思いついたタスクを投げ込むと、Mnema がタスクがどのリストに属するか判断し、  
プロジェクトやスケジュールとして整理し、期日が近づいたタスクのリマインドや  
週次の振り返りまで「能動的に」一緒に行うことを目指します。

- ローカルファーストな Vault 構造
- LLM による inbox 整理・スケジューリング・週次レビュー
- プロジェクト単位のビュー（リスト・ボード・カレンダー・ガント）
- ユーザーの代わりに気づき・提案・通知を行う AI エージェントをインターフェイスとして利用

> ステータス: **初期設計 / プロトタイピング中**  
> 実装、構成、採用技術は今後大きく変わる可能性があります。

---

## なぜ Mnema か

人間の脳は「考えること」は得意だが、「覚えておくこと」にはあまり向いていない。

Mnema はこの前提に立ち、次のような役割を担うことを目指す:

- タスクを **頭の外に退避させるための信頼できる置き場** になること
- ユーザーからの簡潔なコマンド・会話を **プロジェクト・マイルストーン・スケジュール** に変換すること
- AI の「秘書」によってタスクの通知やスケジューリング、  
  さらにユーザーが何も言わなくても **1 週間のふりかえりや再計画の提案を能動的に行うこと**

---

## コアアイデア

### ローカルファーストが前提

- データの正本はローカルに置く。
- 同期はあくまでオプションであり、必須要件ではない（Obsidian の Vault + Sync のイメージに近い）。
- 初期状態では SQLite を構造化データの正本にする。Docker や PostgreSQL の導入なしでアプリ単体を触れるようにする。
- PostgreSQL は self-hosted / サーバー運用向けの任意バックエンドとして残す。
- ボルト内にはプロジェクトの説明、ユーザーの趣向、添付、エクスポートなどをファイルとして保存する。

### AI は自律してユーザーをサポートする「秘書」

- LLM が行うことの例:
  - inbox に放り込まれたタスクの分類（プロジェクト振り分け / personal など）
  - 期日やスケジュールの案出し
  - マイルストーンやサブタスク構造の提案
  - 週次レビューのサマリ生成
  - 期日が近い / 詰まりが発生しているタスクへの能動的なリマインドや再計画の提案
- ユーザー側が保持すること:
  - 最終決定権（自動処理はログに残し、可能な範囲でロールバック可能）
  - 自動化のレベル（機能ごとに OFF / 要確認 / 自動 + レビュー などを設定）
  - 「どこまで能動的に動いてよいか」を項目別にコントロールする権限

### プロジェクト・マイルストーン・ビュー

- **Project**
  - タスクとマイルストーンを束ねる単位。
  - 期間（start/end）や説明を持つ。
- **Task**
  - 期日、コスト（cost point）、依存関係などを持つ実行単位。
  - priority は永続しない（期日やコストなどから都度算出）。
- **Milestone**
  - プロジェクト内のチェックポイント・節目を表現するエンティティ。
  - `NOT_DONE / DONE` の固定ステータスを持つ。

予定しているビュー:

- List（一覧）
- Board（ステータスごとのカンバン）
- Calendar
- Gantt（プロジェクト単位）

### 「リスト」であって「仕事」にはしない

Mnema は UX として **手間を増やさない** ことを大事にする:

- グローバルショートカットからのすばやいキャプチャ（inbox 行き）
- `/due`, `/estime` などのスラッシュコマンドで日付やプロジェクトを指定可能
- 必須項目は最小限にし、可能な部分は AI に推定させる
- 必要なときは AI 側からタスクの詰まり・偏りを検知して「今やるべきこと」を提案する

---

## アーキテクチャ（高レベル）

※ 詳細は `docs/specification-v0.1.md` を参照。

- **言語**: Rust（コアロジック / バックエンド）
- **デスクトップシェル**: 検討中（現時点では Tauri を本命候補として評価中）
- **ストレージ**:
  - 既定は SQLite
  - PostgreSQL は `MNEMA_STORAGE_BACKEND=postgres` と `MNEMA_DATABASE_URL` で任意利用
  - ボルト内には添付・エクスポート・アセット・ローカル設定などのファイルを配置
- **LLM レイヤ**:
  - プロバイダ切替可能（ローカル: Ollama / クラウド: OpenAI 互換 API など）
  - 「計画・思考」用モデルと「分類・雑務」用モデルを分けて設定できるようにする
- **自動化 / バックグラウンド処理**:
  - Inbox 分類
  - 期日・スケジュール案出し
  - 週次レビュー
  - ユーザーへの能動的な通知・提案（AFK 時や一日の終わりなどにまとめて提示）
  をバックグラウンドジョブとして実行

---

## データベース

Mnema は既定で SQLite を使う。ローカルアプリとして触るだけなら Docker や PostgreSQL のインストールは不要。

SQLite DB は同期フォルダのファイルロックを避けるため、既定では Vault 外に置く。

- Windows: `%LOCALAPPDATA%\Mnema\mnema.sqlite`
- Linux/macOS 風の環境: `$XDG_DATA_HOME/mnema/mnema.sqlite` または `$HOME/.local/share/mnema/mnema.sqlite`

保存先は `MNEMA_SQLITE_PATH` で上書きできる。

PostgreSQL を使う場合:

```bash
export MNEMA_STORAGE_BACKEND=postgres
export MNEMA_DATABASE_URL=postgres://postgres:postgres@localhost/mnema
```

PostgreSQL モードで `MNEMA_DATABASE_URL` が未設定の場合、開発用デフォルトとして `postgres://postgres:postgres@localhost/mnema` を試す。

DB なしでスケジューラのデモを確認する場合:

```bash
cargo run -p mnema-desktop -- --demo-plan
```

既定の SQLite backend で試す場合:

```bash
cargo run -p mnema-desktop -- add "Write first task" --due 2026-06-25 --minutes 45
cargo run -p mnema-desktop -- list
cargo run -p mnema-desktop -- plan --save
cargo run -p mnema-desktop -- schedule
```

---

## リポジトリ構成（案）

まだ確定ではないが、イメージとして:

```text
mnema/
  README.md
  README.ja.md
  CONTRIBUTING.md
  LICENSE
  docs/
    specification-v0.1.md
  crates/
    core/         # ドメインモデル・サービス
    scheduler/    # 決定的な計画生成・スケジュール提案ロジック
    infra/        # DB, LLM クライアント, 同期周り
    desktop/      # デスクトップアプリ（Tauri など）
```

------

## ドキュメント

- 設計・仕様（ドラフト）:
  - `docs/specification-v0.1.md`

今後、API・UI フロー・同期仕様などの詳細ドキュメントを追加していく予定。

------

## コントリビュート方法

Mnema はまだ初期段階だが、フィードバックや issue、PR は歓迎する。

- 大きめの変更・新機能については、まず issue で方針を相談してから PR を送るのが望ましい。
- 開発フローやコーディングルールはまだ流動的なため、詳細は今後 `CONTRIBUTING.md` に追記していく。

詳しくは `CONTRIBUTING.md` を参照。

------

## ライセンス

Mnema は現在 **Apache License 2.0** のもとで公開している。
 詳細は `LICENSE` を参照。
