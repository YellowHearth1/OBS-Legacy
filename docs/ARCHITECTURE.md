# Архітектура OBS-Legacy

Технічний довідник для розробки. Огляд для користувачів — у [`README.md`](../README.md), план перевірки
на справжньому залізі — у [`TESTING.md`](TESTING.md).

## Складові

| Частина | Файли | Роль |
|---|---|---|
| Рушій | `scripts/build-obs-nvenc-legacy-artix.sh` (bash, ~3700 рядків) | Уся реальна робота: профіль заліза, перевірка, збирання OBS, плагінів і ffmpeg, пакети pacman (`*-legacy-580xx`), встановлення через `pacman -U`, відкат, видалення, pacman-хук у пакеті |
| TUI | `src/` (Rust, ratatui 0.30) | Інтерфейс поверх рушія: меню, панель стану, вбудований термінал, плагіни, налаштування |
| Установник | `install.sh`, `assets/obs-legacy.svg` | Збирає TUI (`--target-dir` репозиторію), ставить у `~/.local/bin` і прибирає `target/`, ярлик у меню й на робочий стіл, пропонує ввімкнути сповіщення |
| Сповіщення | `src/updates.rs`, `src/notify.rs`, `src/service.rs` | Фонова перевірка оновлень (`obs-legacy --watch`) і сповіщення на робочому столі; служба користувача systemd, dinit, OpenRC, runit, s6 або автозапуск сесії (див. «Сповіщення про оновлення») |
| Імітатор | `dev/fake-build.sh` | Друкує те саме, що рушій (разом зі звітом), але нічого не робить — для розробки інтерфейсу |
| Скріншоти | `dev/screenshot.py` | PNG зі знімка `tmux capture-pane -e -p -N` (Pillow) — для `docs/screenshots/` |
| Іконка й банер | `dev/make-assets.py`, `assets/third-party/` | `assets/obs-legacy.svg` і `assets/banner.png` (1280×640, шапка README і прев'ю репозиторію) з іконки «graphics-card» Phosphor Icons (MIT) і шрифту JetBrains Mono (OFL), текст — кривими. Без логотипів NVIDIA й OBS: це чужі торгові марки, а їхні правила забороняють використання без ліцензії |
| CI | `.github/workflows/ci.yml` | `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `shellcheck`, `plugins --tsv` |

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
| `warn` | `[УВАГА] ` | щось варто знати, але робота триває |
| `fail` / `die "що" ["що робити"]` | `[ПОМИЛКА] ` | зупинка; другий аргумент `die` друкується як порада |
| `hint` | `[ПОРАДА] ` | що робити далі — пишеться одразу після попередження чи помилки, до яких належить |
| `cancelled` | `[СКАСОВАНО] ` | людина відповіла «n»: нічого не змінено, код завершення 10 (`EXIT_CANCELLED`) |
| `fix команда "опис"` | `[ВИПРАВИТИ] ` | що можна виправити однією командою скрипта (`deps`, `clean`, `check`, `rebuild`); TUI робить із цього кнопку «Виправити» |
| `summary` | `Підсумок: ` | наприкінці `install`/`update`/`rebuild`: що змінилося («OBS 33.0.0 → 33.0.1», «оновлень немає…»); TUI показує першим рядком картки результату |

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
«Плагіни»). CEF завантажує curl з `--retry 3`.

`LABEL` = `<YYYYmmdd-HHMMSS>_<git describe OBS>` — назва набору в сховищі й `label=` у BUILDINFO.
`PREFIX` = `/usr` (як в OBS з репозиторію): OBS 33 запікає абсолютні шляхи плагінів з
`CMAKE_INSTALL_PREFIX`, тож сторонні плагіни в `/usr/lib/obs-plugins` чи `/usr/lib/obs-modules/plugins`
підхоплюються.

### Безпекові гарантії (порушувати не можна)

- Система змінюється лише через pacman: `pacman -S --needed` (пакети для збірки) і `pacman -U` (пакети
  OBS-Legacy) — обидва після `confirm` і без `--noconfirm`, тож pacman ще й сам питає про заміни.
  `pacman -Syu` скрипт не запускає. Файли чужих пакетів не перезаписуються: власників файлів нового пакета
  скрипт додає в `conflicts` (pacman запропонує замінити), а файл без власника — зупинка з порадою.
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
| pacman-хук | `/usr/share/libalpm/hooks/obs-legacy.hook` → `/usr/lib/obs-legacy/check-libs` (у пакеті OBS) |
| Набори пакетів | `~/.local/share/obs-studio-nvenc-artix/packages/<LABEL>/` (`*.pkg.tar.zst`, `BUILDINFO.txt` набору, `.complete`) |
| Номер збирання | `~/.local/share/obs-studio-nvenc-artix/build-number` (pkgrel наборів) |
| Стара схема (прибирається) | `/opt/obs-studio-nvenc/{versions,current}`, `/usr/local/bin/obs`, `~/.local/share/applications/com.obsproject.Studio.desktop`, `/etc/pacman.d/hooks/90-obs-legacy.hook` → `/usr/local/lib/obs-legacy/check-libs` |
| Кеш | `~/.cache/obs-studio-nvenc-artix/` (`obs-studio-src`, `nv-codec-headers-*`, `downloads`, `cef`, `build`, `build-check`, `stage`, `plugins/<id>/{src,build,stage,build.log,tried-commit,qt-private.cmake}`, `makepkg/` (PKGBUILD, makepkg.conf, журнали, `build/`), `sources/` (SRCDEST: git ffmpeg), `ffmpeg-pkgbuild/` (PKGBUILD Arch), `gnupg/` (ключі для підпису ffmpeg), `reports`) |
| Стан | `~/.local/share/obs-studio-nvenc-artix/` (`packages/`, `config-backups/`, `history.tsv`, `build-number`, `*-original.backup`, хеші CEF) |
| Налаштування | `~/.config/obs-legacy/settings.env` |
| TUI | `~/.local/bin/obs-legacy`, `~/.local/share/applications/obs-legacy.desktop`, `~/.local/share/icons/hicolor/scalable/apps/obs-legacy.svg` |
| Дані TUI | `~/.local/share/obs-legacy/` (`build-obs-nvenc-legacy-artix.sh` — розпакований скрипт, `source-dir` — звідки встановлено) |
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
під процесор: `install.sh` додає `RUSTFLAGS=-C target-cpu=native` (CI збирає без нього). Системні бібліотеки
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

Вміст пакетів: OBS — дерево `stage/usr` + `share/obs-legacy/BUILDINFO.txt` + хук і `check-libs` +
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

Коли в репозиторії виходить новий ffmpeg, `pacman -Syu` наш пакет не чіпає (його немає в репозиторіях), а
хук із пакета OBS пише «у репозиторії новий ffmpeg — перезбери». Якщо новий ffmpeg змінить sonames, pacman
сам запропонує замінити `ffmpeg-legacy-580xx` репозиторним (NVENC в ffmpeg тоді зникне до перезбирання;
власний NVENC OBS від цього не залежить).

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
`вибрано (1/0)`, `типово (1/0)`, `репозиторій`, `пакети`, `опис`, `назва пакета`.

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

## pacman-хук

У пакеті OBS (`add_hook_to_tree`, якщо `INSTALL_PACMAN_HOOK=1`):
`/usr/share/libalpm/hooks/obs-legacy.hook` спрацьовує на `Upgrade`/`Remove` будь-якого пакета
(`Target = *`), `When = PostTransaction`, `Exec = /usr/lib/obs-legacy/check-libs`. Скрипт бере файли всіх
встановлених `*-legacy-580xx` (`pacman -Qlq`), перевіряє ldd для ELF у `bin/` і `*.so*`, а якщо стоїть
`ffmpeg-legacy-580xx` — порівнює (`vercmp`) версію `ffmpeg` у репозиторії з `arch_version` з
`/usr/share/obs-legacy/ffmpeg.txt`. Друкує щось лише тоді, коли знайшов проблеми (рядки проблем — з
6 пробілами, їх читає `updates::parse_libcheck`), і завжди завершується з кодом 0. Хук іде й зникає разом
із пакетом OBS.

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
`OBS_LEGACY_SETTINGS=<шлях>`) і розбирає вивід (`src/analyzer.rs`):

| Вивід скрипта | Що робить TUI |
|---|---|
| `==> Назва` (`say`) | новий етап у списку |
| `[OK] …` | — (етап за замовчуванням успішний) |
| `[УВАГА] …` | етап позначається ⚠, лічильник попереджень |
| `[ПОМИЛКА] …` | етап позначається ✗, лічильник помилок |
| `[N/M] …` (ninja) | смуга «Компіляція»/«Плагін» з оцінкою часу |
| `####   45.2%` (curl `--progress-bar`, оновлення через `\r`) | смуга «Завантаження» |
| незавершений рядок з `[y/N]`/`[Y/n]` | банер «введи y або n» + дзвінок. Поки завдання йде, у скрипт ідуть літери, Enter, Backspace, Tab і Ctrl+…; стрілки, Home/End гортають вивід, а Esc, Delete, Insert, Shift+Tab не йдуть нікуди — інакше в рядку введення лишалося б сміття |
| незавершений рядок з `password`/`пароль` | банер про пароль sudo/doas |
| незавершений рядок на `:`/`?`, тиша > 2 с | банер «схоже, чекає на введення» |
| `[ПОРАДА] …` | додається до останнього попередження чи помилки; картка результату показує «Що робити» |
| `[СКАСОВАНО] …` | картка «скасовано» (жовта), а не «помилка» |
| `[ВИПРАВИТИ] команда: опис` | додається до останньої проблеми; головна кнопка картки (Enter) запускає дію з білого списку `app::fix_action` (`deps` → діалог і `deps`, `clean`, `check`); невідомі команди ігноруються |
| `Звіт: <шлях>` | кнопки «Переглянути звіт» (`o`), «Скопіювати шлях» (`c`), «Відкрити в програмі» (`e`); `~/` розгортається |
| код завершення | ✓ або ✗ (або «Перервано» після Ctrl+C) |

Після завершення картка результату показує: для помилки — останній `[ПОМИЛКА]` і його поради, для
успіху — зауваження з порадою (або типовий «Що далі» для дії), для скасування — повідомлення
`[СКАСОВАНО]`. Без жодної поради TUI радить переглянути звіт і створити issue.

Старі звіти TUI читає тим самим аналізатором (`src/reports.rs`): результат — з рядка `Результат:` (для
звітів старих версій — за `ПЕРЕВІРКУ ПРОЙДЕНО`/`ГОТОВО:`/`[ПОМИЛКА]`), головна проблема — остання
помилка з порадами. На цьому будується порада «Що далі» на головному екрані (`src/advice.rs`).

Якщо змінюєш префікси виводу в скрипті (`say`/`ok`/`warn`/`fail`, `Звіт:`), онови `analyzer.rs` і тести.

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
| `main.rs` | аргументи (`--help`, `--version`, `--script-path`, `--probe-nvenc`, `--no-relaunch`, `--check-updates`, `--watch`, `--notifier …`), перезапуск у терміналі, ініціалізація ratatui, миша й bracketed paste |
| `app.rs` | стан, `MENU` (основні кроки + розділ «Додатково», `Row`), екрани (`Intro`, `Home`, `Run`, `Plugins`, `Rollback`, `Settings`, `Reports`, `Report`, `Help`), діалоги, сповіщення, клавіатура й миша, фонові потоки (`Msg`), `key_to_bytes` для PTY |
| `advice.rs` | порада «Що далі»: чиста функція від `SysInfo`, нових версій (`updates::Facts`), звітів, вибраних плагінів і стану сповіщень → тон, крок (перевірка/встановлення/запуск), текст, дія для Enter, друга кнопка, звіт для `o` |
| `reports.rs` | список звітів, результат і головна проблема кожного (хвіст 64 КБ), текст для переглядача, класифікація рядків |
| `desktop.rs` | `xdg-open` (від'єднано через `setsid`) і буфер обміну: wl-copy → xclip → xsel + OSC 52 |
| `runner.rs` | `Job`: portable-pty + потік читання + `vt100::Parser` (10 000 рядків історії) + `Analyzer`, resize, Ctrl+C, kill |
| `analyzer.rs` | розбір виводу (див. контракт) |
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
DejaVu Sans Mono): ◎ ▼ ▶ ⊕ ⋯ ≡ ◫ ◷ ↩ ◈ ↪ ⌫ ✗ ? × ◆ ● ○ ✓ ⚠ ■ ⌨. Уникаємо ★ ↺ ↻ ☰ ⚿ ℹ — у JetBrains Mono їх
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

## Сповіщення про оновлення

Фонова перевірка — той самий бінарник у режимі `obs-legacy --watch`. Його запускає служба користувача
(`service.rs`), а сповіщення показує `notify.rs`. Без root і без змін у системі.

### Що перевіряється

| Що | Коли | Звідки |
|---|---|---|
| Новий OBS | раз на 12 год | `git ls-remote --tags` obsproject/obs-studio, теги `X.Y.Z`; порівняння з `obs_version` активної збірки (`system::newer`) |
| Нові релізи плагінів активної збірки | разом з OBS | `git ls-remote` репозиторію з каталогу; теги `v?X.Y…` — так само, як їх бере скрипт (`resolve_plugin_ref`) |
| Новий ffmpeg у репозиторіях | разом з OBS | `pacman -Si ffmpeg` (локальна база, без мережі) проти `arch_version` з `/usr/share/obs-legacy/ffmpeg.txt` (`vercmp`); лише коли `ffmpeg-legacy-580xx` встановлено й «Збирати ffmpeg» не вимкнено. Те саме — у пораді «Що далі» й рядку «Оновлення» |
| Новий OBS-Legacy | разом з OBS | теги origin теки з вихідними кодами (`source-dir`) або `YellowHearth1/OBS-Legacy`; порівняння з `CARGO_PKG_VERSION` |
| Бібліотеки пакетів OBS-Legacy | після кожної транзакції pacman | змінився mtime `/var/lib/pacman/local` і немає `db.lck` → `/usr/lib/obs-legacy/check-libs` (той, що й у pacman-хука; для старої схеми — `/usr/local/lib/…`); якщо хук вимкнено — не перевіряється |

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
