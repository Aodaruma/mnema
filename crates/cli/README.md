# Mnema CLI

GUIなしでScheduling MVPを操作する基準interfaceです。既定は `./vault` とOS標準のSQLite保存先を利用します。

```powershell
cargo run -p mnema-cli -- --help
cargo run -p mnema-cli -- hours set --timezone Asia/Tokyo --work-start 09:00 --work-end 17:00 --sleep-start 23:00 --sleep-end 07:00 --travel-minutes 15
cargo run -p mnema-cli -- habit add "Morning walk" --minutes 30 --earliest 07:00 --latest 09:00
cargo run -p mnema-cli -- task add "Write proposal" --due 2026-08-20 --minutes 60
cargo run -p mnema-cli -- plan preview --days 7
```

PreviewはDBのScheduleBlockを変更しません。反映時は、表示されたfingerprintをそのまま渡します。

```powershell
cargo run -p mnema-cli -- plan apply --days 7 --fingerprint <previewのfingerprint>
cargo run -p mnema-cli -- schedule list
```

主なsubcommand:

- `task`: add / list / update / complete / delete
- `habit`: add / list / expand / skip / snooze / disable
- `hours`: IANA timezone、Work、Sleep、Travel buffer
- `plan`: 1〜31日のpreview / apply / repair
- `calendar`: Google OAuth、calendar選択、sync、managed calendar、write-back

Google連携には `MNEMA_GOOGLE_CLIENT_ID` と `MNEMA_GOOGLE_REDIRECT_URI` が必要です。既定ではtokenをOS Keyringへ保存します。headless環境では、32-byte keyのBase64URL表現を `MNEMA_CREDENTIAL_KEY` または `MNEMA_CREDENTIAL_KEY_FILE` で渡すと、Vault内の暗号化ファイルへ保存します。

CLIのOAuth code/verifierを引数で渡すとshell historyへ残り得るため、通常は `mnema-server` のWeb Calendar画面から接続してください。CLIが必要な場合は `MNEMA_GOOGLE_OAUTH_CODE`、`MNEMA_GOOGLE_OAUTH_VERIFIER`、`MNEMA_GOOGLE_OAUTH_STATE`、`MNEMA_GOOGLE_OAUTH_RETURNED_STATE` の環境変数も利用できます。
