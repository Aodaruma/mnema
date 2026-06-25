# UI framework decision

作成日: 2026-06-25

## 結論

当面の Mnema desktop は egui / eframe で進める。

理由:

- 既存コードが Rust workspace で完結しており、egui は Rust 側の application service / repository を直接つなぎやすい。
- eframe は native と web を同じ egui app として扱えるため、プロトタイプの速度が出る。
- 公式情報上、eframe は Web / Linux / Mac / Windows / Android を対象にできる。
- immediate mode UI は、Today / Inbox / Schedule のような状態表示と操作のスパイクを短時間で作りやすい。

## Slint との比較

Slint は宣言的UI、Material 3 components、見た目の整えやすさが強い。
公式情報上も desktop / mobile / web / embedded を狙える。

ただし現時点では、Mnema はまだ画面仕様やUXの検証段階であり、UI定義を固めるよりも domain / scheduler / storage と画面を素早く接続することを優先する。
そのため、初期UIは egui で作り、以下の条件が見えてきたら Slint への移行またはSlint版フロントの追加を再検討する。

- 画面構成が安定し、宣言的UIで保守したい段階になった。
- モバイルのタッチUI品質を優先したい。
- デザイントークンやMaterial系コンポーネントの比重が高くなった。

## 現時点の実装方針

- `crates/desktop` は CLI と GUI を同居させる。
- 引数なしの `mnema-desktop` はGUIを起動する。
- `plan`, `add`, `list`, `schedule` は従来通りCLIとして残す。
- SQLite default backend を使い、ローカルアプリ単体で触れる状態を優先する。

参考:

- egui / eframe: https://github.com/emilk/egui
- eframe docs: https://docs.rs/eframe
- Slint: https://slint.dev/
- Slint docs: https://docs.slint.dev/
