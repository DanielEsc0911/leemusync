# Textos de LeemuSync para Rust (español neutro latinoamericano). Mismas claves que en/leemusync.ftl.

## Ayuda
help-about = Mantiene tus partidas guardadas sincronizadas entre tus dispositivos, con un almacenamiento que es tuyo.
help-about-json = Muestra JSON para programas (las claves siempre van en inglés).
help-init = Crea un repo cifrado nuevo en el almacenamiento que configuraste.
help-join = Une este dispositivo a un repo existente.
help-join-recovery = Usa la clave de recuperación en lugar de la frase de contraseña.
help-backend = Configura dónde se guarda el repo.
help-backend-add = Elige el almacenamiento.
help-backend-add-gdrive = Usa Google Drive (LeemuSync solo ve los archivos que creó).
help-backend-add-gdrive-client_id = Tu id de cliente OAuth (tipo "App de escritorio").
help-backend-add-s3 = Usa un bucket compatible con S3 (Cloudflare R2, AWS S3, Backblaze B2…).
help-backend-add-s3-endpoint = El endpoint, por ejemplo https://<id de cuenta>.r2.cloudflarestorage.com
help-backend-add-s3-bucket = El nombre del bucket.
help-backend-add-s3-access_key_id = El id de la clave de acceso. La clave secreta se pide después y se guarda en el llavero del sistema.
help-backend-add-s3-region = La región (R2 usa auto).
help-backend-add-fs = Usa una carpeta local o montada.
help-backend-add-fs-path = La carpeta.
help-backend-login = Vuelve a iniciar sesión en Google Drive.
help-grant = Elige qué carpetas puede leer y escribir LeemuSync.
help-grant-add = Permite una carpeta (la carpeta del emulador).
help-grant-add-path = La carpeta.
help-grant-list = Lista las carpetas permitidas.
help-players = Administra los jugadores.
help-players-add = Agrega un jugador.
help-players-add-name = El nombre del jugador.
help-players-list = Lista los jugadores.
help-scan = Busca emuladores, cuentas y partidas guardadas en este dispositivo.
help-accounts = Conecta jugadores con cuentas del emulador en este dispositivo.
help-accounts-map = Usa una cuenta del emulador para un jugador en este dispositivo.
help-accounts-map-player = El nombre del jugador.
help-accounts-map-emulator = El id del emulador, por ejemplo cemu.
help-accounts-map-account = El id de la cuenta, por ejemplo 80000002.
help-accounts-map-keep = Conserva las partidas guardadas de la cuenta (se suben y luego resuelves un conflicto).
help-accounts-map-replace = Reemplaza las partidas guardadas de la cuenta por las más recientes del jugador (las actuales van primero al historial local).
help-accounts-recover = Guarda una partida huérfana como la primera versión de un jugador.
help-accounts-recover-emulator = El id del emulador, por ejemplo cemu.
help-accounts-recover-account = El id de cuenta de la carpeta huérfana.
help-accounts-recover-player = El jugador que recibe esta partida guardada.
help-sync = Sincroniza ahora.
help-sync-dry_run = Muestra qué pasaría sin cambiar nada.
help-sync-yes = Confirma la primera sincronización en este dispositivo.
help-status = Muestra la configuración y las partidas guardadas de este dispositivo.
help-history = Muestra el historial de versiones de un jugador para un juego.
help-history-player = El nombre del jugador.
help-history-emulator = El id del emulador, por ejemplo cemu.
help-history-game = El id del juego, por ejemplo 101c9400.
help-restore = Restaura una versión o una copia local (antes se respaldan los archivos actuales).
help-restore-player = El nombre del jugador.
help-restore-emulator = El id del emulador, por ejemplo cemu.
help-restore-game = El id del juego, por ejemplo 101c9400.
help-restore-what = Un id de versión o el nombre de una copia de `leemusync history`.
help-restore-yes = No pide confirmación.
help-doctor = Revisa el llavero del sistema, el almacenamiento y las carpetas del emulador.

## Errores ($detail nunca se traduce: rutas, ids, texto del proveedor)
error-usage = { $detail }
error-config = El archivo de configuración no es válido: { $detail }
error-backend = Problema con el almacenamiento: { $detail }
error-backend_auth = Falló el inicio de sesión del almacenamiento ({ $detail }). Ejecuta `leemusync backend login`.
error-io = Problema con un archivo: { $detail }
error-crypto = Problema de cifrado: { $detail }
error-unlock = La frase de contraseña o la clave de recuperación no es correcta.
error-locked = Ya hay otro proceso de LeemuSync en ejecución ({ $detail }).
error-emulator_running = { $detail } está abierto. Ciérralo y vuelve a intentarlo.
error-not_granted = LeemuSync no tiene permiso para usar { $detail }. Ejecuta `leemusync grant add <carpeta>`.
error-not_set_up = Todavía falta configurar: { $detail }
error-secrets = Falló el llavero del sistema: { $detail }
error-data = El repo tiene datos no válidos: { $detail }
error-needs_confirmation = Es la primera sincronización en este dispositivo. Primero respalda las partidas guardadas de este dispositivo y luego ejecuta `leemusync sync --yes`.
error-busy = { $detail } cambió mientras se leía. Vuelve a intentarlo cuando el juego esté en reposo.
error-injected = Falla de prueba en { $detail }.
error-needs-choice = Usa --keep o --replace (la cuenta ya tiene partidas guardadas de: { $games }).
error-needs-tty = Esto necesita una terminal para preguntarte: { $what }.

