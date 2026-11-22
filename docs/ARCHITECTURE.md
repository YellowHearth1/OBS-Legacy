# Архітектура OBS-Legacy

Технічний довідник для розробки. Огляд для користувачів — у [`README.md`](../README.md), план перевірки
на справжньому залізі — у [`TESTING.md`](TESTING.md).

## Складові

| Частина | Файли | Роль |
|---|---|---|
| Рушій | `scripts/build-obs-nvenc-legacy-artix.sh` (bash, ~3700 рядків) | Уся реальна робота: профіль заліза, перевірка, збирання OBS, плагінів і ffmpeg, пакети pacman (`*-legacy-580xx`), встановлення через `pacman -U`, відкат, видалення, pacman-хук у пакеті |
| TUI | `src/` (Rust, ratatui 0.30) | Інтерфейс поверх рушія: меню, панель стану, вбудований термінал, плагіни, налаштування |
| Установник | `install.sh`, `assets/obs-legacy.svg` | Без теки з вихідними кодами (`curl … \| bash`) на Arch/Artix ставить пакет `obs-legacy` з релізу (SHA-256, `sudo pacman -U` локального файлу; копію з `~/.local/bin` пакет переймає сам); без pacman чи з `--no-package` — готовий `obs-legacy-x86_64` у `~/.local/bin`. У теці з вихідними кодами збирає TUI (`--target-dir` репозиторію) у `~/.local/bin` і прибирає `target/`. Далі однаково: ярлик у меню (у пакета — свій) і на робочий стіл, пропонує ввімкнути сповіщення |
| Сповіщення | `src/updates.rs`, `src/notify.rs`, `src/service.rs` | Фонова перевірка оновлень (`obs-legacy --watch`) і сповіщення на робочому столі; служба користувача systemd, dinit, OpenRC, runit, s6 або автозапуск сесії (див. «Сповіщення про оновлення») |
| Імітатор | `dev/fake-build.sh` | Друкує те саме, що рушій (разом зі звітом), але нічого не робить — для розробки інтерфейсу |
| Тест keep-libs | `dev/test-keep-libs.sh` | Хук збереження бібліотек без root: генерує його з рушія, «ховає» бібліотеку з кешу ldconfig і перевіряє pre/post (на машині з пакетами OBS-Legacy; без них — пропускає) |
| Скріншоти | `dev/screenshot.py` | PNG зі знімка `tmux capture-pane -e -p -N` (Pillow) — для `docs/screenshots/` |
| Іконка й банер | `dev/make-assets.py`, `assets/third-party/` | `assets/obs-legacy.svg` і `assets/banner.png` (1280×640, шапка README і прев'ю репозиторію) з іконки «graphics-card» Phosphor Icons (MIT) і шрифту JetBrains Mono (OFL), текст — кривими. Без логотипів NVIDIA й OBS: це чужі торгові марки, а їхні правила забороняють використання без ліцензії |
| Пакет програми | `packaging/PKGBUILD`, `packaging/obs-legacy.desktop`, `.github/workflows/package.yml` | Готовий пакет pacman самої програми (`obs-legacy-<версія>-1-x86_64.pkg.tar.zst`) для кожного релізу — див. «Пакет програми» |
| CI | `.github/workflows/ci.yml` | `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo check` на Rust 1.88 (MSRV), `shellcheck`, `plugins --tsv`; `push` — лише в `main`, PR — через `pull_request` |

TUI **вбудовує** скрипт (`include_str!` у `src/script.rs`) і перед запуском розпаковує його в
`~/.local/share/obs-legacy/build-obs-nvenc-legacy-artix.sh`. Тому зміни в скрипті потрапляють у TUI лише
після перезбирання (`./install.sh --update`). Для перевірки зміненого скрипта без перезбирання:
`OBS_LEGACY_SCRIPT=$PWD/scripts/build-obs-nvenc-legacy-artix.sh obs-legacy`.

## Рушій: команди

| Команда | Що робить | sudo |
|---|---|---|
| `check` | інвентаризація, профіль заліза, план ffmpeg, пакети, вихідні коди OBS, nv-codec-headers (auto), CEF, вихідні коди плагінів, пробна конфігурація CMake, які пакети буде замінено | ні |
| `install` | `build_set` (усе з `check` + пакети для збірки, компіляція, `obs-nvenc-test`, плагіни, ldd, ffmpeg, makepkg) + `pacman -U` набору, прибирання старої схеми /opt | так |
| `update` | те саме, але після `check_driver_vs_headers` — `update_reasons` (див. «Оновлення й перезбирання»): нічого нового → «ОНОВЛЕНЬ НЕМАЄ», без збирання | так, якщо є що оновлювати |
| `rebuild` | те саме, що `install`, але CMake OBS начисто (`configure_obs … fresh`) і спершу `clear_stale_git_locks` | так |
| `package` | `build_set` без встановлення: лише набір пакетів у сховищі | лише якщо бракує пакетів для збірки |
| `prepare` | як `update`, але без встановлення й без жодного питання (бракує пакетів для збірки → `die` з `fix deps`); так його запускає `--watch` (`AUTO_PREPARE`) | ні |
| `repair` | як `prepare`, але лише коли є збережені копії keep-libs чи бібліотек бракує (інакше одразу `Підсумок:` без мережі) і тих самих версій: `OBS_REF` = встановлений `obs_commit`, `NV_HEADERS_TAG` — встановлений, плагіни — встановлені (`repair_plugin_pin`: їхній коміт і мітка версії), ffmpeg — лише за `ffmpeg_needs_rebuild`; так його запускає добудовувач (див. «Автоматичне оновлення разом із системою») | ні |
| `upgrade-system` | `repo-sync` → пробне `pacman -Sup` на копії баз → за потреби спершу `update` (інші soname libav* у репозиторії) і «місток» (`pacman -Syu ffmpeg`: на час оновлення ffmpeg з репозиторію замість `ffmpeg-legacy-580xx` — лише для пакетів без keep-libs, коли той заблокував би оновлення чи втратив бібліотеку, `ffmpeg_libs_change`) → `pacman -Syu` → `update` дочірнім процесом | так |
| `install-files` | для пакетних менеджерів: найновіший повний набір, новіший за встановлений, з бібліотеками на місці (`set_libs_ok`) → на stdout `install <файл>` і `remove <пакет>` (`stale_plugin_packages`), перед тим `backup_obs_config`; нічого не ставить (див. «Пакетні менеджери й AUR-помічники») | ні |
| `repo-sync` | власна копія баз синхронізації в `~/.local/state/obs-legacy/pacman` (`fakeroot pacman -Sy --disable-sandbox --dbpath`, `local` — посилання на `/var/lib/pacman/local`); `repo_version` бере новішу з двох баз | ні |
| `status` | встановлені пакети, BUILDINFO, check-libs, набори в сховищі, оновлення OBS і плагінів | ні |
| `rollback [НАБІР]` | `pacman -U` збереженого набору (питає підтвердження); поки пакетів немає — стара схема: симлінк `current` у /opt | так |
| `uninstall` | `pacman -S ffmpeg` (замість `ffmpeg-legacy-580xx`), `pacman -R` інших `*-legacy-580xx`, залишки старої схеми | так |
| `deps` | лише пакети для збірки: `install_dependencies` (confirm, `pacman -S --needed`, без `-Syu`) | так |
| `deps --missing` | для TUI: чого бракує, рядки `repo <пакет>` / `norepo <пакет>`; швидко, без лока й звіту | ні |
| `clean` | видаляє проміжні каталоги збірки (не вихідні коди й не архів CEF) | ні |
| `plugins [--tsv]` | каталог плагінів (людський або машинний формат для TUI) | ні |
| `help`, `version` | довідка, версія | ні |

Без аргументів — простий текстовий `choose_from_menu`. `check`, `install`, `rollback`, `uninstall`
загорнуті в `run_logged`: вивід дублюється у `~/.cache/obs-studio-nvenc-artix/reports/<дата>-<команда>.txt`
(без кольорів), у кінець звіту дописується `Результат: успішно | скасовано | помилка (код N)`, а в
термінал — рядок `Звіт: <шлях>`.

### Повідомлення для людини

| Функція | Префікс | Коли |
|---|---|---|
| `say` | `==> ` | новий етап |
| `ok` | `[OK] ` | етап удався |
| `warn` | `[УВАГА] ` / `[WARNING] ` | щось варто знати, але робота триває |
| `fail` / `die "що" ["що робити"]` | `[ПОМИЛКА] ` / `[ERROR] ` | зупинка; другий аргумент `die` друкується як порада |
| `hint` | `[ПОРАДА] ` / `[HINT] ` | що робити далі — пишеться одразу після попередження чи помилки, до яких належить |
| `cancelled` | `[СКАСОВАНО] ` / `[CANCELLED] ` | людина відповіла «n»: нічого не змінено, код завершення 10 (`EXIT_CANCELLED`) |
| `fix команда "опис"` | `[ВИПРАВИТИ] ` / `[FIX] ` | що можна виправити однією командою скрипта (`deps`, `clean`, `check`, `rebuild`); TUI робить із цього кнопку «Виправити» |
| `summary` | `Підсумок: ` / `Summary: ` | наприкінці `install`/`update`/`rebuild`: що змінилося («OBS 33.0.0 → 33.0.1», «оновлень немає…»); TUI показує першим рядком картки результату |

Префікс друкується мовою повідомлень (`TAG_*`, див. «Мова»), а текст — через `t "українською" "in English"`:
`warn "$(t "Не вдалося $x" "Could not $x")"`, формат `printf` — `printf "$(t '%s пакетів\n' '%s packages\n')" "$n"`.

Порада має бути конкретною: яку команду виконати, який пункт меню обрати, що перевірити. Назву пункту
меню бере `install_action` («Встановити / оновити OBS» у TUI, `obs-legacy → …` або команда без нього).

`confirm` (і `confirm`/`confirm_yes` в `install.sh`): Enter — типова відповідь (`[y/N]` — ні); `y`, `yes`,
«так» — так; `n`, `no`, «ні» — ні. Перед розбором `clean_answer` відкидає коди стрілок і Esc, керівні
символи й пробіли (`sed` з `LC_ALL=C`: в українській локалі діапазон `[@-~]` не охоплює латинських літер).
«т» і «н» неоднозначні («так» / «ні», але й клавіші `N` і `Y` в українській розкладці) — підказка про
розкладку й питання ще раз; будь-яка інша незрозуміла відповідь — теж питання ще раз, а не «ні». Без
введення (EOF) — ні.

`check` без обов'язкових пакетів (`DEP_INSTALL_REQ`) не запускає пробну конфігурацію CMake — вона однаково
впала б, — а завершується успішно з попередженням і порадою, що їх поставить `install`.

### Потік `install` (і `package`)

`preflight_common` → `select_priv_tool` → `acquire_lock` (flock) → `detect_foreign_install` → `print_header` →
**`build_set`**: `resolve_selected_plugins` → `show_inventory` → **`detect_build_profile`** → **`ffmpeg_plan`**
→ `check_disk_space` → `install_dependencies` (pacman -S --needed після `confirm`) → `detect_mbedtls` →
`prepare_obs_source` (git, `OBS_REF`, сабмодулі, `OBS_MAJOR`) → `prepare_headers` (тег auto під драйвер) →
`check_driver_vs_headers` → `prepare_cef` → `build_cmake_args` → `configure_obs` (reuse з підписом) →
`build_obs` (ninja, `nice`) → `run_nvenc_test` → `stage_install` (DESTDIR у кеш, `strip_debug_info`,
BUILDINFO) → `build_plugins` (кожен у свій `stage/`) → `verify_build` (ldd OBS і кожного плагіна) →
`begin_set` (номер збирання, каталог набору) → `build_ffmpeg_package` → `package_set` (makepkg: OBS, плагіни)
→ `finalize_set` (BUILDINFO набору, `.complete`).

Далі лише `install`: `backup_obs_config` → `install_set` (план замін, `confirm`, `pacman -U`) →
`remove_legacy_install` (якщо лишилася стара схема) → `history.tsv` → `prune_store` (`KEEP_VERSIONS`) →
`cleanup_stages`.

Мережа: git clone/fetch (OBS і сабмодулі, nv-codec-headers, плагіни, PKGBUILD ffmpeg) іде через `net_retry` —
невдала спроба повторюється ще раз за 15 с (один раз: без мережі кожна чекає на з'єднання до ~2 хв).
Невдача для OBS і заголовків — зупинка з порадою (`NET_HINT`), для плагіна — плагін пропускається (див.
«Плагіни»). CEF завантажує curl з `--retry 3`. Нічого не висить вічно: скрипт експортує
`GIT_TERMINAL_PROMPT=0` (зниклий чи закритий репозиторій — помилка, а не запит логіна в терміналі) і
`GIT_HTTP_LOW_SPEED_LIMIT=1` / `GIT_HTTP_LOW_SPEED_TIME=120`, а curl (тут, в `install.sh` і в «Оновити
OBS-Legacy» з релізу) має `--speed-limit 1 --speed-time 120`: з'єднання, що дві хвилини не передає жодного
байта (зник інтернет, а комп'ютер працює від ДБЖ), обривається, і спрацьовує повтор.

`LABEL` = `<YYYYmmdd-HHMMSS>_<git describe OBS>` — назва набору в сховищі й `label=` у BUILDINFO.
`PREFIX` = `/usr` (як в OBS з репозиторію): OBS 33 запікає абсолютні шляхи плагінів з
`CMAKE_INSTALL_PREFIX`, тож сторонні плагіни в `/usr/lib/obs-plugins` чи `/usr/lib/obs-modules/plugins`
підхоплюються.

### Безпекові гарантії (порушувати не можна)

- Система змінюється лише через pacman: `pacman -S --needed` (пакети для збірки) і `pacman -U` (пакети
  OBS-Legacy) — обидва після `confirm` і без `--noconfirm`, тож pacman ще й сам питає про заміни.
  `pacman -Syu` — лише в `upgrade-system`, яку людина запускає сама (`obs-legacy --upgrade-system`), і теж
  без `--noconfirm`. Виняток — добудовувач з пакета `obs-legacy` (див. «Автоматичне оновлення разом із
  системою»): після транзакції pacman, яку запустила людина, він ставить `pacman -U --noconfirm` копії пакетів
  `*-legacy-580xx` зі сховища того, хто запустив оновлення. Файли чужих пакетів не перезаписуються: власників файлів нового пакета
  скрипт додає в `conflicts` (pacman запропонує замінити), а файл без власника — зупинка з порадою.
- pacman-хуки пакета OBS змінюють лише своє: `keep-libs` кладе копії бібліотек у `/usr/lib/obs-legacy/kept-libs`
  (тека з пакета, у `ld.so.conf.d`) і список у `/var/lib/obs-legacy/kept-libs.tsv`, викликає `ldconfig`, ніколи
  не зупиняє транзакцію (код завжди 0, без `AbortOnFail`); `check-libs` нічого не змінює.
- Компіляція, тимчасове встановлення й makepkg — без root. `sudo`/`doas` потрібні лише для pacman і
  прибирання старої схеми (`/opt/obs-studio-nvenc`, `/usr/local/bin/obs`, хук у `/etc`).
- Пакети ставляться лише після `obs-nvenc-test` і перевірки бібліотек. Інакше поточний OBS не змінюється.
  ffmpeg ставиться лише після власного тесту NVENC (`ffmpeg_smoke_test`).
- `rm` дозволено лише в межах кешу (`rm_cache_path`), сховища наборів (`rm_store_path`) або в
  `/opt/obs-studio-nvenc` з маркером `ROOT_MARK` (стара схема).
- Усі файли, створені скриптом, містять `MARKER="Managed by build-obs-nvenc-legacy-artix"`. **`SELF_NAME` і
  `MARKER` не змінювати**, бо за ними скрипт впізнає свої попередні встановлення.
- Скрипт відмовляється працювати від root.

## Шляхи

| Що | Де |
|---|---|
| Пакети | `obs-studio-legacy-580xx`, `<назва в AUR>-legacy-580xx`, `ffmpeg-legacy-580xx` — файли в `/usr` |
| Відомості про збірку | `/usr/share/obs-legacy/BUILDINFO.txt` (пакет OBS), `plugins/<id>` (пакет плагіна), `ffmpeg.txt` (пакет ffmpeg) |
| pacman-хуки | `/usr/share/libalpm/hooks/obs-legacy.hook` → `/usr/lib/obs-legacy/check-libs`; `00-obs-legacy-keep-libs-{pre,post}.hook` → `/usr/lib/obs-legacy/keep-libs` (у пакеті OBS) |
| Збережені копії бібліотек | `/usr/lib/obs-legacy/kept-libs/` (`/etc/ld.so.conf.d/obs-legacy-kept-libs.conf`), список `/var/lib/obs-legacy/kept-libs.tsv` (keep-libs) |
| Набори пакетів | `~/.local/share/obs-studio-nvenc-artix/packages/<LABEL>/` (`*.pkg.tar.zst`, `BUILDINFO.txt` набору, `.complete`) |
| Номер збирання | `~/.local/share/obs-studio-nvenc-artix/build-number` (pkgrel наборів) |
| Стара схема (прибирається) | `/opt/obs-studio-nvenc/{versions,current}`, `/usr/local/bin/obs`, `~/.local/share/applications/com.obsproject.Studio.desktop`, `/etc/pacman.d/hooks/90-obs-legacy.hook` → `/usr/local/lib/obs-legacy/check-libs` |
| Кеш | `~/.cache/obs-studio-nvenc-artix/` (`obs-studio-src`, `nv-codec-headers-*`, `downloads`, `cef`, `build`, `build-check`, `stage`, `plugins/<id>/{src,build,stage,build.log,tried-commit,qt-private.cmake}`, `makepkg/` (PKGBUILD, makepkg.conf, журнали, `build/`), `sources/` (SRCDEST: git ffmpeg), `ffmpeg-pkgbuild/` (PKGBUILD Arch), `gnupg/` (ключі для підпису ffmpeg), `reports`) |
| Стан | `~/.local/share/obs-studio-nvenc-artix/` (`packages/`, `config-backups/`, `history.tsv`, `build-number`, `*-original.backup`, хеші CEF) |
| Налаштування | `~/.config/obs-legacy/settings.env` |
| TUI | `~/.local/bin/obs-legacy`, `~/.local/share/applications/obs-legacy.desktop`, `~/.local/share/icons/hicolor/scalable/apps/obs-legacy.svg` |
| Дані TUI | `~/.local/share/obs-legacy/` (`build-obs-nvenc-legacy-artix.sh` — розпакований скрипт, `source-dir` — тека з вихідними кодами, з якої встановлено, або `release-install` — тег релізу, якщо програму поставлено готовою) |
| Сповіщення | `~/.local/state/obs-legacy/` (`updates.env` — що вже показано, `updates.log` — журнал, `watch.lock` — PID фонової перевірки); служба — див. таблицю в «Сповіщення про оновлення» |

## Налаштування

`load_settings_file` читає `settings.env` **до** блоку значень за замовчуванням:

- лише рядки `KEY=VALUE` з ключами зі `SETTINGS_KEYS` або `PLUGIN_REF_*`. Нічого не виконується як код;
- лапки довкола значення знімаються;
- якщо змінна вже задана в середовищі (навіть порожня), файл її не перекриває;
- перемагає перше входження ключа.

TUI пише цей самий файл (`src/settings.rs`), зберігаючи коментарі й невідомі рядки, і передає скрипту
`OBS_LEGACY_SETTINGS=<шлях>`. Ключ `UI_BACKGROUND` стосується лише TUI: скрипт його не знає і пропускає.

`OBS_PLUGINS`: незадано → `obs-multi-rtmp`; `none` або порожньо → без плагінів; розділювачі — кома або
пробіл. `CPU_TUNING` (native | generic | x86-64-v2/v3/v4), `OPT_LEVEL` (2 | 3), `BUILD_FFMPEG` (1 | 0),
`NV_HEADERS_TAG` (auto | тег) — див. «Профіль заліза» і «ffmpeg-legacy-580xx». У TUI `CPU_TUNING` і
`OPT_LEVEL` — поля `Kind::Choice` (Enter перебирає варіанти).

## Профіль заліза

`detect_build_profile` (етап «Оптимізація під залізо» в `check`, `install`, `package`) визначає:

| Що | Звідки | Куди йде |
|---|---|---|
| Дистрибутив | `/etc/os-release` (`ID`, `PRETTY_NAME`) | звіт, BUILDINFO `distro` |
| Базові прапорці | `makepkg_var`: `/etc/makepkg.conf` + `makepkg.conf.d/*.conf` + `~/.config/pacman/makepkg.conf` (у підоболонці bash) | `tune_flags` |
| Процесор | `/proc/cpuinfo` (`model name`, `flags`), `gcc -march=native -Q --help=target` (назва, напр. `ivybridge`), `ld-linux-x86-64.so.2 --help` (рівні x86-64-vN з «supported») | `-march`, BUILDINFO `cpu_*`, `isa_level` |
| Відеокарта | `nvidia-smi --query-gpu=name,driver_version,compute_cap` (старі драйвери — без `compute_cap`); кілька відеокарт — найстарша архітектура | CUDA-архітектура ffmpeg (`sm_61`), BUILDINFO `gpu`, `gpu_cc` |
| Пакет драйвера | `pacman -Qqo /usr/lib/libnvidia-encode.so.1` | optdepends пакетів |
| NVENC API | `nvenc_driver_api` (dlopen libnvidia-encode) | тег nv-codec-headers (`pick_headers_tag`) |
| Потоки | `compute_jobs` (CPU і RAM ~2 ГБ/потік) | ninja, `MAKEFLAGS` |

`tune_flags`: прапорці дистрибутива без `-march=*`, `-mtune=*`, `-O*` і без прапорців лише для профілювання
(`-fno-omit-frame-pointer`, `-mno-omit-leaf-frame-pointer`), а спереду — `MARCH_FLAGS -O$OPT_LEVEL`.
`MARCH_FLAGS`: native → `-march=native -mtune=native`; generic → `-march/-mtune` дистрибутива;
x86-64-vN → `-march=x86-64-vN` (якщо процесор його не підтримує — зупинка з порадою). Зміцнення
дистрибутива (`_FORTIFY_SOURCE`, `-fstack-clash-protection`, `-fcf-protection`, `-D_GLIBCXX_ASSERTIONS`,
relro/now) лишається.

OBS і плагіни: `profile_cmake_flags` → `CMAKE_BUILD_TYPE=RelWithDebInfo`, `CMAKE_C/CXX_FLAGS` = профіль,
`CMAKE_C/CXX_FLAGS_RELWITHDEBINFO=-DNDEBUG` (без `-g`: його однаково прибирає strip; `-O` — з профілю),
`CMAKE_*_LINKER_FLAGS` = LDFLAGS дистрибутива. Release з IPO не використовується (за даними LFS, OBS з ним
падає). Плагін, що не зібрався з прапорцями дистрибутива (старі плагіни й `-Werror=format-security` тощо),
отримує другу спробу лише з `MARCH_FLAGS -O$OPT_LEVEL`.

`PROFILE_SIG` — хеш тюнінгу, `-march` процесора, `-O`, прапорців і CUDA-архітектури: за ним `ffmpeg_plan`
вирішує, чи перезбирати ffmpeg. `NV_HEADERS_TAG=auto`: `pick_headers_tag` бере найновіший тег `nX.Y.*` з
X.Y ≤ NVENC API драйвера (580xx → n13.0.19.1), а `obs_min_nvenc_api` (з `find_package(FFnvcodec N)` у
`plugins/obs-nvenc/CMakeLists.txt`) не пускає заголовки, старіші за мінімум OBS (OBS 33 — 12.0).

Встановлену збірку під `native` перенесли на інший процесор (BUILDINFO `cpu_model` ≠ поточний) —
попередження в `check`/`install`, порада «Що далі» і рядок у «Твоя система».

Кілька відеокарт NVIDIA: CUDA-фільтри ffmpeg збираються (clang, `--enable-cuda-llvm`) у PTX, а драйвер
компілює PTX під конкретну відеокарту під час роботи. PTX під старшу архітектуру працює й на новіших, навпаки —
ні, тож `GPU_CC` — найменша compute capability серед усіх відеокарт (одна відеокарта — її власна, і
`PROFILE_SIG` від цього не змінюється). `GPU_NAME` — перша відеокарта, з приміткою «і ще N».

ffmpeg: `configure` бере `CFLAGS` з makepkg (тобто профіль) і додає свій `-O3`, а його асемблерний код вибирає
SIMD-інструкції під процесор під час роботи (`runtime cpu detection`). Програма `obs-legacy` теж збирається
під процесор: `install.sh` у теці з вихідними кодами додає `RUSTFLAGS=-C target-cpu=native` (CI, пакет і
готова програма з релізу — без нього, бо ставляться на будь-який комп'ютер). Системні бібліотеки
(Qt, x264, mbedTLS) не перезбираються: найважчі з них самі вибирають SIMD під процесор; CEF — готовий архів
OBS Project, з якого лише прибирається налагоджувальна інформація.

## Пакети pacman

Дерева OBS і плагінів збираються й перевіряються нашим конвеєром, а makepkg лише пакує їх
(`make_repack_package`: згенерований PKGBUILD, `package()` = `cp -a "$_tree/." "$pkgdir/"`,
`options=(!strip !debug !lto emptydirs)`, `makepkg --nodeps`). makepkg запускається без root з власним
`makepkg.conf` (`write_makepkg_conf`: `source /etc/makepkg.conf` + `.d` + файл користувача, потім прапорці
профілю, `MAKEFLAGS=-j$JOBS`, `OPTIONS` з `!debug`, `BUILDENV` з `!sign`, `PKGEXT=.pkg.tar.zst`) і
`PKGDEST` = каталог набору.

| Пакет | depends | provides | conflicts |
|---|---|---|---|
| `obs-studio-legacy-580xx` | `elf_pkg_deps`: власники бібліотек з ldd (наш `ffmpeg-legacy-580xx` → `ffmpeg`) | `obs-studio=<ver>` + `obs-browser`, `obs-websocket`, `obs-vst`, `obs-studio-plugin-browser`, якщо модулі є | `obs-studio obs-studio-plugin-browser obs-browser obs-linuxbrowser obs-websocket obs-vst` + власники файлів |
| `<назва в AUR>-legacy-580xx` | `obs-studio-legacy-580xx` + ldd | `<назва в AUR>=<ver>` | усі назви з 7-го поля каталогу + власники файлів |
| `ffmpeg-legacy-580xx` | з PKGBUILD Arch | PKGBUILD Arch (sonames) + `ffmpeg=<epoch:ver>` | `ffmpeg` |

Через `provides=obs-studio` + `conflicts=obs-studio` pacman сам пропонує замінити будь-який OBS
(`conflicts` у pacman зіставляються і з `provides` встановлених пакетів), а пакети, що потребують
`obs-studio` (плагіни з AUR), задовольняються. `add_file_conflicts`: `pacman -Qo` для шляхів, що вже є в
системі; власник (не з нашої родини) → у `conflicts`; файл без власника → зупинка з порадою.

Версії: OBS — `pkgver_from` (33.0.0; `33.0.0-3-gabc` → `33.0.0.r3.gabc`; `33.1.0-beta1` → `33.1.0beta1`),
плагіни — тег без `v`. pkgrel усіх пакетів набору = `BUILD_NUMBER` (`next_build_number`: лічильник у стані,
не менший за pkgrel встановленого пакета), тож кожен новий набір для pacman «новіший». ffmpeg —
`<pkgrel Arch>.<n>` (напр. `2:9.0.2-2.1`).

Вміст пакетів: OBS — дерево `stage/usr` + `share/obs-legacy/BUILDINFO.txt` + хуки, `check-libs`, `keep-libs`,
порожня `lib/obs-legacy/kept-libs` і `/etc/ld.so.conf.d/obs-legacy-kept-libs.conf` +
`share/licenses/obs-studio-legacy-580xx/COPYING`; плагін — `plugins/<id>/stage/usr` +
`share/obs-legacy/plugins/<id>` (`plugin.<id>=<версія> <коміт12>`) + ліцензія (`guess_license` за текстом
LICENSE/COPYING). `strip_debug_info` прибирає `.debug_*` з бінарників CEF (libcef.so з архіву OBS — 2 ГБ з
налагоджувальною інформацією).

Набір: `~/.local/share/obs-studio-nvenc-artix/packages/<LABEL>/` — пакети, `BUILDINFO.txt` набору (BUILDINFO
OBS + рядки плагінів + `ffmpeg=` + `packages=`) і `.complete`. `install_set`: план (що буде замінено —
`replaced_packages` за `SET_CONFLICTS`; які пакети плагінів приберуться; чи прибиратиметься стара схема),
`confirm`, `pacman -U`, а потім `remove_stale_plugins`: наші пакети плагінів, яких немає в наборі (зняли
галочку), — `pacman -R` (pacman сам питає підтвердження). Пакети плагінів залежать від `obs-studio-legacy-580xx`
без версії, тож інакше лишився б плагін, зібраний під інший OBS. `ffmpeg-legacy-580xx` живе окремо від наборів
і не прибирається. `prune_store` лишає `KEEP_VERSIONS` повних наборів разом зі встановленим. Відкат —
`pacman -U` іншого набору (pacman попереджає про старіші версії) і так само `remove_stale_plugins`.

## ffmpeg-legacy-580xx

`ffmpeg_plan` (до встановлення пакетів для збірки): `BUILD_FFMPEG=0` → не збирається; версія в репозиторіях
— `pacman -Si ffmpeg` (перший `Version`); збирається, якщо наш пакет не встановлено, або `arch_version` у
`/usr/share/obs-legacy/ffmpeg.txt` інша, або змінився `PROFILE_SIG`, або «Щоразу чиста збірка». Інакше —
пропуск. `prepare_ffmpeg_pkgbuild`: git-кеш `gitlab.archlinux.org/archlinux/packaging/packages/ffmpeg`, тег
`<epoch>-<pkgver>-<pkgrel>` (2:9.0.2-2 → `2-9.0.2-2`; немає — головна гілка з попередженням), версія й
залежності — з `.SRCINFO` (без виконання коду; без `ffnvcodec-headers` і без залежностей-sonames).
`deps --missing` бере їх з уже завантаженого `.SRCINFO` (`load_ffmpeg_deps 1`).

`build_ffmpeg_package`: каталог `makepkg/ffmpeg-legacy-580xx/` = файли репозиторію пакета (латки, ключі) +
PKGBUILD = PKGBUILD Arch + блок OBS-Legacy: `pkgname`, `pkgrel`, `pkgdesc+=`, `provides+=(ffmpeg=…)`,
`conflicts=(ffmpeg)`, optdepends `nvidia-utils:` → пакет драйвера, `makedepends` без `ffnvcodec-headers`,
`package()` = оригінальний + `ffmpeg.txt` (`eval "_obs_legacy_$(declare -f package)"`), а для CUDA —
`sed` у `declare -f build`: `--enable-cuda-llvm` → `… --nvccflags='--cuda-gpu-arch=sm_XY -O2'`.

PKGBUILD Arch сам робить `export PKG_CONFIG_PATH=/usr/lib/mbedtls2/pkgconfig`, тож приватні заголовки 13.0
подаються через обгортку `makepkg/shim/pkg-config` першою в `PATH` (дописує наш pcdir попереду). Підписаний
тег ffmpeg перевіряється ключами з `keys/pgp/` в окремому `GNUPGHOME` (`~/.cache/…/gnupg`), без змін у
наборі ключів користувача. makepkg іде у фоні з журналом (`makepkg/ffmpeg-legacy-580xx.log`), а в термінал
що хвилину — «збирається N хв; скомпільовано файлів: M». Коди makepkg: 8 — бракує залежностей (порада +
`[ВИПРАВИТИ] deps`), 16 — PGP. Після збирання `ffmpeg_smoke_test` кодує тестове відео `h264_nvenc` і
`hevc_nvenc` зібраним бінарником (`LD_LIBRARY_PATH` = бібліотеки з `pkg/`); не вийшло — пакет не
ставиться. Будь-яка невдача ffmpeg — попередження, OBS ставиться без нього.

Коли в репозиторії виходить новий ffmpeg, `pacman -Syu` наш пакет не чіпає (його немає в репозиторіях).
`ffmpeg_plan` перезбирає ffmpeg лише за справжньої потреби: нова версія в репозиторії (`version_no_rel`:
лише `epoch:pkgver`, новий pkgrel — це здебільшого перезбирання під нові бібліотеки, і його ловить таке),
бібліотеки, які зберіг keep-libs (`kept_libs ffmpeg`), бібліотеки, яких бракує (`ffmpeg_missing_lib`), новий
`FFMPEG_RECIPE_VERSION` (`recipe=` у `ffmpeg.txt`), профіль заліза чи «Щоразу чиста збірка»
(`ffmpeg_needs_rebuild`). Тег PKGBUILD Arch для версії Artix (`2:9.1-1.1`) — спершу точний, потім без
припису до pkgrel (`2-9.1-1`). У BUILDINFO набору — `ffmpeg_arch=` (з якої версії репозиторію зібрано).

**Залежності без жорсткої версії soname.** makepkg перетворює голе `libass.so` у `depends` на `libass.so=9-64`
(`find_libdepends`), і тоді нова libass зупиняла б `pacman -Syu` («breaks dependency … required by
ffmpeg-legacy-580xx»). Тому PKGBUILD ffmpeg переписує `*.so` на `*.so>=0` (і на верхньому рівні, і в
`package()`), а `pkg_deps_for_libs` для OBS і плагінів пише `libjack.so>=0` (будь-який пакет із цим soname,
будь-якої версії; `pacman -T 'libass.so>=0'` — так, `'libass.so=8-64'` після оновлення — ні). Те, що
бібліотеку справді замінено, ловить keep-libs (нижче). `RECIPE_VERSION` і `FFMPEG_RECIPE_VERSION` = 2: пакети
з жорсткими версіями перезбираються першим же «Оновити».

Оновлення системи (`upgrade-system`) — пробний `pacman -Sup` на копії баз. Коли стоїть keep-libs
(`keep_libs_installed`) нові бібліотеки нічому не заважають: `pacman -Syu`, потім `update`. «Місток» (на час
оновлення ffmpeg з репозиторію замість `ffmpeg-legacy-580xx`: той самий soname libav*, NVENC в OBS — власний
модуль) лишився для пакетів, зібраних до рецепта 2: коли пробний прогін падає через `-legacy-580xx` чи keep-libs
ще немає, а бібліотеки ffmpeg міняються (`ffmpeg_libs_change`). Якщо в репозиторії ffmpeg з іншим soname
libav*, спершу `update` збирає й ставить OBS-Legacy під нього.

**Готовий набір** (`ready_set`, замість колишнього `pending_set`): `install` і `update` (і `prepare`) перед
збиранням шукають повний набір, новіший за встановлений, зібраний саме з того, що зібралося б зараз
(`set_matches_target`: OBS, заголовки, профіль, рецепт, цільові версії плагінів `PLUGIN_TARGET` — плагін, що
не зібрався з того самого коміту, може бути відсутній, — і `ffmpeg_arch` = версія в репозиторії), і
перевіряють ldd-ом розпакований набір (`set_libs_ok`: набір міг зібратися до оновлення системи). Такий
набір ставиться без збирання.

## Перехід зі старої схеми /opt

До пакетів OBS-Legacy ставив збірки в `/opt/obs-studio-nvenc/versions/<LABEL>` з симлінком `current`,
обгорткою `/usr/local/bin/obs` (перекривала `/usr/bin/obs`), ярликом-перекриттям у
`~/.local/share/applications` і хуком у `/etc/pacman.d/hooks`. `legacy_leftovers_present` бачить будь-що з
цього; після вдалого `pacman -U` (і в `rollback` на набір пакетів) `remove_legacy_install` повертає
збережені `*-original.backup` (або прибирає наші файли), прибирає старий хук і, якщо жодна стара збірка не
запущена, `/opt/obs-studio-nvenc`. Поки пакетів немає, `rollback` перемикає `current` між старими
збірками (`rollback_legacy`), а TUI показує їх із позначкою `/opt`.

## CEF (Browser Source)

OBS 32+ **не має** `buildspec.json`. Дані про залежності лежать у `CMakePresets.json`: пресет з
`"name": "dependencies"` → `vendor."obsproject.com/obs-studio".dependencies.cef` з полями `version`,
`baseUrl`, `hashes["ubuntu-x86_64"]` і `revision["ubuntu-x86_64"]`. Ім'я архіву:
`cef_binary_<version>_linux_x86_64[_v<revision>].tar.xz`.

- OBS 33.0.0: CEF `7871`, без ревізії, SHA-256 `8ffc64d06ea74da613eeda7f2aa5d4e98661af6fa3ed8c68724c9a6e4e89ea8e`.
- OBS 32.2.2: CEF `6533`, ревізія `6`, SHA-256 `7963335519a19ccdc5233f7334c5ab023026e2f3e9a0cc417007c09d86608146`.

Ті самі URL і хеші є у Flatpak-маніфесті OBS (`build-aux/com.obsproject.Studio.json`).

`cef_spec_from_source` (Python) читає `CMakePresets.json`, а для старих OBS — `buildspec.json`.
`detect_cef_build` заповнює `CEF_BUILD_RESOLVED`, `CEF_SPEC_URL` і `CEF_SPEC_SHA`. Без Python номер версії
дістає awk, а посилання шукається перебором `_v12…_v1`. Порядок перевірки SHA-256:
`CEF_SHA256` → офіційний хеш (якщо ім'я файлу збігається з офіційним) → TOFU (хеш першого завантаження в
`~/.local/share/obs-studio-nvenc-artix/`).

> Початкова версія скрипта шукала лише `buildspec.json`. На OBS 32/33 автовизначення CEF через це не
> працювало й вимагало ручного `CEF_BUILD`. Виправлено в коміті «Скрипт: плагіни…».

## Плагіни

### Каталог

Масив `PLUGIN_CATALOG`, рядки `id|назва|git-репозиторій|пакети pacman|параметри CMake|опис|назви в AUR`.
Параметр виду `<33?-DFOO=ON` додається лише тоді, коли `OBS_MAJOR < 33`; `CXXFLAGS+=…` і `CFLAGS+=…`
дописуються до прапорців компілятора цього плагіна (RewardsTheater: `-Wno-error=deprecated-declarations`).
Назви в AUR: перша + `-legacy-580xx` — назва пакета (`plugin_pkgname`) і `provides`, усі — `conflicts`
(немає поля — `obs-<id>`). TUI отримує каталог командою `plugins --tsv`. Формат TSV: `id`, `назва`,
`вибрано (1/0)`, `типово (1/0)`, `репозиторій`, `пакети`, `опис`, `назва пакета`, `свій (1/0)`.

Свої плагіни власника машини — рядки того самого формату у `plugins.local` поруч із `settings.env`
(`OBS_LEGACY_PLUGINS_LOCAL` — інший файл). `load_local_plugins` дописує їх у кінець `PLUGIN_CATALOG`
до `readonly` і запам'ятовує в `PLUGIN_LOCAL`. Рядок перевіряється: `id` — `[a-z0-9][a-z0-9._-]*`, є назва,
репозиторій починається з `https://`, `ssh://`, `git@`, `file://` або `/`, пакети й назви в AUR — лише
допустимі для pacman символи, без табуляцій. Невдалий рядок чи `id` з вбудованого каталогу пропускається з
попередженням. Нічого з файлу не виконується: поля йдуть у ті самі функції, що й поля каталогу, а
параметри CMake — окремими аргументами.

### Конвеєр (`build_plugins` → `build_plugin` у підоболонці)

1. `plugin_checkout`: git-кеш `plugins/<id>/src`. Розбіжності не з'ясовуються, а виправляються:
   `reset --hard`, `clean -ffdx`, сабмодулі. Версія — `PLUGIN_REF_<id>` або найновіший тег
   `^v?N(.N)+$`, порівняння з `sort -V` без префікса `v`. Якщо тегів немає — `origin/HEAD`. Журнал
   `build.log` починається заново ще до завантаження й отримує помилки git (`tee_plain`, без кольорів);
   коміт, який збиратимуть, — у `plugins/<id>/tried-commit`.
2. `plugin_compile`: `cmake` з прапорцями профілю (`profile_cmake_flags`), `-DCMAKE_INSTALL_PREFIX=/usr
   -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PREFIX_PATH=$STAGED_TREE -DCMAKE_COMPILE_WARNING_AS_ERROR=OFF
   -DQT_VERSION=6 -DLINUX_PORTABLE=OFF` + параметри з каталогу. `libobs`/`obs-frontend-api` знаходяться
   через cmake-конфіги **тимчасового дерева OBS**: вони переносні й не прив'язані до префікса. До прапорців
   кожного плагіна додається `PLUGIN_WARN_FLAGS` (`-Wno-error=deprecated-declarations
   -Wno-error=discarded-qualifiers`): нові OBS і компілятори приносять нові попередження, а частина плагінів
   збирається з власним `-Werror`. Якщо CMake-файли плагіна згадують `Qt6::…Private`,
   `plugin_qt_private_arg` пише `plugins/<id>/qt-private.cmake` з `find_package(Qt6 QUIET COMPONENTS
   …Private)` і передає його як `-DCMAKE_PROJECT_INCLUDE` (Qt 6.10+ створює ці цілі лише на запит). Не
   вийшло — друга спроба лише з `-march`/`-O` (див. «Профіль заліза»).
3. `cmake --build` (вивід ninja йде в термінал, і TUI малює прогрес), потім `DESTDIR=plugins/<id>/stage
   cmake --install --strip`.
4. Перевірки: є `.so` у `lib/obs-plugins/` або `lib/obs-modules/plugins/`; файли плагіна не перетинаються
   з файлами OBS і вже зібраних плагінів (інакше в pacman був би конфлікт файлів). Дерево плагіна лишається
   окремим (`plugins/<id>/stage/usr`) і пакується окремо.
5. Результат іде в `plugins/results.tsv`; версія — у файл пакета `share/obs-legacy/plugins/<id>`, невдалі —
   у `plugins_failed=…` BUILDINFO OBS, а ті, що завантажилися, але не зібралися, — ще й у
   `plugin_failed.<id>=<коміт12>` (з `tried-commit`). Збій мережі коміту не має: «Оновити» спробує знову.

Плагін, що не зібрався, **не блокує** встановлення OBS: буде попередження, а попередня збірка з ним
лишається для відкату. Щоб скасовувати встановлення в такому разі, є `PLUGINS_STRICT=1`. `check`
лише завантажує вихідні коди плагінів і показує, які версії буде зібрано (`check_plugins`).

### Нотатки щодо рецептів

Рецепти складено за CMake-файлами плагінів станом на 2026-10-28. Справжнім збиранням з OBS 33.0.1 (Artix,
gcc 16.2, Qt 6.12, 2026-10-30) підтверджено всі 18: obs-multi-rtmp 0.8.0.0, move-transition 3.2.1,
source-record 0.4.8, shaderfilter 2.6.1, vertical-canvas 1.6.7, pipewire-audio-capture 1.2.1, composite-blur,
advanced-masks і stroke-glow-shadow v1.6.1, retro-effects 1.1.1, scale-to-sound 1.2.5, downstream-keyer
0.4.5, transition-table 0.3.1, freeze-filter 0.3.5, replay-source 1.8.1, gradient-source 0.3.3, waveform
v1.9.1, RewardsTheater 1.1.5. У самому OBS (2026-10-29, OBS 33.0.0) перевірено obs-multi-rtmp,
pipewire-audio-capture, composite-blur і waveform.

| Плагін | Особливість |
|---|---|
| obs-multi-rtmp | новий plugin-template: потрібні `-DENABLE_FRONTEND_API=ON -DENABLE_QT=ON` (у шаблоні вони OFF) і C++20 (`-DCMAKE_CXX_STANDARD=20`, як у пресеті `template`). Теги: `0.8.0.0` — найновіший (під OBS 32.2.1); бувають теги з суфіксом `-obs32.1.2`, фільтр їх відкидає |
| advanced-masks | plugin-template з FE/QT ON у пресеті → передаємо ON |
| composite-blur, stroke-glow-shadow, retro-effects, scale-to-sound | plugin-template, FE/QT OFF |
| exeldro (move-transition, downstream-keyer, transition-table, vertical-canvas від Aitum) | самі визначають `BUILD_OUT_OF_TREE`. Їхні helpers ставлять `.so` **одночасно** в `lib/obs-plugins` і `lib/obs-modules/plugins` — це навмисно, для сумісності з OBS 32 і 33 |
| старі exeldro (source-record, shaderfilter, freeze-filter, replay-source, gradient-source) | вбудований `ObsPluginHelpers.cmake` має `LINUX_PORTABLE` ON за замовчуванням (розкладка `obs-plugins/64bit`), тому передаємо `-DLINUX_PORTABLE=OFF`. `find_qt(VERSION ${QT_VERSION})` з AUTO може взяти Qt5, тому `-DQT_VERSION=6` |
| source-record, replay-source | збираються з `-Werror`, а в OBS 33.0.1 застарів `obs_properties_add_button` → без `-Wno-error=deprecated-declarations` (`PLUGIN_WARN_FLAGS`) не збираються |
| shaderfilter | збирається з `-Werror`, а в C23 з glibc 2.42 `strrchr` від `const char *` повертає `const char *` → без `-Wno-error=discarded-qualifiers` (`PLUGIN_WARN_FLAGS`) не збирається |
| transition-table 0.3.1 | посилається на `Qt::GuiPrivate`, не попросивши його в `find_package` (downstream-keyer і vertical-canvas просять самі) → `qt-private.cmake` |
| pipewire-audio-capture | для OBS ≥ 33 ставиться в `lib/obs-modules/plugins`, для старіших потрібен `-DLEGACY=ON` (`<33?-DLEGACY=ON`). Свій `FindPipeWire.cmake` |
| waveform | `-DPACKAGED_INSTALL=ON` (інакше ставиться в `~/.config`-подібну розкладку), потрібен пакет `fftw` |

Розглянуто, але не додано: obs-vkcapture (Vulkan-шар треба ставити поза `/opt`), obs-backgroundremoval
(onnxruntime/OpenCV, важко), Advanced Scene Switcher (багато залежностей), aitum-multistream (дублює
multi-rtmp).

### OBS 33: нова розкладка плагінів

OBS 33 має менеджер плагінів (`frontend/plugin-manager/PluginModuleLoader_Linux.cpp`):

- нові плагіни: `${CMAKE_INSTALL_PREFIX}/lib/obs-modules/plugins/<id>.so`, дані —
  `share/obs/obs-modules/plugins/<id>`;
- старі (legacy): `${CMAKE_INSTALL_PREFIX}/lib/obs-plugins/<id>.so`, дані — `share/obs/obs-plugins/<id>`;
- вбудовані модулі: `lib/obs-modules/core/` (шукаються відносно бінарника);
- користувацькі: `$XDG_DATA_HOME/obs-studio/plugins/…` і старе `~/.config/obs-studio/plugins/<id>/bin/64bit`.

Шляхи з `CMAKE_INSTALL_PREFIX` **абсолютні й запікаються під час збирання**. Тому `PREFIX` дорівнює
остаточному каталогу версії, і саме тому каталог збірки не можна переносити.

## Перевірка бібліотек

`ldd_problems` аналізує і stdout, і **stderr** `ldd`. Рядки `version 'Qt_6.x' not found` ldd пише в
stderr, а з'являються вони після оновлень Qt/ffmpeg. Формат результату: `libfoo.so.1` або
`libQt6Core.so.6 (Qt_6.9)`. `verify_tree <дерево> [додаткові дерева]` запускає ldd з `LD_LIBRARY_PATH` =
`lib/` цих дерев (після встановлення вони будуть у `/usr/lib`; для плагіна додаткове дерево — OBS) і
пропускає для `.so` бібліотеки, що є в деревах деінде. Розбір тим самим `sed` вбудовано в `check-libs`
pacman-хука.

## Команда `obs` і ярлик OBS

Звичайні, з пакета: `/usr/bin/obs` і `/usr/share/applications/com.obsproject.Studio.desktop`. Обгортки
`/usr/local/bin/obs` і ярлика-перекриття (стара схема) більше немає — їх прибирає `remove_legacy_install`.

## pacman-хуки

У пакеті OBS (`add_hook_to_tree`, якщо `INSTALL_PACMAN_HOOK=1`) — три хуки.

**keep-libs** (як preserved-libs у Gentoo): `00-obs-legacy-keep-libs-pre.hook` (`PreTransaction`,
`NeedsTargets`) і `00-obs-legacy-keep-libs-post.hook` (`PostTransaction`; «00-», щоб іти раніше за інші, зокрема
`obs-legacy.hook`), обидва `Upgrade`/`Remove`, `Target = *`, `Exec = /usr/lib/obs-legacy/keep-libs pre|post`.
- `pre`: файли бібліотек пакетів транзакції (`pacman -Qlq <цілі>`, лише `*.so*`; нових пакетів pacman ще не
  знає — пропускає) → якщо цілей без бібліотек, вихід за ~0,05 с. Інакше: NEEDED усіх ELF у `bin/` і `*.so*`
  пакетів `*-legacy-580xx` (один `pacman -Ql`, `readelf -d`) і збережених копій → де їх знаходить завантажувач
  (`ldconfig -p`, x86-64) → ті, що лежать у файлах цілей, плюс їхні залежності, які теж зникнуть, копіюються
  (`cp -L`) у `kept-libs.staged`. ~0,3 с.
- `post`: якщо нічого не збережено й не підготовлено — одразу вихід. Інакше `ldconfig` (кеш уже після
  транзакції), підготовлені копії бібліотек, яких система тепер не має (`system_has`: шлях з кешу поза
  `kept-libs`, файл існує), переходять у `kept-libs`; лишаються лише копії, потрібні пакетам OBS-Legacy прямо
  чи через іншу копію (до сталого стану); список `kept-libs.tsv` («бібліотека<TAB>пакети»), `ldconfig`, якщо
  щось змінилося, і повідомлення в кінці виводу pacman.
- Тека `/usr/lib/obs-legacy/kept-libs` — у пакеті OBS (порожня), її шлях — у
  `/etc/ld.so.conf.d/obs-legacy-kept-libs.conf` (теж у пакеті). Копія знаходиться лише тоді, коли такого
  soname в системі вже немає, тож нових бібліотек вона не затуляє.
- Копії прибираються самі, щойно перезібрані пакети встановлено (їх більше ніхто не потребує); `uninstall`
  прибирає теку й список, коли пакетів OBS-Legacy не лишилося (хук `post` пішов разом з пакетом).
- `ldd_problems` позначає бібліотеку, знайдену лише в `kept-libs`, як `(kept)`: набір, зібраний до оновлення
  (`set_libs_ok`), не вважається готовим, а `verify_tree` не пропустить такий файл у пакет.
- Хто чекає на копії: `kept_libs ffmpeg|obs` у скрипті (`ffmpeg_needs_rebuild`, рядок в `update_reasons`),
  `updates::kept_libs` у TUI (фонова підготовка й сповіщення), `status`.
- Рахуються лише копії для пакетів **набору** — власників файлів `/usr/share/obs-legacy` (`set_packages` у
  скрипті, `updates::set_packages` у TUI, `kept_for_set` у добудовувача). keep-libs зберігає бібліотеки для
  будь-якого пакета `*-legacy-580xx`, але чужий чи забутий пакет з тією самою припискою набір не перезбере:
  якби він рахувався, кожне оновлення системи перезбирало б усе по колу.
- Тест без root — `dev/test-keep-libs.sh`: файл підключається (`source`, `main` тоді не виконується), теки,
  список і `ldmap` підставляються, бібліотека чужого пакета «зникає» з кешу ldconfig.

**check-libs**:
`/usr/share/libalpm/hooks/obs-legacy.hook` спрацьовує на `Upgrade`/`Remove` будь-якого пакета
(`Target = *`), `When = PostTransaction`, `Exec = /usr/lib/obs-legacy/check-libs`. Скрипт бере файли всіх
встановлених `*-legacy-580xx` (`pacman -Qlq`), перевіряє ldd для ELF у `bin/` і `*.so*`, а якщо стоїть
`ffmpeg-legacy-580xx` — порівнює (`vercmp`, без pkgrel) версію `ffmpeg` у репозиторії з `arch_version` з
`/usr/share/obs-legacy/ffmpeg.txt`. Друкує щось лише тоді, коли знайшов проблеми (рядки проблем — з
6 пробілами, їх читає `updates::parse_libcheck`), і завжди завершується з кодом 0. Хук іде й зникає разом
із пакетом OBS.

## Пакетні менеджери й AUR-помічники

Щоб «Оновити все» в пакетному менеджері (Мацаки, AUR-помічник) не мало окремого кроку для OBS-Legacy і не
ламало його. Усе нижче — стабільний інтерфейс: змінювати лише сумісно.

**Які пакети наші.** Ті, що мають файли в `/usr/share/obs-legacy/` (`BUILDINFO.txt` — OBS, `ffmpeg.txt` —
ffmpeg, `plugins/<id>` — плагіни; `pacman -Qqo /usr/share/obs-legacy`). Назви закінчуються на `-legacy-580xx`,
але на назву краще не покладатися. Їх немає ні в репозиторіях, ні в AUR: не шукати в AUR («не знайдено» —
не помилка), не пропонувати замінити пакетами з репозиторію чи AUR (у наших `conflicts` — `obs-studio`,
`ffmpeg`, назви плагінів з AUR, тож у зворотний бік заміну пропонує pacman, і це правильно), не вважати
«зібраними поза AUR — перезберіть самі»: їх перезбирає OBS-Legacy.

**Бібліотеки.** Залежності — без жорсткої версії soname (`libass.so>=0`), тож оновлення системи пакети
OBS-Legacy не зупиняють. Якщо стоїть `/usr/lib/obs-legacy/keep-libs` (хуки keep-libs, рецепт 2), бібліотеку,
яку оновлення забрало б у наших пакетів, хук зберігає в `/usr/lib/obs-legacy/kept-libs` (список —
`/var/lib/obs-legacy/kept-libs.tsv`: «бібліотека<TAB>пакети»): для наших пакетів «оновлення бібліотеки зламає
програму» — не причина відкладати оновлення бібліотеки. Без keep-libs (старі пакети) — радити спершу
«Встановити / оновити OBS» в OBS-Legacy.

**Якщо стоїть пакет `obs-legacy` з добудовувачем** (`/usr/lib/obs-legacy/auto-update`, з 0.0.8), кроки 2–3 після
транзакції pacman робить сам його хук (див. «Автоматичне оновлення разом із системою»): пакетному менеджеру їх
робити не треба — інакше те саме збиралося б двічі (без шкоди: `--prepare` дочекається блокування рушія й не
знайде, що ставити). Показати людині, що відбувається, можна з журналу `/var/log/obs-legacy/auto-update.log`.

**Порядок «Оновити все»:**
1. Системні пакети (`pacman -Syu`), за бажанням разом з пакетами для збирання OBS-Legacy, яких бракує:
   `obs-legacy --missing-deps` → рядки `repo <пакет>` (з репозиторіїв) і `norepo <пакет>` (там немає).
2. `obs-legacy --prepare` — від імені користувача, **не root**, без питань (stdin не потрібен, термінал
   теж); вивід — для людини (етапи `==> …`, `[OK]`/`[УВАГА]`/`[ПОРАДА]`, прогрес ninja `[N/M]`). Код 0 — готово
   або збирати нічого; 10 — скасовано; інше — помилка (поради — у рядках `[ПОРАДА]`, звіт — у
   `~/.cache/obs-studio-nvenc-artix/reports/*-prepare.txt`, шлях — у рядку `Звіт:`). Триває від кількох
   секунд (нічого нового: перевірка версій OBS і плагінів через мережу) до години (новий OBS і ffmpeg). Якщо
   фонова перевірка саме готує пакети (блокування `~/.cache/obs-studio-nvenc-artix/.lock`, у файлі
   «PID режим»), `--prepare` спершу її дочекається.
3. `obs-legacy --install-files` — рядки `install <файл>` → одна транзакція `pacman -U` цих файлів (на
   питання pacman про заміну `ffmpeg`, `obs-studio` чи плагінів з AUR — «так»), потім `pacman -R` пакетів з
   рядків `remove <пакет>`. Порожньо — ставити нічого. Копію налаштувань OBS робить сама команда.
4. AUR-пакети, що залежать від `obs-studio` (сторонні плагіни: `obs-vaapi`, `obs-vkcapture`…), — після кроку 3.
   `libobs.so` між версіями OBS має той самий soname, тож бібліотеки цього не покажуть: такі плагіни варто
   перезібрати, коли змінилася основна версія `obs-studio-legacy-580xx` (33.x → 34.x).

Без пакетного менеджера те саме роблять фонова перевірка (крок 2 — сама після транзакції pacman) і
«Встановити / оновити OBS» (крок 3).

**Перезбирається лише потрібне.** Коли всі причини `update_reasons` — рядок `ffmpeg: …` (бібліотеки
ffmpeg-legacy-580xx, збережені копії, рецепт ffmpeg), OBS-Legacy вже встановлено пакетами, а основна версія ffmpeg
у репозиторії та сама (`version_major`: між основними версіями змінюються sonames libav*), `ffmpeg_only` вмикає
`ffmpeg_only_build`: збирається лише ffmpeg-legacy-580xx — у теку `~/.local/share/obs-studio-nvenc-artix/ffmpeg-packages/<мітка>/`
(BUILDINFO з `kind=ffmpeg`, `ffmpeg`, `ffmpeg_arch`, `profile`, `recipe`, `built=` — як у `ffmpeg.txt` пакета), без OBS,
CEF, плагінів і їхнього пакування; якщо нова збірка дає інші sonames libav*, ніж встановлена, — помилка з порадою
«Перезібрати OBS». `prepare` її лише готує, `update` ставить тільки її (`finish_ffmpeg_only`, плагіни не прибираються),
`--install-files` без повного набору дає рядок `install` для неї (`ready_ffmpeg any`), фонова перевірка сповіщає
«ffmpeg для OBS-Legacy готовий» (`updates::prepare::ready_ffmpeg`). Бібліотеки OBS і плагінів перевіряються окремо
(`pkgs_missing_lib` по `set_packages`), тож брак бібліотеки лише в ffmpeg не тягне за собою OBS. Коли ж OBS працює на
збережених копіях чи без бібліотеки, `obs_relink_only` прибирає з кешу збирання лише скомпоновані файли (правила
`*_LINKER` у build.ninja) — ninja перекомпоновує їх (37 файлів, ~1 хв), не компілюючи наново; начисто — лише коли кешу
немає чи CMakeCache пам'ятає шлях до бібліотеки, якої вже немає.

## Автоматичне оновлення разом із системою

Щоб звичайне `sudo pacman -Syu`, `paru -Syu` чи «Оновити все» в Мацаках саме перезбирало й ставило пакети
OBS-Legacy, коли оновлення зачепило їхні бібліотеки, — без окремої команди й без другого пароля. Живе в пакеті
самої програми (`packaging/`), а не в пакеті OBS: хук має лежати в `/usr`, а пакет програми оновлюється з
релізами, не чекаючи перезбирання OBS. Копія від install.sh у `~/.local/bin` цього не дає.

- `packaging/90-obs-legacy-auto-update.hook` (`PostTransaction`, Install/Upgrade/Remove, `Target = *`; «90-» — після
  `00-obs-legacy-keep-libs-post.hook`, тож `kept-libs.tsv` уже свіжий) → `/usr/lib/obs-legacy/auto-update hook`.
- `hook` (root, усередині транзакції): виходить одразу, якщо це наш власний `pacman -U`
  (`OBS_LEGACY_AUTO_UPDATE=1` — хуки бачать оточення pacman), пакетів OBS-Legacy немає (`BUILDINFO.txt`) чи
  користувача не знайдено. Користувач: `SUDO_UID` → `PKEXEC_UID` → `DOAS_USER` → записаний минулого разу
  (`/var/lib/obs-legacy/auto-update.user`) → єдиний з тих, у кого є `~/.local/share/obs-studio-nvenc-artix/packages`;
  лише UID ≥ 1000, не nobody, рядок схожий на ім'я чи число. Два запасні варіанти (pacman запущено без sudo, pkexec
  чи doas, напр. з `su -`) — лише для члена групи `wheel` (`is_admin`): той і так може стати root, а звичайний
  користувач без права sudo інакше міг би підкласти root свій пакет. Тоді `setsid -f … run <користувач>` з усіма трьома
  потоками на `/dev/null` (pacman читає вивід хука до кінця й інакше чекав би) і, якщо є збережені копії,
  рядок для людини наприкінці виводу pacman. Код завжди 0.
- `run` (root, уже окремо): журнал `/var/log/obs-legacy/auto-update.log` (0644; понад 1 МіБ — у `.old`);
  `flock` на `/run/obs-legacy-auto-update.lock` (другий запуск чекає до 6 год); чекає, доки зникне `db.lck`
  (`pacman-conf DBPath`, до 1 год). Далі від імені користувача — `setpriv --reuid --regid --init-groups --reset-env
  --no-new-privs`, `XDG_RUNTIME_DIR=/run/user/<uid>`, `ionice -c 3 nice -n 19`, робоча тека `/`:
  1. `obs-legacy --repair` (рушій `repair`): без потреби — вихід за секунди без мережі; інакше перезбирає лише
     зламане (ffmpeg окремо, OBS перекомпоновкою). Код ≠ 0 — відбиток стану (`sha256` від `kept-libs.tsv` і
     `BUILDINFO.txt`) у `/var/lib/obs-legacy/auto-update.failed`: з тим самим станом наступні транзакції лагодження
     не повторюють (лише шукають готові пакети), доки щось не зміниться; сповіщення `failed`.
  2. `obs-legacy --install-files` → рядки `install <файл>`: це може бути й набір, який фонова перевірка
     підготувала для нової версії OBS чи плагінів, — тоді й він ставиться з цим оновленням. Файл береться, лише
     якщо шлях абсолютний, справжній шлях — у домашній теці користувача, власник — він, назва
     `*-legacy-580xx-*.pkg.tar.{zst,xz,gz}`; копія — у `/var/cache/obs-legacy/auto-update` (0700, root), і вже в копії
     `.PKGINFO` (`bsdtar`) має `pkgname = *-legacy-580xx`, і пакет із цією назвою зараз встановлено (`pacman -Q`;
     ні — пропуск з порадою: плагін, який прибрали чи замінили своїм пакетом, повертає лише людина — на
     конфлікті «вилучити інший пакет?» `--noconfirm` відповідає «ні», і зупинився б увесь набір). Рядки `remove` не виконуються (прибрати плагін —
     «Встановити / оновити OBS»).
  3. Знову чекає `db.lck`, тоді `OBS_LEGACY_AUTO_UPDATE=1 pacman -U --noconfirm -- <копії>` (ті самі назви, що вже
     встановлені, тож питань про заміну немає; інакше `--noconfirm` відповість «ні» й транзакція не пройде).
     keep-libs після неї прибирає копії, які більше не потрібні.
  4. `obs-legacy --auto-update-notice started|ok|failed [файли]` — сповіщення в графічній сесії
     (`updates::auto`, `notify::find_session`).
- Довіра — як в AUR-помічника: зібране користувачем ставить root, і лише після транзакції, яку користувач сам
  запустив з паролем. Перевірки файлів захищають від помилок, а не від шкідливого коду в обліковому записі
  користувача (той і так міг би перехопити пароль sudo).
- Фонова перевірка (`--watch`), коли добудовувач стоїть (`updates::auto::installed`), не готує пакети через
  збережені копії чи брак бібліотек і не сповіщає про них — це робить добудовувач; нові версії готує, як і
  раніше (`AUTO_PREPARE`), а сповіщення «Пакети готові» каже, що вони встановляться з наступним оновленням системи.
- `use-package` (з `packaging/obs-legacy.install`, `post_install`/`post_upgrade`): якщо в того, хто ставить пакет,
  є `~/.local/bin/obs-legacy` від install.sh, від його імені запускається `obs-legacy --use-package` (`timeout 60`);
  користувача не знайдено — лише підказка з командою.
- Тест без root — `dev/test-auto-update.sh` (і в CI): файл підключається (`source`), шляхи, pacman, obs-legacy,
  `setsid`, `getent`, `id` і користувач підставляються.

## BUILDINFO.txt

`/usr/share/obs-legacy/BUILDINFO.txt` (пакет OBS): `label`, `obs_version` (git describe), `obs_commit`,
`obs_ref`, `cef_build`, `nv_headers_tag` (уже з auto розв'язаний), `nvenc_api_headers`, `nvenc_api_driver`,
`nvenc_test` (`passed`/`skipped`/`unknown`), `tls`, `distro`, `cpu_model`, `cpu_march`, `cpu_tuning`,
`isa_level`, `opt_level`, `cflags`, `gpu`, `gpu_cc`, `driver`, `driver_pkg`, `profile` (`PROFILE_SIG`),
`built` (ISO 8601), `builder`, `plugins_failed=<id …>`, `package=obs-studio-legacy-580xx <ver>-<rel>`,
`build_number`. Плагіни — `share/obs-legacy/plugins/<id>` (`plugin.<id>=<версія> <коміт12>`), ffmpeg —
`share/obs-legacy/ffmpeg.txt` (`arch_version`, `pkgbuild_version`, `nv_headers_tag`, `cpu_march`,
`cuda_arch`, `profile`…). BUILDINFO набору в сховищі = BUILDINFO OBS + рядки плагінів + `ffmpeg=` +
`packages=`. TUI читає їх напряму (`src/system.rs`: `installed_info_from` додає `ffmpeg.*`).

## Контракт TUI ↔ рушій

TUI запускає `bash <скрипт> <команда>` у PTY (cwd = `$HOME`, `TERM=xterm-256color`, `OBS_LEGACY_TUI=1`,
`OBS_LEGACY_SETTINGS=<шлях>`, `OBS_LEGACY_LANG=uk|en`) і розбирає вивід (`src/analyzer.rs`). Префікси —
обома мовами (таблиця `TAGS` в `analyzer.rs`), тож TUI розуміє і англійський вивід, і старі українські звіти:

| Вивід скрипта | Що робить TUI |
|---|---|
| `==> Назва` (`say`) | новий етап у списку |
| `[OK] …` | — (етап за замовчуванням успішний) |
| `[УВАГА] …` / `[WARNING] …` | етап позначається ⚠, лічильник попереджень |
| `[ПОМИЛКА] …` / `[ERROR] …` | етап позначається ✗, лічильник помилок |
| `[N/M] …` (ninja) | смуга «Компіляція»/«Плагін» з оцінкою часу |
| `####   45.2%` (curl `--progress-bar`, оновлення через `\r`) | смуга «Завантаження» |
| незавершений рядок з `[y/N]`/`[Y/n]` | банер «введи y або n» + дзвінок. Поки завдання йде, у скрипт ідуть літери, Enter, Backspace, Tab і Ctrl+…; стрілки, Home/End гортають вивід, а Esc, Delete, Insert, Shift+Tab не йдуть нікуди — інакше в рядку введення лишалося б сміття |
| незавершений рядок з `password`/`пароль` | банер про пароль sudo/doas |
| незавершений рядок на `:`/`?`, тиша > 2 с | банер «схоже, чекає на введення» |
| `[ПОРАДА] …` / `[HINT] …` | додається до останнього попередження чи помилки; картка результату показує «Що робити» |
| `[СКАСОВАНО] …` / `[CANCELLED] …` | картка «скасовано» (жовта), а не «помилка» |
| `[ВИПРАВИТИ] команда: опис` / `[FIX] …` | додається до останньої проблеми; головна кнопка картки (Enter) запускає дію з білого списку `app::fix_action` (`deps` → діалог і `deps`, `clean`, `check`); невідомі команди ігноруються |
| `Звіт: <шлях>` / `Report: <шлях>` | кнопки «Переглянути звіт» (`o`), «Скопіювати шлях» (`c`), «Відкрити в програмі» (`e`); `~/` розгортається |
| код завершення | ✓ або ✗ (або «Перервано» після Ctrl+C) |

Після завершення картка результату показує: для помилки — останній `[ПОМИЛКА]` і його поради, для
успіху — зауваження з порадою (або типовий «Що далі» для дії), для скасування — повідомлення
`[СКАСОВАНО]`. Без жодної поради TUI радить переглянути звіт і створити issue.

Старі звіти TUI читає тим самим аналізатором (`src/reports.rs`): результат — з рядка `Результат:` чи
`Result: success|cancelled|error (code N)` (для
звітів старих версій — за `ПЕРЕВІРКУ ПРОЙДЕНО`/`ГОТОВО:`/`[ПОМИЛКА]`), головна проблема — остання
помилка з порадами. На цьому будується порада «Що далі» на головному екрані (`src/advice.rs`).

Якщо змінюєш префікси виводу в скрипті (`say`/`ok`/`warn`/`fail`, `Звіт:`), онови `analyzer.rs` і тести — обидві
мови.

## Мова (українська / англійська)

Інтерфейс, скрипт, звіти, сповіщення, `install.sh` і pacman-хук говорять українською або англійською.

- **Вибір.** TUI (`src/i18n.rs`): змінна `UI_LANG` → `UI_LANG` у `settings.env` (поле «Мова інтерфейсу»:
  `auto` | `uk` | `en`) → `LC_ALL` / `LC_MESSAGES` / `LANG` → `/etc/locale.conf` → англійська; локаль `uk*` —
  українська. Скрипт (`resolve_lang`) і `install.sh` — те саме, але першою йде `OBS_LEGACY_LANG`: її TUI
  передає в кожне завдання й у `plugins --tsv`, тож скрипт завжди говорить мовою TUI. `--watch` перечитує
  мову з налаштувань перед кожною перевіркою.
- **TUI:** `tr!("українською", "in English")` і `trf!("… {x}", "… {x}", …)` (формат як у `format!`); пункти меню
  й поля налаштувань — пари `(uk, en)` з методами `label()`/`about()`/`help()`. Довідка — дві функції
  (`text_uk`, `text_en`). У тестах мова — на потік, типово українська: `i18n::with(Lang::En, || …)`.
- **Скрипт:** `t "uk" "en"` (див. «Повідомлення для людини»); довідка `usage` і текстове меню — два heredoc;
  описи плагінів — `$(t …)` у полі опису `PLUGIN_CATALOG`; `pkgdesc`/`optdepends` пакетів — мовою, якою
  збирали. Шаблони `case` для відповідей («так», «ні», «т»/«н», «к») і коментарі в згенерованих файлах
  лишаються українськими навмисно.
- **pacman-хук** працює від root під час транзакції й налаштувань користувача не бачить: `check-libs` бере
  мову з локалі pacman і має обидві підказки (`hint_uk`, `hint_en`). Рядки проблем — з 6 пробілами в будь-якій
  мові (`update_reasons` і `updates::parse_libcheck` шукають саме їх).
- **Підсумок встановлення** (`install_summary`) розбирає причини з `update_reasons` обома мовами.
- `dev/fake-build.sh` — теж двомовний (за `OBS_LEGACY_LANG` чи локаллю): з ним знято українські
  (`docs/screenshots/`) і англійські (`docs/screenshots/en/`) скріншоти.

Пакети, яких бракує, TUI дізнається командою `deps --missing` (у фоні, на старті й після кожного завдання):
у «Твоїй системі» з'являється рядок «Пакети» (✓ або ⚠), а коли щось бракує, «Встановити пакети» стає
основним кроком меню одразу після перевірки. Після успішного `deps` головна кнопка картки — повторна
перевірка.

Що TUI читає напряму (без скрипта): `/usr/share/obs-legacy/{BUILDINFO.txt,plugins/*,ffmpeg.txt}`,
`~/.local/share/obs-studio-nvenc-artix/packages/*/{BUILDINFO.txt,.complete}`, поки пакетів немає — ще
`/opt/obs-studio-nvenc/{current,versions/*}` (позначка `legacy`), маркер у хуку (`/usr/share/libalpm/hooks`
або старий `/etc/pacman.d/hooks`) і в `/usr/local/bin/obs`, `settings.env`, `/proc/driver/nvidia/*`,
`/proc/cpuinfo`, `nvidia-smi`, `pacman -Q`, `git ls-remote` (оновлення). Для демо й тестів:
`OBS_LEGACY_ROOT=<тека>` — корінь, від якого читаються `usr/share/obs-legacy` і `usr/lib/obs-legacy`; якщо
задано лише `INSTALL_ROOT`, корінь — він (старі демо не бачать справжній OBS).

## TUI: модулі

| Модуль | Роль |
|---|---|
| `main.rs` | аргументи (`--help`, `--version`, `--script-path`, `--probe-nvenc`, `--no-relaunch`, `--check-updates`, `--upgrade-system` — команда скрипта просто в цьому терміналі, `--prepare` / `--install-files` / `--missing-deps` — для пакетних менеджерів, без термінала (`run_script`; `--prepare` спершу `updates::wait_script_free`), `--watch`, `--notifier …`), перезапуск у терміналі, ініціалізація ratatui, миша й bracketed paste |
| `app.rs` | стан, `MENU` (основні кроки + розділ «Додатково», `Row`), екрани (`Intro`, `Home`, `Run`, `Plugins`, `Rollback`, `Settings`, `Reports`, `Report`, `Help`), діалоги, сповіщення, клавіатура й миша, фонові потоки (`Msg`), `key_to_bytes` для PTY |
| `advice.rs` | порада «Що далі»: чиста функція від `SysInfo`, нових версій (`updates::Facts`), звітів, вибраних плагінів і стану сповіщень → тон, крок (перевірка/встановлення/запуск), текст, дія для Enter, друга кнопка, звіт для `o` |
| `reports.rs` | список звітів, результат і головна проблема кожного (хвіст 64 КБ), текст для переглядача, класифікація рядків |
| `desktop.rs` | `xdg-open` (від'єднано через `setsid`) і буфер обміну: wl-copy → xclip → xsel + OSC 52 |
| `runner.rs` | `Job`: portable-pty + потік читання + `vt100::Parser` (10 000 рядків історії) + `Analyzer`, resize, Ctrl+C, kill |
| `analyzer.rs` | розбір виводу (див. контракт) |
| `i18n.rs` | мова інтерфейсу (`Lang`, `resolve`, `set`/`lang`), макроси `tr!` і `trf!` |
| `term_view.rs` | віджет, що малює екран vt100 з кольорами; типові кольори й 16 кольорів ANSI — з теми |
| `system.rs` | панель стану: ОС, ресурси, процесор, GPU/драйвер (`/proc/driver/nvidia`, потім `nvidia-smi`), пакети (`obs-studio-legacy-580xx`, `ffmpeg-legacy-580xx`, інший OBS), збірки (`read_builds`: набори + встановлена + стара схема), профіль (`BuildInfo::profile`), зміна процесора (`built_for_other_cpu`), команда obs (`obs_cmd_ok`), оновлення (`git ls-remote`) |
| `nvenc.rs` | `--probe-nvenc`: `dlopen(libnvidia-encode.so.1)` → `NvEncodeAPIGetMaxSupportedVersion` в окремому процесі |
| `catalog.rs` | каталог плагінів зі скрипта (`plugins --tsv`) |
| `settings.rs` | `settings.env`: поля (`Kind`: Text, Number, Toggle, Choice), перевірка значень, збереження без втрати чужих рядків |
| `script.rs` | вбудований скрипт, розпакування, XDG-шляхи |
| `launcher.rs` | пошук термінала і перезапуск у ньому (120×36) |
| `updates.rs` | нові версії OBS, плагінів і OBS-Legacy (`Facts`, ті самі для «Що далі» і сповіщень), тексти сповіщень (`Notice`), стан `updates.env`, журнал, `--check-updates`, фонова перевірка `--watch` |
| `notify.rs` | пошук графічної сесії (`/proc/<pid>/environ`), `notify-send` з кнопкою «Відкрити OBS-Legacy», запасний `gdbus` |
| `service.rs` | `--notifier`: виявлення init-системи, файли служб, увімкнення/вимкнення/перезапуск, стан |
| `cli.rs` | вивід команд без меню в форматі скрипта (`==>`, `[OK]`, `[УВАГА]`, `[ПОМИЛКА]`, `[ПОРАДА]`) |
| `theme.rs`, `logo.rs` | кольори (truecolor/256/16/NO_COLOR), суцільний фон, логотип ANSI Shadow з анімованим градієнтом |
| `ui/` | `intro`, `home`, `run` (картка результату), `report` (список і переглядач звітів), `lists` (плагіни, відкат, налаштування), `help`, спільні віджети в `mod.rs` (`top_bar`, `footer`, `panel`, `buttons`, `wrap_text`, `wrap_spans`) |

### Суцільний фон

`ui::draw` спершу заливає весь екран кольором полотна (`Theme::base`), панелі мають трохи світліший
фон (`Theme::panel`), а замість `Clear` використовується `ui::fill`. У Kitty, Alacritty, Konsole, foot
прозорими лишаються лише клітинки з **типовим** фоном термінала, тож явний колір робить інтерфейс
непрозорим. Контраст тексту з усіма фонами ≥ 4.5:1 перевіряє тест `theme::tests`. Вимикається
`UI_BACKGROUND=0` (налаштування «Суцільний фон»): тоді полотно, панелі й текст — кольору термінала.

### Значки

Значки меню й станів підібрано так, щоб вони були в поширених моноширинних шрифтах (JetBrains Mono,
DejaVu Sans Mono): ◎ ▼ ▲ ▶ ⊕ ⋯ ≡ ◫ ◷ ↩ ◈ ↪ ⌫ ✗ ? × ◆ ● ○ ✓ ⚠ ■ ⌨. Уникаємо ★ ↺ ↻ ☰ ⚿ ℹ — у JetBrains Mono їх
немає.

### Перезапуск у терміналі

Якщо stdin/stdout — не TTY (запуск з `.desktop`, у якому `Terminal=false`, або з файлового менеджера),
`launcher::relaunch_in_terminal` перебирає термінали: `$TERMINAL` → `xdg-terminal-exec` → термінал
поточного DE (`XDG_CURRENT_DESKTOP`) → kitty, alacritty, wezterm, foot, ghostty, konsole, gnome-terminal,
xfce4-terminal, mate-terminal, qterminal, lxterminal, terminator, tilix, urxvt, st, xterm. Для кожного
відомий свій синтаксис запуску, а де можна — розмір 120×36. Дочірній процес отримує
`OBS_LEGACY_NO_RELAUNCH=1`, щоб не відкривати вікна по колу. Якщо термінала не знайдено — `notify-send`.

### Самооновлення

`install.sh` записує шлях до клону в `~/.local/share/obs-legacy/source-dir`. Тоді в меню з'являється
«Оновити OBS-Legacy»: `git pull --ff-only` + `./install.sh --update` (неінтерактивно; бінарник
замінюється атомарно через `mv`). Після цього TUI треба перезапустити.

Однією командою (`curl … | bash`, `install.sh` без вихідних кодів поруч) на Arch/Artix ставиться пакет
`obs-legacy` з релізу (`install_package`): тег — закріплений у копії install.sh з релізу (`RELEASE_TAG`), інакше з
переадресації `releases/latest` (`release_tag`); `obs-legacy-<версія>-1-x86_64.pkg.tar.zst` і `.sha256` — у
тимчасову теку, сума (`sha256_matches`), тоді `sudo pacman -U <файл>` (або doas). Пакет тієї самої версії вже
стоїть — pacman не запускається. Без термінала (CI) pacman отримує `--noconfirm` (`pacman_opts`): відповісти
нікому, а sudo без пароля там дозволено явно. Копію з `~/.local/bin`, якщо була, переймає `obs-legacy.install`
(`auto-update use-package` від імені `SUDO_UID`), а `install.sh` про всяк випадок ще раз запускає
`/usr/bin/obs-legacy --use-package` (повтор нешкідливий, `adopt_home_copy`). Ярлик на робочому столі від
install.sh перехід прибирає, тож install.sh пам'ятає, що він був, і пише його знову з `Exec=/usr/bin/obs-legacy`.
`--uninstall` вимикає сповіщення тією програмою, що є, і прибирає пакет (`sudo pacman -R obs-legacy`).

Без pacman чи з `--no-package` програма — той самий бінарник, що в пакеті, з релізу (`obs-legacy-x86_64` +
`.sha256`) у `~/.local/bin`. `install.sh` пише тег релізу в
`release-install` і прибирає `source-dir` (і навпаки: збирання з вихідних кодів прибирає `release-install`,
тож діє останній спосіб). `script::self_source()` повертає `SelfSource::Git(тека)` чи `SelfSource::Release` (або `Package`, див. нижче);
другий — лише коли запущено саме `~/.local/bin/obs-legacy`. Для `Release` пункт меню завантажує
`releases/latest/download/install.sh` і запускає його з `--prebuilt --update` (`--prebuilt` — щоб тимчасова
тека не вважалася теками з вихідними кодами), а сповіщення радить той самий пункт меню. Там, де є pacman,
цей install.sh ставить уже пакет, тож копія з релізу так сама переходить на пакет;
`SelfSource::updates_with_pacman` каже діалогу й опису пункту, що знадобиться пароль.

Під `curl … | bash` stdin — сам скрипт, тому `main` тоді читає з `/dev/tty` (якщо термінал є): питання
працюють, а наприкінці `exec obs-legacy` відкриває меню. Без термінала (CI) — без питань, як `--update`.
`OBS_LEGACY_TAG` вибирає інший реліз, `OBS_LEGACY_DOWNLOAD_URL` — теку з тими самими файлами замість релізу
(перевірки, `file://…`).

Поставлену готовим пакетом програму (запущено саме `script::PACKAGE_EXE` = `/usr/bin/obs-legacy`)
`self_source()` визначає першою — `SelfSource::Package`, навіть якщо десь лишилася `source-dir`: копію з
пакета оновлює лише новий пакет. Пункт меню (`self_update_args`, PTY) питає `releases/latest` лише
заголовками (`curl -I`, переадресація на сторінку тега дає версію), завантажує
`obs-legacy-<версія>-1-x86_64.pkg.tar.zst` і `.sha256` (назва — з `package.yml`) у `~/.cache/obs-legacy`
(`package_cache_dir`; старі файли там спершу видаляються), перевіряє `sha256sum -c` (не збіглося — файл
видаляється) і ставить локальний файл `sudo pacman -U` (або `doas`). Не `pacman -U <URL>`: підписів `.sig`
у релізах немає, а з `SigLevel = Required` pacman їх вимагає. `facts_from` отримує `Option<&SelfSource>`:
`Package` → `SelfInstall::Package`, і сповіщення радить той самий пункт меню (з поясненням, що ставить
pacman); опис пункту в меню для пакета показує «Пароль: знадобиться» і «Система: змінюється». Тест
`package_update_from_real_release` (`--ignored`, мережа) проганяє команду на справжньому релізі з
підставними sudo/doas. `install.sh --no-package` (і збирання з вихідних кодів) попереджає, якщо пакет
`obs-legacy` уже стоїть: копія в `~/.local/bin` його перекрила б.

### Пакет програми

**Перехід з install.sh на пакет** — `obs-legacy --use-package` (`src/package_move.rs`). pacman у домашні теки не
пише, тож копія install.sh (`~/.local/bin/obs-legacy` + позначка `source-dir`/`release-install` у
`~/.local/share/obs-legacy`) інакше лишилася б першою в PATH, її ярлик з позначкою install.sh затуляв би ярлик
пакета, а служба сповіщень запускала б її. Команда працює лише з `/usr/…` (програма пакета) і не від root:
спершу `--notifier restart` (служба переписується на поточну програму) і перевірка, що файли служби більше не
називають стару копію, — тоді видаляються стара програма, ярлики з позначкою install.sh (меню й робочий стіл),
значок і позначки; налаштування, набори, звіти й розпакований скрипт лишаються. Не вдалося перевести службу —
нічого не видаляється. `packaging/obs-legacy.install` після `pacman -U` запускає її сам від імені того, хто ставить
пакет (`auto-update use-package`), а якщо користувача не знайдено — нагадує про команду.

`packaging/PKGBUILD` збирає з архіву тегу (`archive/refs/tags/v<версія>.tar.gz`) звичайний пакет `obs-legacy`:
`cargo fetch --locked`, `cargo build --frozen --release` **без** `target-cpu=native` (пакет ставиться на будь-який
комп'ютер; під процесор програму збирає лише `install.sh`), `cargo test`, а в пакет — `/usr/bin/obs-legacy`,
ярлик `packaging/obs-legacy.desktop` (`Exec=obs-legacy`), іконка, README, ліцензія, а також добудовувач
`/usr/lib/obs-legacy/auto-update` і його хук `90-obs-legacy-auto-update.hook`. OBS, плагіни й ffmpeg у ньому
немає: їх програма збирає вже на комп'ютері користувача. Залежності: `bash`, `gcc-libs`, `git`, `glibc`, `libarchive` (`bsdtar`), `util-linux` (`setpriv`, `flock`); решту
пакетів для збирання OBS ставить сама програма (`deps`).

`.github/workflows/package.yml` збирає його в контейнері `archlinux:base-devel` (у `.BUILDINFO` — лише пакети
контейнера): на опублікований реліз — з `pkgver` тегу, `pkgrel=1` і свіжими сумами (`updpkgsums`), і прикріплює
пакет і `.sha256` до релізу; вручну (Actions → Package → Run workflow, тег) — те саме для наявного релізу; на PR,
що змінює `packaging/` чи сам workflow, — лише збирає й зберігає пакет як артефакт. Для релізного пакета
версію тегу й суми підставляє сам workflow; `pkgver` і `sha256sums` у `PKGBUILD` репозиторію оновлюють після
релізу (`updpkgsums`), щоб працювало й ручне `makepkg -si` у теці `packaging/`.

З того самого збирання workflow бере `usr/bin/obs-legacy` як `obs-legacy-x86_64` (+ `.sha256`) і кладе
`install.sh` з підставленим тегом (`RELEASE_TAG="${OBS_LEGACY_TAG:-vX.Y.Z}"`): установник релізу бере програму
саме з нього, а `releases/latest/download/install.sh` — завжди з найновішого. Перед вивантаженням крок
«Try install.sh» ставить і прибирає програму так, як `curl … | bash` (`bash -s < install.sh`, файли — з
`file://` замість релізу, `OBS_LEGACY_TAG` — версія пакета): спершу `--no-package` (копія в `~/.local/bin`), тоді
пакетом через sudo без пароля — він має перейняти копію, — ще раз (пакет уже стоїть) і `--uninstall`, після якого
пакета немає. PR, що змінює `install.sh`, теж його запускає.

## Сповіщення про оновлення

Фонова перевірка — той самий бінарник у режимі `obs-legacy --watch`. Його запускає служба користувача
(`service.rs`), а сповіщення показує `notify.rs`. Без root і без змін у системі.

Сповіщення ввімкнені типово: перший запуск меню (`service::enable_by_default` з фонового потоку
`spawn_sys`) вмикає службу, якщо її немає і людина ще не вирішувала, і показує про це рядок унизу.
Рішення — позначка `~/.local/state/obs-legacy/notifier-decided`: її пишуть `--notifier enable|disable` і
`install.sh`, коли питає. Запуск для розробки (`OBS_LEGACY_SCRIPT`) і від root нічого не вмикає.

### Що перевіряється

| Що | Коли | Звідки |
|---|---|---|
| Новий OBS | раз на 12 год | `git ls-remote --tags` obsproject/obs-studio, теги `X.Y.Z`; порівняння з `obs_version` активної збірки (`system::newer`) |
| Нові релізи плагінів активної збірки | разом з OBS | `git ls-remote` репозиторію з каталогу; теги `v?X.Y…` — так само, як їх бере скрипт (`resolve_plugin_ref`) |
| Новий ffmpeg у репозиторіях | разом з OBS | `pacman -Si ffmpeg` (локальна база, без мережі) проти `arch_version` з `/usr/share/obs-legacy/ffmpeg.txt` (`vercmp`); лише коли `ffmpeg-legacy-580xx` встановлено й «Збирати ffmpeg» не вимкнено. Те саме — у пораді «Що далі» й рядку «Оновлення» |
| Новий OBS-Legacy | разом з OBS | теги origin теки з вихідними кодами (`source-dir`) або `YellowHearth1/OBS-Legacy`; порівняння з `CARGO_PKG_VERSION` |
| Копія баз pacman | разом з OBS, перед перевіркою | команда скрипта `repo-sync` (без root): новий ffmpeg видно ще до `pacman -Syu` людини; `versions::repo_package_version` бере новішу з двох баз |
| Бібліотеки пакетів OBS-Legacy | після кожної транзакції pacman (пробудження щохвилини, `TICK`) | змінився mtime `/var/lib/pacman/local` і немає `db.lck` → `/usr/lib/obs-legacy/check-libs` (той, що й у pacman-хука; для старої схеми — `/usr/local/lib/…`) і список keep-libs (`updates::kept_libs`); якщо хук вимкнено — не перевіряється |

**Фонова підготовка пакетів** (`updates/prepare.rs`, `AUTO_PREPARE`, типово 1): якщо є причина
(`prepare::Reasons`) — одразу: пакети працюють на збережених копіях (keep-libs) чи бібліотек бракує
(`check-libs`); разом з оновленням системи, тобто лише після транзакції pacman (`Watch::upgraded`): нова версія
ffmpeg (без pkgrel), новий OBS, нові плагіни (з останньої перевірки, перераховані без мережі —
`facts_offline`), — і її відбиток (`Reasons::key`) ще не пробували (`prepared=` в `updates.env`: одна спроба на причину, без
повторів по колу), pacman і скрипт вільні (`db.lck`, `flock` на `~/.cache/obs-studio-nvenc-artix/.lock`),
`--watch` запускає `ionice -c 3 nice -n 19 bash <скрипт> prepare` і не чекає. Сповіщення: «готую» (замість
окремих про ffmpeg і бібліотеки), потім «Пакети OBS-Legacy готові» (новіший за встановлений повний набір,
`prepare::ready_label`) або «не вдалося» з порадою зі звіту `*-prepare.txt`. Скрипт пише в блокування
`PID режим`, тож друге вікно, що натрапило на `prepare`, пояснює: «у фоні готуються пакети — дочекайся».

Зафіксовані версії не дають сповіщень: `OBS_REF` ≠ `stable` — про OBS, `PLUGIN_REF_<id>` — про плагін.
Оновлення OBS і плагінів збирає одне сповіщення (плагіни оновлюються разом з OBS). `updates::facts_from`
рахує те саме для поради «Що далі», тож сповіщення й головний екран кажуть одне.

`git ls-remote` іде з `GIT_TERMINAL_PROMPT=0` і `GIT_SSH_COMMAND="ssh -o BatchMode=yes"`: служба не може
ввести пароль. Закритий репозиторій OBS-Legacy перевіряється лише тоді, коли git має збережені облікові
дані (наприклад, `gh auth`); інакше версія OBS-Legacy просто невідома.

### Розклад і стан

- Перша перевірка — через 2 хв після запуску (сесія й мережа встигають піднятися), але не раніше ніж за
  12 год після останньої вдалої (`updates.env`: `checked`, `ok`). Невдала (немає мережі) — повтор через
  30 хв. Прокидається що 5 хв: бібліотеки після pacman і сповіщення, які чекають на графічну сесію.
  Час — настінний (`SystemTime`), тож після сну комп'ютера перевірка не запізнюється.
- Про кожну новину — одне сповіщення: `notified.obs`, `notified.plugin.<id>`, `notified.self` — версія,
  `notified.libs` — відбиток (FNV-1a) списку проблем; коли бібліотеки знову на місці, ключ знімається.
  Видалити `updates.env` — сповістить про наявне ще раз.
- `watch.lock`: `flock` + PID. Друга фонова перевірка (скажімо, служба й автозапуск одночасно) не
  працює паралельно, а чекає на блокування, тож перезапуски наглядачів (runit, s6, `restart = true` у
  dinit) не ходять по колу. `running_pid()` перевіряє блокування без очікування.
- Журнал — `updates.log` (останні 400 рядків); у режимі `--watch` ті самі рядки йдуть у stdout, тобто в
  журнал служби (`dinitctl --user catlog obs-legacy-updates`, `journalctl --user -u obs-legacy-updates`).

### Доставка

Служба користувача працює поза графічною сесією: у її середовищі немає `WAYLAND_DISPLAY`/`DISPLAY`, а
шина D-Bus служб і шина графічної сесії бувають різними. На Artix з dinit так і є: `dinit --user` запускає
userspawn (чи turnstile) з шиною `$XDG_RUNTIME_DIR/bus`, а служба сповіщень живе на шині сесії
(`unix:path=/tmp/dbus-…`). Тому `notify::find_session`:

1. бере власне середовище, якщо в ньому є дисплей (запуск із сесії, терміналу, автозапуску);
2. інакше читає `/proc/<pid>/environ` процесів цього ж користувача з дисплеєм і вибирає найчастішу
   комбінацію (шина, `WAYLAND_DISPLAY`, `DISPLAY`); шина `unix:path=`, сокета якої вже немає,
   пропускається. Переносяться лише змінні сесії (`SESSION_VARS`).

Сесії немає (ще не увійшли) — сповіщення чекає в пам'яті до наступного пробудження. `notify-send
--app-name=OBS-Legacy --action=open=…`: з кнопкою notify-send чекає, поки сповіщення закриють, тож його
дочікується окремий потік, а «open» запускає OBS-Legacy (через `setsid`, з середовищем сесії — він сам
відкриє термінал). Немає `-A` (старий libnotify) — без кнопки; немає notify-send — `gdbus call …Notify`.
Бібліотеки, яких бракує, — сповіщення `critical` (зазвичай не зникає саме).

### Служби

`obs-legacy --notifier enable [служба]` бере першу, що працює (`service::available`), прибирає наші
служби в інших init-системах (одна служба на користувача), пише файл з маркером
`Managed by obs-legacy --notifier`, запускає й надсилає пробне сповіщення. Чужий файл з такою назвою не
перезаписується.

| Служба | Файл | Чи працює (виявлення) | Увімкнути / перезапустити / вимкнути |
|---|---|---|---|
| systemd | `~/.config/systemd/user/obs-legacy-updates.service` (`Type=simple`, `Restart=on-failure`, `RestartSec=60`, `StartLimitIntervalSec=600` + `StartLimitBurst=4`, `WantedBy=default.target`) | `/run/systemd/system` і `systemctl --user show-environment` | `daemon-reload` + `enable --now` / `restart` / `disable --now`, файл, `daemon-reload` |
| dinit | `~/.config/dinit.d/obs-legacy-updates` (`type = process`, `restart = true`, `restart-delay = 10`, не більше 3 перезапусків за 300 с, `log-type = buffer`) | `dinitctl --user list` | `enable` (посилання в `boot.d` + старт) / `reload` (не вийшло — `stop` + `reload`) + `restart` / `disable`, `stop`, `unload`, файл |
| OpenRC 0.60+ | `~/.config/rc/init.d/obs-legacy-updates` (`supervisor=supervise-daemon`, `respawn_delay=60`, `respawn_max=3`, `respawn_period=600`) | `rc-update` і тека `$XDG_RUNTIME_DIR/openrc` (OpenRC користувача запущено) | `rc-update --user add … default` + `rc-service --user … start` / `restart` / `stop`, `del`, файл |
| runit | `<тека runsvdir>/obs-legacy-updates/run` + `finish` (пауза 60 с після падіння) | процес `runsvdir` користувача з текою в `$HOME` (turnstile: `~/.config/service`) | тека з'являється готовою (`.назва.new` → `rename`) / `sv restart` / `sv down` + `sv exit`, тека |
| s6 | `<тека s6-svscan>/obs-legacy-updates/run` + `finish` + `timeout-finish` (70000) | процес `s6-svscan` користувача з текою в `$HOME` | те саме + `s6-svscanctl -a` / `s6-svc -r` / `s6-svc -dx`, тека, `s6-svscanctl -an` |
| автозапуск | `~/.config/autostart/obs-legacy-updates.desktop` (`NoDisplay=true`) | завжди (останній варіант) | файл + одразу `obs-legacy --watch` через `setsid` / зупинити за PID і запустити / файл + SIGTERM |

Теки runsvdir/s6-svscan у `/run` пропускаються (зникнуть після перезавантаження) — тоді автозапуск.
`install.sh --update` викликає `--notifier restart`: фонова перевірка перезапускається з новим
бінарником. `install.sh --uninstall` — `--notifier disable` і прибирає `~/.local/state/obs-legacy`.
Перевірено на Artix з dinit (userspawn); systemd, OpenRC, runit і s6 складено за документацією й звірено з
чужими службами користувача (Syncthing для systemd, `ssh-agent` з Gentoo для OpenRC).

Хто запускає менеджер служб користувача: на Artix — **userspawn** (системна служба; на сигнал `UserNew` від
elogind виконує `~/.config/userspawn/userspawnrc` чи `/etc/xdg/userspawn/userspawnrc` → `exec dinit --user`;
обидва файли й скелет `~/.config/dinit.d/boot` дає пакет `userspawn-dinit`, для систем з runit є
`userspawn-runit`) або **turnstile** (PAM-модуль + `turnstiled`; працює й з seatd без elogind;
бекенд dinit: `dinit --user` з `~/.config/dinit.d`, `/etc/dinit.d/user`, `/usr/local/lib/dinit.d/user`,
`/usr/lib/dinit.d/user` і власним `boot` з `waits-for.d = ~/.config/dinit.d/boot.d`; бекенд runit:
`runsvdir -P ~/.config/service`). В обох випадках dinit користувача слухає типовий сокет
(`$XDG_RUNTIME_DIR/dinitctl`), тож `dinitctl --user` знаходить його без налаштувань.

### Безпека служб

Головне правило: служба сповіщень ніколи не повинна завадити ні завантаженню системи, ні входу.

- **Лише служби користувача.** Init системи (PID 1) читає свої теки (`/etc/dinit.d`, `/usr/lib/dinit.d`…),
  а наші файли лежать у `$HOME` і їх читає менеджер служб користувача вже після входу. Від root
  `--notifier enable` відмовляється.
- **Падіння не ходять по колу.** Ліміти — у таблиці вище; `--watch` сам по собі не завершується (лише сигнал
  чи помилка `watch.lock`). Зникла програма: dinit — «failed to start» без повторів, systemd і OpenRC —
  у межах ліміту, runit/s6 — раз на хвилину (`finish`).
- **dinit — найкрихкіший.** Перевірено на dinit 0.22.1 в окремому екземплярі (`dinit -u -d <тека> -p ./sock`):
  якщо опис служби, на яку посилається `boot.d`, не читається (невідомий параметр, хибне значення), dinit не
  завантажує `boot` і **не запускається взагалі** — разом з pipewire, wireplumber та іншими службами
  користувача; запис `boot.d` без опису він пропускає («Ignoring unresolved dependency»). Тому:
  1. опис перевіряє `dinit-check` (у старих версіях `dinitcheck`) у тимчасовій теці **до** запису в
     `~/.config/dinit.d`; попередження (скажімо, «не знайдено програму») помилками не вважаються;
  2. `dinitctl enable` спершу читає опис і зламаного до `boot.d` не додає (перевірено);
  3. при `--notifier restart` опис, якого `dinitctl reload` не прийняв, замінюється попереднім (його dinit уже
     прийняв, і на нього посилається `boot.d`);
  4. невдале перше ввімкнення прибирається повністю (`deactivate`), а при вимкненні запис `boot.d`
     прибирається за назвою (dinit шукає службу саме за нею), раніше за опис і навіть коли dinit не працює;
  5. `--notifier status` ще раз перевіряє встановлений опис `dinit-check`.
- **Без `NoNewPrivileges`** і схожого «загартування» systemd: кнопка «Відкрити OBS-Legacy» запускає TUI як
  нащадка служби, і з цим прапорцем у ньому не спрацював би `sudo`.
- **Якщо все ж зламалося** — флешка не потрібна: текстова консоль (Ctrl+Alt+F2), `obs-legacy --notifier
  disable` або вручну `rm ~/.config/dinit.d/boot.d/obs-legacy-updates ~/.config/dinit.d/obs-legacy-updates`.

Тести (`service.rs`): опис dinit через справжній `dinit-check` (і що зламаний він відкидає), `sh -n` для
скриптів OpenRC/runit/s6, `finish` (пауза лише після падіння), `systemd-analyze verify` (де є systemd —
у CI), `desktop-file-validate` для автозапуску.

Розробка: `INSTALL_ROOT=<тека з підставною збіркою>` + `XDG_STATE_HOME=<тимчасова тека>` +
`obs-legacy --check-updates` показує, що побачить фонова перевірка. Підставна стара збірка —
`<тека>/versions/<мітка>_32.2.2/BUILDINFO.txt` з `obs_version=32.2.2`, файл `.complete` і посилання `current`;
середовище, як у служби (без дисплея): `env -i HOME=$HOME USER=$USER PATH=/usr/bin
XDG_RUNTIME_DIR=/run/user/$UID INSTALL_ROOT=<тека> XDG_STATE_HOME=<тимчасова тека> target/debug/obs-legacy
--check-updates` (або `--watch`: перша перевірка через 2 хв). Сповіщення будуть справжні.

## Оновлення й перезбирання

TUI запускає `install`, коли OBS-Legacy ще не встановлено, і `update` — коли встановлено (діалог «Оновити OBS?»,
типова кнопка «Оновити»; друга, клавіша R, — `rebuild`). Звіти: `*-install.txt`, `*-update.txt`, `*-rebuild.txt`.

`update_reasons` порівнює встановлене (`/usr/share/obs-legacy/BUILDINFO.txt` і `plugins/<id>`) з тим, що
зібралося б зараз, — по рядку на причину:

- OBS — за комітом (`obs_commit`), заголовки NVENC (`nv_headers_tag`), профіль заліза (`profile`);
- рецепт: `recipe=` у BUILDINFO проти `RECIPE_VERSION` (без рядка — 1). **Версію скрипта не порівнюємо**:
  `RECIPE_VERSION` підвищують лише тоді, коли зміна рушія вимагає перезібрати пакети;
- плагіни: ціль `plugin_target` (fetch + `resolve_plugin_ref`, без checkout) за комітом; вибраний, але не
  встановлений — «буде додано», якщо тільки це не той самий коміт, що минулого разу не зібрався
  (`plugin_failed.<id>=<коміт12>` у BUILDINFO, пише `package_set`): тоді «Оновити» його не повторює, а
  «Перезібрати» — так. Плагін, що не завантажився (мережа), такого рядка не має, тож «Оновити» поверне
  його, навіть якщо pacman уже прибрав старий пакет; встановлений, але не вибраний — «буде прибрано»;
- ffmpeg (`FFMPEG_PLAN == build`) і бібліотеки (`check-libs` з пакета каже «бракує бібліотек»).

Нового немає, але в сховищі є повний набір, новіший за встановлений і зібраний з того самого (OBS, заголовки,
профіль, рецепт, плагіни) — скажімо, встановлення скасували на останньому запитанні: `pending_set` його
знаходить, `use_stored_set` заповнює `SET_DIR`/`SET_FILES`/`SET_CONFLICTS`, і «Оновити» ставить його без
перезбирання («Готовий набір, ще не встановлений»; `Підсумок: поставлено готовий набір …`). TUI про такий набір
нагадує сам: остання спроба `install`/`update`/`rebuild` скасована, а в «Твоїй системі» є повний набір, новіший
за встановлений, — порада «Пакети зібрано, але не встановлено» з «Встановити / оновити OBS».

`install_summary` перетворює причини на рядок `Підсумок:` (у минулому часі, з плагінами, що не зібралися, і
станом ffmpeg). Обірване збирання TUI бачить сам: останній звіт `install`/`update`/`rebuild` без рядка
`Результат:`, новіший за встановлену збірку, а скрипт не тримає блокування кешу (`system::build_running`) —
порада «Минуле збирання обірвалося» → «Перезібрати OBS». Після вимкнення світла лишаються ще блокування
git (`*.lock` у кеші — їх прибирає `rebuild`, коли жоден git не працює) і `db.lck` pacman (`check_pacman_lock`
перед `pacman -S/-U/-R` зупиняє з порадою `sudo rm …`, якщо сам pacman не працює). Невдала компіляція
пропонує `fix rebuild`.
