# LeemuSync Rust strings (English source). Keys describe meaning (i18n §Rules).
# Every key here must exist in es/leemusync.ftl (cargo xtask i18n-check).

## Help
help-about = Keeps your emulator saves in sync across your devices, through storage you own.
help-about-json = Print machine-readable JSON (keys are always in English).
help-init = Create a new encrypted repo on the storage you set up.
help-join = Join an existing repo from this device.
help-join-recovery = Use the recovery key instead of the passphrase.
help-backend = Set up where the repo is stored.
help-backend-add = Choose the storage.
help-backend-add-gdrive = Use Google Drive (LeemuSync only sees the files it created).
help-backend-add-gdrive-client_id = Your OAuth client id (type "Desktop app").
help-backend-add-s3 = Use an S3-compatible bucket (Cloudflare R2, AWS S3, Backblaze B2…).
help-backend-add-s3-endpoint = The endpoint, for example https://<account id>.r2.cloudflarestorage.com
help-backend-add-s3-bucket = The bucket name.
help-backend-add-s3-access_key_id = The access key id. The secret key is asked for next and kept in the system keyring.
help-backend-add-s3-region = The region (R2 uses auto).
help-backend-add-fs = Use a local or mounted folder.
help-backend-add-fs-path = The folder.
help-backend-login = Log in to Google Drive again.
help-grant = Choose which folders LeemuSync may read and write.
help-grant-add = Allow a folder (an emulator's own folder).
help-grant-add-path = The folder.
help-grant-list = List the allowed folders.
help-players = Manage players.
help-players-add = Add a player.
help-players-add-name = The player's name.
help-players-list = List the players.
help-scan = Find emulators, accounts and saves on this device.
help-accounts = Connect players to emulator accounts on this device.
help-accounts-map = Use an emulator account for a player on this device.
help-accounts-map-player = The player's name.
help-accounts-map-emulator = The emulator id, for example cemu.
help-accounts-map-account = The account id, for example 80000002.
help-accounts-map-keep = Keep the account's current saves (they're uploaded, then you resolve a conflict).
help-accounts-map-replace = Replace the account's saves with the player's latest (the current ones go to local history first).
help-accounts-recover = Keep an orphaned save as a player's first version.
help-accounts-recover-emulator = The emulator id, for example cemu.
help-accounts-recover-account = The orphaned folder's account id.
help-accounts-recover-player = The player who gets this save.
help-sync = Sync now.
help-sync-dry_run = Show what would happen without changing anything.
help-sync-yes = Confirm the first sync on this device.
help-status = Show this device's setup and saves.
help-history = Show a player's version history for a game.
help-history-player = The player's name.
help-history-emulator = The emulator id, for example cemu.
help-history-game = The game id, for example 101c9400.
help-restore = Put back a version or a local backup (the current files are backed up first).
help-restore-player = The player's name.
help-restore-emulator = The emulator id, for example cemu.
help-restore-game = The game id, for example 101c9400.
help-restore-what = A version id or a backup name from `leemusync history`.
help-restore-yes = Don't ask for confirmation.
help-doctor = Check the keystore, the storage and the emulator folders.

## Errors ($detail is never translated: paths, ids, provider text)
error-usage = { $detail }
error-config = The config file is invalid: { $detail }
error-backend = Storage problem: { $detail }
error-backend_auth = The storage login failed ({ $detail }). Run `leemusync backend login`.
error-io = File problem: { $detail }
error-crypto = Encryption problem: { $detail }
error-unlock = Wrong passphrase or recovery key.
error-locked = Another LeemuSync process is running ({ $detail }).
error-emulator_running = { $detail } is running. Close it and try again.
error-not_granted = LeemuSync isn't allowed to use { $detail }. Run `leemusync grant add <folder>`.
error-not_set_up = Not set up yet: { $detail }
error-secrets = The system keyring failed: { $detail }
error-data = The repo has invalid data: { $detail }
error-needs_confirmation = This is the first sync on this device. Back up this device's saves first, then run `leemusync sync --yes`.
error-busy = { $detail } changed while it was being read. Try again when the game is idle.
error-injected = Test fault at { $detail }.
error-needs-choice = Use --keep or --replace (the account already has saves for: { $games }).
error-needs-tty = This needs a terminal to ask you: { $what }.