## Errores con clave: error-<código>-<clave>. Tras `clave:` el motor pasa datos ($detail: una ruta, id o nombre).
error-not_set_up-backend = Todavía no hay almacenamiento configurado. Ejecuta `leemusync backend add fs <carpeta>`, `leemusync backend add s3 --endpoint <url> --bucket <nombre> --access-key-id <id>` o `leemusync backend add gdrive --client-id <id>`.
error-not_set_up-s3_key = La clave secreta de S3 no está en el llavero del sistema de este dispositivo. Vuelve a ejecutar `leemusync backend add s3`.
error-not_set_up-gdrive_login = No iniciaste sesión en Google Drive en este dispositivo. Ejecuta `leemusync backend login`.
error-not_set_up-gdrive_backend = Google Drive no es el almacenamiento de este dispositivo. Ejecuta `leemusync backend add gdrive --client-id <id>`.
error-not_set_up-repo = Este dispositivo todavía no tiene un repo. Ejecuta `leemusync init` o `leemusync join`.
error-not_set_up-repo_key = Falta la clave del repo en el llavero del sistema. Vuelve a ejecutar `leemusync join`.
error-usage-repo_exists = El almacenamiento ya tiene un repo. Mejor ejecuta `leemusync join`.
error-usage-repo_race = Otro dispositivo creó un repo en el almacenamiento al mismo tiempo. Mejor ejecuta `leemusync join`.
error-usage-already_joined = Este dispositivo ya usa otro repo.
error-usage-passphrase_too_short = La frase de contraseña necesita al menos 8 caracteres.
error-usage-player_name_invalid = El nombre de un jugador necesita de 1 a 32 caracteres visibles.
error-usage-player_name_taken = Ese nombre de jugador ya está en uso.
error-usage-file_too_large = { $detail } es demasiado grande para sincronizarse.
error-usage-file_name_invalid = { $detail } tiene un nombre de archivo que no se puede sincronizar.
error-usage-grant_path_missing = Esa carpeta no existe.
error-usage-grant_path_not_allowed = No se puede permitir esa carpeta: elige la carpeta del emulador, no / ni tu carpeta personal.
error-usage-invalid_game_or_account = Ese id de juego o de cuenta no es válido.
error-usage-restore_reason_invalid = Nombre de copia no válido: { $detail }
error-usage-unknown_player = No hay ningún jugador { $detail }. Ejecuta `leemusync players add`.
error-usage-no_install = No se encontró { $detail } en este dispositivo. Ejecuta `leemusync scan`.
error-usage-no_account = Este dispositivo no tiene la cuenta del emulador { $detail }. Créala en el emulador y luego ejecuta `leemusync scan`.
error-usage-account_taken = { $detail } ya usa esa cuenta.
error-usage-not_orphaned = { $detail } no es una partida huérfana en este dispositivo.
error-usage-already_has_save = El jugador ya tiene una partida guardada de { $detail }. Mejor recupérala para un jugador nuevo.
error-usage-folder_empty = Esa carpeta de guardado está vacía.
error-usage-not_mapped = El jugador no tiene una cuenta de { $detail } en este dispositivo. Ejecuta `leemusync accounts map`.
error-usage-unknown_restore_point = { $detail } no es una versión ni una copia. Consulta `leemusync history`.
error-config-repo_mismatch = El repo del almacenamiento no es el que usa este dispositivo.
error-config-repo_config = La lista de jugadores del repo no es válida.
error-config-no_home_dir = No se encontró tu carpeta personal.
error-config-invalid_flatpak_app_id = Un perfil de emulador tiene un id de app de Flatpak no válido.
error-config-invalid_regex = Un perfil de emulador tiene un patrón no válido.
error-config-unsupported_capture = Un perfil de emulador usa un patrón no compatible.
error-secrets-bad_master_key = La clave del repo en el llavero del sistema está dañada. Vuelve a ejecutar `leemusync join`.
error-data-repo_format_newer = El repo lo creó una versión más nueva de LeemuSync. Actualiza LeemuSync en este dispositivo.
error-data-history_changed = Una copia del historial local cambió mientras se subía: { $detail }
error-data-blob_corrupt = Un archivo del repo está dañado ({ $detail }).
error-data-manifest_invalid = Una versión del repo no es válida.
error-data-wrong_size = Un archivo del repo tiene un tamaño incorrecto: { $detail }
error-data-file_too_large = Un archivo del repo es demasiado grande: { $detail }

## Preguntas
prompt-passphrase = Frase de contraseña:
prompt-passphrase-again = Escribe la frase de contraseña otra vez:
prompt-passphrases-differ = Las frases de contraseña no coinciden.
prompt-recovery = Clave de recuperación:
prompt-client-secret = Secreto del cliente OAuth (no se muestra):
prompt-s3-secret = Clave de acceso secreta (no se muestra):
prompt-first-sync = Es la primera sincronización en este dispositivo. ¿Respaldaste las partidas guardadas de este dispositivo?
prompt-keep-or-replace = La cuenta { $account } ya tiene partidas guardadas de { $games }. ¿Las conservas (se suben y resuelves un conflicto) o las reemplazas por la partida guardada más reciente de { $player } (los archivos actuales van primero al historial local)?
prompt-keep-or-replace-keys = [c]onservar / [r]eemplazar:
prompt-restore = ¿Reemplazar los archivos actuales de { $player } por { $what }? Los archivos actuales van primero al historial local.
prompt-yes-no = [s/N]:
answer-yes = s
answer-keep = c
answer-replace = r

## Resultados
login-open-url = Abre esta dirección en tu navegador para que LeemuSync use su propia carpeta de Google Drive:
login-waiting = Esperando al navegador (hasta 5 minutos)…
login-done = Google Drive está conectado.
backend-fs-done = El repo se guardará en { $path }.
backend-s3-done = El repo se guardará en el bucket { $bucket } de { $endpoint }.
init-done = Se creó el repo { $repo } en { $device }.
init-recovery = Tu clave de recuperación. Anótala ahora: solo se muestra una vez, y sin ella o sin la frase de contraseña no se pueden recuperar tus partidas guardadas.
join-done = { $device } se unió al repo { $repo }.
grant-done = Permitido: { $path }
grant-none = Todavía no hay carpetas permitidas.
players-added = Se agregó el jugador { $name }.
players-none = Todavía no hay jugadores.
map-done = { $player } ahora usa la cuenta { $account } de { $emulator } en este dispositivo.
recover-done = La partida huérfana ahora es la primera versión de { $game } de { $player }.
restore-done = Se restauró { $what } para { $player }. Ejecuta `leemusync sync` para subirla.
sync-line = { $player } · { $game } · { $action }
dry-run = Prueba: no se cambió nada.
sync-conflicts = { $count ->
    [one] Una partida guardada tiene un conflicto. Se conservan ambas versiones; resolverlo llega en la próxima versión.
   *[other] { $count } partidas guardadas tienen un conflicto. Se conservan todas las versiones; resolverlos llega en la próxima versión.
}

action-nothing = al día
action-adopt = ya coincide con el repo
action-fast_forward = se restauró la versión más reciente
action-upload = subida
action-upload_then_conflict = subida, con un conflicto por resolver
action-conflict = conflicto por resolver
action-local_missing = falta la carpeta de la partida guardada
action-remote_behind = el repo volvió a un estado anterior; no se cambió nada
action-needs_account = no hay cuenta en este dispositivo: ejecuta `leemusync accounts map`
action-skipped = omitida (el emulador está abierto)

warning-emulator_running = { $detail } está abierto, así que se omitieron sus partidas guardadas.
warning-not_granted = Todavía sin permiso: { $detail }. Ejecuta `leemusync grant add`.
warning-shared_folder_not_empty = { $detail } lo comparten todas las cuentas y todavía no se sincroniza.
warning-orphaned_save = Partida huérfana { $detail }: se borró su cuenta. Ejecuta `leemusync accounts recover` para conservarla.
warning-config_merged = Se combinaron cambios de dos dispositivos.
warning-player_renamed = Dos jugadores tenían el mismo nombre; uno ahora se llama { $detail }.
warning-remote_behind = Al repo le faltan versiones que este dispositivo vio ({ $detail }). No se cambió nada.
warning-restore_rolled_back = Se deshizo una restauración interrumpida en { $detail }.
warning-invalid_version = Se ignoraron datos no válidos del repo: { $detail }
warning-setup_incomplete = Se creó el repo, pero este dispositivo no quedó configurado del todo ({ $detail }). Ejecuta `leemusync join` con tu frase de contraseña para terminar.
warning-unreadable_folder = No se pudo leer una carpeta de guardado de forma segura y se omitió: { $detail }

scan-install = { $emulator } en { $base }
scan-needs-grant = Permítelo: leemusync grant add { $path }
scan-account = cuenta { $id } · { $name } · { $player }
scan-unknown = (ninguno)
scan-save = partida guardada { $game }/{ $account } · { $files ->
    [one] 1 archivo
   *[other] { $files } archivos
}
scan-orphan = partida huérfana { $game }/{ $account }: ejecuta `leemusync accounts recover`

status-device = Dispositivo: { $device }
status-repo = Repo: { $repo } en { $backend }
status-no-repo = Todavía no hay un repo: ejecuta `leemusync init` o `leemusync join`.
status-stream = { $player } · { $emulator } { $game } · cuenta { $account }

history-version = { $id } · { $files ->
    [one] 1 archivo
   *[other] { $files } archivos
} { $head }
history-head = (la más reciente)
history-backup = copia local { $name }

doctor-ok = ok
doctor-fail = FALLÓ
doctor-check = { $status } { $name } { $detail }