## Keyed errors: error-<code>-<key>. After `key:` the engine passes data ($detail: a path, id or name).
error-not_set_up-backend = No storage set up yet. Run `leemusync backend add fs <folder>`, `leemusync backend add s3 --endpoint <url> --bucket <name> --access-key-id <id>` or `leemusync backend add gdrive --client-id <id>`.
error-not_set_up-s3_key = The S3 secret key isn't in this device's system keyring. Run `leemusync backend add s3` again.
error-not_set_up-gdrive_login = Google Drive isn't logged in on this device. Run `leemusync backend login`.
error-not_set_up-gdrive_backend = Google Drive isn't the storage on this device. Run `leemusync backend add gdrive --client-id <id>`.
error-not_set_up-repo = This device has no repo yet. Run `leemusync init` or `leemusync join`.
error-not_set_up-repo_key = This device's repo key is missing from the system keyring. Run `leemusync join` again.
error-usage-repo_exists = The storage already has a repo. Run `leemusync join` instead.
error-usage-repo_race = Another device created a repo on the storage at the same time. Run `leemusync join` instead.
error-usage-already_joined = This device already uses another repo.
error-usage-passphrase_too_short = The passphrase needs at least 8 characters.
error-usage-player_name_invalid = A player name needs 1 to 32 visible characters.
error-usage-player_name_taken = That player name is taken.
error-usage-file_too_large = { $detail } is too large to sync.
error-usage-file_name_invalid = { $detail } has a file name that can't be synced.
error-usage-grant_path_missing = That folder doesn't exist.
error-usage-grant_path_not_allowed = That folder can't be allowed: choose the emulator's own folder, not / or your home folder.
error-usage-invalid_game_or_account = That game id or account id isn't valid.
error-usage-restore_reason_invalid = Invalid backup name: { $detail }
error-usage-unknown_player = There's no player { $detail }. Run `leemusync players add`.
error-usage-no_install = { $detail } wasn't found on this device. Run `leemusync scan`.
error-usage-no_account = This device has no emulator account { $detail }. Create it in the emulator, then run `leemusync scan`.
error-usage-account_taken = That account is already used by { $detail }.
error-usage-not_orphaned = { $detail } isn't an orphaned save on this device.
error-usage-already_has_save = The player already has a save for { $detail }. Recover it to a new player instead.
error-usage-folder_empty = That save folder is empty.
error-usage-not_mapped = The player has no { $detail } account on this device. Run `leemusync accounts map`.
error-usage-unknown_restore_point = { $detail } is neither a version nor a backup. See `leemusync history`.
error-config-repo_mismatch = The repo on the storage isn't the one this device joined.
error-config-repo_config = The repo's player list is invalid.
error-config-no_home_dir = Your home folder couldn't be found.
error-config-invalid_flatpak_app_id = An emulator profile has an invalid Flatpak app id.
error-config-invalid_regex = An emulator profile has an invalid pattern.
error-config-unsupported_capture = An emulator profile uses an unsupported pattern.
error-secrets-bad_master_key = The repo key in the system keyring is damaged. Run `leemusync join` again.
error-data-repo_format_newer = The repo was made by a newer LeemuSync. Update LeemuSync on this device.
error-data-history_changed = A local history copy changed while it was being uploaded: { $detail }
error-data-blob_corrupt = A file in the repo is damaged ({ $detail }).
error-data-manifest_invalid = A version in the repo is invalid.
error-data-wrong_size = A file in the repo has the wrong size: { $detail }
error-data-file_too_large = A file in the repo is too large: { $detail }

## Prompts
prompt-passphrase = Passphrase:
prompt-passphrase-again = Type the passphrase again:
prompt-passphrases-differ = The passphrases don't match.
prompt-recovery = Recovery key:
prompt-client-secret = OAuth client secret (it isn't shown):
prompt-s3-secret = Secret access key (it isn't shown):
prompt-first-sync = This is the first sync on this device. Did you back up this device's saves?
prompt-keep-or-replace = Account { $account } already has saves for { $games }. Keep them (they're uploaded and you resolve a conflict) or replace them with { $player }'s latest save (the current files go to local history first)?
prompt-keep-or-replace-keys = [k]eep / [r]eplace:
prompt-restore = Replace { $player }'s current files with { $what }? The current files go to local history first.
prompt-yes-no = [y/N]:
answer-yes = y
answer-keep = k
answer-replace = r

## Results
login-open-url = Open this address in your browser to let LeemuSync use its own Google Drive folder:
login-waiting = Waiting for the browser (up to 5 minutes)…
login-done = Google Drive is connected.
backend-fs-done = The repo will be stored in { $path }.
backend-s3-done = The repo will be stored in bucket { $bucket } at { $endpoint }.
init-done = Repo { $repo } created on { $device }.
init-recovery = Your recovery key. Write it down now: it's shown only once, and without it or the passphrase your saves can't be recovered.
join-done = { $device } joined repo { $repo }.
grant-done = Allowed: { $path }
grant-none = No folders allowed yet.
players-added = Player { $name } added.
players-none = No players yet.
map-done = { $player } now uses { $emulator } account { $account } on this device.
recover-done = The orphaned save is now { $player }'s first { $game } version.
restore-done = Restored { $what } for { $player }. Run `leemusync sync` to upload it.
sync-line = { $player } · { $game } · { $action }
sync-line-moved = { $player } · { $game } · { $action } · { $files ->
    [one] 1 file
   *[other] { $files } files
}, { $size } MB
sync-elapsed = Done in { $secs } s.
progress-checking = { $player } · { $game } · checking…
progress-uploading = { $player } · { $game } · uploading { $bar } { $done }/{ $total } · { $size_done }/{ $size_total } MB · { $rate } MB/s
progress-downloading = { $player } · { $game } · downloading { $bar } { $done }/{ $total } · { $size_done }/{ $size_total } MB · { $rate } MB/s
dry-run = Dry run: nothing was changed.
sync-conflicts = { $count ->
    [one] One save has a conflict. Both versions are kept; resolving comes in the next release.
   *[other] { $count } saves have a conflict. All versions are kept; resolving comes in the next release.
}

action-nothing = up to date
action-adopt = already matches the repo
action-fast_forward = restored the latest version
action-upload = uploaded
action-upload_then_conflict = uploaded, conflict to resolve
action-conflict = conflict to resolve
action-local_missing = the save folder is missing
action-remote_behind = the repo went back in time; nothing changed
action-needs_account = no account on this device: run `leemusync accounts map`
action-skipped = skipped (the emulator is running)

warning-emulator_running = { $detail } is running, so its saves were skipped.
warning-not_granted = Not allowed yet: { $detail }. Run `leemusync grant add`.
warning-shared_folder_not_empty = { $detail } is shared by every account and isn't synced yet.
warning-orphaned_save = Orphaned save { $detail }: its account was deleted. Run `leemusync accounts recover` to keep it.
warning-config_merged = Changes from two devices were merged.
warning-player_renamed = Two players had the same name; one is now { $detail }.
warning-remote_behind = The repo is missing versions this device saw ({ $detail }). Nothing was changed.
warning-restore_rolled_back = An interrupted restore was undone in { $detail }.
warning-invalid_version = Ignored invalid data in the repo: { $detail }
warning-setup_incomplete = The repo was created, but this device isn't fully set up ({ $detail }). Run `leemusync join` with your passphrase to finish.
warning-unreadable_folder = A save folder couldn't be read safely and was skipped: { $detail }

scan-install = { $emulator } at { $base }
scan-needs-grant = Allow it: leemusync grant add { $path }
scan-account = account { $id } · { $name } · { $player }
scan-unknown = (none)
scan-save = save { $game }/{ $account } · { $files ->
    [one] 1 file
   *[other] { $files } files
}
scan-orphan = orphaned save { $game }/{ $account }: run `leemusync accounts recover`

status-device = Device: { $device }
status-repo = Repo: { $repo } on { $backend }
status-no-repo = No repo yet: run `leemusync init` or `leemusync join`.
status-stream = { $player } · { $emulator } { $game } · account { $account }

history-version = { $id } · { $files ->
    [one] 1 file
   *[other] { $files } files
} { $head }
history-head = (latest)
history-backup = local backup { $name }

doctor-ok = ok
doctor-fail = FAILED
doctor-check = { $status } { $name } { $detail }
