#!/usr/bin/env bash
# shellcheck disable=SC1111  # типографські лапки “…” в англійських повідомленнях — навмисні
# =============================================================================
#  build-obs-nvenc-legacy-artix.sh — рушій проєкту OBS-Legacy
#  https://github.com/YellowHearth1/OBS-Legacy
#  OBS Studio з робочим NVENC для відеокарт NVIDIA на драйверах 580.xx
# -----------------------------------------------------------------------------
#  УВАГА: цей скрипт і TUI-оболонку OBS-Legacy створено за допомогою
#  Claude Code — ШІ-асистента для програмування від Anthropic — на замовлення
#  і під керівництвом автора репозиторію. Код не проходив незалежного аудиту:
#  спершу запусти «check» (нічого не змінює) і переглянь, що робитиме скрипт.
#  Використовуєш на власний ризик (див. LICENSE).
#
#  Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy — якби не цей
#  проєкт, не було б і OBS-Legacy. Дякуємо його автору!
# -----------------------------------------------------------------------------
#  Для кого: Artix / Arch, legacy-відеокарти NVIDIA (Maxwell / Pascal / Volta)
#  на драйверах 580.xx. Ці драйвери підтримують NVENC API лише до 13.0, а OBS і
#  ffmpeg з репозиторіїв зібрані з NVENC SDK 13.1, тому NVENC там не вмикається.
#
#  Що робить скрипт:
#    * сам визначає дистрибутив, процесор, відеокарту й драйвер і підбирає
#      параметри збирання: прапорці компілятора під цей процесор
#      (-march=native; решта — з makepkg.conf дистрибутива), заголовки NVENC
#      під версію драйвера (13.0 для 580xx), CUDA-архітектуру відеокарти;
#    * збирає OBS з офіційного репозиторію з приватною копією nv-codec-headers
#      та офіційним CEF від OBS (Browser Source і браузерні доки); SHA-256
#      архіву CEF звіряється з офіційним хешем із вихідних кодів OBS;
#    * збирає вибрані плагіни (за замовчуванням obs-multi-rtmp) з їхніх
#      останніх релізів саме під цю збірку OBS — оновлюються разом з OBS;
#    * збирає ffmpeg з PKGBUILD Arch, але з NVENC під цей драйвер
#      (вимкнути: BUILD_FFMPEG=0);
#    * перевіряє збірку тестом obs-nvenc-test і ldd;
#    * пакує все в пакети pacman з припискою -legacy-580xx
#      (obs-studio-legacy-580xx, <плагін>-legacy-580xx, ffmpeg-legacy-580xx) і
#      ставить їх через pacman -U у ті самі місця, що й OBS з репозиторію
#      (/usr): pacman сам запропонує замінити obs-studio / obs-studio-git /
#      ffmpeg, а сторонні плагіни (AUR) ставляться на цей OBS як завжди;
#    * зберігає набори пакетів для відкату (~/.local/share/…/packages);
#    * пакет OBS містить pacman-хук, який після оновлень системи перевіряє,
#      чи вистачає бібліотек, і підказує, коли перезібрати.
#
#  Налаштування: змінні середовища або файл
#  ~/.config/obs-legacy/settings.env (його редагує TUI obs-legacy).
#
#  Гарантії безпеки:
#    * система змінюється лише через pacman: pacman -S --needed для пакетів
#      збірки і pacman -U для пакетів OBS-Legacy, обидва — лише після твого
#      підтвердження (без --noconfirm); pacman -Syu скрипт не запускає;
#    * компіляція і пакування (makepkg) ідуть без root; sudo/doas потрібен
#      лише для pacman і прибирання старої схеми /opt;
#    * пакети встановлюються тільки після obs-nvenc-test і перевірки
#      бібліотек; якщо щось не так — поточний OBS не змінюється;
#    * перед кожним встановленням робиться архівна копія ~/.config/obs-studio;
#    * pacman-хук лише перевіряє бібліотеки і нічого не змінює
#      (вимкнути: INSTALL_PACMAN_HOOK=0); плагін або ffmpeg, що не зібрався,
#      не блокує встановлення OBS — про нього буде попередження (суворо для
#      плагінів: PLUGINS_STRICT=1).
#
#  Довідка: ./build-obs-nvenc-legacy-artix.sh help
# =============================================================================

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true
umask 022

readonly SCRIPT_VERSION="2026.10.31.1"
# Номер рецепта збирання й пакування. Підвищувати, лише коли зміна в рушії вимагає
# перезібрати вже встановлені пакети (прапорці, розкладка, залежності): тоді «Оновити»
# збере їх знову. Зміни інтерфейсу й підказок його не чіпають.
readonly RECIPE_VERSION=1
readonly SELF_NAME="build-obs-nvenc-legacy-artix"
readonly MARKER="Managed by $SELF_NAME"
SELF_PATH="$(readlink -f -- "${BASH_SOURCE[0]}" 2>/dev/null || printf '%s' "${BASH_SOURCE[0]}")"

# ---------------------------------------------------------------------------
# Файл налаштувань (KEY=VALUE). Його пише TUI obs-legacy, але він працює і
# без TUI. Вміст не виконується як код: читаються лише відомі ключі, а змінна
# середовища (навіть порожня) завжди має пріоритет над файлом.
# ---------------------------------------------------------------------------
SETTINGS_FILE="${OBS_LEGACY_SETTINGS:-${XDG_CONFIG_HOME:-${HOME:-}/.config}/obs-legacy/settings.env}"
readonly -a SETTINGS_KEYS=(
  OBS_REF OBS_REPO NV_HEADERS_REPO NV_HEADERS_TAG CEF_BUILD CEF_ARCHIVE_URL CEF_SHA256 CEF_CDN
  BUILD_JOBS KEEP_VERSIONS EXTRA_CMAKE_ARGS FORCE_CLEAN_BUILD SKIP_NVENC_TEST ALLOW_MISSING_LIBS
  OBS_PLUGINS PLUGINS_STRICT INSTALL_PACMAN_HOOK CPU_TUNING OPT_LEVEL BUILD_FFMPEG UI_LANG
)
load_settings_file() {
  local line key val k known
  [[ -f "$SETTINGS_FILE" && -r "$SETTINGS_FILE" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line=${line%$'\r'}
    [[ "$line" =~ ^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]] || continue
    key=${BASH_REMATCH[1]} val=${BASH_REMATCH[2]}
    known=0
    for k in "${SETTINGS_KEYS[@]}"; do
      if [[ "$k" == "$key" ]]; then known=1; break; fi
    done
    if (( ! known )) && [[ "$key" =~ ^PLUGIN_REF_[A-Za-z0-9_]+$ ]]; then known=1; fi
    (( known )) || continue
    if [[ "$val" =~ ^\"(.*)\"$ || "$val" =~ ^\'(.*)\'$ ]]; then val=${BASH_REMATCH[1]}; fi
    if [[ -z "${!key+x}" ]]; then
      printf -v "$key" '%s' "$val"
      export "${key?}"
    fi
  done < "$SETTINGS_FILE"
}
load_settings_file

# Мова повідомлень: OBS_LEGACY_LANG (її передає TUI) → UI_LANG (auto | uk | en) →
# локаль системи (без неї — /etc/locale.conf): uk… — українська, інакше англійська.
resolve_lang() {
  local v=${OBS_LEGACY_LANG:-} loc
  if [[ "$v" != uk && "$v" != en ]]; then
    case "${UI_LANG:-auto}" in
      uk | en) v=$UI_LANG ;;
      *)
        loc=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
        if [[ -z "$loc" && -r /etc/locale.conf ]]; then
          loc=$(sed -n 's/^LANG=//p' /etc/locale.conf 2>/dev/null | tr -d "\"'" | head -n1 || true)
        fi
        if [[ "$loc" == uk* ]]; then v=uk; else v=en; fi
        ;;
    esac
  fi
  if [[ "$v" == en ]]; then LANG_EN=1; else LANG_EN=0; fi
}
resolve_lang

# Текст мовою інтерфейсу: t "українською" "in English".
t() { if ((LANG_EN)); then printf '%s' "$2"; else printf '%s' "$1"; fi; }

# ---------------------------------------------------------------------------
# Налаштування (кожне можна перевизначити змінною середовища)
# ---------------------------------------------------------------------------
OBS_REF="${OBS_REF:-stable}"
OBS_REPO="${OBS_REPO:-https://github.com/obsproject/obs-studio.git}"
NV_HEADERS_REPO="${NV_HEADERS_REPO:-https://github.com/FFmpeg/nv-codec-headers.git}"
# auto — найновіший тег, чий NVENC API знає драйвер (580xx → n13.0.19.1).
NV_HEADERS_TAG="${NV_HEADERS_TAG:-auto}"
CEF_BUILD="${CEF_BUILD:-}"
CEF_ARCHIVE_URL="${CEF_ARCHIVE_URL:-}"
CEF_SHA256="${CEF_SHA256:-}"
CEF_CDN="${CEF_CDN:-https://cdn-fastly.obsproject.com/downloads}"
BUILD_JOBS="${BUILD_JOBS:-}"
KEEP_VERSIONS="${KEEP_VERSIONS:-2}"
EXTRA_CMAKE_ARGS="${EXTRA_CMAKE_ARGS:-}"
FORCE_CLEAN_BUILD="${FORCE_CLEAN_BUILD:-0}"
SKIP_NVENC_TEST="${SKIP_NVENC_TEST:-0}"
ALLOW_MISSING_LIBS="${ALLOW_MISSING_LIBS:-0}"
INSTALL_ROOT="${INSTALL_ROOT:-/opt/obs-studio-nvenc}"
# Плагіни через кому або пробіл (див. команду plugins); none — без плагінів.
OBS_PLUGINS="${OBS_PLUGINS-obs-multi-rtmp}"
PLUGINS_STRICT="${PLUGINS_STRICT:-0}"
INSTALL_PACMAN_HOOK="${INSTALL_PACMAN_HOOK:-1}"
# Під який процесор збирати: native — саме під цей (найшвидше, але лише для
# нього); generic — як пакети з репозиторію; x86-64-v2/v3/v4 — рівень ISA.
CPU_TUNING="${CPU_TUNING:-native}"
# Рівень оптимізації компілятора: 2 (як у дистрибутива) або 3.
OPT_LEVEL="${OPT_LEVEL:-2}"
# Збирати ffmpeg-legacy-580xx (системний ffmpeg на 580xx не вміє NVENC).
BUILD_FFMPEG="${BUILD_FFMPEG:-1}"
FFMPEG_PKGBUILD_REPO="${FFMPEG_PKGBUILD_REPO:-https://gitlab.archlinux.org/archlinux/packaging/packages/ffmpeg.git}"

# Пакети OBS-Legacy: стандартні назви з припискою, ставляться в /usr.
readonly PKG_SUFFIX="legacy-580xx"
readonly OBS_PKG="obs-studio-$PKG_SUFFIX"
readonly FFMPEG_PKG="ffmpeg-$PKG_SUFFIX"
readonly PREFIX="/usr"
# Відомості про встановлену збірку (усередині пакетів).
readonly INFO_DIR="/usr/share/obs-legacy"
readonly PKG_HOOK="/usr/share/libalpm/hooks/obs-legacy.hook"
readonly PKG_LIBCHECK="/usr/lib/obs-legacy/check-libs"
# Типові заголовки NVENC, якщо драйвер не відповів (максимум для 580xx).
readonly NV_HEADERS_DEFAULT="n13.0.19.1"

# Стара схема (до пакетів): збірки в /opt, обгортка, ярлик-перекриття й хук.
# Під час встановлення пакетів скрипт прибирає їх сам.
readonly WRAPPER="/usr/local/bin/obs"
readonly DESKTOP_ID="com.obsproject.Studio.desktop"
readonly HOOK_FILE="/etc/pacman.d/hooks/90-obs-legacy.hook"
readonly LIBCHECK="/usr/local/lib/obs-legacy/check-libs"

CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"

# Назви каталогів збігаються з попередньою версією скрипта, щоб повторно
# використати вже завантажені вихідні коди та архів CEF.
BUILD_ROOT="$CACHE_HOME/obs-studio-nvenc-artix"
STATE_DIR="$DATA_HOME/obs-studio-nvenc-artix"
HEADERS_SRC="$BUILD_ROOT/nv-codec-headers-src"
HEADERS_PREFIX="$BUILD_ROOT/nv-codec-headers-prefix"
OBS_SRC="$BUILD_ROOT/obs-studio-src"
DOWNLOADS="$BUILD_ROOT/downloads"
CEF_DIR="$BUILD_ROOT/cef"
BUILD_DIR="$BUILD_ROOT/build"
CHECK_DIR="$BUILD_ROOT/build-check"
STAGE_DIR="$BUILD_ROOT/stage"
TOOLS_DIR="$BUILD_ROOT/tools"
REPORT_DIR="$BUILD_ROOT/reports"
PLUGINS_DIR="$BUILD_ROOT/plugins"
# makepkg: згенеровані PKGBUILD і makepkg.conf, BUILDDIR; SRCDEST — окремо, щоб
# «Очистити кеш» не змушував знову завантажувати git-репозиторій ffmpeg.
MAKEPKG_DIR="$BUILD_ROOT/makepkg"
SOURCES_DIR="$BUILD_ROOT/sources"
FFMPEG_PKGBUILD_DIR="$BUILD_ROOT/ffmpeg-pkgbuild"
GNUPG_DIR="$BUILD_ROOT/gnupg"
# Набори зібраних пакетів (для встановлення й відкату).
PKG_STORE="$STATE_DIR/packages"
VERSIONS_DIR="$INSTALL_ROOT/versions"
CURRENT_LINK="$INSTALL_ROOT/current"
ROOT_MARK="$INSTALL_ROOT/.$SELF_NAME"

# Пакети з репозиторіїв. Перевірка йде через `pacman -T`, тож пакет-замінник
# (напр., pipewire-jack замість jack2 або інша збірка ffmpeg) теж зараховується.
# mbedtls3 / mbedtls додається окремо — див. tls_package().
readonly -a REQUIRED_PKGS=(
  git cmake ninja pkgconf curl gcc make binutils xz fakeroot zstd
  qt6-base qt6-svg
  ffmpeg x264 jansson speexdsp luajit python swig
  libx11 libxcb libxcomposite libxinerama libxrandr libxkbcommon libxkbcommon-x11
  wayland wayland-protocols extra-cmake-modules vulkan-headers
  libpulse alsa-lib pipewire libdrm mesa libva v4l-utils pciutils dbus glib2
  fontconfig freetype2
  nlohmann-json asio websocketpp simde uthash rnnoise libdatachannel librist srt
)
# Рекомендовані: Wayland-плагін Qt і системні бібліотеки, потрібні CEF.
# Якщо якоїсь назви немає в репозиторіях — не критично: відсутні бібліотеки
# все одно виявить перевірка ldd перед встановленням.
readonly -a RECOMMENDED_PKGS=(
  qt6-wayland
  nss nspr at-spi2-core libcups pango cairo libxdamage libxfixes libxext expat
)

# Каталог плагінів, які скрипт уміє зібрати під власну збірку OBS.
# Формат: id|назва|git-репозиторій|пакети pacman|параметри CMake|опис|назви в AUR
# Параметр виду "<33?-DFOO=ON" додається лише для OBS, старішого за 33;
# "CXXFLAGS+=…" і "CFLAGS+=…" додаються до прапорців компілятора цього плагіна.
# Назви в AUR: перша — основа назви пакета (<назва>-legacy-580xx, provides),
# усі — conflicts, тож pacman сам запропонує замінити пакет з AUR.
# Береться найновіший тег-реліз (vX.Y або X.Y.Z); зафіксувати версію:
# PLUGIN_REF_<id, де "-" замінено на "_">=тег, напр. PLUGIN_REF_obs_multi_rtmp=0.7.4.5
PLUGIN_CATALOG=(
  "obs-multi-rtmp|Multiple RTMP Outputs|https://github.com/sorayuki/obs-multi-rtmp.git||-DENABLE_FRONTEND_API=ON -DENABLE_QT=ON -DCMAKE_CXX_STANDARD=20|$(t "Стрім одночасно на кілька платформ (Twitch, YouTube, Kick…) з окремими налаштуваннями кожного виходу" "Stream to several platforms at once (Twitch, YouTube, Kick…), each output with its own settings")|obs-multi-rtmp obs-multi-rtmp-git obs-multi-rtmp-bin"
  "move-transition|Move Transition|https://github.com/exeldro/obs-move-transition.git|||$(t "Плавна анімація положення, розміру й властивостей джерел між сценами та фільтри руху" "Smooth animation of source position, size and properties between scenes, plus move filters")|obs-move-transition obs-move-transition-git"
  "source-record|Source Record|https://github.com/exeldro/obs-source-record.git|||$(t "Запис окремих джерел або сцен в окремі файли паралельно з основним записом" "Record individual sources or scenes to separate files alongside the main recording")|obs-source-record obs-source-record-git"
  "shaderfilter|Shaderfilter|https://github.com/exeldro/obs-shaderfilter.git|||$(t "Фільтри на власних шейдерах: свічення, розмиття, хромакей, спотворення та десятки готових ефектів" "Filters built on custom shaders: glow, blur, chroma key, distortion and dozens of ready-made effects")|obs-shaderfilter obs-shaderfilter-git"
  "vertical-canvas|Aitum Vertical Canvas|https://github.com/Aitum/obs-vertical-canvas.git|||$(t "Окреме вертикальне полотно 9:16 для TikTok / Shorts / Reels: стрім і запис паралельно з основним" "A separate vertical 9:16 canvas for TikTok / Shorts / Reels: stream and record alongside the main one")|obs-vertical-canvas obs-vertical-canvas-git"
  "pipewire-audio-capture|PipeWire Audio Capture|https://github.com/dimtpap/obs-pipewire-audio-capture.git||<33?-DLEGACY=ON|$(t "Захоплення звуку окремих програм і пристроїв через PipeWire" "Capture audio from individual apps and devices via PipeWire")|obs-pipewire-audio-capture obs-pipewire-audio-capture-git"
  "composite-blur|Composite Blur|https://github.com/FiniteSingularity/obs-composite-blur.git|||$(t "Якісні фільтри розмиття: гаусове, кругове, рухом, пікселізація, з масками" "High-quality blur filters: Gaussian, radial, motion, pixelate, with masks")|obs-composite-blur obs-composite-blur-git"
  "advanced-masks|Advanced Masks|https://github.com/FiniteSingularity/obs-advanced-masks.git||-DENABLE_FRONTEND_API=ON -DENABLE_QT=ON|$(t "Маски-форми й градієнтні маски для джерел" "Shape masks and gradient masks for sources")|obs-advanced-masks obs-advanced-masks-git"
  "stroke-glow-shadow|Stroke Glow Shadow|https://github.com/FiniteSingularity/obs-stroke-glow-shadow.git|||$(t "Обведення, свічення й тіні для будь-якого джерела" "Stroke, glow and shadow for any source")|obs-stroke-glow-shadow obs-stroke-glow-shadow-git"
  "retro-effects|Retro Effects|https://github.com/FiniteSingularity/obs-retro-effects.git|||$(t "Ретро-ефекти: CRT, VHS, глітч, дизеринг, хроматична аберація" "Retro effects: CRT, VHS, glitch, dithering, chromatic aberration")|obs-retro-effects obs-retro-effects-git"
  "scale-to-sound|Scale to Sound|https://github.com/dimtpap/obs-scale-to-sound.git|||$(t "Джерело масштабується в такт звуку (наприклад, аватар, що «говорить»)" "Scales a source to the beat of the sound (e.g. a “talking” avatar)")|obs-scale-to-sound obs-scale-to-sound-git"
  "downstream-keyer|Downstream Keyer|https://github.com/exeldro/obs-downstream-keyer.git|||$(t "Шар поверх усіх сцен (логотип, титри), який не зникає при перемиканні сцен" "A layer on top of all scenes (logo, captions) that stays when you switch scenes")|obs-downstream-keyer obs-downstream-keyer-git"
  "transition-table|Transition Table|https://github.com/exeldro/obs-transition-table.git|||$(t "Свій перехід для кожної пари сцен" "A separate transition for every pair of scenes")|obs-transition-table obs-transition-table-git"
  "freeze-filter|Freeze Filter|https://github.com/exeldro/obs-freeze-filter.git|||$(t "Заморожування кадру джерела фільтром або гарячою клавішею" "Freeze a source's frame with a filter or a hotkey")|obs-freeze-filter obs-freeze-filter-git"
  "replay-source|Replay Source|https://github.com/exeldro/obs-replay-source.git|||$(t "Миттєвий повтор (instant replay) прямо в сцені" "Instant replay right in the scene")|obs-replay-source obs-replay-source-git"
  "gradient-source|Gradient Source|https://github.com/exeldro/obs-gradient-source.git|||$(t "Джерело-градієнт з кількома кольорами" "A gradient source with several colors")|obs-gradient-source obs-gradient-source-git"
  "waveform|Waveform|https://github.com/phandasm/waveform.git|fftw|-DPACKAGED_INSTALL=ON|$(t "Аудіовізуалізатор (спектр або хвиля) як джерело" "Audio visualizer (spectrum or wave) as a source")|obs-waveform obs-waveform-git"
  "rewards-theater|RewardsTheater|https://github.com/gottagofaster236/RewardsTheater.git|boost openssl fmt|-DENABLE_FRONTEND_API=ON -DENABLE_QT=ON CXXFLAGS+=-Wno-error=deprecated-declarations|$(t "Глядачі замовляють відео чи звуки на стрімі за бали каналу Twitch (черга, модерація, випадкове відео з VLC-джерела)" "Viewers order videos or sounds on stream for Twitch channel points (queue, moderation, a random video from a VLC source)")|rewards-theater-obs rewards-theater-obs-git"
)
readonly -a PLUGIN_CATALOG

# Глобальні змінні, які заповнюються під час роботи.
SUDO_CMD=''
OBS_RESOLVED='' OBS_REF_LABEL='' OBS_COMMIT='' OBS_DESCRIBE='' OBS_MAJOR=''
HEADERS_API='' DRIVER_NVENC_API='' NV_HEADERS_RESOLVED='' HEADERS_PCDIR=''
CEF_BUILD_RESOLVED='' CEF_ROOT='' CEF_SHA_PENDING='' CEF_SPEC_URL='' CEF_SPEC_SHA=''
TLS_MODE='system' MBEDTLS3_CMAKE_DIR='' MBEDTLS3_INCLUDE='' MBEDTLS3_LIBDIR='' MBEDTLS3_PCDIR=''
LABEL='' STAGED_TREE='' MENU_CMD='' NVENC_TEST_RESULT=''
# Профіль заліза (detect_build_profile).
DISTRO_ID='' DISTRO_NAME='' CPU_MODEL='' CPU_MARCH='' ISA_LEVEL='' CPU_FEATURES=''
GPU_NAME='' GPU_CC='' GPU_ARCH='' DRIVER_VERSION='' DRIVER_PKG=''
MARCH_FLAGS='' OPT_CFLAGS='' OPT_CXXFLAGS='' OPT_LDFLAGS='' PROFILE_SIG='' JOBS=''
# install — зібрати й поставити; update — те саме, але лише коли є що оновлювати;
# rebuild — збирання OBS начисто (після обірваного збирання тощо).
INSTALL_MODE=install NOTHING_NEW=0
declare -a UPDATE_REASONS=()
# Пакети: номер збирання (pkgrel), набір у сховищі, ffmpeg.
BUILD_NUMBER='' SET_DIR='' OBS_PKGVER=''
FFMPEG_REPO_VER='' FFMPEG_PLAN='' FFMPEG_REASON='' FFMPEG_FILE='' FFMPEG_PKGBUILD_VER=''
declare -a CMAKE_ARGS=() FEATURES=() DEP_INSTALL=() DEP_INSTALL_REQ=() DEP_NOREPO_REQ=() DEP_NOREPO_REC=()
declare -a PLUGINS_SELECTED=() PLUGINS_OK=() PLUGINS_FAILED=() SET_FILES=() SET_CONFLICTS=() FFMPEG_DEPS=()
# Пакет, який зараз генерується (make_repack_package).
PKG_NAME='' PKG_VER='' PKG_REL='' PKG_DESC='' PKG_URL='' PKG_LICENSE='' PKG_TREE=''
declare -a PKG_DEPENDS=() PKG_OPTDEPENDS=() PKG_PROVIDES=() PKG_CONFLICTS=()

# ---------------------------------------------------------------------------
# Вивід
# ---------------------------------------------------------------------------
if [[ -t 2 ]]; then
  C_RED=$'\e[31m' C_GRN=$'\e[32m' C_YEL=$'\e[33m' C_BLU=$'\e[34m' C_CYN=$'\e[36m' C_BLD=$'\e[1m' C_RST=$'\e[0m'
else
  C_RED='' C_GRN='' C_YEL='' C_BLU='' C_CYN='' C_BLD='' C_RST=''
fi
# Код завершення, коли користувач сам відмовився (відповів «n»).
readonly EXIT_CANCELLED=10
# Префікси, які розбирає TUI (src/analyzer.rs) — мовою інтерфейсу.
TAG_WARN=$(t УВАГА WARNING) TAG_ERR=$(t ПОМИЛКА ERROR) TAG_HINT=$(t ПОРАДА HINT) TAG_FIX=$(t ВИПРАВИТИ FIX)
TAG_CANCEL=$(t СКАСОВАНО CANCELLED) TAG_SUMMARY=$(t Підсумок Summary)
readonly TAG_WARN TAG_ERR TAG_HINT TAG_FIX TAG_CANCEL TAG_SUMMARY
say()  { printf '%s==>%s %s\n' "$C_BLU$C_BLD" "$C_RST" "$*" >&2; }
ok()   { printf '%s[OK]%s %s\n' "$C_GRN" "$C_RST" "$*" >&2; }
warn() { printf '%s[%s]%s %s\n' "$C_YEL" "$TAG_WARN" "$C_RST" "$*" >&2; }
fail() { printf '%s[%s]%s %s\n' "$C_RED" "$TAG_ERR" "$C_RST" "$*" >&2; }
# Що робити далі. TUI показує пораду під останньою помилкою чи попередженням.
hint() { printf '%s[%s]%s %s\n' "$C_CYN" "$TAG_HINT" "$C_RST" "$*" >&2; }
# Що програма може виправити сама: «[ВИПРАВИТИ] команда: опис». TUI показує це
# кнопкою «Виправити» і запускає ту саму команду скрипта (лише з дозволених:
# deps, clean, check). Пишеться одразу після проблеми, до якої належить.
fix()  { printf '%s[%s]%s %s: %s\n' "$C_GRN" "$TAG_FIX" "$C_RST" "$1" "$2" >&2; }
# Підсумок запуску одним рядком («OBS 33.0.0 → 33.0.1»): TUI показує його першим у
# картці результату, тож видно, що саме змінилося.
summary() { printf '%s: %s\n' "$TAG_SUMMARY" "$*" >&2; }
# die "що сталося" ["що робити"]
die()  { fail "$1"; if (($# > 1)); then hint "$2"; fi; exit 1; }
# Користувач відмовився — це не помилка, нічого не змінено.
cancelled() {
  printf '%s[%s]%s %s\n' "$C_YEL" "$TAG_CANCEL" "$C_RST" "${1:-$(t 'Нічого не змінено.' 'Nothing was changed.')}" >&2
  exit "$EXIT_CANCELLED"
}

# Як поставити пакети для збірки: у TUI — кнопкою «Виправити», без нього — командою.
deps_action() {
  if [[ "${OBS_LEGACY_TUI:-}" == 1 ]]; then
    printf "$(t 'кнопкою «Виправити»' 'with the “Fix” button')"
  else
    printf "$(t 'командою %s deps' 'with the command %s deps')" "$SELF_PATH"
  fi
}

# Як запустити перевірку: у TUI — пунктом меню, без нього — командою.
check_action() {
  if [[ "${OBS_LEGACY_TUI:-}" == 1 ]]; then
    printf "$(t '«Перевірити систему»' '“Check system”')"
  else
    printf '%s check' "$SELF_PATH"
  fi
}

# Як запустити встановлення: у TUI — пунктом меню, без нього — через TUI
# (якщо він є) або командою.
install_action() {
  if [[ "${OBS_LEGACY_TUI:-}" == 1 ]]; then
    printf "$(t '«Встановити / оновити OBS»' '“Install / update OBS”')"
  elif have obs-legacy; then
    printf "$(t 'obs-legacy → «Встановити / оновити OBS»' 'obs-legacy → “Install / update OBS”')"
  else
    printf '%s install' "$SELF_PATH"
  fi
}

# Як перезібрати начисто: у TUI — пункт «Додатково», без нього — через TUI або командою.
rebuild_action() {
  if [[ "${OBS_LEGACY_TUI:-}" == 1 ]]; then
    printf "$(t '«Перезібрати OBS» (кнопка в діалозі оновлення або «Додатково»)' \
                '“Rebuild OBS” (a button in the update dialog or in “More”)')"
  elif have obs-legacy; then
    printf "$(t 'obs-legacy → Додатково → «Перезібрати OBS»' 'obs-legacy → More → “Rebuild OBS”')"
  else
    printf '%s rebuild' "$SELF_PATH"
  fi
}

on_error() {
  local rc=$? line=$1 cmd=$2
  fail "$(t "Команда завершилась з кодом $rc (рядок $line): $cmd" "Command failed with code $rc (line $line): $cmd")"
}
trap 'on_error "$LINENO" "$BASH_COMMAND"' ERR

have() { command -v "$1" >/dev/null 2>&1; }

# Відповідь без того, що потрапило в рядок мимохідь: коди стрілок і Esc (натиснутих
# під час довгого збирання), інші керівні символи, пробіли. LC_ALL=C: в українській
# локалі діапазон [@-~] не охоплює латинських літер, а кирилицю C не чіпає.
clean_answer() {   # $1 = рядок
  local esc=$'\e'
  printf '%s' "$1" | LC_ALL=C sed -E "s|${esc}\\[[0-9;?]*[ -/]*[@-~]||g; s|${esc}O.||g; s|[[:cntrl:]]||g; s|[[:space:]]||g"
}

# confirm "питання" → 0, якщо користувач відповів так. Enter — ні. Незрозуміла
# відповідь — питання ще раз: після години збирання випадкове натискання не має
# усе скасовувати. «т» і «н» неоднозначні («так» / «ні», але й клавіші N і Y в
# українській розкладці), тож для них — підказка й питання ще раз.
confirm() {
  local answer='' a
  while :; do
    printf '%s [y/N]: ' "$1" >&2
    if ! read -r answer; then
      printf '\n' >&2
      return 1
    fi
    a=$(clean_answer "$answer")
    case "$a" in
      [yY]|[yY][eE][sS]|так|Так|ТАК) return 0 ;;
      ''|[nN]|[nN][oO]|ні|Ні|НІ) return 1 ;;
      т|Т|н|Н)
        printf "$(t '  «%s» — схоже, увімкнена українська розкладка (клавіша Y дає «н», N — «т»). Введи y чи n латиницею або напиши «так» / «ні».\n' \
                    '  “%s” — looks like the Ukrainian keyboard layout is on (the Y key gives “н”, N gives “т”). Type y or n in the Latin layout, or write “yes” / “no”.\n')" "$a" >&2 ;;
      *) printf "$(t '  Не зрозумів відповідь «%s». Введи y (так) або n (ні).\n' \
                     '  Did not understand the answer “%s”. Type y (yes) or n (no).\n')" "$a" >&2 ;;
    esac
  done
}

usage() {
  local me
  me="$(basename -- "$0")"
  if ((LANG_EN)); then
    cat <<EOF
$SELF_NAME $SCRIPT_VERSION — OBS Studio with NVENC for NVIDIA 580.xx (Artix/Arch)
Part of OBS-Legacy (https://github.com/YellowHearth1/OBS-Legacy).
Made with Claude Code (Anthropic). Inspired by:
https://github.com/sl5net/build-obs-nvenc-legacy. No warranty — see LICENSE.

Usage: $me [command]

Commands:
  check              Check: system, hardware profile, driver, sources, CEF, plugins,
                     ffmpeg, CMake configuration. Installs nothing, no sudo needed.
  install            Build OBS, the selected plugins and ffmpeg for this computer, test NVENC,
                     pack them into pacman packages (-$PKG_SUFFIX) and install them.
  update             The same, but first check what is new (OBS, plugins, ffmpeg, hardware
                     profile, libraries) and build only when there is something to update.
  rebuild            Rebuild from scratch (the OBS build cache is cleared) and install again —
                     if the last build was interrupted (a power cut) or OBS behaves strangely.
  package            The same as install, but only build the packages (no installation).
  status             Installed packages, hardware profile, saved package sets, updates.
  rollback [SET]     Install the previous (or the given) saved package set.
  uninstall          Remove the OBS-Legacy packages (and the old build in /opt, if one is left).
  deps               Install only the build dependencies (pacman -S --needed after you confirm,
                     without a system upgrade). "deps --missing" shows what is missing.
  clean              Delete intermediate build files (sources, the CEF archive and package sets
                     are kept).
  plugins            The catalog of plugins that can be built together with OBS.
  help               This help.
Without a command — an interactive menu (the TUI is nicer: obs-legacy).

Settings: environment variables or the file $SETTINGS_FILE
(KEY=VALUE lines; an environment variable takes priority over the file).
  OBS_REF=stable          stable = the newest stable tag; or a tag (32.2.2),
                          a branch (master) or a commit
  CPU_TUNING=native       which CPU to build for: native (this one, fastest),
                          generic (like the repository), x86-64-v2 / v3 / v4
  OPT_LEVEL=2             optimization level: 2 (like the distribution) or 3
  BUILD_FFMPEG=1          build $FFMPEG_PKG (0 — keep the system ffmpeg)
  NV_HEADERS_TAG=auto     nv-codec-headers tag; auto — matches the driver's NVENC API
                          (580xx → $NV_HEADERS_DEFAULT)
  CEF_BUILD=7871          CEF version; by default it is taken from the OBS sources
                          (CMakePresets.json or buildspec.json) with the official SHA-256
  CEF_ARCHIVE_URL=URL     exact link to the CEF archive
  CEF_SHA256=HASH         expected SHA-256 of the CEF archive
  OBS_PLUGINS="a,b"       plugins to build (see the plugins command); none — no plugins.
                          Default: obs-multi-rtmp
  PLUGIN_REF_<id>=TAG     pin a plugin version (replace "-" in the id with "_")
  PLUGINS_STRICT=1        cancel the installation if any plugin fails to build
  INSTALL_PACMAN_HOOK=0   do not put the library-check pacman hook into the OBS package
  BUILD_JOBS=4            compile jobs (by default chosen with RAM in mind)
  KEEP_VERSIONS=2         how many package sets to keep for rollback (with the active one)
  EXTRA_CMAKE_ARGS="..."  extra CMake options, e.g. "-DENABLE_WEBSOCKET=OFF"
  FORCE_CLEAN_BUILD=1     do not reuse the previous build to speed things up
  SKIP_NVENC_TEST=1       NOT recommended: install without a passed obs-nvenc-test
  ALLOW_MISSING_LIBS=1    NOT recommended: install even though ldd found missing libraries
  UI_LANG=auto            message language: auto (from the system locale), uk or en

Examples:
  $me check
  $me install
  OBS_REF=32.2.2 $me install      # pin a specific OBS version
  OBS_PLUGINS="obs-multi-rtmp,move-transition" $me install
  CPU_TUNING=generic $me package  # portable packages (for another computer)
  $me rollback

Files:
  Packages: $OBS_PKG, <plugin>-$PKG_SUFFIX, $FFMPEG_PKG (pacman -Qs $PKG_SUFFIX)
  Sets:     $PKG_STORE/<label>/
  Cache:    $BUILD_ROOT   (reports: $REPORT_DIR, plugins: $PLUGINS_DIR)
  State:    $STATE_DIR   (OBS settings backups: config-backups/)
  Hook:     $PKG_HOOK → $PKG_LIBCHECK
EOF
    return
  fi
  cat <<EOF
$SELF_NAME $SCRIPT_VERSION — OBS Studio з NVENC для NVIDIA 580.xx (Artix/Arch)
Частина OBS-Legacy (https://github.com/YellowHearth1/OBS-Legacy).
Створено за допомогою Claude Code (Anthropic). Натхнення:
https://github.com/sl5net/build-obs-nvenc-legacy. Без гарантій — див. LICENSE.

Використання: $me [команда]

Команди:
  check              Перевірка: система, профіль заліза, драйвер, вихідні коди, CEF, плагіни,
                     ffmpeg, конфігурація CMake. Нічого не встановлює, sudo не потрібен.
  install            Зібрати OBS, вибрані плагіни й ffmpeg під цей комп'ютер, перевірити NVENC,
                     запакувати в пакети pacman (-$PKG_SUFFIX) і встановити їх.
  update             Те саме, але спершу перевірити, що нового (OBS, плагіни, ffmpeg, профіль
                     заліза, бібліотеки), і збирати лише тоді, коли є що оновлювати.
  rebuild            Перезібрати начисто (кеш збірки OBS очищається) і встановити знову — якщо
                     минуле збирання обірвалося (вимкнулося світло) чи OBS поводиться дивно.
  package            Те саме, що install, але лише зібрати пакети (без встановлення).
  status             Встановлені пакети, профіль заліза, збережені набори, оновлення.
  rollback [НАБІР]   Поставити попередній (або вказаний) збережений набір пакетів.
  uninstall          Прибрати пакети OBS-Legacy (і стару збірку з /opt, якщо лишилася).
  deps               Встановити лише пакети для збірки (pacman -S --needed після підтвердження,
                     без оновлення системи). «deps --missing» — показати, чого бракує.
  clean              Видалити проміжні файли збірки (вихідні коди, архів CEF і набори пакетів
                     лишаються).
  plugins            Каталог плагінів, які можна зібрати разом з OBS.
  help               Ця довідка.
Без команди — інтерактивне меню (зручніше — TUI: obs-legacy).

Налаштування: змінні середовища або файл $SETTINGS_FILE
(рядки KEY=VALUE; змінна середовища має пріоритет над файлом).
  OBS_REF=stable          stable = найновіший стабільний тег; або тег (32.2.2),
                          гілка (master) чи коміт
  CPU_TUNING=native       під який процесор збирати: native (цей, найшвидше),
                          generic (як у репозиторії), x86-64-v2 / v3 / v4
  OPT_LEVEL=2             рівень оптимізації: 2 (як у дистрибутива) або 3
  BUILD_FFMPEG=1          збирати $FFMPEG_PKG (0 — лишити системний ffmpeg)
  NV_HEADERS_TAG=auto     тег nv-codec-headers; auto — під NVENC API драйвера
                          (580xx → $NV_HEADERS_DEFAULT)
  CEF_BUILD=7871          версія CEF; за замовчуванням береться з вихідних кодів OBS
                          (CMakePresets.json або buildspec.json) разом з офіційним SHA-256
  CEF_ARCHIVE_URL=URL     точне посилання на архів CEF
  CEF_SHA256=HASH         очікуваний SHA-256 архіву CEF
  OBS_PLUGINS="a,b"       плагіни для збирання (див. команду plugins); none — без плагінів.
                          За замовчуванням: obs-multi-rtmp
  PLUGIN_REF_<id>=ТЕГ     зафіксувати версію плагіна ("-" в id замінити на "_")
  PLUGINS_STRICT=1        скасувати встановлення, якщо хоч один плагін не зібрався
  INSTALL_PACMAN_HOOK=0   не класти в пакет OBS pacman-хук перевірки бібліотек
  BUILD_JOBS=4            потоки компіляції (за замовчуванням — з урахуванням RAM)
  KEEP_VERSIONS=2         скільки наборів пакетів тримати для відкату (разом з активним)
  EXTRA_CMAKE_ARGS="..."  додаткові параметри CMake, напр. "-DENABLE_WEBSOCKET=OFF"
  FORCE_CLEAN_BUILD=1     не використовувати попередню збірку для пришвидшення
  SKIP_NVENC_TEST=1       НЕ рекомендовано: встановити без пройденого obs-nvenc-test
  ALLOW_MISSING_LIBS=1    НЕ рекомендовано: встановити, хоча ldd знайшов відсутні бібліотеки
  UI_LANG=auto            мова повідомлень: auto (за локаллю системи), uk або en

Приклади:
  $me check
  $me install
  OBS_REF=32.2.2 $me install      # зафіксувати конкретну версію OBS
  OBS_PLUGINS="obs-multi-rtmp,move-transition" $me install
  CPU_TUNING=generic $me package  # переносні пакети (для іншого комп'ютера)
  $me rollback

Файли:
  Пакети:  $OBS_PKG, <плагін>-$PKG_SUFFIX, $FFMPEG_PKG (pacman -Qs $PKG_SUFFIX)
  Набори:  $PKG_STORE/<мітка>/
  Кеш:     $BUILD_ROOT   (звіти: $REPORT_DIR, плагіни: $PLUGINS_DIR)
  Стан:    $STATE_DIR   (копії налаштувань OBS: config-backups/)
  Хук:     $PKG_HOOK → $PKG_LIBCHECK
EOF
}

# ---------------------------------------------------------------------------
# Допоміжні функції
# ---------------------------------------------------------------------------
existing_ancestor() {
  local p=$1
  while [[ ! -e "$p" && "$p" != / ]]; do p=$(dirname -- "$p"); done
  printf '%s\n' "$p"
}
free_gib() {
  local p
  p=$(existing_ancestor "$1")
  df -Pk -- "$p" 2>/dev/null | awk 'NR == 2 { printf "%d\n", $4 / 1048576 }' || true
}
mem_avail_gib() {
  awk '/^MemAvailable:/ { printf "%d\n", $2 / 1048576 }' /proc/meminfo 2>/dev/null || echo 0
}
sanitize_label() { printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'; }

# Видаляє лише шляхи всередині кешу скрипта.
rm_cache_path() {
  local p
  for p in "$@"; do
    [[ "$p" == "$BUILD_ROOT"/?* && "$p" != *'/..'* ]] \
      || die "$(t "Внутрішня помилка: спроба видалити шлях поза кешем: $p" \
                  "Internal error: an attempt to delete a path outside the cache: $p")"
    rm -rf -- "$p"
  done
}

# Видаляє лише набори пакетів у сховищі.
rm_store_path() {
  local p
  for p in "$@"; do
    [[ "$p" == "$PKG_STORE"/?* && "$p" != *'/..'* ]] \
      || die "$(t "Внутрішня помилка: спроба видалити шлях поза сховищем пакетів: $p" \
                  "Internal error: an attempt to delete a path outside the package store: $p")"
    rm -rf -- "$p"
  done
}

priv() { "$SUDO_CMD" "$@"; }

# Якщо pacman обірвало (вимкнулося світло, перезавантаження), лишається його
# блокування db.lck, і pacman більше не працює, доки його не прибрати.
check_pacman_lock() {
  local lck
  lck="$(pacman-conf DBPath 2>/dev/null || printf '/var/lib/pacman/')"
  lck="${lck%/}/db.lck"
  [[ -e "$lck" ]] || return 0
  if pgrep -x pacman >/dev/null 2>&1; then
    die "$(t "Зараз працює інший pacman (оновлення системи чи встановлення пакетів)." \
             "Another pacman is running right now (a system upgrade or a package installation).")" \
      "$(t "Дочекайся, поки він завершиться, і повтори." "Wait until it finishes and try again.")"
  fi
  die "$(t "pacman заблоковано: лишився файл $lck, хоча сам pacman не працює (мабуть, минулого разу його обірвало вимкнення світла чи перезавантаження)." \
           "pacman is locked: the file $lck is left over, although pacman itself is not running (probably a power cut or a reboot interrupted it last time).")" \
    "$(t "Переконайся, що ніде не йде оновлення системи, прибери блокування: sudo rm $lck — і повтори. Якщо pacman обірвався посеред встановлення OBS-Legacy, потім обери $(rebuild_action)." \
         "Make sure no system upgrade is running anywhere, remove the lock: sudo rm $lck — and try again. If pacman was interrupted in the middle of an OBS-Legacy installation, then choose $(rebuild_action).")"
}
priv_validate() {
  if [[ "$SUDO_CMD" == sudo ]]; then sudo -v; else doas true; fi
}
# Підказка, якщо sudo/doas не пустив.
PRIV_HINT="$(t "Перевір пароль. Твій користувач має бути в групі wheel (для sudo) або мати дозвіл у /etc/doas.conf (для doas)." \
               "Check the password. Your user must be in the wheel group (for sudo) or be allowed in /etc/doas.conf (for doas).")"
readonly PRIV_HINT
select_priv_tool() {
  if have sudo; then SUDO_CMD=sudo
  elif have doas; then SUDO_CMD=doas
  else die "$(t "Не знайдено ні sudo, ні doas." "Neither sudo nor doas was found.")" \
           "$(t "Встанови один із них: pacman -S sudo (від root) і додай свого користувача до групи wheel." \
                "Install one of them: pacman -S sudo (as root) and add your user to the wheel group.")"
  fi
}

preflight_common() {
  (( EUID != 0 )) || die "$(t "Скрипт запущено від root або через sudo." \
                              "The script was started as root or via sudo.")" \
    "$(t "Запусти його від свого звичайного користувача: пароль sudo/doas скрипт попросить сам, коли треба." \
         "Run it as your normal user: the script asks for the sudo/doas password itself when needed.")"
  [[ "$(uname -m)" == x86_64 ]] || die "$(t "Підтримується лише x86_64." "Only x86_64 is supported.")"
  have pacman || die "$(t "Не знайдено pacman: потрібен Artix/Arch або сумісний дистрибутив." \
                          "pacman was not found: Artix/Arch or a compatible distribution is required.")"
  [[ -n "${HOME:-}" && -d "$HOME" ]] || die "$(t "Змінна HOME не задана або каталог не існує." \
                                                 "The HOME variable is not set or the directory does not exist.")"
  [[ "$INSTALL_ROOT" =~ ^/opt/[A-Za-z0-9._-]+$ ]] || die "$(t "INSTALL_ROOT має бути виду /opt/назва (зараз: $INSTALL_ROOT)." \
                                                              "INSTALL_ROOT must look like /opt/name (now: $INSTALL_ROOT).")"
  [[ "$BUILD_ROOT" == /?*/?* ]] || die "$(t "Некоректний шлях кешу: $BUILD_ROOT" "Invalid cache path: $BUILD_ROOT")"
  [[ "$KEEP_VERSIONS" =~ ^[1-9][0-9]*$ ]] || die "$(t "KEEP_VERSIONS має бути додатним цілим." \
                                                      "KEEP_VERSIONS must be a positive integer.")"
  if [[ -n "$BUILD_JOBS" && ! "$BUILD_JOBS" =~ ^[1-9][0-9]*$ ]]; then
    die "$(t "BUILD_JOBS має бути додатним цілим." "BUILD_JOBS must be a positive integer.")"
  fi
  case "$CPU_TUNING" in
    native|generic|x86-64-v2|x86-64-v3|x86-64-v4) ;;
    *) die "$(t "Невідоме значення CPU_TUNING=$CPU_TUNING." "Unknown value CPU_TUNING=$CPU_TUNING.")" \
         "$(t "У Налаштуваннях → «Під який процесор» вкажи native (рекомендовано), generic або x86-64-v2/v3/v4." \
              "In Settings → “Tune for CPU” choose native (recommended), generic or x86-64-v2/v3/v4.")" ;;
  esac
  [[ "$OPT_LEVEL" =~ ^[23]$ ]] || die "$(t "OPT_LEVEL має бути 2 або 3 (зараз: $OPT_LEVEL)." \
                                           "OPT_LEVEL must be 2 or 3 (now: $OPT_LEVEL).")"
}

# Пакет pacman, з якого береться програма.
tool_package() {
  case "$1" in
    pkg-config) echo pkgconf ;;
    sha256sum|timeout|nice|od|df) echo coreutils ;;
    flock) echo util-linux ;;
    find) echo findutils ;;
    awk) echo gawk ;;
    ldd) echo glibc ;;
    strip|readelf) echo binutils ;;
    makepkg|vercmp) echo pacman ;;
    gpg) echo gnupg ;;
    *) echo "$1" ;;
  esac
}

require_tools() {
  local t
  local -a missing=() pkgs=()
  for t in "$@"; do
    if ! have "$t"; then missing+=("$t"); pkgs+=("$(tool_package "$t")"); fi
  done
  if ((${#missing[@]})); then
    mapfile -t pkgs < <(printf '%s\n' "${pkgs[@]}" | sort -u)
    fail "$(t "Бракує програм: ${missing[*]}." "Missing programs: ${missing[*]}.")"
    hint "$(t "Постав пакети для збірки $(deps_action) (або sudo pacman -S --needed ${pkgs[*]}) і повтори." \
              "Install the build dependencies $(deps_action) (or sudo pacman -S --needed ${pkgs[*]}) and try again.")"
    fix deps "$(t "Встановити пакети для збірки" "Install the build dependencies")"
    exit 1
  fi
}

acquire_lock() {
  mkdir -p -- "$BUILD_ROOT"
  exec 9>"$BUILD_ROOT/.lock"
  flock -n 9 || die "$(t "Скрипт уже працює в іншому вікні (lock: $BUILD_ROOT/.lock)." \
                         "The script is already running in another window (lock: $BUILD_ROOT/.lock).")" \
    "$(t "Дочекайся, поки той запуск завершиться, або закрий його вікно і повтори." \
         "Wait until that run finishes, or close its window, and try again.")"
}

# Стара схема: збірки в /opt/obs-studio-nvenc (створені цим скриптом до пакетів).
legacy_install_present() { [[ -f "$ROOT_MARK" ]]; }

# Чи лишилося щось від старої схеми (збірки в /opt, обгортка, ярлик, хук).
legacy_leftovers_present() {
  local desktop="$DATA_HOME/applications/$DESKTOP_ID"
  legacy_install_present \
    || grep -qs -- "$MARKER" "$WRAPPER" \
    || grep -qs -- "$MARKER" "$HOOK_FILE" \
    || grep -qs -- "$MARKER" "$desktop"
}

# Чужий /opt/... не чіпаємо: тепер OBS-Legacy ставиться пакетами в /usr, тож
# каталог лише не прибирається.
detect_foreign_install() {
  if [[ ! -e "$INSTALL_ROOT" && ! -L "$INSTALL_ROOT" ]] || legacy_install_present; then return 0; fi
  warn "$(t "$INSTALL_ROOT існує, але створений не цим скриптом — не чіпаю його." \
            "$INSTALL_ROOT exists but was not created by this script — leaving it alone.")"
}

print_header() {
  printf '\n%s%s %s — %s%s\n' "$C_BLD" "$SELF_NAME" "$SCRIPT_VERSION" "$1" "$C_RST" >&2
  printf "$(t '  Дата: %s\n  OBS_REF=%s  NV_HEADERS_TAG=%s  CEF_BUILD=%s  CPU_TUNING=%s  OPT_LEVEL=%s  BUILD_FFMPEG=%s\n' \
              '  Date: %s\n  OBS_REF=%s  NV_HEADERS_TAG=%s  CEF_BUILD=%s  CPU_TUNING=%s  OPT_LEVEL=%s  BUILD_FFMPEG=%s\n')" \
    "$(date '+%F %T')" "$OBS_REF" "$NV_HEADERS_TAG" "${CEF_BUILD:-auto}" "$CPU_TUNING" "$OPT_LEVEL" "$BUILD_FFMPEG" >&2
  printf "$(t '  Пакети: %s, <плагін>-%s%s → %s\n  Кеш: %s\n\n' \
              '  Packages: %s, <plugin>-%s%s → %s\n  Cache: %s\n\n')" "$OBS_PKG" "$PKG_SUFFIX" \
    "$( [[ "$BUILD_FFMPEG" == 1 ]] && printf ', %s' "$FFMPEG_PKG" )" "$PREFIX" "$BUILD_ROOT" >&2
}

# ---------------------------------------------------------------------------
# Інвентаризація системи
# ---------------------------------------------------------------------------
# Питає в драйвера максимальну версію NVENC API (як OBS під час запуску).
nvenc_driver_api() {
  local cc='' src="$TOOLS_DIR/nvenc-api-probe.c" bin="$TOOLS_DIR/nvenc-api-probe"
  if have cc; then cc=cc; elif have gcc; then cc=gcc; else return 1; fi
  mkdir -p -- "$TOOLS_DIR"
  cat > "$src" <<'EOF'
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
typedef int (*get_max_ver_fn)(uint32_t *);
int main(void)
{
    void *lib = dlopen("libnvidia-encode.so.1", RTLD_NOW | RTLD_LOCAL);
    if (!lib)
        return 2;
    get_max_ver_fn fn = (get_max_ver_fn)dlsym(lib, "NvEncodeAPIGetMaxSupportedVersion");
    if (!fn)
        return 3;
    uint32_t v = 0;
    if (fn(&v) != 0)
        return 4;
    printf("%u.%u\n", v >> 4, v & 0xF);
    return 0;
}
EOF
  "$cc" -O1 -o "$bin" "$src" >/dev/null 2>&1 || "$cc" -O1 -o "$bin" "$src" -ldl >/dev/null 2>&1 || return 1
  "$bin" 2>/dev/null
}

api_to_int() {
  [[ "$1" =~ ^([0-9]+)\.([0-9]+)$ ]] || return 1
  printf '%d\n' $(( BASH_REMATCH[1] * 16 + BASH_REMATCH[2] ))
}

current_label() {
  local t
  t=$(readlink -- "$CURRENT_LINK" 2>/dev/null) || return 1
  printf '%s\n' "${t##*/}"
}

show_inventory() {
  local os drv cur p
  say "$(t "Система" "System")"
  os=$( (. /etc/os-release 2>/dev/null || true; printf '%s' "${PRETTY_NAME:-$(t невідомо unknown)}") )
  printf "$(t '  ОС: %s; ядро %s; init: %s\n' \
              '  OS: %s; kernel %s; init: %s\n')" "$os" "$(uname -r)" "$(ps -p 1 -o comm= 2>/dev/null || echo '?')" >&2
  printf "$(t '  CPU: %s потоків; доступно RAM: %s GiB\n' \
              '  CPU: %s threads; RAM available: %s GiB\n')" "$(nproc 2>/dev/null || echo '?')" "$(mem_avail_gib)" >&2
  printf "$(t '  Вільно: кеш %s GiB, %s %s GiB\n' \
              '  Free: cache %s GiB, %s %s GiB\n')" "$(free_gib "$BUILD_ROOT")" "$PREFIX" "$(free_gib "$PREFIX")" >&2

  say "NVIDIA"
  if have nvidia-smi; then
    nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null | sed "$(t 's/^/  GPU, драйвер: /' \
                                                                                            's/^/  GPU, driver: /')" >&2 \
      || warn "$(t "nvidia-smi не відповідає." "nvidia-smi is not responding.")"
  else
    warn "$(t "nvidia-smi не знайдено." "nvidia-smi was not found.")"
  fi
  drv=$(nvenc_driver_api || true)
  if [[ -n "$drv" ]]; then
    printf "$(t '  Драйвер підтримує NVENC API до: %s\n' '  The driver supports NVENC API up to: %s\n')" "$drv" >&2
  else
    warn "$(t "Не вдалося запитати версію NVENC API в драйвера (немає libnvidia-encode.so.1 або компілятора)." \
              "Could not ask the driver for its NVENC API version (no libnvidia-encode.so.1 or no compiler).")"
  fi

  say "OBS"
  # pacman -Q obs-studio знаходить і пакет, що його «надає» (obs-studio-git),
  # тож без sort -u той самий пакет друкувався б двічі.
  for p in obs-studio "$OBS_PKG" obs-studio-git obs-studio-browser obs-studio-tytan652 ffmpeg "$FFMPEG_PKG"; do
    pacman -Q "$p" 2>/dev/null || true
  done | sort -u | sed "$(t 's/^/  Пакет pacman: /' 's/^/  pacman package: /')" >&2
  cur=$(installed_label || true)
  printf '  OBS-Legacy: %s\n' "${cur:-$(t 'ще не встановлено' 'not installed yet')}" >&2
  cur=$(current_label || true)
  if [[ -n "$cur" ]]; then
    printf "$(t '  Стара збірка в %s: %s (прибереться після встановлення пакетів)\n' \
                '  Old build in %s: %s (it will be removed after the packages are installed)\n')" "$INSTALL_ROOT" "$cur" >&2
  fi
  printf "$(t '  Команда obs → %s\n' \
              '  Command obs → %s\n')" "$(command -v obs || echo "$(t 'не знайдена' 'not found')")" >&2
}

# Мітка встановленої збірки OBS-Legacy (з BUILDINFO у пакеті).
installed_label() {
  sed -n 's/^label=//p' "$INFO_DIR/BUILDINFO.txt" 2>/dev/null | head -n1 | grep . || return 1
}

installed_info() {   # $1 = ключ BUILDINFO встановленої збірки
  sed -n "s/^$1=//p" "$INFO_DIR/BUILDINFO.txt" 2>/dev/null | head -n1 || true
}

# ---------------------------------------------------------------------------
# Профіль заліза: параметри збирання саме під цей комп'ютер
# ---------------------------------------------------------------------------
# Змінна з makepkg.conf дистрибутива (з урахуванням makepkg.conf.d і файла
# користувача) — ті самі прапорці, з якими зібрано пакети з репозиторію.
makepkg_var() {   # $1 = CFLAGS|CXXFLAGS|LDFLAGS|…
  bash -c '
    source /etc/makepkg.conf >/dev/null 2>&1 || true
    for c in /etc/makepkg.conf.d/*.conf; do
      if [[ -r $c ]]; then source "$c" >/dev/null 2>&1 || true; fi
    done
    u="${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf"
    if [[ -r $u ]]; then source "$u" >/dev/null 2>&1 || true
    elif [[ -r $HOME/.makepkg.conf ]]; then source "$HOME/.makepkg.conf" >/dev/null 2>&1 || true; fi
    printf "%s" "${!1-}"' _ "$1" 2>/dev/null | tr -s ' \t\n' '   ' | sed 's/^ //; s/ $//'
}

# Прапорці дистрибутива з нашими -march/-mtune/-O. Прапорці лише для
# профілювання (вказівник кадру) прибираються: вони сповільнюють код.
tune_flags() {   # $1 = базові прапорці
  local f
  local -a in=() out=()
  read -r -a in <<<"$1"
  for f in "${in[@]}"; do
    case "$f" in
      -march=*|-mtune=*|-O|-O[0-9s]|-Ofast|-Og|-Oz) continue ;;
      -fno-omit-frame-pointer|-mno-omit-leaf-frame-pointer) continue ;;
    esac
    out+=("$f")
  done
  printf '%s\n' "$MARCH_FLAGS -O$OPT_LEVEL ${out[*]}" | tr -s ' ' | sed 's/^ //; s/ $//'
}

# Рівні x86-64, які підтримує процесор (за glibc), найвищий — останній.
isa_levels() {
  local ld
  for ld in /usr/lib/ld-linux-x86-64.so.2 /lib64/ld-linux-x86-64.so.2; do
    if [[ -x "$ld" ]]; then
      "$ld" --help 2>/dev/null | sed -nE 's/^[[:space:]]*(x86-64-v[0-9]+) \(supported.*/\1/p' | sort -V
      return 0
    fi
  done
}

gpu_arch_name() {   # $1 = compute capability (6.1) → покоління
  case "$1" in
    3.*) echo Kepler ;; 5.*) echo Maxwell ;; 6.*) echo Pascal ;; 7.0|7.2) echo Volta ;;
    7.5) echo Turing ;; 8.0|8.6|8.7) echo Ampere ;; 8.9) echo Ada ;; 9.*) echo Hopper ;;
    10.*|12.*) echo Blackwell ;; *) echo '' ;;
  esac
}

detect_build_profile() {
  local base_c base_cxx base_ld line cc lvl f prev_cpu prev_tuning
  say "$(t "Оптимізація під залізо" "Hardware optimization")"
  DISTRO_ID=$( (. /etc/os-release 2>/dev/null || true; printf '%s' "${ID:-linux}") )
  DISTRO_NAME=$( (. /etc/os-release 2>/dev/null || true; printf '%s' "${PRETTY_NAME:-${NAME:-Linux}}") )
  base_c=$(makepkg_var CFLAGS) base_cxx=$(makepkg_var CXXFLAGS) base_ld=$(makepkg_var LDFLAGS)
  if [[ -z "$base_c" ]]; then
    base_c="-O2 -pipe -fno-plt" base_cxx="-O2 -pipe -fno-plt" base_ld="-Wl,-O1 -Wl,--as-needed"
    warn "$(t "Не вдалося прочитати /etc/makepkg.conf — беру прості прапорці ($base_c)." \
              "Could not read /etc/makepkg.conf — using simple flags ($base_c).")"
  fi

  CPU_MODEL=$(sed -n 's/^model name[[:space:]]*:[[:space:]]*//p' /proc/cpuinfo 2>/dev/null | head -n1)
  CPU_MARCH=$(gcc -march=native -Q --help=target 2>/dev/null | awk '$1 == "-march=" { print $2; exit }' || true)
  ISA_LEVEL=$(isa_levels | tail -n1 || true)
  CPU_FEATURES=''
  line=$(grep -m1 '^flags' /proc/cpuinfo 2>/dev/null || true)
  for f in sse4_2 avx avx2 fma avx512f; do
    if [[ " $line " == *" $f "* ]]; then CPU_FEATURES+="${CPU_FEATURES:+ }${f/sse4_2/SSE4.2}"; fi
  done
  CPU_FEATURES=${CPU_FEATURES^^}

  case "$CPU_TUNING" in
    native)  MARCH_FLAGS="-march=native -mtune=native" ;;
    generic) MARCH_FLAGS=$(grep -oE -- '-m(arch|tune)=[^ ]+' <<<"$base_c" | tr '\n' ' ' || true) ;;
    *)
      if [[ " $(isa_levels | tr '\n' ' ') " != *" $CPU_TUNING "* ]]; then
        die "$(t "Процесор не підтримує $CPU_TUNING (підтримує: $(isa_levels | tr '\n' ' '))." \
                 "The CPU does not support $CPU_TUNING (it supports: $(isa_levels | tr '\n' ' ')).")" \
          "$(t "Налаштування → «Під який процесор»: native (рекомендовано) або рівень, не вищий за ${ISA_LEVEL:-x86-64-v1}." \
               "Settings → “Tune for CPU”: native (recommended) or a level no higher than ${ISA_LEVEL:-x86-64-v1}.")"
      fi
      MARCH_FLAGS="-march=$CPU_TUNING" ;;
  esac
  OPT_CFLAGS=$(tune_flags "$base_c")
  OPT_CXXFLAGS=$(tune_flags "${base_cxx:-$base_c}")
  OPT_LDFLAGS=$base_ld

  GPU_NAME='' GPU_CC='' DRIVER_VERSION=''
  if have nvidia-smi; then
    local -a gpus=()
    local name drv
    mapfile -t gpus < <(timeout 15 nvidia-smi --query-gpu=name,driver_version,compute_cap --format=csv,noheader 2>/dev/null || true)
    if [[ "${gpus[0]:-}" != *','*','* ]]; then
      # Старі драйвери не знають compute_cap.
      mapfile -t gpus < <(timeout 15 nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null || true)
    fi
    # Кілька відеокарт: CUDA-фільтри ffmpeg збираються в PTX, а драйвер компілює його під кожну
    # відеокарту під час роботи — тож беремо найстаршу архітектуру, щоб фільтри працювали на всіх.
    for line in "${gpus[@]}"; do
      IFS=',' read -r name drv cc <<<"$line"
      name=$(sed 's/^ *//; s/ *$//' <<<"$name") cc=${cc// /}
      if [[ -z "$GPU_NAME" ]]; then GPU_NAME=$name DRIVER_VERSION=${drv// /}; fi
      if [[ "$cc" =~ ^[0-9]+\.[0-9]+$ ]] && { [[ -z "$GPU_CC" ]] || [[ "$(printf '%s\n' "$cc" "$GPU_CC" | sort -V | sed -n 1p)" == "$cc" ]]; }; then
        GPU_CC=$cc
      fi
    done
    if (( ${#gpus[@]} > 1 )); then GPU_NAME+="$(t " і ще $(( ${#gpus[@]} - 1 ))" \
                                                  " and $(( ${#gpus[@]} - 1 )) more")"; fi
  fi
  GPU_ARCH=$(gpu_arch_name "$GPU_CC")
  DRIVER_PKG=$(pacman -Qqo /usr/lib/libnvidia-encode.so.1 2>/dev/null | head -n1 || true)
  JOBS=$(compute_jobs)
  PROFILE_SIG=$(printf '%s\n' "$CPU_TUNING" "$CPU_MARCH" "$OPT_LEVEL" "$OPT_CFLAGS" "$OPT_CXXFLAGS" "$OPT_LDFLAGS" "$GPU_CC" \
    | sha256sum | cut -c1-12)

  printf "$(t '  Дистрибутив: %s (%s) — базові прапорці з makepkg.conf, як у пакетів з репозиторію\n' \
              '  Distribution: %s (%s) — base flags from makepkg.conf, like the repository packages\n')" "$DISTRO_NAME" "$DISTRO_ID" >&2
  case "$CPU_TUNING" in
    native)  lvl="$(t "під цей процесор: ${CPU_MARCH:-native}" "for this CPU: ${CPU_MARCH:-native}")" ;;
    generic) lvl="$(t "переносно, як у репозиторії (${MARCH_FLAGS:-без -march})" \
                      "portable, like the repository (${MARCH_FLAGS:-no -march})")" ;;
    *)       lvl="$(t "рівень $CPU_TUNING" "level $CPU_TUNING")" ;;
  esac
  printf "$(t '  Процесор: %s → %s\n' '  CPU: %s → %s\n')" "${CPU_MODEL:-$(t невідомо unknown)}" "$lvl" >&2
  printf "$(t '  Розширення: %s · найвищий рівень ISA: %s\n' \
              '  Extensions: %s · highest ISA level: %s\n')" "${CPU_FEATURES:-—}" "${ISA_LEVEL:-x86-64}" >&2
  if [[ -n "$GPU_NAME" ]]; then
    printf "$(t '  Відеокарта: %s%s · драйвер %s%s\n' '  GPU: %s%s · driver %s%s\n')" "$GPU_NAME" \
      "${GPU_CC:+ (${GPU_ARCH:+$GPU_ARCH, }CUDA $GPU_CC)}" "${DRIVER_VERSION:-?}" "${DRIVER_PKG:+ ($DRIVER_PKG)}" >&2
  else
    warn "$(t "nvidia-smi не показав відеокарту — CUDA-архітектуру для ffmpeg не визначено." \
              "nvidia-smi did not show a GPU — the CUDA architecture for ffmpeg is unknown.")"
  fi
  printf '  C/C++: %s\n' "$OPT_CFLAGS" >&2
  printf "$(t '  Компонування: %s\n' '  Linking: %s\n')" "${OPT_LDFLAGS:-—}" >&2
  printf "$(t '  Потоки збирання: %s (CPU: %s, вільна RAM: %s GiB)\n' \
              '  Build jobs: %s (CPU: %s, free RAM: %s GiB)\n')" "$JOBS" "$(nproc 2>/dev/null || echo '?')" "$(mem_avail_gib)" >&2

  # Збірку під конкретний процесор не варто запускати на іншому.
  prev_cpu=$(installed_info cpu_model) prev_tuning=$(installed_info cpu_tuning)
  if [[ "$prev_tuning" == native && -n "$prev_cpu" && -n "$CPU_MODEL" && "$prev_cpu" != "$CPU_MODEL" ]]; then
    warn "$(t "Встановлений OBS-Legacy зібрано під інший процесор ($prev_cpu)." \
              "The installed OBS-Legacy was built for a different CPU ($prev_cpu).")"
    hint "$(t "Перезбери його $(install_action): на цьому процесорі стара збірка може аварійно завершуватися." \
              "Rebuild it via $(install_action): the old build may crash on this CPU.")"
  fi
  case "$CPU_TUNING" in
    native)  lvl="$(t "усе збирається саме під цей комп'ютер" "everything is built for exactly this computer")" ;;
    generic) lvl="$(t "переносні пакети, як у репозиторії" "portable packages, like the repository")" ;;
    *)       lvl="$(t "пакети для процесорів рівня $CPU_TUNING і новіших" \
                      "packages for CPUs of level $CPU_TUNING and newer")" ;;
  esac
  ok "$(t "Профіль: ${CPU_TUNING/native/${CPU_MARCH:-native}} · -O$OPT_LEVEL${GPU_CC:+ · CUDA sm_${GPU_CC/./}} — $lvl." \
          "Profile: ${CPU_TUNING/native/${CPU_MARCH:-native}} · -O$OPT_LEVEL${GPU_CC:+ · CUDA sm_${GPU_CC/./}} — $lvl.")"
}

ffmpeg_nvenc_info() {
  local who
  have ffmpeg || return 0
  who=$(pacman -Qqo "$(command -v ffmpeg)" 2>/dev/null || echo ffmpeg)
  say "$(t "Довідково: NVENC у ffmpeg ($who)" "For reference: NVENC in ffmpeg ($who)")"
  if timeout 20 ffmpeg -hide_banner -loglevel error -nostdin -f lavfi -i testsrc2=size=640x360:rate=30 \
       -frames:v 5 -an -c:v h264_nvenc -f null - >/dev/null 2>&1; then
    ok "$(t "ffmpeg кодує через NVENC." "ffmpeg encodes via NVENC.")"
  elif [[ "$who" == "$FFMPEG_PKG" ]]; then
    warn "$(t "$FFMPEG_PKG не кодує через NVENC (драйвер не завантажено чи NVENC зайнятий?)." \
              "$FFMPEG_PKG does not encode via NVENC (is the driver not loaded or is NVENC busy?).")"
  else
    # Це очікувано і ні на що не впливає, тож не попередження, а примітка.
    printf "$(t '  Системний ffmpeg не кодує через NVENC — так і має бути: його зібрано з SDK 13.1.\n' \
                '  The system ffmpeg does not encode via NVENC — as expected: it is built with SDK 13.1.\n')" >&2
    if [[ "$BUILD_FFMPEG" == 1 ]]; then
      printf "$(t '  Це виправить %s (збирається разом з OBS). Сам OBS кодує NVENC власним модулем.\n' \
                  '  %s fixes this (it is built together with OBS). OBS itself encodes with NVENC through its own module.\n')" "$FFMPEG_PKG" >&2
    else
      printf "$(t '  На OBS-Legacy це не впливає: OBS має власний модуль NVENC.\n' \
                  '  This does not affect OBS-Legacy: OBS has its own NVENC module.\n')" >&2
    fi
  fi
}

check_disk_space() {
  local c o need=10
  if [[ -d "$BUILD_DIR" ]]; then need=5; fi
  # ffmpeg: git-репозиторій (~0,5 ГБ, один раз) і збирання з LTO (~3 ГБ).
  if [[ "$FFMPEG_PLAN" == build ]]; then need=$(( need + 5 )); fi
  c=$(free_gib "$BUILD_ROOT"); o=$(free_gib "$PREFIX")
  if (( ${c:-0} < need )); then
    fail "$(t "Для збірки потрібно ≥$need GiB вільного місця в $(existing_ancestor "$BUILD_ROOT") (зараз ${c:-?} GiB)." \
              "The build needs ≥$need GiB of free space in $(existing_ancestor "$BUILD_ROOT") (now ${c:-?} GiB).")"
    hint "$(t "Звільни місце на диску. Проміжні файли старих збірок прибирає «Очистити кеш збірки» (або $SELF_PATH clean)." \
              "Free up disk space. “Clean the build cache” removes intermediate files of old builds (or $SELF_PATH clean).")"
    fix clean "$(t "Очистити кеш збірки" "Clean the build cache")"
    exit 1
  fi
  if (( ${o:-0} < 2 )); then
    die "$(t "Для встановлення пакетів потрібно ≥2 GiB вільного місця в $PREFIX (зараз ${o:-?} GiB)." \
             "Installing the packages needs ≥2 GiB of free space in $PREFIX (now ${o:-?} GiB).")" \
      "$(t "Звільни місце на системному розділі (наприклад, очисти кеш pacman: sudo paccache -rk1)." \
           "Free up space on the system partition (for example, clean the pacman cache: sudo paccache -rk1).")"
  fi
  ok "$(t "Місця достатньо: кеш ${c} GiB, $PREFIX ${o} GiB." "Enough space: cache ${c} GiB, $PREFIX ${o} GiB.")"
}

# ---------------------------------------------------------------------------
# Залежності з репозиторіїв
# ---------------------------------------------------------------------------
tls_package() {
  if pacman -Q mbedtls3 >/dev/null 2>&1 || pacman -Si mbedtls3 >/dev/null 2>&1; then
    echo mbedtls3
  else
    echo mbedtls
  fi
}

plan_dependencies() {
  local tls p
  local -a miss_req=() miss_rec=() miss_ff=() plugin_pkgs=()
  DEP_INSTALL=() DEP_INSTALL_REQ=() DEP_NOREPO_REQ=() DEP_NOREPO_REC=()
  tls=$(tls_package)
  mapfile -t plugin_pkgs < <(plugin_packages)
  mapfile -t miss_req < <(pacman -T "${REQUIRED_PKGS[@]}" "$tls" "${plugin_pkgs[@]}" 2>/dev/null || true)
  mapfile -t miss_rec < <(pacman -T "${RECOMMENDED_PKGS[@]}" 2>/dev/null || true)
  # Залежності ffmpeg (з .SRCINFO PKGBUILD Arch) — лише коли його збиратимемо.
  # Для OBS вони не потрібні, тож пробну конфігурацію CMake не блокують.
  if ((${#FFMPEG_DEPS[@]})); then
    mapfile -t miss_ff < <(pacman -T "${FFMPEG_DEPS[@]}" 2>/dev/null || true)
  fi
  for p in "${miss_req[@]}"; do
    [[ -n "$p" ]] || continue
    if pacman -Si "$p" >/dev/null 2>&1; then DEP_INSTALL+=("$p"); DEP_INSTALL_REQ+=("$p"); else DEP_NOREPO_REQ+=("$p"); fi
  done
  for p in "${miss_ff[@]}" "${miss_rec[@]}"; do
    [[ -n "$p" && " ${DEP_INSTALL[*]} " != *" $p "* ]] || continue
    if pacman -Si "$p" >/dev/null 2>&1; then DEP_INSTALL+=("$p"); else DEP_NOREPO_REC+=("$p"); fi
  done
}

NOREPO_HINT="$(t "Можливо, база пакетів застаріла: повністю онови систему (sudo pacman -Syu, окремо -Sy не роби) і повтори. В Artix деякі пакети можуть називатися інакше." \
                 "The package database may be out of date: fully upgrade the system (sudo pacman -Syu, never -Sy alone) and try again. Some packages may have other names in Artix.")"
readonly NOREPO_HINT

report_dependencies() {
  say "$(t "Пакети для збірки" "Build dependencies")"
  plan_dependencies
  if ((${#DEP_INSTALL[@]})); then
    warn "$(t "Ще не встановлено: ${DEP_INSTALL[*]}" "Not installed yet: ${DEP_INSTALL[*]}")"
    hint "$(t "Постав їх зараз $(deps_action) — лише ці пакети, без оновлення системи (pacman сам додасть їхні залежності). Або нічого не роби: їх поставить $(install_action)." \
              "Install them now $(deps_action) — only these packages, without a system upgrade (pacman adds their dependencies itself). Or do nothing: $(install_action) will install them.")"
    fix deps "$(t "Встановити пакети: ${DEP_INSTALL[*]}" "Install packages: ${DEP_INSTALL[*]}")"
  else
    ok "$(t "Усі потрібні пакети вже встановлені." "All required packages are already installed.")"
  fi
  if ((${#DEP_NOREPO_REQ[@]})); then
    warn "$(t "Обов'язкових пакетів немає в налаштованих репозиторіях: ${DEP_NOREPO_REQ[*]}" \
              "Required packages are missing from the configured repositories: ${DEP_NOREPO_REQ[*]}")"
    hint "$NOREPO_HINT"
  fi
  if ((${#DEP_NOREPO_REC[@]})); then
    warn "$(t "Рекомендованих пакетів немає в репозиторіях (не критично): ${DEP_NOREPO_REC[*]}" \
              "Recommended packages are missing from the repositories (not critical): ${DEP_NOREPO_REC[*]}")"
  fi
}

install_dependencies() {
  say "$(t "Пакети для збірки" "Build dependencies")"
  plan_dependencies
  if ((${#DEP_NOREPO_REQ[@]})); then
    warn "$(t "Пакетів немає в налаштованих репозиторіях: ${DEP_NOREPO_REQ[*]}. Продовжую: CMake покаже, чи вони справді потрібні, ще до компіляції." \
              "Packages are missing from the configured repositories: ${DEP_NOREPO_REQ[*]}. Continuing: CMake will show whether they are really needed, before compiling.")"
    hint "$NOREPO_HINT"
  fi
  if ((${#DEP_NOREPO_REC[@]})); then
    warn "$(t "Рекомендованих пакетів немає в репозиторіях (не критично): ${DEP_NOREPO_REC[*]}" \
              "Recommended packages are missing from the repositories (not critical): ${DEP_NOREPO_REC[*]}")"
  fi
  if ((${#DEP_INSTALL[@]} == 0)); then
    ok "$(t "Усі потрібні пакети вже встановлені." "All required packages are already installed.")"
    return 0
  fi
  printf "$(t 'Буде встановлено (pacman -S --needed, БЕЗ оновлення системи):\n' \
              'Will be installed (pacman -S --needed, WITHOUT a system upgrade):\n')" >&2
  printf '  %s\n' "${DEP_INSTALL[@]}" >&2
  warn "$(t "Якщо pacman запропонує ВИДАЛИТИ якийсь пакет через конфлікт — відповідай N і розберись вручну." \
            "If pacman offers to REMOVE some package because of a conflict, answer N and sort it out manually.")"
  check_pacman_lock
  confirm "$(t "Встановити ці пакети?" "Install these packages?")" \
    || cancelled "$(t "Без цих пакетів OBS не зібрати, тому нічого не змінено. Запусти встановлення ще раз, коли будеш готовий." \
                      "OBS cannot be built without these packages, so nothing was changed. Start the installation again when you are ready.")"
  if ! priv pacman -S --needed "${DEP_INSTALL[@]}"; then
    die "$(t "pacman не встановив пакети." "pacman did not install the packages.")" \
      "$(t "Якщо бачиш помилки 404 чи «failed retrieving file», спершу повністю онови систему: sudo pacman -Syu (окремо -Sy не роби), потім повтори. Якщо не прийняв пароль — $PRIV_HINT" \
           "If you see 404 errors or “failed retrieving file”, first fully upgrade the system: sudo pacman -Syu (never -Sy alone), then try again. If it did not accept the password — $PRIV_HINT")"
  fi
  mkdir -p -- "$STATE_DIR"
  printf '%s\n' "${DEP_INSTALL[@]}" >> "$STATE_DIR/packages-installed-by-script.txt"
  ok "$(t "Пакети встановлено." "Packages installed.")"
}

# ---------------------------------------------------------------------------
# mbedtls: Arch перейшов на mbedtls 4, а OBS потребує 3.x (пакет mbedtls3).
# ---------------------------------------------------------------------------
detect_mbedtls() {
  local files cfg v
  TLS_MODE=system MBEDTLS3_CMAKE_DIR='' MBEDTLS3_INCLUDE='' MBEDTLS3_LIBDIR='' MBEDTLS3_PCDIR=''
  if pacman -Q mbedtls3 >/dev/null 2>&1; then
    files=$(pacman -Qlq mbedtls3 2>/dev/null || true)
    cfg=$(grep -m1 '/MbedTLSConfig\.cmake$' <<<"$files" || true)
    MBEDTLS3_INCLUDE=$(grep -m1 -E '/include/mbedtls3/?$' <<<"$files" || true)
    MBEDTLS3_INCLUDE=${MBEDTLS3_INCLUDE%/}
    MBEDTLS3_LIBDIR=$(grep -m1 -E '/libmbedtls\.so$' <<<"$files" || true)
    MBEDTLS3_LIBDIR=${MBEDTLS3_LIBDIR%/*}
    MBEDTLS3_PCDIR=$(grep -m1 -E '/pkgconfig/mbedtls\.pc$' <<<"$files" || true)
    MBEDTLS3_PCDIR=${MBEDTLS3_PCDIR%/*}
    if [[ -z "$cfg" || -z "$MBEDTLS3_INCLUDE" ]]; then
      die "$(t "Пакет mbedtls3 має неочікувану структуру (немає MbedTLSConfig.cmake або include/mbedtls3). Перевір: pacman -Ql mbedtls3" \
               "The mbedtls3 package has an unexpected layout (no MbedTLSConfig.cmake or include/mbedtls3). Check: pacman -Ql mbedtls3")"
    fi
    MBEDTLS3_CMAKE_DIR=${cfg%/*}
    TLS_MODE=mbedtls3
    ok "mbedtls3: $MBEDTLS3_CMAKE_DIR, $MBEDTLS3_INCLUDE"
    return 0
  fi
  v=$(pacman -Q mbedtls 2>/dev/null | awk '{print $2}' || true)
  case "$v" in
    2.*|3.*) ok "$(t "Системний mbedtls $v — додаткові шляхи не потрібні." \
                     "System mbedtls $v — no extra paths needed.")" ;;
    '')      warn "$(t "mbedtls не встановлено — CMake, ймовірно, його не знайде (install поставить mbedtls3)." \
                       "mbedtls is not installed — CMake will probably not find it (install will add mbedtls3).")" ;;
    *)       warn "$(t "Встановлено mbedtls $v без mbedtls3; з mbedtls 4 OBS, імовірно, не збереться. Встанови mbedtls3." \
                       "mbedtls $v is installed without mbedtls3; OBS will probably not build with mbedtls 4. Install mbedtls3.")" ;;
  esac
}

# ---------------------------------------------------------------------------
# Git
# ---------------------------------------------------------------------------
# Після вимкнення світла посеред git fetch у кеші лишаються *.lock, і git далі
# відмовляється працювати. Прибираємо їх (лише в кеші скрипта), коли жоден git не працює.
clear_stale_git_locks() {
  local d f
  if pgrep -x git >/dev/null 2>&1; then return 0; fi
  for d in "$OBS_SRC" "$HEADERS_SRC" "$FFMPEG_PKGBUILD_DIR" "$PLUGINS_DIR"/*/src; do
    [[ -d "$d/.git" ]] || continue
    while IFS= read -r -d '' f; do
      rm -f -- "$f"
      printf "$(t '  Прибрано застаріле блокування git: %s\n' \
                  '  Removed a stale git lock: %s\n')" "${f#"$BUILD_ROOT"/}" >&2
    done < <(find "$d/.git" -maxdepth 8 -type f -name '*.lock' -print0 2>/dev/null)
  done
}

# Підказка, якщо не вдалося завантажити вихідні коди.
NET_HINT="$(t "Перевір мережу (чи відкривається github.com) і запусти те саме ще раз — уже завантажене лишається в кеші." \
              "Check the network (does github.com open?) and run the same thing again — what is already downloaded stays in the cache.")"
readonly NET_HINT

# Мережа чи GitHub інколи не відповідають хвилину-дві. Через один такий збій
# плагін випав би з набору (а pacman прибрав би встановлений пакет), тож
# завантаження повторюється ще раз. Лише раз: без мережі кожна спроба чекає на
# з'єднання до ~2 хв.
net_retry() {   # команда з аргументами
  "$@" && return 0
  printf "$(t '  Не вдалося завантажити (мережа чи GitHub?) — пробую ще раз за 15 с.\n' \
              '  Download failed (network or GitHub?) — retrying in 15 s.\n')" >&2
  sleep 15
  "$@"
}

# git clone у кеш; невдала спроба може лишити неповний каталог — його прибираємо.
clone_fresh() {   # $1 = каталог у кеші, далі — аргументи git clone
  local dir=$1
  shift
  rm_cache_path "$dir"
  git clone "$@" "$dir"
}

git_prepare_repo() {   # $1 = каталог, $2 = URL
  local dir=$1 url=$2 origin
  if [[ -d "$dir/.git" ]]; then
    origin=$(git -C "$dir" remote get-url origin 2>/dev/null || true)
    [[ "$origin" == "$url" ]] || die "$(t "У $dir несподіваний origin '$origin' (очікувався $url). Це кеш скрипта — видали каталог: rm -rf '$dir'" \
                                          "Unexpected origin '$origin' in $dir (expected $url). This is the script's cache — delete the directory: rm -rf '$dir'")"
    if [[ -n "$(git -C "$dir" status --porcelain --untracked-files=normal --ignore-submodules=all 2>/dev/null)" ]]; then
      die "$(t "У $dir є локальні зміни. Це кеш скрипта — видали каталог: rm -rf '$dir'" \
               "$dir has local changes. This is the script's cache — delete the directory: rm -rf '$dir'")"
    fi
  elif [[ -e "$dir" ]]; then
    die "$(t "$dir існує, але це не git-репозиторій. Видали його: rm -rf '$dir'" \
             "$dir exists but is not a git repository. Delete it: rm -rf '$dir'")"
  else
    mkdir -p -- "${dir%/*}"
    net_retry clone_fresh "$dir" "$url" || die "$(t "Не вдалося завантажити $url." \
                                                    "Could not download $url.")" "$NET_HINT"
  fi
}

# Те саме для git fetch: зупинка з порадою замість «команда завершилась з кодом 128».
git_fetch_or_die() {   # $1 = каталог, далі — аргументи git fetch
  local dir=$1
  shift
  net_retry git -C "$dir" fetch "$@" \
    || die "$(t "Не вдалося оновити $(git -C "$dir" remote get-url origin 2>/dev/null || echo "$dir")." \
                "Could not update $(git -C "$dir" remote get-url origin 2>/dev/null || echo "$dir").")" "$NET_HINT"
}

# ---------------------------------------------------------------------------
# nv-codec-headers (приватна копія в кеші)
# ---------------------------------------------------------------------------
# Найновіший тег nv-codec-headers, чий NVENC API знає драйвер. У тегах nX.Y.*
# X.Y — це і є версія NVENC API (n13.0.19.1 → 13.0).
pick_headers_tag() {   # $1 = NVENC API драйвера
  local drv t best=''
  drv=$(api_to_int "$1") || return 1
  while IFS= read -r t; do
    [[ "$t" =~ ^n([0-9]+)\.([0-9]+)(\.[0-9]+)+$ ]] || continue
    if (( $(api_to_int "${BASH_REMATCH[1]}.${BASH_REMATCH[2]}") <= drv )); then best=$t; fi
  done < <(git -C "$HEADERS_SRC" tag -l 'n*' | sort -V)
  [[ -n "$best" ]] && printf '%s\n' "$best"
}

# Найстаріший NVENC SDK, з яким збирається obs-nvenc цієї версії OBS
# (find_package(FFnvcodec 12) → 12.0).
obs_min_nvenc_api() {
  local v
  v=$(sed -nE 's/.*find_package\(FFnvcodec[[:space:]]+([0-9]+(\.[0-9]+)?).*/\1/p' \
        "$OBS_SRC/plugins/obs-nvenc/CMakeLists.txt" 2>/dev/null | head -n1 || true)
  if [[ "$v" =~ ^[0-9]+$ ]]; then v="$v.0"; fi
  printf '%s\n' "$v"
}

prepare_headers() {
  local h pcdir='' d ver inc major minor want drv min
  say "$(t "nv-codec-headers (приватна копія в кеші, системні файли не змінюються)" \
           "nv-codec-headers (a private copy in the cache, system files are not changed)")"
  git_prepare_repo "$HEADERS_SRC" "$NV_HEADERS_REPO"
  git_fetch_or_die "$HEADERS_SRC" --quiet --tags --force --prune origin
  NV_HEADERS_RESOLVED=$NV_HEADERS_TAG
  if [[ "$NV_HEADERS_TAG" == auto ]]; then
    drv=$(nvenc_driver_api || true)
    if [[ "$drv" =~ ^[0-9]+\.[0-9]+$ ]]; then
      NV_HEADERS_RESOLVED=$(pick_headers_tag "$drv" || true)
      [[ -n "$NV_HEADERS_RESOLVED" ]] || die "$(t "Немає заголовків nv-codec-headers для NVENC API $drv." \
                                                  "No nv-codec-headers for NVENC API $drv.")" \
        "$(t "Драйвер надто старий для NVENC в OBS. Для GTX 900/1000 встанови гілку 580xx (nvidia-580xx-dkms і nvidia-580xx-utils) і перезавантаж комп'ютер." \
             "The driver is too old for NVENC in OBS. For a GTX 900/1000, install the 580xx branch (nvidia-580xx-dkms and nvidia-580xx-utils) and reboot.")"
      printf "$(t '  Драйвер знає NVENC API %s → найновіші сумісні заголовки: %s\n' \
                  '  The driver knows NVENC API %s → the newest compatible headers: %s\n')" "$drv" "$NV_HEADERS_RESOLVED" >&2
    else
      NV_HEADERS_RESOLVED=$NV_HEADERS_DEFAULT
      warn "$(t "Не вдалося запитати NVENC API в драйвера — беру $NV_HEADERS_DEFAULT (максимум для 580xx). Остаточно перевірить obs-nvenc-test." \
                "Could not ask the driver for its NVENC API — using $NV_HEADERS_DEFAULT (the maximum for 580xx). obs-nvenc-test will check it for sure.")"
    fi
  fi
  git -C "$HEADERS_SRC" rev-parse -q --verify "refs/tags/$NV_HEADERS_RESOLVED^{commit}" >/dev/null \
    || die "$(t "Тег $NV_HEADERS_RESOLVED не знайдено в $NV_HEADERS_REPO." \
                "Tag $NV_HEADERS_RESOLVED was not found in $NV_HEADERS_REPO.")" \
      "$(t "Перевір значення «nv-codec-headers» у Налаштуваннях (типове — auto) та інтернет-з'єднання." \
           "Check the “nv-codec-headers” value in Settings (the default is auto) and the internet connection.")"
  git -C "$HEADERS_SRC" -c advice.detachedHead=false checkout -q --detach "refs/tags/$NV_HEADERS_RESOLVED"
  rm_cache_path "$HEADERS_PREFIX"
  make -s -C "$HEADERS_SRC" PREFIX="$HEADERS_PREFIX" install >/dev/null

  h="$HEADERS_PREFIX/include/ffnvcodec/nvEncodeAPI.h"
  [[ -r "$h" ]] || die "$(t "Після встановлення заголовків немає $h." \
                            "$h is missing after the headers were installed.")"
  for d in lib lib64 share; do
    if [[ -f "$HEADERS_PREFIX/$d/pkgconfig/ffnvcodec.pc" ]]; then pcdir="$HEADERS_PREFIX/$d/pkgconfig"; break; fi
  done
  [[ -n "$pcdir" ]] || die "$(t "Не знайдено ffnvcodec.pc у $HEADERS_PREFIX." \
                                "ffnvcodec.pc was not found in $HEADERS_PREFIX.")"
  HEADERS_PCDIR=$pcdir
  export PKG_CONFIG_PATH="$pcdir${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

  ver=$(pkg-config --modversion ffnvcodec 2>/dev/null || true)
  [[ -n "$ver" ]] || die "$(t "pkg-config не бачить локальні заголовки ffnvcodec." \
                              "pkg-config does not see the local ffnvcodec headers.")"
  inc=$(pkg-config --variable=includedir ffnvcodec 2>/dev/null || true)
  [[ "$inc" == "$HEADERS_PREFIX"/* ]] || die "$(t "pkg-config вказує не на локальні заголовки: $inc" \
                                                  "pkg-config points to headers that are not the local ones: $inc")"
  # Теги на гілках підтримки мають у ffnvcodec.pc ще й номер ревізії:
  # n13.0.19.1 → 13.0.19.1.2, n12.1.14.1 → 12.1.14.1.2, n13.1.15.0 → 13.1.15.0.1.
  # Тож версія має збігатися з тегом або починатися з нього. Сумісність із
  # драйвером визначає не цей номер, а NVENCAPI_*_VERSION із заголовка (нижче).
  if [[ "$NV_HEADERS_RESOLVED" =~ ^n([0-9]+(\.[0-9]+)*)$ ]]; then
    want=${BASH_REMATCH[1]}
    if [[ "$ver" != "$want" && "$ver" != "$want".* ]]; then
      die "$(t "Тег $NV_HEADERS_RESOLVED, а pkg-config бачить заголовки версії $ver." \
               "The tag is $NV_HEADERS_RESOLVED, but pkg-config sees headers of version $ver.")" \
        "$(t "Кеш заголовків, схоже, зіпсовано. Видали $HEADERS_SRC і повтори — скрипт завантажить їх заново." \
             "The headers cache seems to be damaged. Delete $HEADERS_SRC and try again — the script will download them again.")"
    fi
  fi
  major=$(awk '/^#define[ \t]+NVENCAPI_MAJOR_VERSION[ \t]/ {print $3; exit}' "$h")
  minor=$(awk '/^#define[ \t]+NVENCAPI_MINOR_VERSION[ \t]/ {print $3; exit}' "$h")
  [[ "$major" =~ ^[0-9]+$ && "$minor" =~ ^[0-9]+$ ]] || die "$(t "Не вдалося прочитати NVENCAPI_*_VERSION з $h." \
                                                                 "Could not read NVENCAPI_*_VERSION from $h.")"
  HEADERS_API="$major.$minor"
  min=$(obs_min_nvenc_api)
  if [[ "$min" =~ ^[0-9]+\.[0-9]+$ ]] && (( $(api_to_int "$HEADERS_API") < $(api_to_int "$min") )); then
    die "$(t "OBS $OBS_DESCRIBE збирає NVENC лише із SDK $min і новіше, а заголовки $NV_HEADERS_RESOLVED — це NVENC API $HEADERS_API." \
             "OBS $OBS_DESCRIBE builds NVENC only with SDK $min or newer, but the $NV_HEADERS_RESOLVED headers are NVENC API $HEADERS_API.")" \
      "$(t "Онови драйвер NVIDIA (для GTX 900/1000 — гілка 580xx) або в Налаштуваннях → «Версія OBS» вкажи старішу версію OBS." \
           "Update the NVIDIA driver (for a GTX 900/1000 — the 580xx branch) or set an older OBS version in Settings → “OBS version”.")"
  fi
  ok "$(t "Заголовки $NV_HEADERS_RESOLVED ($ver) готові: NVENC API $HEADERS_API." \
          "Headers $NV_HEADERS_RESOLVED ($ver) are ready: NVENC API $HEADERS_API.")"
}

# Порівнює NVENC API драйвера і заголовків ще до компіляції.
check_driver_vs_headers() {
  local drv
  drv=$(nvenc_driver_api || true)
  if [[ ! "$drv" =~ ^[0-9]+\.[0-9]+$ ]]; then
    warn "$(t "Не вдалося визначити версію NVENC API драйвера. Остаточну перевірку зробить obs-nvenc-test після компіляції." \
              "Could not determine the driver's NVENC API version. obs-nvenc-test will do the final check after compiling.")"
    return 0
  fi
  DRIVER_NVENC_API=$drv
  if (( $(api_to_int "$drv") < $(api_to_int "$HEADERS_API") )); then
    die "$(t "Драйвер підтримує NVENC API лише до $drv, а заголовки $NV_HEADERS_RESOLVED потребують $HEADERS_API." \
             "The driver supports NVENC API only up to $drv, but the $NV_HEADERS_RESOLVED headers need $HEADERS_API.")" \
      "$(t "Онови драйвер (для GTX 900/1000 — гілка 580xx: nvidia-580xx-dkms і nvidia-580xx-utils) і перезавантаж комп'ютер. Або в Налаштуваннях задай тег nv-codec-headers, сумісний з API $drv." \
           "Update the driver (for a GTX 900/1000 — the 580xx branch: nvidia-580xx-dkms and nvidia-580xx-utils) and reboot. Or set an nv-codec-headers tag compatible with API $drv in Settings.")"
  fi
  ok "$(t "NVENC API: драйвер $drv, заголовки $HEADERS_API — сумісно." \
          "NVENC API: driver $drv, headers $HEADERS_API — compatible.")"
}

# ---------------------------------------------------------------------------
# Вихідні коди OBS
# ---------------------------------------------------------------------------
resolve_obs_ref() {
  local ref="$OBS_REF" tag
  if [[ "$ref" == stable ]]; then
    tag=$(git -C "$OBS_SRC" tag -l | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1 || true)
    [[ -n "$tag" ]] || die "$(t "Не знайдено жодного стабільного тега OBS." "No stable OBS tag was found.")" \
      "$(t "Перевір інтернет-з'єднання: вихідні коди OBS завантажуються з GitHub." \
           "Check the internet connection: the OBS sources are downloaded from GitHub.")"
    OBS_RESOLVED="refs/tags/$tag"
    OBS_REF_LABEL="$(t "$tag (найновіший стабільний)" "$tag (newest stable)")"
  elif git -C "$OBS_SRC" show-ref --verify -q "refs/remotes/origin/$ref"; then
    OBS_RESOLVED="refs/remotes/origin/$ref"
    OBS_REF_LABEL="$(t "гілка $ref" "branch $ref")"
  elif git -C "$OBS_SRC" show-ref --verify -q "refs/tags/$ref"; then
    OBS_RESOLVED="refs/tags/$ref"
    OBS_REF_LABEL="$(t "тег $ref" "tag $ref")"
  elif git -C "$OBS_SRC" rev-parse -q --verify "$ref^{commit}" >/dev/null 2>&1; then
    OBS_RESOLVED="$ref"
    OBS_REF_LABEL="$(t "коміт $ref" "commit $ref")"
  else
    die "$(t "Версію OBS «$ref» не знайдено (ні гілка, ні тег, ні коміт)." \
             "OBS version “$ref” was not found (not a branch, a tag or a commit).")" \
      "$(t "У Налаштуваннях → «Версія OBS» вкажи stable (найновіша стабільна) або наявний тег, наприклад 33.0.0." \
           "In Settings → “OBS version” set stable (the newest stable one) or an existing tag, for example 33.0.0.")"
  fi
  OBS_COMMIT=$(git -C "$OBS_SRC" rev-parse "$OBS_RESOLVED^{commit}")
}

prepare_obs_source() {
  say "$(t "Вихідні коди OBS ($OBS_REPO)" "OBS sources ($OBS_REPO)")"
  git_prepare_repo "$OBS_SRC" "$OBS_REPO"
  git_fetch_or_die "$OBS_SRC" --quiet --prune --tags --force origin '+refs/heads/*:refs/remotes/origin/*'
  resolve_obs_ref
  git -C "$OBS_SRC" -c advice.detachedHead=false checkout -q --detach "$OBS_COMMIT"
  # Сабмодулі — частина кешу скрипта, тому --force прийнятний.
  git -C "$OBS_SRC" submodule --quiet sync --recursive
  net_retry git -C "$OBS_SRC" submodule update --init --recursive --force --jobs 4 \
    || die "$(t "Не вдалося завантажити сабмодулі OBS (obs-browser, obs-websocket…)." \
                "Could not download the OBS submodules (obs-browser, obs-websocket…).")" "$NET_HINT"
  [[ -f "$OBS_SRC/plugins/obs-nvenc/CMakeLists.txt" ]] \
    || die "$(t "У цій версії OBS немає plugins/obs-nvenc — версія надто стара для цього скрипта." \
                "This OBS version has no plugins/obs-nvenc — the version is too old for this script.")"
  [[ -f "$OBS_SRC/plugins/obs-browser/CMakeLists.txt" ]] \
    || die "$(t "Немає plugins/obs-browser (сабмодуль не завантажився)." \
                "No plugins/obs-browser (the submodule did not download).")"
  OBS_DESCRIBE=$(git -C "$OBS_SRC" describe --tags --always 2>/dev/null || git -C "$OBS_SRC" rev-parse --short=12 HEAD)
  OBS_MAJOR=$(awk '/^#define[ \t]+LIBOBS_API_MAJOR_VER[ \t]/ {print $3; exit}' "$OBS_SRC/libobs/obs-config.h" 2>/dev/null || true)
  ok "OBS: $OBS_REF_LABEL → $OBS_DESCRIBE (${OBS_COMMIT:0:12})"
}

# ---------------------------------------------------------------------------
# CEF
# ---------------------------------------------------------------------------
# Опис Linux-архіву CEF з вихідних кодів OBS: OBS 32+ тримає його в
# CMakePresets.json (пресет "dependencies"), старіші версії — у buildspec.json.
# Друкує "версія<TAB>URL<TAB>офіційний SHA-256" (невідомі поля порожні).
cef_spec_from_source() {
  local py
  if have python3; then py=python3; elif have python; then py=python; else return 1; fi
  "$py" - "$OBS_SRC" <<'EOF'
import json, os, sys

src = sys.argv[1]

def from_presets(data):
    for preset in data.get("configurePresets", []):
        if preset.get("name") == "dependencies":
            return preset["vendor"]["obsproject.com/obs-studio"]["dependencies"]["cef"]
    return None

def from_buildspec(data):
    return data["dependencies"]["cef"]

cef = None
for name, getter in (("CMakePresets.json", from_presets), ("buildspec.json", from_buildspec)):
    path = os.path.join(src, name)
    if not os.path.isfile(path):
        continue
    try:
        with open(path, encoding="utf-8") as f:
            cef = getter(json.load(f))
    except (KeyError, TypeError, ValueError):
        cef = None
    if cef:
        break
if not cef or not str(cef.get("version", "")).isdigit():
    sys.exit(1)
platform = "ubuntu-x86_64"
version = str(cef["version"])
revision = (cef.get("revision") or {}).get(platform)
sha = (cef.get("hashes") or {}).get(platform, "")
base = str(cef.get("baseUrl", "")).rstrip("/")
url = ""
if base:
    url = "%s/cef_binary_%s_linux_x86_64%s.tar.xz" % (base, version, "_v%s" % revision if revision else "")
print("%s\t%s\t%s" % (version, url, sha))
EOF
}

# Визначає версію CEF (CEF_BUILD_RESOLVED), а коли вона збігається з тією, яку
# вказує OBS, — ще й офіційні посилання та SHA-256 (CEF_SPEC_URL, CEF_SPEC_SHA).
detect_cef_build() {
  local spec='' ver='' url='' sha='' f
  CEF_BUILD_RESOLVED='' CEF_SPEC_URL='' CEF_SPEC_SHA=''
  spec=$(cef_spec_from_source 2>/dev/null || true)
  if [[ -n "$spec" ]]; then
    IFS=$'\t' read -r ver url sha <<<"$spec"
  else
    # Без Python: лише номер версії, посилання шукається перебором.
    for f in "$OBS_SRC/CMakePresets.json" "$OBS_SRC/buildspec.json"; do
      [[ -f "$f" ]] || continue
      ver=$(awk '
        /"cef"[[:space:]]*:/ { f = 1 }
        f && match($0, /"version"[[:space:]]*:[[:space:]]*"[0-9]+"/) {
          s = substr($0, RSTART, RLENGTH); gsub(/[^0-9]/, "", s); print s; exit
        }' "$f")
      if [[ -n "$ver" ]]; then break; fi
    done
  fi
  if [[ -n "$CEF_BUILD" ]]; then
    CEF_BUILD_RESOLVED=$CEF_BUILD
  else
    CEF_BUILD_RESOLVED=$ver
  fi
  if [[ -n "$ver" && "$CEF_BUILD_RESOLVED" == "$ver" ]]; then
    CEF_SPEC_URL=$url CEF_SPEC_SHA=${sha,,}
  fi
  [[ "$CEF_BUILD_RESOLVED" =~ ^[0-9]+$ ]]
}

url_exists() { curl -fsSL --max-time 25 --range 0-0 -o /dev/null -- "$1" 2>/dev/null; }

download_file() {   # $1 = URL, $2 = файл
  local url=$1 dest=$2 part="$2.part"
  rm -f -- "$part"
  say "$(t "Завантажую $url" "Downloading $url")"
  if ! curl --fail --location --retry 3 --retry-delay 3 --connect-timeout 20 --progress-bar -o "$part" -- "$url"; then
    rm -f -- "$part"
    return 1
  fi
  [[ -s "$part" ]] || { rm -f -- "$part"; return 1; }
  mv -f -- "$part" "$dest"
}

# Друкує шлях до локального архіву CEF (за потреби завантажує його).
find_cef_archive() {
  local build=$1 url='' f rev name cache="$DOWNLOADS/cef-$1.url" dest
  mkdir -p -- "$DOWNLOADS"
  if [[ -n "$CEF_ARCHIVE_URL" ]]; then
    url=$CEF_ARCHIVE_URL
  elif [[ -n "$CEF_SPEC_URL" ]]; then
    # Точна назва архіву відома з вихідних кодів OBS — перебір не потрібен.
    url=$CEF_SPEC_URL
  else
    f=$(find "$DOWNLOADS" -maxdepth 1 -type f -name "cef_binary_${build}_linux_x86_64*.tar.xz" -printf '%f\n' 2>/dev/null | sort -V | tail -n1 || true)
    if [[ -n "$f" ]]; then
      printf '%s\n' "$DOWNLOADS/$f"
      return 0
    fi
    if [[ -s "$cache" ]]; then
      url=$(<"$cache")
    else
      say "$(t "Шукаю офіційний Linux-архів CEF $build на $CEF_CDN" \
               "Looking for the official Linux CEF archive $build on $CEF_CDN")"
      for rev in 12 11 10 9 8 7 6 5 4 3 2 1 ''; do
        name="cef_binary_${build}_linux_x86_64${rev:+_v$rev}.tar.xz"
        if url_exists "$CEF_CDN/$name"; then url="$CEF_CDN/$name"; break; fi
      done
      [[ -n "$url" ]] || return 1
      printf '%s\n' "$url" > "$cache"
    fi
  fi
  dest="$DOWNLOADS/$(basename -- "${url%%\?*}")"
  if [[ ! -s "$dest" ]]; then
    download_file "$url" "$dest" || return 1
  fi
  printf '%s\n' "$dest"
}

verify_cef_archive() {
  local archive=$1 sum rec
  sum=$(sha256sum -- "$archive" | awk '{print $1}')
  printf '  SHA-256 %s: %s\n' "$(basename -- "$archive")" "$sum" >&2
  if [[ -n "$CEF_SHA256" ]]; then
    [[ "${CEF_SHA256,,}" == "$sum" ]] || die "$(t "SHA-256 архіву CEF не збігається з CEF_SHA256. Видали $archive і перевір посилання." \
                                                  "The CEF archive's SHA-256 does not match CEF_SHA256. Delete $archive and check the link.")"
    ok "$(t "SHA-256 збігається з CEF_SHA256." "SHA-256 matches CEF_SHA256.")"
    return 0
  fi
  if [[ -n "$CEF_SPEC_SHA" && -z "$CEF_ARCHIVE_URL" \
        && "$(basename -- "$archive")" == "$(basename -- "${CEF_SPEC_URL%%\?*}")" ]]; then
    [[ "$CEF_SPEC_SHA" == "$sum" ]] \
      || die "$(t "SHA-256 архіву CEF не збігається з офіційним хешем із вихідних кодів OBS ($CEF_SPEC_SHA). Архів пошкоджений або підмінений — видали $archive і повтори." \
                  "The CEF archive's SHA-256 does not match the official hash from the OBS sources ($CEF_SPEC_SHA). The archive is damaged or tampered with — delete $archive and try again.")"
    ok "$(t "SHA-256 збігається з офіційним хешем OBS." "SHA-256 matches the official OBS hash.")"
    return 0
  fi
  mkdir -p -- "$STATE_DIR"
  rec="$STATE_DIR/$(basename -- "$archive").sha256"
  if [[ -s "$rec" ]]; then
    [[ "$(<"$rec")" == "$sum" ]] || die "$(t "Архів CEF відрізняється від завантаженого раніше (еталон: $rec). Якщо це очікувано — видали $rec і $archive, потім повтори." \
                                             "The CEF archive differs from the one downloaded earlier (reference: $rec). If this is expected, delete $rec and $archive, then try again.")"
    ok "$(t "SHA-256 збігається з раніше збереженим." "SHA-256 matches the one saved earlier.")"
  else
    # Записується лише після успішної перевірки вмісту (див. prepare_cef).
    CEF_SHA_PENDING="$sum $rec"
    warn "$(t "OBS не публікує SHA-256 для Linux-архіву CEF; хеш буде збережено, і наступні завантаження звірятимуться з ним." \
              "OBS does not publish a SHA-256 for the Linux CEF archive; the hash will be saved, and later downloads will be checked against it.")"
  fi
}

validate_cef() {   # $1 = корінь CEF, $2 = очікувана збірка, $3 = архів (якщо щойно розпаковано)
  local root=$1 build=$2 archive=${3:-} hv wrapper lib
  [[ -r "$root/include/cef_version.h" ]] || die "$(t "У $root немає include/cef_version.h." \
                                                     "$root has no include/cef_version.h.")"
  hv=$(awk '/^#define[ \t]+CHROME_VERSION_BUILD[ \t]/ {print $3; exit}' "$root/include/cef_version.h")
  wrapper=$(find "$root" -type f -name 'libcef_dll_wrapper.a' -print -quit 2>/dev/null || true)
  lib=$(find "$root" -type f -name 'libcef.so' -print -quit 2>/dev/null || true)
  [[ -n "$wrapper" ]] || die "$(t "У CEF немає зібраного libcef_dll_wrapper.a — потрібен саме архів від OBS." \
                                  "CEF has no prebuilt libcef_dll_wrapper.a — it must be the archive from OBS.")"
  [[ -n "$lib" ]] || die "$(t "У CEF немає libcef.so." "CEF has no libcef.so.")"
  if [[ "$hv" != "$build" ]]; then
    if [[ -n "$CEF_BUILD" || -n "${CEF_ROOT_DIR:-}" ]]; then
      warn "$(t "CEF має збірку $hv, а не $build; продовжую, бо CEF задано вручну." \
                "CEF is build $hv, not $build; continuing because CEF was set manually.")"
    elif [[ -n "$archive" ]]; then
      die "$(t "Архів $(basename -- "$archive") містить CEF збірки '$hv', а потрібна $build. Видали архів і повтори: rm -f '$archive'" \
               "The archive $(basename -- "$archive") contains CEF build '$hv', but $build is needed. Delete the archive and try again: rm -f '$archive'")"
    else
      die "$(t "CEF у кеші має збірку '$hv', а потрібна $build. Видали $CEF_DIR/$build і повтори." \
               "The cached CEF is build '$hv', but $build is needed. Delete $CEF_DIR/$build and try again.")"
    fi
  fi
  ok "CEF $hv: $root"
}

prepare_cef() {
  local build root='' archive dest tmp hdr
  say "$(t "CEF (Chromium Embedded Framework) для Browser Source" \
           "CEF (Chromium Embedded Framework) for Browser Source")"
  detect_cef_build \
    || die "$(t "Не вдалося визначити версію CEF з вихідних кодів OBS (CMakePresets.json / buildspec.json). Задай CEF_BUILD=<номер> (напр., 7871 для OBS 33, 6533 для OBS 32)." \
                "Could not determine the CEF version from the OBS sources (CMakePresets.json / buildspec.json). Set CEF_BUILD=<number> (e.g. 7871 for OBS 33, 6533 for OBS 32).")"
  build=$CEF_BUILD_RESOLVED
  if [[ -n "$CEF_SPEC_SHA" ]]; then
    printf "$(t '  Офіційний архів: %s\n' '  Official archive: %s\n')" "${CEF_SPEC_URL:-—}" >&2
  fi
  if [[ -n "${CEF_ROOT_DIR:-}" ]]; then
    root=$(realpath -e -- "$CEF_ROOT_DIR" 2>/dev/null) || die "$(t "CEF_ROOT_DIR не існує: $CEF_ROOT_DIR" \
                                                                   "CEF_ROOT_DIR does not exist: $CEF_ROOT_DIR")"
  else
    dest="$CEF_DIR/$build"
    if [[ -f "$dest/.root" ]]; then
      root=$(<"$dest/.root")
    else
      archive=$(find_cef_archive "$build") \
        || die "$(t "Не вдалося знайти або завантажити Linux-архів CEF $build." \
                    "Could not find or download the Linux CEF archive $build.")" \
          "$(t "Перевір інтернет: CEF (рушій Browser Source) завантажується з ${CEF_CDN%%/downloads*}. Якщо сайт тимчасово недоступний — повтори пізніше. Досвідченим: CEF_ARCHIVE_URL=… або CEF_ROOT_DIR=…" \
               "Check the internet: CEF (the Browser Source engine) is downloaded from ${CEF_CDN%%/downloads*}. If the site is temporarily down, try again later. Advanced: CEF_ARCHIVE_URL=… or CEF_ROOT_DIR=…")"
      verify_cef_archive "$archive"
      tmp="$dest.tmp"
      rm_cache_path "$tmp"
      mkdir -p -- "$tmp"
      say "$(t "Розпаковую $(basename -- "$archive")" "Unpacking $(basename -- "$archive")")"
      tar -xf "$archive" -C "$tmp" --no-same-owner \
        || die "$(t "Не вдалося розпакувати $archive (архів пошкоджений? Видали його і повтори)." \
                    "Could not unpack $archive (is the archive damaged? Delete it and try again).")"
      hdr=$(find "$tmp" -maxdepth 4 -path '*/include/cef_version.h' -print -quit)
      [[ -n "$hdr" ]] || die "$(t "В архіві немає include/cef_version.h." "The archive has no include/cef_version.h.")"
      root="${hdr%/include/cef_version.h}"
      # Перевіряємо ДО того, як розпакований CEF потрапить у кеш.
      validate_cef "$root" "$build" "$archive"
      if [[ -n "$CEF_SHA_PENDING" ]]; then
        printf '%s\n' "${CEF_SHA_PENDING%% *}" > "${CEF_SHA_PENDING#* }"
        CEF_SHA_PENDING=''
      fi
      rm_cache_path "$dest"
      mv -- "$tmp" "$dest"
      root="$dest${root#"$tmp"}"
      printf '%s\n' "$root" > "$dest/.root"
    fi
  fi
  validate_cef "$root" "$build"
  CEF_ROOT=$root
}

# ---------------------------------------------------------------------------
# CMake
# ---------------------------------------------------------------------------
pc_has() { pkg-config --exists "$1" 2>/dev/null; }

# Прапорці компілятора з профілю заліза. Тип збірки RelWithDebInfo (як в
# офіційних збірках OBS: Release з IPO, за даними LFS, дає segfault), але без -g
# (налагоджувальну інформацію однаково прибирає strip) і з нашим -O.
profile_cmake_flags() {   # $1 = CFLAGS, $2 = CXXFLAGS
  printf '%s\n' \
    -DCMAKE_BUILD_TYPE=RelWithDebInfo \
    "-DCMAKE_C_FLAGS=$1" "-DCMAKE_CXX_FLAGS=$2" \
    -DCMAKE_C_FLAGS_RELWITHDEBINFO=-DNDEBUG -DCMAKE_CXX_FLAGS_RELWITHDEBINFO=-DNDEBUG \
    "-DCMAKE_EXE_LINKER_FLAGS=$OPT_LDFLAGS" "-DCMAKE_SHARED_LINKER_FLAGS=$OPT_LDFLAGS" \
    "-DCMAKE_MODULE_LINKER_FLAGS=$OPT_LDFLAGS"
}

build_cmake_args() {
  local -a extra=() flags=()
  mapfile -t flags < <(profile_cmake_flags "$OPT_CFLAGS" "$OPT_CXXFLAGS")
  CMAKE_ARGS=(
    -G Ninja
    -DCMAKE_INSTALL_LIBDIR=lib
    "${flags[@]}"
    "-DCMAKE_PREFIX_PATH=$HEADERS_PREFIX"
    "-DFFnvcodec_INCLUDE_DIR:PATH=$HEADERS_PREFIX/include"
    "-DCEF_ROOT_DIR:PATH=$CEF_ROOT"
    -DENABLE_BROWSER=ON
    -DENABLE_BROWSER_PANELS=ON
    -DENABLE_NVENC=ON
    -DENABLE_FFMPEG_NVENC=OFF
    -DENABLE_AJA=OFF
    -DENABLE_PORTABLE_CONFIG=OFF
    -DOBS_COMPILE_DEPRECATION_AS_WARNING=ON
  )
  FEATURES=()
  if [[ "$TLS_MODE" == mbedtls3 ]]; then
    CMAKE_ARGS+=("-DMbedTLS_DIR=$MBEDTLS3_CMAKE_DIR" "-DCMAKE_INCLUDE_PATH=$MBEDTLS3_INCLUDE")
    if [[ -n "$MBEDTLS3_LIBDIR" ]]; then CMAKE_ARGS+=("-DCMAKE_LIBRARY_PATH=$MBEDTLS3_LIBDIR"); fi
    if [[ -n "$MBEDTLS3_PCDIR" ]]; then export PKG_CONFIG_PATH="$MBEDTLS3_PCDIR${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"; fi
  fi
  # Необов'язкові модулі вмикаються лише тоді, коли їхні бібліотеки є в системі,
  # щоб їхня відсутність не ламала конфігурацію.
  if pc_has libvlc; then CMAKE_ARGS+=(-DENABLE_VLC=ON);  FEATURES+=("$(t "VLC-джерело: так" "VLC source: yes")")
  else                   CMAKE_ARGS+=(-DENABLE_VLC=OFF); FEATURES+=("$(t "VLC-джерело: ні (немає libvlc)" \
                                                                         "VLC source: no (no libvlc)")"); fi
  if pc_has jack;   then CMAKE_ARGS+=(-DENABLE_JACK=ON);  FEATURES+=("$(t "JACK: так" "JACK: yes")")
  else                   CMAKE_ARGS+=(-DENABLE_JACK=OFF); FEATURES+=("$(t "JACK: ні (немає jack.pc)" \
                                                                          "JACK: no (no jack.pc)")"); fi
  if pc_has vpl;    then CMAKE_ARGS+=(-DENABLE_QSV11=ON);  FEATURES+=("$(t "Intel QSV: так" "Intel QSV: yes")")
  else                   CMAKE_ARGS+=(-DENABLE_QSV11=OFF); FEATURES+=("$(t "Intel QSV: ні (немає libvpl)" \
                                                                           "Intel QSV: no (no libvpl)")"); fi
  # AAC-кодер FDK якісніший за вбудований (так збирає й Arch), якщо бібліотека є.
  if pc_has fdk-aac; then CMAKE_ARGS+=(-DENABLE_LIBFDK=ON); FEATURES+=("$(t "AAC (FDK): так" "AAC (FDK): yes")")
  else                    FEATURES+=("$(t "AAC (FDK): ні (немає libfdk-aac)" "AAC (FDK): no (no libfdk-aac)")"); fi
  if [[ -n "$EXTRA_CMAKE_ARGS" ]]; then
    read -r -a extra <<<"$EXTRA_CMAKE_ARGS"
    CMAKE_ARGS+=("${extra[@]}")
  fi
}

cache_get() {   # $1 = каталог збірки, $2 = змінна
  sed -n -E "s/^$2(:[A-Z_]+)?=(.*)\$/\\2/p" "$1/CMakeCache.txt" 2>/dev/null | head -n1 || true
}

verify_cmake_cache() {
  local dir=$1 log="$1/cmake-configure.log" bad problems=0 f
  if [[ "$(cache_get "$dir" ENABLE_BROWSER)" != ON ]]; then fail "$(t "CMake вимкнув Browser Source (ENABLE_BROWSER)." \
                                                                      "CMake disabled Browser Source (ENABLE_BROWSER).")"; problems=1; fi
  if [[ "$(cache_get "$dir" ENABLE_NVENC)" != ON ]]; then fail "$(t "CMake вимкнув NVENC (ENABLE_NVENC)." \
                                                                    "CMake disabled NVENC (ENABLE_NVENC).")"; problems=1; fi
  if [[ "$(cache_get "$dir" FFnvcodec_INCLUDE_DIR)" != "$HEADERS_PREFIX/include" ]]; then
    fail "$(t "CMake використовує не локальні заголовки NVENC." \
              "CMake is not using the local NVENC headers.")"; problems=1
  fi
  if [[ "$(cache_get "$dir" CEF_ROOT_DIR)" != "$CEF_ROOT" ]]; then fail "$(t "CMake використовує не перевірений CEF." \
                                                                             "CMake is not using the verified CEF.")"; problems=1; fi
  if grep -Eiq 'Browser sources? (are|is) not enabled|obs-browser.{0,40}not available' "$log" 2>/dev/null; then
    fail "$(t "CMake не ввімкнув плагін obs-browser." "CMake did not enable the obs-browser plugin.")"; problems=1
  fi
  if [[ "$TLS_MODE" == mbedtls3 ]]; then
    bad=$(grep -iE '^[A-Za-z0-9_]*mbed[A-Za-z0-9_]*(:[A-Z_]+)?=/' "$dir/CMakeCache.txt" | grep -v 'mbedtls3' || true)
    if [[ -n "$bad" ]]; then
      fail "$(t "CMake підхопив не mbedtls3:" "CMake picked up something other than mbedtls3:")"
      sed 's/^/    /' <<<"$bad" >&2
      problems=1
    else
      ok "$(t "MbedTLS: використовується mbedtls3." "MbedTLS: mbedtls3 is used.")"
    fi
  fi
  printf "$(t '  Тип збірки: %s; прапорці: %s\n' \
              '  Build type: %s; flags: %s\n')" "$(cache_get "$dir" CMAKE_BUILD_TYPE)" "$(cache_get "$dir" CMAKE_C_FLAGS)" >&2
  for f in "${FEATURES[@]}"; do printf '  %s\n' "$f" >&2; done
  if (( problems )); then
    die "$(t "Конфігурація CMake не відповідає очікуваній (див. вище). Журнал: $log" \
             "The CMake configuration is not what was expected (see above). Log: $log")"
  fi
  ok "$(t "CMake: Browser Source, власний NVENC (SDK $HEADERS_API), CEF $CEF_BUILD_RESOLVED." \
          "CMake: Browser Source, own NVENC (SDK $HEADERS_API), CEF $CEF_BUILD_RESOLVED.")"
}

# Чого не знайшов CMake (назви з журналу конфігурації), через пробіл.
cmake_missing_packages() {   # $1 = журнал
  sed -nE \
    -e 's/.*Could NOT find ([A-Za-z0-9_+.-]+).*/\1/p' \
    -e 's/.*Could not find a package configuration file provided by "([^"]+)".*/\1/p' \
    -e "s/.*No package '([^']+)' found.*/\\1/p" \
    -- "$1" 2>/dev/null | sort -u | tr '\n' ' '
}

configure_obs() {   # $1 = каталог збірки, $2 = префікс встановлення, $3 = fresh|reuse
  local dir=$1 prefix=$2 mode=$3 sig log missing
  sig=$(printf '%s\n' "$OBS_COMMIT" "$CEF_ROOT" "$NV_HEADERS_RESOLVED" "$TLS_MODE" "$prefix" "${CMAKE_ARGS[@]}" \
        | sha256sum | awk '{print $1}')
  if [[ "$mode" == fresh || "$FORCE_CLEAN_BUILD" == 1 || ! -f "$dir/.signature" || "$(<"$dir/.signature")" != "$sig" ]]; then
    rm_cache_path "$dir"
    mkdir -p -- "$dir"
    say "$(t "Чиста конфігурація CMake" "Clean CMake configuration")"
  else
    say "$(t "Кеш збірки вже має OBS $OBS_DESCRIBE з тими самими параметрами — перекомпілюю лише змінене" \
             "The build cache already has OBS $OBS_DESCRIBE with the same parameters — recompiling only what changed")"
    rm -f -- "$dir/.signature"
  fi
  log="$dir/cmake-configure.log"
  if ! cmake -S "$OBS_SRC" -B "$dir" "-DCMAKE_INSTALL_PREFIX=$prefix" "${CMAKE_ARGS[@]}" >"$log" 2>&1; then
    printf "$(t 'Останні рядки журналу CMake:\n' 'Last lines of the CMake log:\n')" >&2
    tail -n 60 "$log" >&2
    missing=$(cmake_missing_packages "$log")
    if [[ -n "$missing" ]]; then
      die "$(t "CMake не зміг сконфігурувати OBS: не знайдено ${missing% }." \
               "CMake could not configure OBS: ${missing% } not found.")" \
        "$(t "Встанови відповідні пакети (назва зазвичай та сама, малими літерами) і повтори. Повний журнал: $log" \
             "Install the matching packages (the name is usually the same, in lowercase) and try again. Full log: $log")"
    fi
    die "$(t "CMake не зміг сконфігурувати OBS (див. журнал вище)." \
             "CMake could not configure OBS (see the log above).")" \
      "$(t "Найчастіше бракує пакета: шукай у журналі «Could not find» чи «not found». Повний журнал: $log" \
           "Most often a package is missing: look for “Could not find” or “not found” in the log. Full log: $log")"
  fi
  verify_cmake_cache "$dir"
  printf '%s\n' "$sig" > "$dir/.signature"
}

# ---------------------------------------------------------------------------
# Компіляція, тест NVENC, тимчасове встановлення
# ---------------------------------------------------------------------------
compute_jobs() {
  local cpus mem by_ram
  if [[ -n "$BUILD_JOBS" ]]; then printf '%s\n' "$BUILD_JOBS"; return 0; fi
  cpus=$(nproc 2>/dev/null || echo 1)
  mem=$(mem_avail_gib)
  by_ram=$(( ${mem:-0} / 2 ))
  if (( by_ram < 1 )); then by_ram=1; fi
  printf '%s\n' $(( cpus < by_ram ? cpus : by_ram ))
}

build_obs() {
  local jobs
  jobs=$(compute_jobs)
  say "$(t "Компіляція OBS ($jobs потоків, знижений пріоритет). На старішому залізі це може тривати годину й більше." \
           "Compiling OBS ($jobs jobs, lowered priority). On older hardware this can take an hour or more.")"
  if ! nice -n 10 cmake --build "$BUILD_DIR" --parallel "$jobs"; then
    fail "$(t "Компіляція не вдалася (див. вивід вище і звіт). Нічого не встановлено і не змінено." \
              "Compilation failed (see the output above and the report). Nothing was installed or changed.")"
    hint "$(t "Перезбери начисто: $(rebuild_action) — це допомагає, якщо попереднє збирання обірвалося чи кеш зіпсовано. Якщо помилка повториться — створи issue на GitHub і додай звіт." \
              "Rebuild from scratch: $(rebuild_action) — this helps if the previous build was interrupted or the cache is damaged. If the error comes back, open an issue on GitHub and attach the report.")"
    fix rebuild "$(t "Перезібрати начисто" "Rebuild from scratch")"
    exit 1
  fi
  ok "$(t "Компіляція завершена." "Compilation finished.")"
}

nvenc_test_failed() {
  if [[ "$SKIP_NVENC_TEST" == 1 ]]; then
    warn "$(t "$1 Продовжую, бо SKIP_NVENC_TEST=1 — NVENC у цій збірці може НЕ працювати." \
              "$1 Continuing because SKIP_NVENC_TEST=1 — NVENC may NOT work in this build.")"
    NVENC_TEST_RESULT=skipped
    return 0
  fi
  die "$(t "$1 Збірку НЕ встановлено, поточний OBS не змінено." \
           "$1 The build was NOT installed, the current OBS was not changed.")" \
    "$(t "Перевір, що драйвер NVIDIA працює (nvidia-smi показує відеокарту) і що після оновлення драйвера комп'ютер перезавантажено. Журнал тесту: $BUILD_ROOT/obs-nvenc-test.log" \
         "Check that the NVIDIA driver works (nvidia-smi shows the GPU) and that the computer was rebooted after the driver update. Test log: $BUILD_ROOT/obs-nvenc-test.log")"
}

run_nvenc_test() {
  local t log="$BUILD_ROOT/obs-nvenc-test.log" rc=0
  t=$(find "$BUILD_DIR" -type f -name obs-nvenc-test -executable -print -quit 2>/dev/null || true)
  if [[ -z "$t" ]]; then
    nvenc_test_failed "$(t "Не зібрано obs-nvenc-test." "obs-nvenc-test was not built.")"
    return 0
  fi
  say "$(t "Перевіряю NVENC тестом OBS: $t" "Testing NVENC with the OBS test: $t")"
  timeout 60 "$t" >"$log" 2>&1 || rc=$?
  sed 's/^/  /' "$log" >&2
  if (( rc != 0 )); then
    nvenc_test_failed "$(t "obs-nvenc-test завершився з кодом $rc." "obs-nvenc-test exited with code $rc.")"
    return 0
  fi
  if ! grep -qx 'nvenc_supported=true' "$log"; then
    nvenc_test_failed "$(t "obs-nvenc-test не підтвердив підтримку NVENC." \
                           "obs-nvenc-test did not confirm NVENC support.")"
    return 0
  fi
  NVENC_TEST_RESULT=passed
  ok "$(t "NVENC працює з цією збіркою ($(grep -m1 '^nvenc_ver=' "$log" || echo 'nvenc_ver=?'))." \
          "NVENC works with this build ($(grep -m1 '^nvenc_ver=' "$log" || echo 'nvenc_ver=?')).")"
}

stage_install() {
  local log="$BUILD_ROOT/stage-install.log" f
  say "$(t "Встановлення у тимчасовий каталог (без root)" "Installing into a temporary directory (without root)")"
  rm_cache_path "$STAGE_DIR"
  mkdir -p -- "$STAGE_DIR"
  if ! DESTDIR="$STAGE_DIR" cmake --install "$BUILD_DIR" --strip >"$log" 2>&1; then
    tail -n 40 "$log" >&2
    die "$(t "Тимчасове встановлення не вдалося. Нічого не змінено. Журнал: $log" \
             "The temporary installation failed. Nothing was changed. Log: $log")"
  fi
  STAGED_TREE="$STAGE_DIR$PREFIX"
  [[ -x "$STAGED_TREE/bin/obs" ]] || die "$(t "У зібраних файлах немає bin/obs." "The built files have no bin/obs.")"
  [[ -n "$(find "$STAGED_TREE" -type f -name 'obs-browser.so' -print -quit)" ]] \
    || die "$(t "Немає плагіна obs-browser — Browser Source не зібрано." \
                "No obs-browser plugin — Browser Source was not built.")"
  [[ -n "$(find "$STAGED_TREE" -type f -name 'obs-nvenc.so' -print -quit)" ]] \
    || die "$(t "Немає плагіна obs-nvenc." "No obs-nvenc plugin.")"
  strip_debug_info "$STAGED_TREE"
  # Бібліотеки CEF ставляться з правами 644; у пакетах бібліотеки — 755
  # (makepkg лише за ними визначає версії залежностей-sonames).
  while IFS= read -r -d '' f; do
    if is_elf "$f"; then chmod 755 -- "$f"; fi
  done < <(find "$STAGED_TREE" -type f -name '*.so*' ! -perm -u=x -print0)
  mkdir -p -- "$STAGED_TREE/share/obs-legacy"
  cat > "$STAGED_TREE/share/obs-legacy/BUILDINFO.txt" <<EOF
label=$LABEL
obs_version=$OBS_DESCRIBE
obs_commit=$OBS_COMMIT
obs_ref=$OBS_REF
cef_build=$CEF_BUILD_RESOLVED
nv_headers_tag=$NV_HEADERS_RESOLVED
nvenc_api_headers=$HEADERS_API
nvenc_api_driver=${DRIVER_NVENC_API:-unknown}
nvenc_test=${NVENC_TEST_RESULT:-unknown}
tls=$TLS_MODE
distro=$DISTRO_ID
cpu_model=$CPU_MODEL
cpu_march=$CPU_MARCH
cpu_tuning=$CPU_TUNING
isa_level=$ISA_LEVEL
opt_level=$OPT_LEVEL
cflags=$OPT_CFLAGS
gpu=$GPU_NAME
gpu_cc=$GPU_CC
driver=$DRIVER_VERSION
driver_pkg=$DRIVER_PKG
profile=$PROFILE_SIG
built=$(date -Is)
builder=$SELF_NAME $SCRIPT_VERSION
recipe=$RECIPE_VERSION
EOF
  ok "$(t "Тимчасове встановлення: $STAGED_TREE" "Temporary installation: $STAGED_TREE")"
}

# CEF з архіву OBS приходить з налагоджувальною інформацією (libcef.so — ~2 ГБ),
# а cmake --install --strip чіпає лише власні цілі OBS. Прибираємо лише
# .debug_*: код не змінюється, а пакет меншає в рази.
strip_debug_info() {   # $1 = дерево
  local f before after n=0 sections
  before=$(du -sm -- "$1" 2>/dev/null | cut -f1 || true)
  while IFS= read -r -d '' f; do
    is_elf "$f" || continue
    sections=$(LC_ALL=C readelf -S -W -- "$f" 2>/dev/null || true)
    [[ "$sections" == *'.debug_info'* ]] || continue
    if strip --strip-debug -- "$f" 2>/dev/null; then
      n=$(( n + 1 ))
    else
      warn "$(t "Не вдалося прибрати налагоджувальну інформацію з ${f#"$1"/} (не критично)." \
                "Could not strip the debug info from ${f#"$1"/} (not critical).")"
    fi
  done < <(find "$1" -type f \( -name '*.so' -o -name '*.so.*' -o -perm -u=x \) -print0)
  after=$(du -sm -- "$1" 2>/dev/null | cut -f1 || true)
  if (( n )); then
    ok "$(t "Налагоджувальну інформацію прибрано з $n файлів (CEF): ${before:-?} МБ → ${after:-?} МБ." \
            "Debug info stripped from $n files (CEF): ${before:-?} MB → ${after:-?} MB.")"
  fi
}

# ---------------------------------------------------------------------------
# Плагіни: збираються з вихідних кодів саме під цю збірку OBS (проти її libobs
# і obs-frontend-api з тимчасового дерева) і потрапляють у той самий каталог
# версії — тож оновлюються, відкочуються і видаляються разом з OBS.
# ---------------------------------------------------------------------------
plugin_record() {   # $1 = id → рядок каталогу
  local r
  for r in "${PLUGIN_CATALOG[@]}"; do
    if [[ "${r%%|*}" == "$1" ]]; then printf '%s\n' "$r"; return 0; fi
  done
  return 1
}

plugin_field() {   # $1 = id, $2 = name|repo|pkgs|cmake|desc|aur
  local r
  local -a f=()
  r=$(plugin_record "$1") || return 1
  IFS='|' read -r -a f <<<"$r"
  case "$2" in
    name)  printf '%s\n' "${f[1]-}" ;;
    repo)  printf '%s\n' "${f[2]-}" ;;
    pkgs)  printf '%s\n' "${f[3]-}" ;;
    cmake) printf '%s\n' "${f[4]-}" ;;
    desc)  printf '%s\n' "${f[5]-}" ;;
    aur)   printf '%s\n' "${f[6]:-obs-$1}" ;;
    *)     return 1 ;;
  esac
}

# Назва пакета плагіна: назва в AUR + приписка (obs-multi-rtmp-legacy-580xx).
plugin_pkgname() {   # $1 = id
  local aur
  aur=$(plugin_field "$1" aur)
  printf '%s-%s\n' "${aur%% *}" "$PKG_SUFFIX"
}

# OBS_PLUGINS → PLUGINS_SELECTED (без повторів і невідомих назв).
resolve_selected_plugins() {
  local id
  local -a ids=()
  PLUGINS_SELECTED=()
  read -r -a ids <<<"${OBS_PLUGINS//,/ }"
  for id in "${ids[@]}"; do
    [[ "$id" != none ]] || continue
    if ! plugin_record "$id" >/dev/null; then
      warn "$(t "Невідомий плагін '$id' в OBS_PLUGINS — пропускаю (каталог: $SELF_PATH plugins)." \
                "Unknown plugin '$id' in OBS_PLUGINS — skipping (catalog: $SELF_PATH plugins).")"
      continue
    fi
    if [[ " ${PLUGINS_SELECTED[*]} " == *" $id "* ]]; then continue; fi
    PLUGINS_SELECTED+=("$id")
  done
}

plugin_packages() {   # пакети pacman, потрібні вибраним плагінам
  local id p
  local -a pk=()
  for id in "${PLUGINS_SELECTED[@]}"; do
    p=$(plugin_field "$id" pkgs)
    if [[ -n "$p" ]]; then
      read -r -a pk <<<"$p"
      printf '%s\n' "${pk[@]}"
    fi
  done
}

plugin_ref_override() {   # $1 = id → значення PLUGIN_REF_<id>
  local var="PLUGIN_REF_${1//-/_}"
  printf '%s' "${!var:-}"
}

# Готує git-кеш плагіна. Це лише кеш скрипта, тому розбіжності не
# з'ясовуються, а виправляються: збірка деяких плагінів пише файли у
# вихідні коди (напр., version.h), і їх треба прибрати перед новою збіркою.
plugin_prepare_repo() {   # $1 = каталог, $2 = URL
  local dir=$1 url=$2 origin=''
  if [[ -d "$dir/.git" ]]; then
    origin=$(git -C "$dir" remote get-url origin 2>/dev/null || true)
  fi
  if [[ -e "$dir" && "$origin" != "$url" ]]; then
    rm_cache_path "$dir"
  fi
  if [[ ! -d "$dir/.git" ]]; then
    mkdir -p -- "${dir%/*}"
    net_retry clone_fresh "$dir" --quiet "$url" || return 1
  fi
  net_retry git -C "$dir" fetch --quiet --prune --tags --force origin '+refs/heads/*:refs/remotes/origin/*' || return 1
}

# Друкує "ref<TAB>версія" — що саме збирати.
resolve_plugin_ref() {   # $1 = каталог, $2 = id
  local src=$1 id=$2 ref tag
  ref=$(plugin_ref_override "$id")
  if [[ -n "$ref" ]]; then
    if git -C "$src" show-ref --verify -q "refs/tags/$ref"; then
      printf 'refs/tags/%s\t%s\n' "$ref" "$ref"
    elif git -C "$src" show-ref --verify -q "refs/remotes/origin/$ref"; then
      printf 'refs/remotes/origin/%s\t%s\n' "$ref" "$ref"
    elif git -C "$src" rev-parse -q --verify "$ref^{commit}" >/dev/null 2>&1; then
      printf '%s\t%s\n' "$ref" "$ref"
    else
      return 1
    fi
    return 0
  fi
  # Найновіший тег-реліз; префікс "v" не впливає на порівняння версій.
  tag=$(git -C "$src" tag -l | grep -E '^v?[0-9]+(\.[0-9]+)+$' \
        | awk '{ v = $0; sub(/^v/, "", v); print v "\t" $0 }' | sort -t $'\t' -k1,1V | tail -n1 | cut -f2 || true)
  if [[ -n "$tag" ]]; then
    printf 'refs/tags/%s\t%s\n' "$tag" "$tag"
    return 0
  fi
  # Релізів немає — головна гілка репозиторію.
  ref=$(git -C "$src" symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null || true)
  [[ -n "$ref" ]] || return 1
  printf 'refs/remotes/%s\t%s\n' "$ref" "${ref#origin/}"
}

# Оновлює вихідні коди плагіна і переходить на потрібну версію.
# Друкує "версія<TAB>коміт".
plugin_checkout() {   # $1 = id
  local id=$1 repo src line ref label commit
  repo=$(plugin_field "$id" repo)
  src="$PLUGINS_DIR/$id/src"
  if ! plugin_prepare_repo "$src" "$repo"; then
    fail "$(t "Не вдалося завантажити $repo" "Could not download $repo")"
    hint "$(t "Схоже на збій мережі чи GitHub. Наступне $(install_action) спробує знову." \
              "Looks like a network or GitHub failure. The next $(install_action) will try again.")"
    return 1
  fi
  line=$(resolve_plugin_ref "$src" "$id") \
    || { fail "$(t "Для $id не знайдено версію $(plugin_ref_override "$id")" \
                   "Version $(plugin_ref_override "$id") was not found for $id")"; return 1; }
  ref=${line%%$'\t'*} label=${line#*$'\t'}
  commit=$(git -C "$src" rev-parse "$ref^{commit}") || return 1
  git -C "$src" -c advice.detachedHead=false checkout --quiet --force --detach "$commit" || return 1
  git -C "$src" clean -q -ffdx || return 1
  git -C "$src" submodule --quiet sync --recursive || return 1
  net_retry git -C "$src" submodule --quiet update --init --recursive --force --jobs 4 || return 1
  git -C "$src" submodule --quiet foreach --recursive 'git reset -q --hard && git clean -q -ffdx' || return 1
  printf '%s\t%s\n' "$label" "$commit"
}

# stdin — і на екран (stderr), і в журнал (без кольорів).
tee_plain() {   # $1 = журнал
  local l
  while IFS= read -r l || [[ -n "$l" ]]; do
    printf '%s\n' "$l" >&2
    printf '%s\n' "$l" | sed -E 's/\x1B\[[0-9;]*[A-Za-z]//g' >> "$1"
  done
}

# Конфігурує й компілює плагін із заданими прапорцями. Нічого не друкує як
# помилку: що робити з невдачею, вирішує build_plugin.
plugin_compile() {   # $1 = id, $2 = CFLAGS, $3 = CXXFLAGS, далі — параметри CMake плагіна
  local id=$1 cf=$2 cxf=$3 bld="$PLUGINS_DIR/$1/build" log="$PLUGINS_DIR/$1/build.log"
  local -a flags=()
  shift 3
  mapfile -t flags < <(profile_cmake_flags "$cf" "$cxf")
  rm_cache_path "$bld"
  mkdir -p -- "$bld"
  # QT_VERSION=6: старі шаблони плагінів інакше можуть підхопити Qt5, а такий
  # плагін аварійно завершить OBS. LINUX_PORTABLE=OFF: системна розкладка
  # lib/obs-plugins замість портативної obs-plugins/64bit.
  if ! cmake -S "$PLUGINS_DIR/$id/src" -B "$bld" -G Ninja -Wno-dev \
       "${flags[@]}" \
       "-DCMAKE_INSTALL_PREFIX=$PREFIX" \
       -DCMAKE_INSTALL_LIBDIR=lib \
       "-DCMAKE_PREFIX_PATH=$STAGED_TREE" \
       -DCMAKE_COMPILE_WARNING_AS_ERROR=OFF \
       -DQT_VERSION=6 \
       -DLINUX_PORTABLE=OFF \
       "$@" >>"$log" 2>&1; then
    tail -n 30 "$log" >&2
    printf "$(t '  CMake не зміг сконфігурувати %s (журнал: %s).\n' \
                '  CMake could not configure %s (log: %s).\n')" "$id" "$log" >&2
    return 1
  fi
  # Вивід компіляції — і на екран (прогрес), і в журнал плагіна (там шукатимуть помилку).
  nice -n 10 cmake --build "$bld" --parallel "$JOBS" 2>&1 | tee -a -- "$log"
}

# Нові OBS і компілятори приносять нові попередження: застарілий API OBS, const у
# C23 (glibc 2.42+: strchr від const char* повертає const char*). Частина плагінів
# збирається з власним -Werror, і через це оновлення OBS ламало б плагін, який
# працює. -Wno-error=… діє незалежно від того, стоїть -Werror до чи після нього.
readonly PLUGIN_WARN_FLAGS="-Wno-error=deprecated-declarations -Wno-error=discarded-qualifiers"

# Qt 6.10+ створює Qt6::GuiPrivate (та інші *Private) лише після
# find_package(Qt6 COMPONENTS GuiPrivate), а старші версії плагінів посилаються на
# ці цілі напряму. Якщо плагін їх згадує, підключаємо їх самі — файлом, який CMake
# виконує після project() (CMAKE_PROJECT_INCLUDE). Друкує параметр CMake або нічого.
plugin_qt_private_arg() {   # $1 = id
  local src="$PLUGINS_DIR/$1/src" inc="$PLUGINS_DIR/$1/qt-private.cmake" comps
  comps=$(grep -rhoE --include=CMakeLists.txt --include='*.cmake' 'Qt6?::[A-Za-z]+Private' -- "$src" 2>/dev/null \
            | sed 's/.*:://' | sort -u | tr '\n' ' ' || true)
  [[ -n "${comps// /}" ]] || return 0
  printf '# %s: приватні модулі Qt для плагіна %s.\nfind_package(Qt6 QUIET COMPONENTS %s)\n' \
    "$MARKER" "$1" "${comps% }" > "$inc"
  printf '%s\n' "-DCMAKE_PROJECT_INCLUDE=$inc"
}

# Збирає плагін в окремий каталог і перевіряє його. Пакується він окремо
# (<назва>-legacy-580xx). Викликається в підоболонці: будь-яка помилка лише
# позначає плагін як невдалий і не чіпає збірку OBS.
build_plugin() {   # $1 = id, $2 = файл результатів
  local id=$1 results=$2 line label commit bld pstage log tried tok f rel oid lic pkg qt_arg
  local cflags=$OPT_CFLAGS cxxflags=$OPT_CXXFLAGS extra_c=" $PLUGIN_WARN_FLAGS" extra_cxx=" $PLUGIN_WARN_FLAGS"
  local -a extra=() args=() clash=() others=()
  bld="$PLUGINS_DIR/$id/build" pstage="$PLUGINS_DIR/$id/stage" log="$PLUGINS_DIR/$id/build.log"
  tried="$PLUGINS_DIR/$id/tried-commit"
  say "$(t "Плагін $id ($(plugin_field "$id" name))" "Plugin $id ($(plugin_field "$id" name))")"
  # Журнал — лише цієї спроби, разом з помилками завантаження: попередження про
  # невдачу посилається саме на нього.
  mkdir -p -- "$PLUGINS_DIR/$id"
  : > "$log"
  rm -f -- "$tried"
  line=$(plugin_checkout "$id" 2> >(tee_plain "$log")) || return 1
  label=${line%%$'\t'*} commit=${line#*$'\t'}
  # Коміт, який пробуємо зібрати: не вийде — package_set запише його в
  # plugin_failed.<id>. Не завантажилося — файлу немає, і «Оновити» спробує знову.
  printf '%s\n' "$commit" > "$tried"
  printf "$(t '  Версія: %s (%s)\n' '  Version: %s (%s)\n')" "$label" "${commit:0:12}" | tee_plain "$log"

  read -r -a extra <<<"$(plugin_field "$id" cmake)"
  for tok in "${extra[@]}"; do
    if [[ "$tok" =~ ^\<([0-9]+)\?(.+)$ ]]; then
      if [[ "$OBS_MAJOR" =~ ^[0-9]+$ ]] && (( OBS_MAJOR < BASH_REMATCH[1] )); then
        args+=("${BASH_REMATCH[2]}")
      fi
    elif [[ "$tok" == CXXFLAGS+=* ]]; then
      extra_cxx+=" ${tok#CXXFLAGS+=}"
    elif [[ "$tok" == CFLAGS+=* ]]; then
      extra_c+=" ${tok#CFLAGS+=}"
    else
      args+=("$tok")
    fi
  done
  cflags+=$extra_c cxxflags+=$extra_cxx
  qt_arg=$(plugin_qt_private_arg "$id")
  [[ -z "$qt_arg" ]] || args+=("$qt_arg")

  rm_cache_path "$pstage"
  if ! plugin_compile "$id" "$cflags" "$cxxflags" "${args[@]}"; then
    # Прапорці дистрибутива (зміцнення, -Werror=format-security) інколи
    # ламають збирання старих плагінів. Друга спроба — лише -march і -O.
    warn "$(t "Плагін $id не зібрався з прапорцями дистрибутива — пробую ще раз лише з $MARCH_FLAGS -O$OPT_LEVEL." \
              "Plugin $id did not build with the distribution flags — trying again with only $MARCH_FLAGS -O$OPT_LEVEL.")"
    printf "$(t '\n==== Друга спроба: лише %s -O%s ====\n' \
                '\n==== Second attempt: only %s -O%s ====\n')" "$MARCH_FLAGS" "$OPT_LEVEL" >> "$log"
    if ! plugin_compile "$id" "$MARCH_FLAGS -O$OPT_LEVEL$extra_c" "$MARCH_FLAGS -O$OPT_LEVEL$extra_cxx" "${args[@]}"; then
      fail "$(t "Не вдалося зібрати $id (журнал: $log)." "Could not build $id (log: $log).")"
      return 1
    fi
  fi
  if ! DESTDIR="$pstage" cmake --install "$bld" --strip >>"$log" 2>&1; then
    tail -n 20 "$log" >&2
    fail "$(t "Встановлення $id не вдалося (журнал: $log)." "Installing $id failed (log: $log).")"
    return 1
  fi
  [[ -d "$pstage$PREFIX" ]] || { fail "$(t "$id встановився не в префікс $PREFIX." \
                                           "$id was not installed into the prefix $PREFIX.")"; return 1; }
  if [[ -z "$(find "$pstage$PREFIX/lib" -type f -name '*.so' \
                \( -path '*/obs-plugins/*' -o -path '*/obs-modules/plugins/*' \) -print -quit 2>/dev/null)" ]]; then
    fail "$(t "$id не встановив модуль OBS (lib/obs-plugins або lib/obs-modules/plugins)." \
              "$id did not install an OBS module (lib/obs-plugins or lib/obs-modules/plugins).")"
    return 1
  fi
  # Плагін не має перезаписувати файли OBS чи інших плагінів (у pacman це
  # був би конфлікт файлів між пакетами).
  mapfile -t others < <(cut -f1 -- "$results" 2>/dev/null || true)
  while IFS= read -r -d '' f; do
    rel=${f#"$pstage$PREFIX"/}
    if [[ -e "$STAGED_TREE/$rel" || -L "$STAGED_TREE/$rel" ]]; then clash+=("$rel"); continue; fi
    for oid in "${others[@]}"; do
      if [[ -n "$oid" && ( -e "$PLUGINS_DIR/$oid/stage$PREFIX/$rel" || -L "$PLUGINS_DIR/$oid/stage$PREFIX/$rel" ) ]]; then
        clash+=("$rel ($oid)")
        break
      fi
    done
  done < <(find "$pstage$PREFIX" \( -type f -o -type l \) -print0)
  if ((${#clash[@]})); then
    fail "$(t "$id перезаписав би наявні файли: ${clash[*]:0:5}" "$id would overwrite existing files: ${clash[*]:0:5}")"
    return 1
  fi
  # Відомості для TUI і сповіщень (версія плагіна) та ліцензія — у пакеті плагіна.
  pkg=$(plugin_pkgname "$id")
  mkdir -p -- "$pstage$PREFIX/share/obs-legacy/plugins"
  printf 'plugin.%s=%s %s\n' "$id" "$label" "${commit:0:12}" > "$pstage$PREFIX/share/obs-legacy/plugins/$id"
  lic=$(find "$PLUGINS_DIR/$id/src" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' \) -print -quit 2>/dev/null || true)
  if [[ -n "$lic" ]]; then
    install -D -m 644 -- "$lic" "$pstage$PREFIX/share/licenses/$pkg/${lic##*/}"
  fi
  printf '%s\t%s\t%s\n' "$id" "$label" "$commit" >> "$results"
}

build_plugins() {
  local id label commit results="$PLUGINS_DIR/results.tsv"
  PLUGINS_OK=() PLUGINS_FAILED=()
  if ((${#PLUGINS_SELECTED[@]} == 0)); then
    say "$(t "Плагіни: не вибрано (OBS_PLUGINS=none)" "Plugins: none selected (OBS_PLUGINS=none)")"
    return 0
  fi
  mkdir -p -- "$PLUGINS_DIR"
  : > "$results"
  for id in "${PLUGINS_SELECTED[@]}"; do
    if ( build_plugin "$id" "$results" ); then
      ok "$(t "Плагін $id зібрано." "Plugin $id built.")"
    else
      PLUGINS_FAILED+=("$id")
      warn "$(t "Плагін $id не зібрався — у цій збірці його не буде. Журнал: $PLUGINS_DIR/$id/build.log" \
                "Plugin $id did not build — it will not be in this build. Log: $PLUGINS_DIR/$id/build.log")"
    fi
  done
  while IFS=$'\t' read -r id label commit; do
    [[ -n "$id" ]] || continue
    PLUGINS_OK+=("$id")
  done < "$results"
  if ((${#PLUGINS_FAILED[@]})); then
    if [[ "$PLUGINS_STRICT" == 1 ]]; then
      die "$(t "Не зібралися плагіни: ${PLUGINS_FAILED[*]} (PLUGINS_STRICT=1). Нічого не встановлено." \
               "Plugins failed to build: ${PLUGINS_FAILED[*]} (PLUGINS_STRICT=1). Nothing was installed.")" \
        "$(t "Зніми галочку з цих плагінів (пункт «Плагіни») або вимкни «Суворо з плагінами» в Налаштуваннях. Журнали: $PLUGINS_DIR/<id>/build.log" \
             "Untick these plugins (the “Plugins” item) or turn off “Strict about plugins” in Settings. Logs: $PLUGINS_DIR/<id>/build.log")"
    fi
  fi
}

# Для check: завантажує вихідні коди плагінів і показує, що буде зібрано.
check_plugins() {
  local id line
  say "$(t "Плагіни" "Plugins")"
  if ((${#PLUGINS_SELECTED[@]} == 0)); then
    ok "$(t "Плагіни не вибрано (OBS_PLUGINS=none)." "No plugins selected (OBS_PLUGINS=none).")"
    return 0
  fi
  mkdir -p -- "$PLUGINS_DIR"
  for id in "${PLUGINS_SELECTED[@]}"; do
    if line=$( plugin_checkout "$id" ); then
      printf "$(t '  %s: буде зібрано %s (%s)\n' \
                  '  %s: %s will be built (%s)\n')" "$id" "${line%%$'\t'*}" "$(cut -c1-12 <<<"${line#*$'\t'}")" >&2
    else
      warn "$(t "Плагін $id: не вдалося підготувати вихідні коди (install збере OBS без нього)." \
                "Plugin $id: could not prepare the sources (install will build OBS without it).")"
    fi
  done
}

# Найновіший тег-реліз плагіна на сервері (для status).
plugin_latest_remote() {   # $1 = id
  local repo
  repo=$(plugin_field "$1" repo) || return 1
  timeout 20 git ls-remote --tags --refs "$repo" 2>/dev/null | awk -F/ '{print $3}' \
    | grep -E '^v?[0-9]+(\.[0-9]+)+$' | awk '{ v = $0; sub(/^v/, "", v); print v "\t" $0 }' \
    | sort -t $'\t' -k1,1V | tail -n1 | cut -f2
}

# ---------------------------------------------------------------------------
# Перевірка бібліотек
# ---------------------------------------------------------------------------
is_elf() {
  local m
  m=$(head -c 4 -- "$1" 2>/dev/null | od -An -tx1 2>/dev/null | tr -d ' \n' || true)
  [[ "$m" == 7f454c46 ]]
}

# Друкує проблеми завантаження ELF-файлу, по одній на рядок:
#   libfoo.so.1              — бібліотеку не знайдено;
#   libQt6Core.so.6 (Qt_6.9) — бібліотека є, але без потрібної версії символів
#                              (буває після оновлення Qt, ffmpeg тощо).
# Повідомлення про версії ldd пише в stderr, тому stderr теж аналізується.
ldd_problems() {
  LC_ALL=C ldd "$1" 2>&1 | sed -nE \
    -e 's/^[[:space:]]*([^[:space:]]+) => not found.*$/\1/p' \
    -e "s/^.*: ([^:]*\/)?([^:\/]+): version \`([^']+)' not found.*$/\2 (\3)/p" || true
}

# Шукає відсутні бібліотеки в усіх ELF-файлах дерева (…/usr). Бібліотеки самого
# дерева й додаткових дерев (OBS — для плагінів) знаходяться через
# LD_LIBRARY_PATH: після встановлення вони будуть у /usr/lib. Для плагінів (.so)
# ігнорує ще й бібліотеки, які є в цих деревах деінде: під час роботи їх уже
# завантажив процес OBS.
verify_tree() {   # $1 = дерево, далі — додаткові дерева
  local dir=$1 d f lib rel libs ldpath=''
  local -A internal=()
  local -a problems=()
  say "$(t "Перевіряю бібліотеки (ldd): $dir" "Checking libraries (ldd): $dir")"
  for d in "$@"; do
    ldpath+="${ldpath:+:}$d/lib"
    while IFS= read -r -d '' f; do internal["${f##*/}"]=1; done \
      < <(find "$d" \( -type f -o -type l \) -name '*.so*' -print0 2>/dev/null)
  done
  while IFS= read -r -d '' f; do
    is_elf "$f" || continue
    rel=${f#"$dir"/}
    while IFS= read -r lib; do
      [[ -n "$lib" ]] || continue
      if [[ "$lib" != *' ('* ]] && [[ "$f" == *.so || "$f" == *.so.* ]] && [[ -n "${internal[$lib]:-}" ]]; then
        continue
      fi
      problems+=("$rel → $lib")
    done < <(LD_LIBRARY_PATH=$ldpath ldd_problems "$f")
  done < <(find "$dir" -type f \( -name '*.so' -o -name '*.so.*' -o -perm -u=x \) -print0 2>/dev/null)
  if ((${#problems[@]})); then
    fail "$(t "Бракує бібліотек:" "Missing libraries:")"
    printf '    %s\n' "${problems[@]}" >&2
    libs=$(printf '%s\n' "${problems[@]}" | awk -F' → ' '{print $2}' | sort -u | tr '\n' ' ')
    warn "$(t "Відсутні: $libs" "Missing: $libs")"
    warn "$(t "Який пакет їх містить: pacman -F <назва>  (база файлів оновлюється командою sudo pacman -Fy)." \
              "Which package has them: pacman -F <name>  (the file database is updated with sudo pacman -Fy).")"
    return 1
  fi
  ok "$(t "Усі бібліотеки знаходяться." "All libraries are found.")"
}

quick_check_binary() {
  [[ -x "$1" ]] || return 1
  LC_ALL=C ldd "$1" >/dev/null 2>&1 || return 1
  [[ -z "$(ldd_problems "$1")" ]]
}

# ---------------------------------------------------------------------------
# Стара схема (до пакетів): збірки в /opt, обгортка /usr/local/bin/obs,
# ярлик-перекриття й хук у /etc. Під час встановлення пакетів усе це
# прибирається; відкат між старими збірками працює, доки пакетів немає.
# ---------------------------------------------------------------------------
list_versions() {
  [[ -d "$VERSIONS_DIR" ]] || return 0
  find "$VERSIONS_DIR" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort || true
}

version_in_use() {   # чи запущено зараз щось із цієї збірки
  local d="$VERSIONS_DIR/$1" p exe
  for p in /proc/[0-9]*; do
    exe=$(readlink -- "$p/exe" 2>/dev/null) || continue
    if [[ "$exe" == "$d"/* ]]; then return 0; fi
  done
  return 1
}

activate_version() {
  local label=$1
  [[ "$label" =~ ^[A-Za-z0-9._-]+$ && -f "$VERSIONS_DIR/$label/.complete" ]] \
    || die "$(t "Збірка $label відсутня або неповна." "Build $label is missing or incomplete.")"
  # Атомарна заміна симлінка: OBS ніколи не бачить "напівактивної" збірки.
  priv ln -sfn -- "versions/$label" "$INSTALL_ROOT/.current.new"
  priv mv -Tf -- "$INSTALL_ROOT/.current.new" "$CURRENT_LINK"
  ok "$(t "Активна збірка: $label" "Active build: $label")"
}

restore_backup() {   # $1 = wrapper|desktop, $2 = шлях, $3 = права, $4 = priv|user
  local kind=$1 target=$2 mode=$3 how=$4 flag bak
  local -a run=()
  flag="$STATE_DIR/$kind-original-existed.txt"
  bak="$STATE_DIR/$kind-original.backup"
  if [[ "$how" == priv ]]; then run=("$SUDO_CMD"); fi
  if [[ -e "$target" ]] && ! grep -qs -- "$MARKER" "$target"; then
    warn "$(t "$target змінено не цим скриптом — залишаю як є." \
              "$target was changed by something other than this script — leaving it as is.")"
  elif [[ "$(cat -- "$flag" 2>/dev/null || true)" == yes && -f "$bak" ]]; then
    "${run[@]}" install -D -m "$mode" -- "$bak" "$target"
    ok "$(t "Відновлено попередній $target" "Restored the previous $target")"
  elif [[ -e "$target" ]]; then
    "${run[@]}" rm -f -- "$target"
    ok "$(t "Прибрано $target" "Removed $target")"
  fi
  rm -f -- "$flag" "$bak"
}

foreign_file() {   # $1 = шлях → 0, якщо файл існує і створений не цим скриптом
  [[ -e "$1" ]] && ! grep -qs -- "$MARKER" "$1"
}

# Старий хук у /etc (тепер хук лежить у пакеті OBS).
remove_pacman_hook() {
  local f removed=0
  for f in "$HOOK_FILE" "$LIBCHECK"; do
    if [[ -e "$f" ]] && ! foreign_file "$f"; then
      priv rm -f -- "$f"
      removed=1
    fi
  done
  if [[ -d "${LIBCHECK%/*}" ]]; then priv rmdir -- "${LIBCHECK%/*}" 2>/dev/null || true; fi
  if (( removed )); then ok "$(t "Старий pacman-хук прибрано (тепер він у пакеті $OBS_PKG)." \
                                 "The old pacman hook was removed (it is now in the $OBS_PKG package).")"; fi
}

# Прибирає стару схему: обгортку (повертає попередній /usr/local/bin/obs, якщо
# він був), ярлик-перекриття, хук у /etc і каталог /opt зі старими збірками.
remove_legacy_install() {
  local desktop="$DATA_HOME/applications/$DESKTOP_ID" l busy=''
  if [[ -f "$STATE_DIR/desktop-path.txt" ]]; then desktop=$(<"$STATE_DIR/desktop-path.txt"); fi
  say "$(t "Прибираю стару схему встановлення ($INSTALL_ROOT, обгортку, ярлик)" \
           "Removing the old installation layout ($INSTALL_ROOT, the wrapper, the shortcut)")"
  if grep -qs -- "$MARKER" "$WRAPPER" || [[ -f "$STATE_DIR/wrapper-original-existed.txt" ]]; then
    restore_backup wrapper "$WRAPPER" 755 priv
  fi
  if grep -qs -- "$MARKER" "$desktop" || [[ -f "$STATE_DIR/desktop-original-existed.txt" ]]; then
    restore_backup desktop "$desktop" 644 user
    if have update-desktop-database; then update-desktop-database -q "${desktop%/*}" 2>/dev/null || true; fi
  fi
  remove_pacman_hook
  rm -f -- "$STATE_DIR/desktop-path.txt"
  if legacy_install_present; then
    while IFS= read -r l; do
      if [[ -n "$l" ]] && version_in_use "$l"; then busy=$l; fi
    done < <(list_versions)
    if [[ -n "$busy" ]]; then
      warn "$(t "OBS зі старої збірки $busy зараз запущений — $INSTALL_ROOT прибереться наступного разу." \
                "OBS from the old build $busy is running right now — $INSTALL_ROOT will be removed next time.")"
    elif [[ "$(readlink -f -- "$INSTALL_ROOT")" != "$INSTALL_ROOT" ]]; then
      warn "$(t "Шлях $INSTALL_ROOT веде деінде — не видаляю." \
                "The path $INSTALL_ROOT points somewhere else — not deleting it.")"
    else
      priv rm -rf --one-file-system -- "$INSTALL_ROOT"
      ok "$(t "Прибрано старі збірки: $INSTALL_ROOT" "Removed the old builds: $INSTALL_ROOT")"
    fi
  fi
  hash -r
}

# ---------------------------------------------------------------------------
# pacman-хук у пакеті OBS: після оновлень системи перевіряє, чи пакетам
# OBS-Legacy вистачає бібліотек і чи не вийшов новий ffmpeg. Нічого не змінює.
# ---------------------------------------------------------------------------
rebuild_hint() {
  if have obs-legacy; then
    printf "$(t 'obs-legacy → «Встановити / оновити OBS»' 'obs-legacy → “Install / update OBS”')"
  else
    printf '%s install' "$SELF_PATH"
  fi
}

write_pkg_libcheck() {   # $1 = файл
  {
    printf '#!/usr/bin/env bash\n'
    printf '# %s\n' "$MARKER"
    printf "$(t '# Викликається pacman-хуком %s після оновлень і видалень пакетів.\n' \
                '# Called by the pacman hook %s after package upgrades and removals.\n')" "$PKG_HOOK"
    printf "$(t '# Лише перевіряє, чи пакети OBS-Legacy (*-%s) знаходять свої бібліотеки\n' \
                '# Only checks whether the OBS-Legacy packages (*-%s) find their libraries\n')" "$PKG_SUFFIX"
    printf "$(t '# і чи не вийшов у репозиторії новий ffmpeg; нічого не змінює.\n' \
                '# and whether a new ffmpeg is out in the repository; changes nothing.\n')"
    printf 'suffix=%q\n' "$PKG_SUFFIX"
    printf 'info=%q\n' "$INFO_DIR"
    printf 'hint_uk=%q\n' "$(LANG_EN=0; rebuild_hint)"
    printf 'hint_en=%q\n' "$(LANG_EN=1; rebuild_hint)"
    cat <<'EOF'
# Мова — за локаллю pacman (хук працює від root, налаштувань користувача не бачить).
loc=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
if [[ -z "$loc" && -r /etc/locale.conf ]]; then
  loc=$(sed -n 's/^LANG=//p' /etc/locale.conf 2>/dev/null | tr -d "\"'" | head -n1 || true)
fi
if [[ "$loc" == uk* ]]; then
  hint=$hint_uk
  m_libs='пакетам OBS-Legacy після оновлення бракує бібліотек:'
  m_more='      … і ще %d\n'
  m_rebuild='    Перезбери OBS: %s\n\n'
  m_ffmpeg='у репозиторії новий ffmpeg %s (ffmpeg-%s зібрано з %s).'
  m_ffmpeg2='    Перезбери його: %s\n\n'
else
  hint=$hint_en
  m_libs='the OBS-Legacy packages are missing libraries after the upgrade:'
  m_more='      … and %d more\n'
  m_rebuild='    Rebuild OBS: %s\n\n'
  m_ffmpeg='a new ffmpeg %s is in the repository (ffmpeg-%s was built from %s).'
  m_ffmpeg2='    Rebuild it: %s\n\n'
fi
mapfile -t pkgs < <(pacman -Qq 2>/dev/null | grep -E -- "-${suffix}\$" || true)
((${#pkgs[@]})) || exit 0
problems=()
while IFS= read -r f; do
  case "$f" in */bin/*|*.so|*.so.*) ;; *) continue ;; esac
  [[ -f "$f" && ! -L "$f" ]] || continue
  [[ "$(head -c 4 -- "$f" 2>/dev/null | od -An -tx1 | tr -d ' \n')" == 7f454c46 ]] || continue
  while IFS= read -r lib; do
    if [[ -n "$lib" ]]; then problems+=("$f: $lib"); fi
  done < <(LC_ALL=C ldd "$f" 2>&1 | sed -nE \
    -e 's/^[[:space:]]*([^[:space:]]+) => not found.*$/\1/p' \
    -e "s/^.*: ([^:]*\/)?([^:\/]+): version \`([^']+)' not found.*$/\2 (\3)/p")
done < <(pacman -Qlq "${pkgs[@]}" 2>/dev/null)
if ((${#problems[@]})); then
  printf '\n==> OBS-Legacy: %s\n' "$m_libs"
  printf '%s\n' "${problems[@]}" | sort -u | head -n 15 | sed 's/^/      /'
  # shellcheck disable=SC2059 # формат — з m_more / m_rebuild вище
  if ((${#problems[@]} > 15)); then printf "$m_more" "$(( ${#problems[@]} - 15 ))"; fi
  # shellcheck disable=SC2059
  printf "$m_rebuild" "$hint"
fi
if pacman -Q "ffmpeg-$suffix" >/dev/null 2>&1; then
  repo=$(LC_ALL=C pacman -Si ffmpeg 2>/dev/null | sed -n 's/^Version *: *//p' | head -n1)
  built=$(sed -n 's/^arch_version=//p' "$info/ffmpeg.txt" 2>/dev/null | head -n1)
  if [[ -n "$repo" && -n "$built" ]] && (( $(vercmp "$repo" "$built") > 0 )); then
    # shellcheck disable=SC2059
    printf "\n==> OBS-Legacy: $m_ffmpeg\n" "$repo" "$suffix" "$built"
    # shellcheck disable=SC2059
    printf "$m_ffmpeg2" "$hint"
  fi
fi
exit 0
EOF
  } > "$1"
}

write_pkg_hook() {   # $1 = файл
  cat > "$1" <<EOF
# $MARKER
$(t "# OBS-Legacy: після оновлень пакетів перевіряє, чи пакетам OBS-Legacy
# (*-$PKG_SUFFIX) вистачає бібліотек і чи не вийшов новий ffmpeg. Нічого не змінює.
# Без хука: INSTALL_PACMAN_HOOK=0 і повторне встановлення." \
    "# OBS-Legacy: after package upgrades, checks whether the OBS-Legacy packages
# (*-$PKG_SUFFIX) have all their libraries and whether a new ffmpeg is out. Changes nothing.
# Without the hook: INSTALL_PACMAN_HOOK=0 and install again.")
[Trigger]
Operation = Upgrade
Operation = Remove
Type = Package
Target = *

[Action]
Description = $(t 'OBS-Legacy: перевірка бібліотек OBS і плагінів...' 'OBS-Legacy: checking OBS and plugin libraries...')
When = PostTransaction
Exec = $PKG_LIBCHECK
EOF
}

add_hook_to_tree() {   # $1 = дерево …/usr
  [[ "$INSTALL_PACMAN_HOOK" == 1 ]] || return 0
  install -d -m 755 -- "$1/lib/obs-legacy" "$1/share/libalpm/hooks"
  write_pkg_libcheck "$1/lib/obs-legacy/check-libs"
  chmod 755 -- "$1/lib/obs-legacy/check-libs"
  bash -n "$1/lib/obs-legacy/check-libs" || die "$(t "Внутрішня помилка: згенерований check-libs некоректний." \
                                                     "Internal error: the generated check-libs is invalid.")"
  write_pkg_hook "$1/share/libalpm/hooks/obs-legacy.hook"
}

# ---------------------------------------------------------------------------
# Пакети pacman (-legacy-580xx). Дерева OBS і плагінів уже зібрано й
# перевірено, тож makepkg лише пакує їх (PKGBUILD генерується тут).
# ---------------------------------------------------------------------------
# Версія для pacman: v1.6.1 → 1.6.1; 33.0.0-3-g2735cc4 → 33.0.0.r3.g2735cc4;
# 33.1.0-beta1 → 33.1.0beta1 (передреліз має бути «меншим» за реліз).
pkgver_from() {
  local v=${1#v}
  if [[ "$v" =~ ^(.+)-([0-9]+)-g([0-9a-f]+)$ ]]; then
    v="${BASH_REMATCH[1]}.r${BASH_REMATCH[2]}.g${BASH_REMATCH[3]}"
  fi
  v=${v//-/}
  v=$(printf '%s' "$v" | tr -c 'A-Za-z0-9._+' '_')
  if [[ ! "$v" =~ ^[0-9] ]]; then v="0.$v"; fi
  printf '%s\n' "$v"
}

# Номер збирання — pkgrel усіх пакетів набору: кожен новий набір для pacman
# новіший за попередній, навіть коли версії OBS і плагінів не змінилися.
next_build_number() {
  local f="$STATE_DIR/build-number" n=0 inst
  if [[ -s "$f" ]]; then n=$(<"$f"); fi
  [[ "$n" =~ ^[0-9]+$ ]] || n=0
  # Якщо стан втрачено, не опускаємося нижче встановленого пакета.
  inst=$(pacman -Q "$OBS_PKG" 2>/dev/null | awk '{print $2}' || true)
  inst=${inst##*-} inst=${inst%%.*}
  if [[ "$inst" =~ ^[0-9]+$ ]] && (( inst > n )); then n=$inst; fi
  n=$(( n + 1 ))
  mkdir -p -- "$STATE_DIR"
  printf '%s\n' "$n" > "$f"
  printf '%s\n' "$n"
}

# Ліцензія плагіна за текстом файла LICENSE/COPYING (SPDX).
guess_license() {   # $1 = вихідні коди
  local f text
  f=$(find "$1" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' \) -print -quit 2>/dev/null || true)
  if [[ -z "$f" ]]; then printf 'LicenseRef-unknown\n'; return 0; fi
  text=$(head -c 4000 -- "$f" 2>/dev/null | tr -s '[:space:]' ' ' || true)
  case "$text" in
    *"GNU LESSER GENERAL PUBLIC LICENSE"*) printf 'LGPL-2.1-or-later\n' ;;
    *"GNU GENERAL PUBLIC LICENSE"*"Version 3"*) printf 'GPL-3.0-or-later\n' ;;
    *"GNU GENERAL PUBLIC LICENSE"*"Version 2"*) printf 'GPL-2.0-or-later\n' ;;
    *"MIT License"*|*"Permission is hereby granted, free of charge"*) printf 'MIT\n' ;;
    *"Apache License"*) printf 'Apache-2.0\n' ;;
    *) printf 'LicenseRef-custom\n' ;;
  esac
}

# Чи відкриває дерево libvlc під час роботи (dlopen) — тоді vlc в optdepends.
tree_uses_vlc() {   # $1 = дерево
  local f
  while IFS= read -r -d '' f; do
    if grep -qa 'libvlc\.so' -- "$f" 2>/dev/null; then return 0; fi
  done < <(find "$1" -type f -name '*.so' -print0)
  return 1
}

# Пакети, яким належать бібліотеки, від яких ELF-файли дерева залежать
# напряму (NEEDED; ldd показує й транзитивні — вони належать залежностям
# залежностей). Бібліотеки самих дерев не рахуються; наші пакети-замінники
# записуються стандартною назвою (ffmpeg-legacy-580xx → ffmpeg).
elf_pkg_deps() {   # $1 = дерево …/usr, далі — додаткові дерева
  local d f ldpath='' needed
  local -a libs=() keep=()
  for d in "$@"; do ldpath+="${ldpath:+:}$d/lib"; done
  mapfile -t libs < <(
    while IFS= read -r -d '' f; do
      is_elf "$f" || continue
      needed=$(LC_ALL=C readelf -d -- "$f" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p' | tr '\n' ' ' || true)
      [[ -n "$needed" ]] || continue
      LC_ALL=C LD_LIBRARY_PATH=$ldpath ldd "$f" 2>/dev/null \
        | awk -v need=" $needed " '$2 == "=>" && $3 ~ /^\// && index(need, " " $1 " ") { print $3 }' || true
    done < <(find "$1" -type f \( -name '*.so' -o -name '*.so.*' -o -perm -u=x \) -print0) | sort -u)
  for f in "${libs[@]}"; do
    for d in "$@"; do
      if [[ "$f" == "$d"/* ]]; then continue 2; fi
    done
    keep+=("$f")
  done
  ((${#keep[@]})) || return 0
  pkg_deps_for_libs "${keep[@]}" | sort -u
}

# Залежність для кожної бібліотеки: soname (libjack.so, libavcodec.so), якщо
# власник його «надає» — тоді підійде будь-який пакет з цим soname (pipewire-jack
# чи jack2, ffmpeg чи ffmpeg-legacy-580xx), а makepkg сам допише версію soname;
# інакше — назва пакета-власника.
pkg_deps_for_libs() {   # шляхи до бібліотек
  local f owner base
  local -A provides=()
  while IFS=$'\t' read -r f owner; do
    [[ -n "$owner" ]] || continue
    if [[ -z "${provides[$owner]+x}" ]]; then
      provides[$owner]=" $(LC_ALL=C pacman -Qi -- "$owner" 2>/dev/null | sed -n 's/^Provides *: //p' | head -n1) "
    fi
    base=${f##*/} base=${base%%.so*}.so
    if [[ "${provides[$owner]}" == *" $base="* || "${provides[$owner]}" == *" $base "* ]]; then
      printf '%s\n' "$base"
    else
      printf '%s\n' "${owner%-"$PKG_SUFFIX"}"
    fi
  done < <(LC_ALL=C pacman -Qo -- "$@" 2>/dev/null | sed -nE 's/^(.*) is owned by ([^ ]+) .*$/\1\t\2/p' || true)
}

# Хто вже володіє шляхами нового пакета: «pkg назва» — встановлений пакет
# (pacman має його замінити), «file шлях» — файл, що не належить жодному пакету.
file_owners() {   # $1 = корінь пакета (каталог з usr/)
  local root=$1 f rel
  local -a paths=()
  while IFS= read -r -d '' f; do
    rel=${f#"$root"}
    if [[ -e "$rel" || -L "$rel" ]]; then paths+=("$rel"); fi
  done < <(find "$root" \( -type f -o -type l \) -print0)
  ((${#paths[@]})) || return 0
  LC_ALL=C pacman -Qo -- "${paths[@]}" 2>&1 | sed -nE \
    -e 's/^.* is owned by ([^ ]+) .*$/pkg \1/p' \
    -e 's/^error: No package owns (.*)$/file \1/p' | sort -u || true
}

# Додає до PKG_CONFLICTS власників файлів, які пакет перезапише (pacman сам
# запропонує їх замінити). Файли без власника pacman не перезапише — тоді стоп.
add_file_conflicts() {   # $1 = корінь пакета
  local line
  local -a unowned=()
  while IFS= read -r line; do
    case "$line" in
      'pkg '*)
        line=${line#pkg }
        # Файли між нашими пакетами pacman переносить сам в одній транзакції.
        if [[ "$line" != *"-$PKG_SUFFIX" && " ${PKG_CONFLICTS[*]} " != *" $line "* ]]; then
          PKG_CONFLICTS+=("$line")
        fi
        ;;
      'file '*) unowned+=("${line#file }") ;;
    esac
  done < <(file_owners "$1")
  if ((${#unowned[@]})); then
    fail "$(t "У системі вже є файли пакета $PKG_NAME, що не належать жодному пакету: ${unowned[*]:0:5}" \
              "The system already has files of the $PKG_NAME package that belong to no package: ${unowned[*]:0:5}")"
    hint "$(t "Їх, мабуть, скопіювали вручну або поставили через «make install». pacman не перезаписує чужих файлів: перенеси чи видали їх (sudo rm …) і повтори." \
              "They were probably copied by hand or installed with “make install”. pacman does not overwrite files it does not own: move or delete them (sudo rm …) and try again.")"
    return 1
  fi
}

q_array() {   # $1 = назва змінної, далі — значення → рядок для PKGBUILD
  local name=$1 v
  shift
  printf '%s=(' "$name"
  for v in "$@"; do printf ' %q' "$v"; done
  printf ' )\n'
}

# makepkg.conf дистрибутива + профіль заліза. З --config makepkg не читає
# ~/.config/pacman/makepkg.conf, тож підключаємо його самі (до наших прапорців).
write_makepkg_conf() {
  local conf="$MAKEPKG_DIR/makepkg.conf"
  mkdir -p -- "$MAKEPKG_DIR"
  {
    printf '# %s\n' "$MARKER"
    printf '# Згенеровано OBS-Legacy: makepkg.conf дистрибутива + профіль заліза цього комп'"'"'ютера.\n'
    cat <<'EOF'
source /etc/makepkg.conf
for _c in /etc/makepkg.conf.d/*.conf; do
  if [[ -r $_c ]]; then source "$_c"; fi
done
if [[ -r "${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf" ]]; then
  source "${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf"
elif [[ -r $HOME/.makepkg.conf ]]; then
  source "$HOME/.makepkg.conf"
fi
unset _c
OPTIONS=("${OPTIONS[@]/#debug/!debug}")
BUILDENV=("${BUILDENV[@]/#sign/!sign}")
PKGEXT='.pkg.tar.zst'
PACKAGER='OBS-Legacy local build <obs-legacy@localhost.invalid>'
EOF
    printf 'CFLAGS=%q\nCXXFLAGS=%q\nLDFLAGS=%q\nMAKEFLAGS=%q\n' "$OPT_CFLAGS" "$OPT_CXXFLAGS" "$OPT_LDFLAGS" "-j$JOBS"
  } > "$conf"
  printf '%s\n' "$conf"
}

# makepkg без root з нашим makepkg.conf; пакети лягають у набір SET_DIR.
run_makepkg() {   # $1 = каталог з PKGBUILD, $2 = журнал, далі — параметри makepkg
  local dir=$1 log=$2 conf
  shift 2
  conf=$(write_makepkg_conf)
  mkdir -p -- "$SET_DIR" "$SOURCES_DIR" "$MAKEPKG_DIR/build"
  ( cd -- "$dir" \
    && PKGDEST="$SET_DIR" SRCDEST="$SOURCES_DIR" BUILDDIR="$MAKEPKG_DIR/build" \
       nice -n 10 makepkg --config "$conf" --nocolor --force "$@" ) >"$log" 2>&1
}

# PKGBUILD-перепаковка з PKG_*: package() лише копіює готове дерево.
# Друкує шлях до пакета.
make_repack_package() {
  local dir="$MAKEPKG_DIR/$PKG_NAME" log="$MAKEPKG_DIR/$PKG_NAME.log" file
  rm_cache_path "$dir"
  mkdir -p -- "$dir"
  {
    printf '# %s\n' "$MARKER"
    printf '# Згенеровано OBS-Legacy: пакує вже зібране й перевірене дерево.\n'
    printf 'pkgname=%q\npkgver=%q\npkgrel=%q\n' "$PKG_NAME" "$PKG_VER" "$PKG_REL"
    printf 'pkgdesc=%q\narch=(x86_64)\nurl=%q\n' "$PKG_DESC" "$PKG_URL"
    q_array license "$PKG_LICENSE"
    q_array depends "${PKG_DEPENDS[@]}"
    q_array optdepends "${PKG_OPTDEPENDS[@]}"
    q_array provides "${PKG_PROVIDES[@]}"
    q_array conflicts "${PKG_CONFLICTS[@]}"
    printf "options=('!strip' '!debug' '!lto' 'emptydirs')\n"
    printf '_tree=%q\n' "$PKG_TREE"
    cat <<'EOF'
package() {
  cp -a --no-preserve=ownership -- "$_tree/." "$pkgdir/"
}
EOF
  } > "$dir/PKGBUILD"
  if ! run_makepkg "$dir" "$log" --nodeps; then
    tail -n 25 "$log" >&2
    fail "$(t "makepkg не зміг запакувати $PKG_NAME (журнал: $log)." "makepkg could not pack $PKG_NAME (log: $log).")"
    return 1
  fi
  file=$(find "$SET_DIR" -maxdepth 1 -type f -name "$PKG_NAME-$PKG_VER-$PKG_REL-*.pkg.tar.*" ! -name '*.sig' -print -quit)
  if [[ -z "$file" ]]; then fail "$(t "Після makepkg немає пакета $PKG_NAME у $SET_DIR." \
                                      "There is no $PKG_NAME package in $SET_DIR after makepkg.")"; return 1; fi
  printf '%s\n' "$file"
}

# Набір пакетів цього збирання: номер (pkgrel) і каталог у сховищі.
begin_set() {
  BUILD_NUMBER=$(next_build_number)
  OBS_PKGVER=$(pkgver_from "$OBS_DESCRIBE")
  SET_DIR="$PKG_STORE/$LABEL"
  if [[ -e "$SET_DIR" ]]; then rm_store_path "$SET_DIR"; fi
  mkdir -p -- "$SET_DIR"
  SET_FILES=() SET_CONFLICTS=()
}

package_obs() {
  local m file
  PKG_NAME=$OBS_PKG PKG_VER=$OBS_PKGVER PKG_REL=$BUILD_NUMBER PKG_TREE=$STAGE_DIR
  PKG_DESC="$(t "OBS Studio з NVENC для NVIDIA $PKG_SUFFIX (SDK $HEADERS_API) і Browser Source; OBS-Legacy, оптимізовано під ${CPU_MARCH:-$CPU_TUNING}" \
                "OBS Studio with NVENC for NVIDIA $PKG_SUFFIX (SDK $HEADERS_API) and Browser Source; OBS-Legacy, optimized for ${CPU_MARCH:-$CPU_TUNING}")"
  PKG_URL="https://obsproject.com" PKG_LICENSE="GPL-2.0-or-later"
  mapfile -t PKG_DEPENDS < <(elf_pkg_deps "$STAGED_TREE")
  PKG_OPTDEPENDS=(
    "$(t "${DRIVER_PKG:-nvidia-utils}: NVENC (драйвер NVIDIA)" "${DRIVER_PKG:-nvidia-utils}: NVENC (NVIDIA driver)")"
    "$(t 'v4l2loopback-dkms: віртуальна камера' 'v4l2loopback-dkms: virtual camera')"
    "$(t 'xdg-desktop-portal-impl: захоплення вікон і екрана у Wayland' \
         'xdg-desktop-portal-impl: window and screen capture on Wayland')"
  )
  if tree_uses_vlc "$STAGED_TREE"; then PKG_OPTDEPENDS+=("$(t 'vlc: джерело «VLC Video Source»' \
                                                              'vlc: the “VLC Video Source” source')"); fi
  # provides=obs-studio і conflicts=obs-studio: pacman сам запропонує замінити
  # будь-який OBS (obs-studio, obs-studio-git, obs-studio-browser…), а пакети,
  # що потребують obs-studio (плагіни з AUR), ставляться на цей.
  PKG_PROVIDES=("obs-studio=$OBS_PKGVER")
  PKG_CONFLICTS=(obs-studio obs-studio-plugin-browser obs-browser obs-linuxbrowser obs-websocket obs-vst)
  for m in obs-browser obs-websocket obs-vst; do
    if [[ -n "$(find "$STAGED_TREE/lib" -type f -name "$m.so" -print -quit 2>/dev/null)" ]]; then
      PKG_PROVIDES+=("$m=$OBS_PKGVER")
    fi
  done
  if [[ " ${PKG_PROVIDES[*]} " == *" obs-browser="* ]]; then PKG_PROVIDES+=("obs-studio-plugin-browser=$OBS_PKGVER"); fi
  add_file_conflicts "$STAGE_DIR" || die "$(t "Пакет $OBS_PKG конфліктує з файлами, що не належать жодному пакету (див. вище)." \
                                              "The $OBS_PKG package conflicts with files that belong to no package (see above).")"
  file=$(make_repack_package) || die "$(t "Не вдалося запакувати $OBS_PKG." "Could not pack $OBS_PKG.")" \
    "$(t "Подивись журнал makepkg вище. Найчастіше бракує fakeroot: постав пакети $(deps_action)." \
         "Look at the makepkg log above. Most often fakeroot is missing: install the packages $(deps_action).")"
  SET_FILES+=("$file") SET_CONFLICTS+=("${PKG_CONFLICTS[@]}")
  ok "$(t "Пакет: ${file##*/} ($(du -h -- "$file" | cut -f1))" "Package: ${file##*/} ($(du -h -- "$file" | cut -f1))")"
}

package_plugin() {   # $1 = id
  local id=$1 pstage="$PLUGINS_DIR/$1/stage" label file
  local -a names=()
  read -r -a names <<<"$(plugin_field "$id" aur)"
  label=$(sed -n "s/^plugin\.$id=\([^ ]*\).*/\1/p" "$pstage$PREFIX/share/obs-legacy/plugins/$id" 2>/dev/null || true)
  PKG_NAME=$(plugin_pkgname "$id") PKG_VER=$(pkgver_from "${label:-0}") PKG_REL=$BUILD_NUMBER PKG_TREE=$pstage
  PKG_DESC="$(t "$(plugin_field "$id" name): $(plugin_field "$id" desc) (плагін OBS; OBS-Legacy, оптимізовано під ${CPU_MARCH:-$CPU_TUNING})" \
                "$(plugin_field "$id" name): $(plugin_field "$id" desc) (OBS plugin; OBS-Legacy, optimized for ${CPU_MARCH:-$CPU_TUNING})")"
  PKG_URL=$(plugin_field "$id" repo) PKG_URL=${PKG_URL%.git}
  PKG_LICENSE=$(guess_license "$PLUGINS_DIR/$id/src")
  mapfile -t PKG_DEPENDS < <({ printf '%s\n' "$OBS_PKG"; elf_pkg_deps "$pstage$PREFIX" "$STAGED_TREE"; } | sort -u)
  PKG_OPTDEPENDS=()
  if tree_uses_vlc "$pstage$PREFIX"; then PKG_OPTDEPENDS+=("$(t 'vlc: відео з «VLC Video Source»' \
                                                                'vlc: videos from “VLC Video Source”')"); fi
  PKG_PROVIDES=("${names[0]}=$PKG_VER")
  PKG_CONFLICTS=("${names[@]}")
  add_file_conflicts "$pstage" || return 1
  file=$(make_repack_package) || return 1
  SET_FILES+=("$file") SET_CONFLICTS+=("${PKG_CONFLICTS[@]}")
  ok "$(t "Пакет: ${file##*/}" "Package: ${file##*/}")"
}

package_set() {
  local id commit
  local -a done_ids=()
  say "$(t "Пакети pacman: $OBS_PKG і плагіни (-$PKG_SUFFIX)" "pacman packages: $OBS_PKG and plugins (-$PKG_SUFFIX)")"
  if ((${#PLUGINS_FAILED[@]})); then
    printf 'plugins_failed=%s\n' "${PLUGINS_FAILED[*]}" >> "$STAGED_TREE/share/obs-legacy/BUILDINFO.txt"
    # Коміт, що не зібрався: «Оновити» не пробуватиме його знову, доки не вийде новий.
    # Вихідні коди не завантажилися (мережа) — коміту немає, і «Оновити» спробує знову.
    for id in "${PLUGINS_FAILED[@]}"; do
      commit=$(head -c 12 -- "$PLUGINS_DIR/$id/tried-commit" 2>/dev/null || true)
      [[ -z "$commit" ]] || printf 'plugin_failed.%s=%s\n' "$id" "$commit" >> "$STAGED_TREE/share/obs-legacy/BUILDINFO.txt"
    done
  fi
  printf 'package=%s %s-%s\nbuild_number=%s\n' "$OBS_PKG" "$OBS_PKGVER" "$BUILD_NUMBER" "$BUILD_NUMBER" \
    >> "$STAGED_TREE/share/obs-legacy/BUILDINFO.txt"
  add_hook_to_tree "$STAGED_TREE"
  if [[ -f "$OBS_SRC/COPYING" ]]; then
    install -D -m 644 -- "$OBS_SRC/COPYING" "$STAGED_TREE/share/licenses/$OBS_PKG/COPYING"
  fi
  package_obs
  for id in "${PLUGINS_OK[@]}"; do
    if package_plugin "$id"; then
      done_ids+=("$id")
    else
      PLUGINS_FAILED+=("$id")
      warn "$(t "Плагін $id не запаковано — у цьому наборі його не буде (журнал: $MAKEPKG_DIR/$(plugin_pkgname "$id").log)." \
                "Plugin $id was not packed — it will not be in this set (log: $MAKEPKG_DIR/$(plugin_pkgname "$id").log).")"
    fi
  done
  PLUGINS_OK=("${done_ids[@]}")
}

# BUILDINFO набору (OBS + версії плагінів + ffmpeg) і позначка «набір повний».
finalize_set() {
  local id
  {
    cat -- "$STAGED_TREE/share/obs-legacy/BUILDINFO.txt"
    for id in "${PLUGINS_OK[@]}"; do
      cat -- "$PLUGINS_DIR/$id/stage$PREFIX/share/obs-legacy/plugins/$id"
    done
    if [[ -n "$FFMPEG_FILE" ]]; then printf 'ffmpeg=%s\n' "$FFMPEG_PKGBUILD_VER"; fi
    printf 'packages=%s\n' "$(printf '%s ' "${SET_FILES[@]##*/}")"
  } > "$SET_DIR/BUILDINFO.txt"
  touch -- "$SET_DIR/.complete"
  ok "$(t "Набір пакетів готовий: $SET_DIR" "The package set is ready: $SET_DIR")"
}

# conflicts із самих файлів пакетів — для відкату, коли набір не збирався в
# цьому запуску (bsdtar -q читає лише .PKGINFO на початку архіву).
set_conflicts_from_files() {   # файли пакетів
  local f
  SET_CONFLICTS=()
  for f in "$@"; do
    mapfile -t -O "${#SET_CONFLICTS[@]}" SET_CONFLICTS \
      < <(bsdtar -qxOf "$f" .PKGINFO 2>/dev/null | sed -n 's/^conflict = //p' || true)
  done
}

# Встановлені пакети, які замінить набір (за conflicts наших пакетів).
replaced_packages() {
  local c p
  local -A seen=()
  for c in "${SET_CONFLICTS[@]}"; do
    while IFS= read -r p; do
      if [[ -n "$p" && "$p" != *"-$PKG_SUFFIX" && -z "${seen[$p]:-}" ]]; then
        seen[$p]=1
        printf '%s\n' "$p"
      fi
    done < <(pacman -Qq -- "$c" 2>/dev/null || true)
  done
}

# Назва пакета з назви файлу: <назва>-<версія>-<pkgrel>-<arch>.pkg.tar.*
pkgname_of_file() {   # $1 = файл пакета
  local n=${1##*/}
  n=${n%-*} n=${n%-*} n=${n%-*}
  printf '%s\n' "$n"
}

# Наші пакети плагінів, яких немає серед файлів набору: з плагіна зняли галочку
# або відкат на набір без нього. Вони зібрані під іншу версію OBS, тож лишати їх
# не можна. ffmpeg-legacy-580xx не чіпаємо: він живе окремо від наборів.
stale_plugin_packages() {   # файли пакетів набору
  local f p
  local -A in_set=()
  for f in "$@"; do in_set[$(pkgname_of_file "$f")]=1; done
  while IFS= read -r p; do
    if [[ -n "$p" && "$p" != "$OBS_PKG" && "$p" != "$FFMPEG_PKG" && -z "${in_set[$p]:-}" ]]; then
      printf '%s\n' "$p"
    fi
  done < <(pacman -Qq 2>/dev/null | grep -E -- "-$PKG_SUFFIX\$" || true)
}

# Після pacman -U: прибрати такі пакети (pacman сам спитає підтвердження).
remove_stale_plugins() {   # файли пакетів набору
  local -a stale=()
  mapfile -t stale < <(stale_plugin_packages "$@")
  ((${#stale[@]})) || return 0
  say "$(t "Прибираю пакети плагінів, яких немає в цьому наборі: ${stale[*]}" \
           "Removing plugin packages that are not in this set: ${stale[*]}")"
  if priv pacman -R -- "${stale[@]}"; then
    ok "$(t "Прибрано: ${stale[*]}" "Removed: ${stale[*]}")"
  else
    warn "$(t "Пакети ${stale[*]} лишилися: їх зібрано під іншу версію OBS, і OBS може з ними падати." \
              "Packages ${stale[*]} are left: they were built for a different OBS version, and OBS may crash with them.")"
    hint "$(t "Прибери їх: sudo pacman -R ${stale[*]} — або знову постав галочку в «Плагіни» і запусти $(install_action)." \
              "Remove them: sudo pacman -R ${stale[*]} — or tick them again in “Plugins” and run $(install_action).")"
  fi
}

install_set() {
  local -a replaced=() stale=()
  say "$(t "Встановлення пакетів (pacman -U)" "Installing the packages (pacman -U)")"
  mapfile -t replaced < <(replaced_packages)
  mapfile -t stale < <(stale_plugin_packages "${SET_FILES[@]}")
  printf "$(t '  Буде встановлено:\n' '  Will be installed:\n')" >&2
  printf '    %s\n' "${SET_FILES[@]##*/}" >&2
  if ((${#replaced[@]})); then
    printf "$(t '  Буде замінено (pacman спитає — відповідай y): %s\n' \
                '  Will be replaced (pacman will ask — answer y): %s\n')" "${replaced[*]}" >&2
  fi
  if ((${#stale[@]})); then
    printf "$(t '  Потім буде прибрано (плагінів немає в наборі): %s\n' \
                '  Will then be removed (the plugins are not in the set): %s\n')" "${stale[*]}" >&2
  fi
  if legacy_leftovers_present; then
    printf "$(t '  Потім буде прибрано стару схему: %s, обгортку %s, ярлик-перекриття і хук у /etc.\n' \
                '  The old layout will then be removed: %s, the wrapper %s, the overriding shortcut and the hook in /etc.\n')" \
      "$INSTALL_ROOT" "$WRAPPER" >&2
  fi
  check_pacman_lock
  if ! confirm "$(t "Встановити пакети OBS-Legacy?" "Install the OBS-Legacy packages?")"; then
    if installed_label >/dev/null; then
      cancelled "$(t "Пакети збережено в $SET_DIR. Поставити їх без перезбирання: знову $(install_action) — «Оновити» помітить цей готовий набір (або sudo pacman -U $SET_DIR/*.pkg.tar.zst)." \
                     "The packages are saved in $SET_DIR. To install them without rebuilding: run $(install_action) again — “Update” will notice this ready set (or sudo pacman -U $SET_DIR/*.pkg.tar.zst).")"
    else
      cancelled "$(t "Пакети збережено в $SET_DIR. Поставити їх: знову $(install_action) — збирання піде швидше, кеш лишився (або sudo pacman -U $SET_DIR/*.pkg.tar.zst)." \
                     "The packages are saved in $SET_DIR. To install them: run $(install_action) again — the build will be faster, the cache is kept (or sudo pacman -U $SET_DIR/*.pkg.tar.zst).")"
    fi
  fi
  priv_validate || die "$(t "Не вдалося отримати права адміністратора." \
                            "Could not get administrator rights.")" "$PRIV_HINT"
  if ! priv pacman -U -- "${SET_FILES[@]}"; then
    die "$(t "pacman не встановив пакети — система не змінилася." \
             "pacman did not install the packages — the system did not change.")" \
      "$(t "Якщо ти відповів «n» на питання pacman — так і має бути: запусти встановлення ще раз і відповідай y (набір уже зібрано: $SET_DIR). Інакше дивись повідомлення pacman вище (найчастіше — конфлікт файлів)." \
           "If you answered “n” to pacman's question, that is expected: start the installation again and answer y (the set is already built: $SET_DIR). Otherwise see the pacman messages above (most often a file conflict).")"
  fi
  ok "$(t "Пакети встановлено." "Packages installed.")"
  remove_stale_plugins "${SET_FILES[@]}"
}

store_sets() {   # назви наборів у сховищі, від найстарішого
  [[ -d "$PKG_STORE" ]] || return 0
  find "$PKG_STORE" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort || true
}

prune_store() {
  local active l excess i
  local -a complete=()
  active=$(installed_label || true)
  while IFS= read -r l; do
    [[ -n "$l" ]] || continue
    if [[ "$l" == "$active" ]]; then complete+=("$l"); continue; fi
    if [[ ! -f "$PKG_STORE/$l/.complete" ]]; then
      rm_store_path "$PKG_STORE/$l"
      continue
    fi
    complete+=("$l")
  done < <(store_sets)
  excess=$(( ${#complete[@]} - KEEP_VERSIONS ))
  for (( i = 0; i < ${#complete[@]} && excess > 0; i++ )); do
    l=${complete[i]}
    if [[ "$l" == "$active" ]]; then continue; fi
    say "$(t "Видаляю старий набір пакетів $l (KEEP_VERSIONS=$KEEP_VERSIONS)" \
             "Deleting the old package set $l (KEEP_VERSIONS=$KEEP_VERSIONS)")"
    rm_store_path "$PKG_STORE/$l"
    excess=$(( excess - 1 ))
  done
}

# ---------------------------------------------------------------------------
# ffmpeg-legacy-580xx: PKGBUILD Arch, але з NVENC під цей драйвер, під цей
# процесор і CUDA-архітектуру цієї відеокарти. Перезбирається лише тоді, коли
# в репозиторії вийшов новий ffmpeg або змінився профіль заліза.
# ---------------------------------------------------------------------------
ffmpeg_info() {   # $1 = ключ → значення з відомостей встановленого ffmpeg-legacy-580xx
  sed -n "s/^$1=//p" "$INFO_DIR/ffmpeg.txt" 2>/dev/null | head -n1 || true
}

# PKGBUILD Arch саме для версії ffmpeg з репозиторіїв (тег 2-9.0.2-2 для
# 2:9.0.2-2), його версія й залежності (з .SRCINFO, без виконання коду).
prepare_ffmpeg_pkgbuild() {
  local dir=$FFMPEG_PKGBUILD_DIR tag ref
  plugin_prepare_repo "$dir" "$FFMPEG_PKGBUILD_REPO" || return 1
  tag=${FFMPEG_REPO_VER/:/-}
  if git -C "$dir" show-ref --verify -q "refs/tags/$tag"; then
    git -C "$dir" -c advice.detachedHead=false checkout -q --force --detach "refs/tags/$tag" || return 1
  else
    ref=$(git -C "$dir" symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null || echo origin/main)
    git -C "$dir" -c advice.detachedHead=false checkout -q --force --detach "$ref" || return 1
    warn "$(t "PKGBUILD для ffmpeg $FFMPEG_REPO_VER (тег $tag) не знайдено — беру найновіший з Arch." \
              "PKGBUILD for ffmpeg $FFMPEG_REPO_VER (tag $tag) was not found — taking the newest one from Arch.")"
  fi
  git -C "$dir" clean -q -ffdx || return 1
  [[ -f "$dir/PKGBUILD" && -f "$dir/.SRCINFO" ]] || return 1
  FFMPEG_PKGBUILD_VER=$(awk -F' = ' '
    $1 == "\tepoch" { e = $2 } $1 == "\tpkgver" { v = $2 } $1 == "\tpkgrel" { r = $2 }
    END { if (v != "") printf "%s%s-%s\n", (e != "" ? e ":" : ""), v, r }' "$dir/.SRCINFO")
  [[ -n "$FFMPEG_PKGBUILD_VER" ]] || return 1
  ffmpeg_srcinfo_deps
  printf '  PKGBUILD: Arch, ffmpeg %s (%s)\n' "$FFMPEG_PKGBUILD_VER" \
    "$(git -C "$dir" log -1 --format='%h %s' 2>/dev/null || true)" >&2
}

# Залежності збирання ffmpeg з .SRCINFO — без ffnvcodec-headers (заголовки
# NVENC наші) і без залежностей-sonames (libass.so тощо: їх дають пакети зі списку).
ffmpeg_srcinfo_deps() {
  FFMPEG_DEPS=()
  [[ -f "$FFMPEG_PKGBUILD_DIR/.SRCINFO" ]] || return 0
  mapfile -t FFMPEG_DEPS < <(awk -F' = ' '$1 ~ /^\t(make)?depends$/ { print $2 }' "$FFMPEG_PKGBUILD_DIR/.SRCINFO" \
    | sed -E 's/[<>=].*//' | grep -vxE 'ffnvcodec-headers|.*\.so' | sort -u || true)
}

# Для deps і deps --missing: пакети для збирання ffmpeg потрібні, лише коли
# його збиратимемо. offline=1 — з уже завантаженого PKGBUILD, без мережі й виводу.
load_ffmpeg_deps() {   # $1 = offline (0/1)
  FFMPEG_DEPS=()
  [[ "$BUILD_FFMPEG" == 1 ]] || return 0
  FFMPEG_REPO_VER=$(LC_ALL=C pacman -Si ffmpeg 2>/dev/null | sed -n 's/^Version *: *//p' | head -n1 || true)
  [[ -n "$FFMPEG_REPO_VER" ]] || return 0
  if pacman -Q "$FFMPEG_PKG" >/dev/null 2>&1 && [[ "$(ffmpeg_info arch_version)" == "$FFMPEG_REPO_VER" ]]; then
    return 0
  fi
  if [[ "${1:-0}" == 1 ]]; then
    ffmpeg_srcinfo_deps
  else
    prepare_ffmpeg_pkgbuild || FFMPEG_DEPS=()
  fi
}

# Чи збирати ffmpeg: FFMPEG_PLAN = build | skip | off, FFMPEG_REASON — чому.
ffmpeg_plan() {
  local inst built
  FFMPEG_PLAN=off FFMPEG_REASON='' FFMPEG_DEPS=()
  say "ffmpeg ($FFMPEG_PKG)"
  if [[ "$BUILD_FFMPEG" != 1 ]]; then
    printf "$(t '  Не збирається (Налаштування → «Збирати ffmpeg» вимкнено): лишається системний ffmpeg.\n' \
                '  Not built (Settings → “Build ffmpeg” is off): the system ffmpeg stays.\n')" >&2
    return 0
  fi
  FFMPEG_REPO_VER=$(LC_ALL=C pacman -Si ffmpeg 2>/dev/null | sed -n 's/^Version *: *//p' | head -n1 || true)
  if [[ -z "$FFMPEG_REPO_VER" ]]; then
    warn "$(t "ffmpeg немає в репозиторіях pacman — $FFMPEG_PKG не збираю." \
              "ffmpeg is not in the pacman repositories — not building $FFMPEG_PKG.")"
    return 0
  fi
  inst=$(pacman -Q "$FFMPEG_PKG" 2>/dev/null | awk '{print $2}' || true)
  built=$(ffmpeg_info arch_version)
  if [[ -z "$inst" ]]; then
    FFMPEG_PLAN=build FFMPEG_REASON="$(t "ще не встановлено" "not installed yet")"
  elif [[ "$built" != "$FFMPEG_REPO_VER" ]]; then
    FFMPEG_PLAN=build FFMPEG_REASON="$(t "у репозиторії ffmpeg $FFMPEG_REPO_VER, а встановлений зібрано з ${built:-?}" \
                                         "the repository has ffmpeg $FFMPEG_REPO_VER, but the installed one is built from ${built:-?}")"
  elif [[ "$(ffmpeg_info profile)" != "$PROFILE_SIG" ]]; then
    FFMPEG_PLAN=build FFMPEG_REASON="$(t "змінився профіль заліза" "the hardware profile changed")"
  elif [[ "$FORCE_CLEAN_BUILD" == 1 ]]; then
    FFMPEG_PLAN=build FFMPEG_REASON="$(t "«Щоразу чиста збірка»" "“Clean build every time”")"
  else
    FFMPEG_PLAN=skip
    ok "$(t "$FFMPEG_PKG $inst уже зібрано з ffmpeg $FFMPEG_REPO_VER під цей профіль — не перезбираю." \
            "$FFMPEG_PKG $inst is already built from ffmpeg $FFMPEG_REPO_VER for this profile — not rebuilding.")"
    return 0
  fi
  printf "$(t '  Буде зібрано %s (%s): PKGBUILD Arch, але з NVENC під цей драйвер і під цей процесор.\n' \
              "  %s (%s) will be built: Arch's PKGBUILD, but with NVENC for this driver and this CPU.\\n")" \
    "$FFMPEG_PKG" "$FFMPEG_REASON" >&2
  if ! prepare_ffmpeg_pkgbuild; then
    FFMPEG_PLAN=off FFMPEG_DEPS=()
    warn "$(t "Не вдалося підготувати PKGBUILD ffmpeg — цього разу ffmpeg не збиратиметься (OBS це не заважає)." \
              "Could not prepare the ffmpeg PKGBUILD — ffmpeg will not be built this time (this does not affect OBS).")"
    hint "$(t "Перевір інтернет: PKGBUILD береться з $FFMPEG_PKGBUILD_REPO. Вимкнути: Налаштування → «Збирати ffmpeg»." \
              "Check the internet: the PKGBUILD comes from $FFMPEG_PKGBUILD_REPO. To turn it off: Settings → “Build ffmpeg”.")"
  fi
}

# Перевірка зібраного ffmpeg: кодує через NVENC на цьому драйвері?
ffmpeg_smoke_test() {   # $1 = корінь пакета (…/pkg/ffmpeg-legacy-580xx), $2 = журнал
  local root=$1 log=$2 bin="$1/usr/bin/ffmpeg" codec
  [[ -x "$bin" ]] || return 1
  for codec in h264_nvenc hevc_nvenc; do
    printf "$(t '\n==== OBS-Legacy: перевірка %s ====\n' '\n==== OBS-Legacy: checking %s ====\n')" "$codec" >> "$log"
    if ! LD_LIBRARY_PATH="$root/usr/lib" timeout 60 "$bin" -hide_banner -loglevel error -nostdin \
         -f lavfi -i testsrc2=size=1280x720:rate=30 -frames:v 30 -an -c:v "$codec" -f null - >>"$log" 2>&1; then
      tail -n 5 "$log" | sed 's/^/  /' >&2
      return 1
    fi
  done
}

build_ffmpeg_package() {
  local dir="$MAKEPKG_DIR/$FFMPEG_PKG" log="$MAKEPKG_DIR/$FFMPEG_PKG.log" conf shim inst n=1 rel k pid rc=0
  local start ticks=0 nvcc='' file pkgroot filters
  FFMPEG_FILE=''
  [[ "$FFMPEG_PLAN" == build ]] || return 0
  say "$(t "ffmpeg $FFMPEG_PKGBUILD_VER з NVENC SDK $HEADERS_API → $FFMPEG_PKG (довго: 15–40 хв)" \
           "ffmpeg $FFMPEG_PKGBUILD_VER with NVENC SDK $HEADERS_API → $FFMPEG_PKG (slow: 15–40 min)")"
  # pkgrel: <pkgrel Arch>.<n> — видно, з якого пакета Arch зібрано, і кожна
  # перезбірка того самого пакета новіша для pacman.
  inst=$(pacman -Q "$FFMPEG_PKG" 2>/dev/null | awk '{print $2}' || true)
  if [[ "${inst%-*}" == "${FFMPEG_PKGBUILD_VER%-*}" && "${inst##*-}" == "${FFMPEG_PKGBUILD_VER##*-}".* ]]; then
    n=${inst##*.}
    [[ "$n" =~ ^[0-9]+$ ]] || n=0
    n=$(( n + 1 ))
  fi
  rel="${FFMPEG_PKGBUILD_VER##*-}.$n"
  if [[ -n "$GPU_CC" ]]; then nvcc="--cuda-gpu-arch=sm_${GPU_CC/./} -O2"; fi

  rm_cache_path "$dir"
  mkdir -p -- "$dir"
  # Латки й ключі з репозиторію пакета; сам PKGBUILD — нижче, з нашими змінами.
  find "$FFMPEG_PKGBUILD_DIR" -mindepth 1 -maxdepth 1 ! -name .git ! -name PKGBUILD -exec cp -a -- {} "$dir/" \;
  cat > "$dir/obs-legacy-ffmpeg.txt" <<EOF
arch_version=$FFMPEG_REPO_VER
pkgbuild_version=$FFMPEG_PKGBUILD_VER
nv_headers_tag=$NV_HEADERS_RESOLVED
nvenc_api_headers=$HEADERS_API
cpu_march=$CPU_MARCH
cpu_tuning=$CPU_TUNING
cuda_arch=${GPU_CC:+sm_${GPU_CC/./}}
profile=$PROFILE_SIG
built=$(date -Is)
builder=$SELF_NAME $SCRIPT_VERSION
EOF
  {
    cat -- "$FFMPEG_PKGBUILD_DIR/PKGBUILD"
    printf '\n# ---- OBS-Legacy: %s ----\n' "$MARKER"
    printf 'pkgname=%q\n' "$FFMPEG_PKG"
    printf 'pkgrel=%q\n' "$rel"
    printf 'pkgdesc+=%q\n' "$(t " — NVENC SDK $HEADERS_API для NVIDIA $PKG_SUFFIX; OBS-Legacy, оптимізовано під ${CPU_MARCH:-$CPU_TUNING}" \
                                " — NVENC SDK $HEADERS_API for NVIDIA $PKG_SUFFIX; OBS-Legacy, optimized for ${CPU_MARCH:-$CPU_TUNING}")"
    printf 'provides+=(%q)\n' "ffmpeg=${FFMPEG_PKGBUILD_VER%-*}"
    printf 'conflicts=(ffmpeg)\n'
    if [[ -n "$DRIVER_PKG" ]]; then
      printf 'optdepends=("${optdepends[@]/#nvidia-utils:/%s:}")\n' "$DRIVER_PKG"
    fi
    cat <<'EOF'
# Заголовки NVENC — приватні (через обгортку pkg-config), не ffnvcodec-headers.
_obs_legacy_md=()
for _d in "${makedepends[@]}"; do
  if [[ $_d != ffnvcodec-headers ]]; then _obs_legacy_md+=("$_d"); fi
done
makedepends=("${_obs_legacy_md[@]}")
unset _d _obs_legacy_md
eval "_obs_legacy_$(declare -f package)"
package() {
  _obs_legacy_package
  install -Dm644 "$startdir/obs-legacy-ffmpeg.txt" "$pkgdir/usr/share/obs-legacy/ffmpeg.txt"
}
EOF
    # CUDA-фільтри ffmpeg (scale_cuda тощо) — під архітектуру цієї відеокарти.
    if [[ -n "$nvcc" ]]; then
      printf 'eval "$(declare -f build | sed %q)"\n' "s/--enable-cuda-llvm /--enable-cuda-llvm --nvccflags='$nvcc' /"
    fi
  } > "$dir/PKGBUILD"
  bash -n "$dir/PKGBUILD" || { warn "$(t "Внутрішня помилка: згенерований PKGBUILD ffmpeg некоректний." \
                                         "Internal error: the generated ffmpeg PKGBUILD is invalid.")"; return 0; }

  # Обгортка pkg-config: PKGBUILD Arch сам перезаписує PKG_CONFIG_PATH, тож
  # приватні заголовки NVENC мають іти першими саме тут.
  shim="$MAKEPKG_DIR/shim"
  mkdir -p -- "$shim"
  [[ "$HEADERS_PCDIR" != *"'"* ]] || die "$(t "Шлях до заголовків NVENC містить апостроф: $HEADERS_PCDIR" \
                                              "The path to the NVENC headers contains an apostrophe: $HEADERS_PCDIR")"
  {
    printf '#!/bin/sh\n# %s\n' "$MARKER"
    printf "PKG_CONFIG_PATH='%s'\"\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}\"\n" "$HEADERS_PCDIR"
    printf 'export PKG_CONFIG_PATH\nexec /usr/bin/pkg-config "$@"\n'
  } > "$shim/pkg-config"
  chmod 755 -- "$shim/pkg-config"
  # Ключі PGP для підписаного тега ffmpeg — в окремий GNUPGHOME, щоб не
  # змінювати твій набір ключів.
  install -d -m 700 -- "$GNUPG_DIR"
  for k in "$dir"/keys/pgp/*.asc; do
    if [[ -f "$k" ]]; then GNUPGHOME="$GNUPG_DIR" gpg --batch --quiet --import -- "$k" >/dev/null 2>&1 || true; fi
  done

  conf=$(write_makepkg_conf)
  mkdir -p -- "$SET_DIR" "$SOURCES_DIR" "$MAKEPKG_DIR/build"
  printf "$(t '  Журнал: %s\n' '  Log: %s\n')" "$log" >&2
  ( cd -- "$dir" \
    && PATH="$shim:$PATH" GNUPGHOME="$GNUPG_DIR" PKGDEST="$SET_DIR" SRCDEST="$SOURCES_DIR" \
       BUILDDIR="$MAKEPKG_DIR/build" nice -n 10 makepkg --config "$conf" --nocolor --force --cleanbuild --nocheck \
  ) >"$log" 2>&1 &
  pid=$!
  start=$SECONDS
  while kill -0 "$pid" 2>/dev/null; do
    sleep 5
    ticks=$(( ticks + 1 ))
    if (( ticks % 12 == 0 )) && kill -0 "$pid" 2>/dev/null; then
      printf "$(t '  … ffmpeg збирається %d хв; скомпільовано файлів: %s\n' \
                  '  … ffmpeg has been building for %d min; files compiled: %s\n')" \
        $(( (SECONDS - start) / 60 )) "$(grep -c '^CC' -- "$log" 2>/dev/null || echo 0)" >&2
    fi
  done
  wait "$pid" || rc=$?
  if (( rc != 0 )); then
    tail -n 30 "$log" >&2
    warn "$(t "ffmpeg не зібрався (код makepkg $rc) — лишається поточний ffmpeg; OBS це не заважає. Журнал: $log" \
              "ffmpeg did not build (makepkg code $rc) — the current ffmpeg stays; this does not affect OBS. Log: $log")"
    # Коди makepkg: 8 — бракує залежностей (без -s), 16 — не пройшла перевірка PGP.
    case "$rc" in
      8)
        hint "$(t "Бракує пакетів для збирання ffmpeg: постав їх $(deps_action) і повтори." \
                  "Packages needed to build ffmpeg are missing: install them $(deps_action) and try again.")"
        fix deps "$(t "Встановити пакети для збирання" "Install the build dependencies")" ;;
      16)
        hint "$(t "Не вдалося перевірити підпис вихідних кодів ffmpeg (PGP). Повтори пізніше; якщо не мине — видали $GNUPG_DIR і $SOURCES_DIR/ffmpeg." \
                  "Could not verify the signature of the ffmpeg sources (PGP). Try again later; if it does not go away, delete $GNUPG_DIR and $SOURCES_DIR/ffmpeg.")" ;;
      *)
        hint "$(t "Спробуй пізніше (PKGBUILD Arch міг змінитися) або вимкни: Налаштування → «Збирати ffmpeg»." \
                  "Try again later (Arch's PKGBUILD may have changed) or turn it off: Settings → “Build ffmpeg”.")" ;;
    esac
    return 0
  fi
  file=$(find "$SET_DIR" -maxdepth 1 -type f -name "$FFMPEG_PKG-*.pkg.tar.*" ! -name "$FFMPEG_PKG-debug-*" ! -name '*.sig' -print -quit)
  pkgroot="$MAKEPKG_DIR/build/$FFMPEG_PKG/pkg/$FFMPEG_PKG"
  if [[ -z "$file" ]]; then
    warn "$(t "Після makepkg немає пакета $FFMPEG_PKG — ffmpeg цього разу не оновлюється. Журнал: $log" \
              "There is no $FFMPEG_PKG package after makepkg — ffmpeg is not updated this time. Log: $log")"
    return 0
  fi
  if ! ffmpeg_smoke_test "$pkgroot" "$log"; then
    rm -f -- "$file"
    warn "$(t "Зібраний ffmpeg не кодує через NVENC — не встановлюю його (журнал: $log)." \
              "The built ffmpeg does not encode via NVENC — not installing it (log: $log).")"
    hint "$(t "Перевір, що драйвер NVIDIA працює (nvidia-smi) і NVENC зараз не зайнятий іншими програмами, і повтори." \
              "Check that the NVIDIA driver works (nvidia-smi) and that NVENC is not busy with other programs right now, and try again.")"
    return 0
  fi
  # Без «| grep -q»: grep закрив би канал раніше, ffmpeg отримав би SIGPIPE, і
  # через pipefail перевірка «не пройшла» б.
  filters=$(LD_LIBRARY_PATH="$pkgroot/usr/lib" "$pkgroot/usr/bin/ffmpeg" -hide_banner -filters 2>/dev/null || true)
  if [[ "$filters" == *scale_cuda* ]]; then
    printf "$(t '  CUDA-фільтри (scale_cuda тощо): є%s\n' \
                '  CUDA filters (scale_cuda etc.): yes%s\n')" "${nvcc:+$(t ", під sm_${GPU_CC/./}" ", for sm_${GPU_CC/./}")}" >&2
  fi
  FFMPEG_FILE=$file
  SET_FILES+=("$file") SET_CONFLICTS+=(ffmpeg)
  ok "$(t "ffmpeg зібрано, NVENC працює (h264_nvenc, hevc_nvenc): ${file##*/} ($(du -h -- "$file" | cut -f1))" \
          "ffmpeg built, NVENC works (h264_nvenc, hevc_nvenc): ${file##*/} ($(du -h -- "$file" | cut -f1))")"
}

backup_obs_config() {
  local cfg="$CONFIG_HOME/obs-studio" dir="$STATE_DIR/config-backups" out old
  [[ -d "$cfg" ]] || return 0
  mkdir -p -- "$dir"
  out="$dir/obs-studio-$(date +%Y%m%d-%H%M%S).tar.gz"
  if tar -czf "$out" -C "$CONFIG_HOME" \
       --exclude='obs-studio/logs' --exclude='obs-studio/crashes' \
       --exclude='obs-studio/profiler_data' --exclude='obs-studio/updates' \
       --exclude='*/obs-browser/Cache' --exclude='*/obs-browser/Code Cache' \
       --exclude='*/obs-browser/GPUCache' obs-studio 2>/dev/null; then
    ok "$(t "Копія налаштувань OBS: $out" "Backup of the OBS settings: $out")"
  else
    rm -f -- "$out"
    warn "$(t "Не вдалося зробити копію $cfg (продовжую)." "Could not back up $cfg (continuing).")"
  fi
  while IFS= read -r old; do
    rm -f -- "$old"
  done < <(find "$dir" -maxdepth 1 -name 'obs-studio-*.tar.gz' -printf '%p\n' | sort -r | tail -n +6)
}

# ---------------------------------------------------------------------------
# Команди
# ---------------------------------------------------------------------------
cmd_check() {
  local id
  local -a pkgs=() replaced=()
  preflight_common
  require_tools git cmake ninja pkg-config make curl tar xz sha256sum flock timeout
  acquire_lock
  detect_foreign_install
  print_header "$(t "перевірка (нічого не встановлюється)" "check (nothing is installed)")"
  resolve_selected_plugins
  show_inventory
  detect_build_profile
  ffmpeg_plan
  report_dependencies
  ffmpeg_nvenc_info
  detect_mbedtls
  prepare_obs_source
  prepare_headers
  check_driver_vs_headers
  prepare_cef
  check_plugins
  build_cmake_args
  say "$(t "Конфігурація CMake (лише перевірка)" "CMake configuration (check only)")"
  if ((${#DEP_INSTALL_REQ[@]})); then
    # Без цих пакетів CMake гарантовано зупиниться (напр., obs-websocket без
    # websocketpp), тож пробна конфігурація нічого не скаже. Її повторить install.
    warn "$(t "Пробну конфігурацію пропущено: спершу потрібні пакети ${DEP_INSTALL_REQ[*]}." \
              "The test configuration was skipped: the packages ${DEP_INSTALL_REQ[*]} are needed first.")"
    fix deps "$(t "Встановити пакети: ${DEP_INSTALL[*]}" "Install packages: ${DEP_INSTALL[*]}")"
    printf "$(t '\n%sПЕРЕВІРКУ ЗАВЕРШЕНО%s: усе готово до встановлення, бракує лише пакетів з репозиторіїв.\n' \
                '\n%sCHECK FINISHED%s: everything is ready for installation, only packages from the repositories are missing.\n')" \
      "$C_YEL$C_BLD" "$C_RST" >&2
    hint "$(t "Постав їх $(deps_action) і перевір систему ще раз — тоді все буде ✓. Або одразу обери $(install_action): він теж їх поставить." \
              "Install them $(deps_action) and check the system again — then everything will be ✓. Or choose $(install_action) right away: it installs them too.")"
  else
    configure_obs "$CHECK_DIR" "$PREFIX" fresh
    printf "$(t '\n%sПЕРЕВІРКУ ПРОЙДЕНО%s: можна встановлювати.\n' \
                '\n%sCHECK PASSED%s: ready to install.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
  fi
  pkgs=("$OBS_PKG")
  for id in "${PLUGINS_SELECTED[@]}"; do pkgs+=("$(plugin_pkgname "$id")"); done
  if [[ "$FFMPEG_PLAN" == build ]]; then pkgs+=("$FFMPEG_PKG"); fi
  SET_CONFLICTS=(obs-studio)
  for id in "${PLUGINS_SELECTED[@]}"; do
    read -r -a replaced <<<"$(plugin_field "$id" aur)"
    SET_CONFLICTS+=("${replaced[@]}")
  done
  if [[ "$FFMPEG_PLAN" == build ]]; then SET_CONFLICTS+=(ffmpeg); fi
  mapfile -t replaced < <(replaced_packages)
  printf '  OBS %s (%s), CEF %s, nv-codec-headers %s, TLS: %s\n' \
    "$OBS_DESCRIBE" "${OBS_COMMIT:0:12}" "$CEF_BUILD_RESOLVED" "$NV_HEADERS_RESOLVED" "$TLS_MODE" >&2
  printf "$(t '  Профіль: %s · -O%s%s\n' \
              '  Profile: %s · -O%s%s\n')" "${CPU_TUNING/native/${CPU_MARCH:-native}}" "$OPT_LEVEL" \
    "${GPU_CC:+ · CUDA sm_${GPU_CC/./}}" >&2
  printf "$(t '  Пакети: %s\n' '  Packages: %s\n')" "${pkgs[*]}" >&2
  if ((${#replaced[@]})); then
    printf "$(t '  Замінять (pacman спитає під час встановлення): %s\n' \
                '  They will replace (pacman asks during installation): %s\n')" "${replaced[*]}" >&2
  fi
  printf "$(t '  Компіляцію, тест NVENC і пакування виконує %s.\n' \
              '  Compilation, the NVENC test and packaging are done by %s.\n')" "$(install_action)" >&2
}

# Спільне для install і package: від перевірок до готового набору пакетів.
# ---------------------------------------------------------------------------
# Оновлення: що нового порівняно зі встановленими пакетами
# ---------------------------------------------------------------------------
# Що зібрало б оновлення для плагіна: «версія<TAB>коміт» (без checkout).
plugin_target() {   # $1 = id
  local id=$1 src="$PLUGINS_DIR/$1/src" line ref
  plugin_prepare_repo "$src" "$(plugin_field "$id" repo)" || return 1
  line=$(resolve_plugin_ref "$src" "$id") || return 1
  ref=${line%%$'\t'*}
  printf '%s\t%s\n' "${line#*$'\t'}" "$(git -C "$src" rev-parse "$ref^{commit}")"
}

# Що зміниться порівняно зі встановленими пакетами — по рядку на причину; порожньо —
# оновлювати нічого. Викликається, коли вже відомі цільові OBS, заголовки й профіль.
update_reasons() {
  local id inst line label commit f failed out
  if [[ ! -f "$INFO_DIR/BUILDINFO.txt" ]]; then
    printf "$(t 'OBS-Legacy ще не встановлено пакетами\n' 'OBS-Legacy is not installed as packages yet\n')"
    return 0
  fi
  if [[ "$(installed_info obs_commit)" != "$OBS_COMMIT" ]]; then
    printf 'OBS %s → %s\n' "$(installed_info obs_version)" "$OBS_DESCRIBE"
  fi
  if [[ "$(installed_info nv_headers_tag)" != "$NV_HEADERS_RESOLVED" ]]; then
    printf "$(t 'заголовки NVENC %s → %s\n' \
                'NVENC headers %s → %s\n')" "$(installed_info nv_headers_tag)" "$NV_HEADERS_RESOLVED"
  fi
  if [[ "$(installed_info profile)" != "$PROFILE_SIG" ]]; then
    printf "$(t 'змінився профіль заліза (тепер %s)\n' \
                'the hardware profile changed (now %s)\n')" "${CPU_MARCH:-$CPU_TUNING}"
  fi
  # Збірки без рядка recipe= зібрано рецептом 1.
  if (( $(installed_info recipe | grep -E '^[0-9]+$' || echo 1) < RECIPE_VERSION )); then
    printf "$(t 'новий рецепт збирання OBS-Legacy (%s → %s)\n' \
                'new OBS-Legacy build recipe (%s → %s)\n')" "$(installed_info recipe | grep . || echo 1)" "$RECIPE_VERSION"
  fi
  for id in "${PLUGINS_SELECTED[@]}"; do
    inst=$(sed -n "s/^plugin\.$id=//p" "$INFO_DIR/plugins/$id" 2>/dev/null | head -n1 || true)
    if ! line=$(plugin_target "$id"); then
      warn "$(t "Плагін $id: не вдалося дізнатися найновішу версію (мережа?) — зберу його знову." \
                "Plugin $id: could not find out the newest version (network?) — building it again.")"
      printf "$(t 'плагін %s: не вдалося перевірити версію\n' 'plugin %s: could not check the version\n')" "$id"
      continue
    fi
    label=${line%%$'\t'*} commit=${line#*$'\t'}
    if [[ -z "$inst" ]]; then
      failed=$(installed_info "plugin_failed.$id")
      if [[ -n "$failed" && "$commit" == "$failed"* ]]; then
        printf "$(t '  Плагін %s %s минулого разу не зібрався — не пробую знову, доки не вийде новіший (спробувати зараз: %s).\n' \
                    '  Plugin %s %s failed to build last time — not trying again until a newer one is out (to try now: %s).\n')" \
          "$id" "$label" "$(rebuild_action)" >&2
      else
        printf "$(t 'плагін %s %s: буде додано\n' 'plugin %s %s: will be added\n')" "$id" "$label"
      fi
    elif [[ "$commit" != "${inst##* }"* ]]; then
      printf "$(t 'плагін %s: %s → %s\n' 'plugin %s: %s → %s\n')" "$id" "${inst%% *}" "$label"
    fi
  done
  for f in "$INFO_DIR"/plugins/*; do
    [[ -f "$f" ]] || continue
    id=${f##*/}
    if [[ " ${PLUGINS_SELECTED[*]} " != *" $id "* ]]; then printf "$(t 'плагін %s: буде прибрано\n' \
                                                                       'plugin %s: will be removed\n')" "$id"; fi
  done
  if [[ "$FFMPEG_PLAN" == build ]]; then printf 'ffmpeg: %s\n' "$FFMPEG_REASON"; fi
  # Рядки з проблемами check-libs друкує з відступом у 6 пробілів — це не залежить від мови.
  if [[ -x "$PKG_LIBCHECK" ]]; then
    out=$("$PKG_LIBCHECK" 2>/dev/null || true)
    if grep -q '^      ' <<<"$out"; then
      printf '%s\n' "$(t 'після оновлення системи пакетам бракує бібліотек' \
                          'the packages are missing libraries after a system upgrade')"
    fi
  fi
}

# Що встановлено зараз — коротко: «OBS 33.0.1; плагіни: …; ffmpeg-legacy-580xx …».
installed_summary() {
  local f plugins='' ff
  for f in "$INFO_DIR"/plugins/*; do
    [[ -f "$f" ]] || continue
    plugins+="${plugins:+, }$(sed -n 's/^plugin\.\([^=]*\)=\([^ ]*\).*/\1 \2/p' "$f" | head -n1)"
  done
  ff=$(pacman -Q "$FFMPEG_PKG" 2>/dev/null | awk '{print $2}' || true)
  printf 'OBS %s%s%s\n' "$(installed_info obs_version)" "${plugins:+$(t "; плагіни: $plugins" "; plugins: $plugins")}" "${ff:+; $FFMPEG_PKG $ff}"
}

# Підсумок після встановлення: що змінилося (причини з update_reasons — уже в минулому часі).
install_summary() {   # $1 = версія OBS до встановлення
  local old=$1 r changes=''
  for r in "${UPDATE_REASONS[@]}"; do
    case "$r" in
      'OBS '*|'OBS-Legacy ще'*|'OBS-Legacy is not'*|'ffmpeg:'*|*'не вдалося перевірити'*|*'could not check'*)
        continue ;;
      *': буде додано') r="${r%: буде додано}: додано" ;;
      *': will be added') r="${r%: will be added}: added" ;;
      *': буде прибрано') r="${r%: буде прибрано}: прибрано" ;;
      *': will be removed') r="${r%: will be removed}: removed" ;;
      'змінився профіль заліза (тепер '*) r="зібрано під новий профіль заліза (${r#*тепер }" ;;
      'the hardware profile changed (now '*) r="built for the new hardware profile (${r#*now }" ;;
      'після оновлення системи'*|*'after a system upgrade') r=$(t 'бібліотеки знову на місці' 'the libraries are back') ;;
    esac
    changes+="${changes:+; }$r"
  done
  if [[ -n "$FFMPEG_FILE" ]]; then
    changes+="$(t "${changes:+; }$FFMPEG_PKG $FFMPEG_PKGBUILD_VER зібрано" \
                  "${changes:+; }$FFMPEG_PKG $FFMPEG_PKGBUILD_VER built")"
  elif [[ "$FFMPEG_PLAN" == build ]]; then
    changes+="$(t "${changes:+; }ffmpeg не зібрався — лишився попередній" \
                  "${changes:+; }ffmpeg did not build — the previous one stays")"
  fi
  if ((${#PLUGINS_FAILED[@]})); then
    changes+="$(t "${changes:+; }не зібралися плагіни: ${PLUGINS_FAILED[*]}" \
                  "${changes:+; }plugins failed to build: ${PLUGINS_FAILED[*]}")"
  fi
  if [[ -z "$old" ]]; then
    printf "$(t 'встановлено OBS %s з NVENC' 'installed OBS %s with NVENC')" "$OBS_DESCRIBE"
  elif [[ "$old" != "$OBS_DESCRIBE" ]]; then
    printf 'OBS %s → %s' "$old" "$OBS_DESCRIBE"
  elif [[ "$INSTALL_MODE" == rebuild ]]; then
    printf "$(t 'OBS %s перезібрано начисто' 'OBS %s rebuilt from scratch')" "$OBS_DESCRIBE"
  else
    printf "$(t 'OBS %s (версія та сама)' 'OBS %s (same version)')" "$OBS_DESCRIBE"
  fi
  if [[ -n "$changes" ]]; then printf '; %s' "$changes"; fi
  printf '\n'
}

# «update»: показати, що оновиться; якщо нічого — NOTHING_NEW=1 (збирати не треба).
report_update_plan() {
  say "$(t "Що нового" "What's new")"
  if ((${#UPDATE_REASONS[@]} == 0)); then
    ok "$(t "Усе вже найновіше: $(installed_summary)." "Everything is up to date: $(installed_summary).")"
    NOTHING_NEW=1
    return 0
  fi
  printf '  %s\n' "${UPDATE_REASONS[@]}" >&2
}

build_set() {
  resolve_selected_plugins
  show_inventory
  detect_build_profile
  ffmpeg_plan
  check_disk_space
  install_dependencies
  require_tools git cmake ninja pkg-config make curl xz timeout nice strip readelf ldd od makepkg fakeroot zstd
  detect_mbedtls
  prepare_obs_source
  prepare_headers
  check_driver_vs_headers
  mapfile -t UPDATE_REASONS < <(update_reasons)
  if [[ "$INSTALL_MODE" == update ]]; then
    report_update_plan
    if (( NOTHING_NEW )); then return 0; fi
  fi
  prepare_cef
  build_cmake_args

  LABEL="$(date +%Y%m%d-%H%M%S)_$(sanitize_label "$OBS_DESCRIBE")"
  say "$(t "Конфігурація CMake" "CMake configuration")"
  if [[ "$INSTALL_MODE" == rebuild ]]; then
    printf "$(t '  Перезбирання начисто: кеш збірки OBS буде очищено.\n' \
                '  Clean rebuild: the OBS build cache will be cleared.\n')" >&2
    configure_obs "$BUILD_DIR" "$PREFIX" fresh
  else
    configure_obs "$BUILD_DIR" "$PREFIX" reuse
  fi
  build_obs
  run_nvenc_test
  stage_install
  build_plugins
  verify_build
  begin_set
  build_ffmpeg_package
  package_set
  finalize_set
}

# ldd для OBS і кожного плагіна. Плагін, якому бракує бібліотек, не пакується.
verify_build() {
  local id
  local -a good=()
  if ! verify_tree "$STAGED_TREE"; then
    if [[ "$ALLOW_MISSING_LIBS" == 1 ]]; then
      warn "$(t "Продовжую, бо ALLOW_MISSING_LIBS=1 — частина функцій OBS може не працювати." \
                "Continuing because ALLOW_MISSING_LIBS=1 — some OBS features may not work.")"
    else
      die "$(t "Пакети НЕ зібрано: бракує системних бібліотек (див. вище)." \
               "The packages were NOT built: system libraries are missing (see above).")" \
        "$(t "Встанови пакети з цими бібліотеками (назву підкаже pacman -F <бібліотека>, спершу sudo pacman -Fy) і запусти $(install_action) ще раз — перезбирати не доведеться." \
             "Install the packages with these libraries (pacman -F <library> tells the name, run sudo pacman -Fy first) and run $(install_action) again — no rebuild needed.")"
    fi
  fi
  for id in "${PLUGINS_OK[@]}"; do
    if verify_tree "$PLUGINS_DIR/$id/stage$PREFIX" "$STAGED_TREE" || [[ "$ALLOW_MISSING_LIBS" == 1 ]]; then
      good+=("$id")
    else
      PLUGINS_FAILED+=("$id")
      warn "$(t "Плагіну $id бракує бібліотек — у цьому наборі його не буде." \
                "Plugin $id is missing libraries — it will not be in this set.")"
    fi
  done
  PLUGINS_OK=("${good[@]}")
  if ((${#PLUGINS_FAILED[@]})) && [[ "$PLUGINS_STRICT" == 1 ]]; then
    die "$(t "Не зібралися плагіни: ${PLUGINS_FAILED[*]} (PLUGINS_STRICT=1). Нічого не встановлено." \
             "Plugins failed to build: ${PLUGINS_FAILED[*]} (PLUGINS_STRICT=1). Nothing was installed.")" \
      "$(t "Зніми галочку з цих плагінів (пункт «Плагіни») або вимкни «Суворо з плагінами» в Налаштуваннях. Журнали: $PLUGINS_DIR/<id>/build.log" \
           "Untick these plugins (the “Plugins” item) or turn off “Strict about plugins” in Settings. Logs: $PLUGINS_DIR/<id>/build.log")"
  fi
}

# Проміжні дерева більше не потрібні: усе вже в пакетах.
cleanup_stages() {
  local id
  rm_cache_path "$STAGE_DIR"
  for id in "${PLUGINS_SELECTED[@]}"; do
    if [[ -d "$PLUGINS_DIR/$id/stage" ]]; then rm_cache_path "$PLUGINS_DIR/$id/stage"; fi
  done
}

print_set_summary() {
  printf "$(t '  Пакети (%s):\n' '  Packages (%s):\n')" "$SET_DIR" >&2
  printf '    %s\n' "${SET_FILES[@]##*/}" >&2
  if ((${#PLUGINS_FAILED[@]})); then
    warn "$(t "Не зібралися плагіни: ${PLUGINS_FAILED[*]} — їх немає в цьому наборі (попередній набір з ними доступний для відкату)." \
              "Plugins failed to build: ${PLUGINS_FAILED[*]} — they are not in this set (the previous set with them is available for rollback).")"
  fi
}

# Набір, який зібрали, але не встановили (скасували на запитанні, обірвався pacman):
# повний, новіший за встановлений і зібраний з того самого — OBS, заголовки, профіль,
# рецепт і плагіни ті самі. Шукається, коли «Оновити» не знайшов нового, тобто
# встановлене й так дорівнює тому, що зібралося б зараз.
pending_set() {
  local cur l info k want have
  cur=$(installed_label) || return 1
  want=$(cat -- "$INFO_DIR"/plugins/* 2>/dev/null | grep '^plugin\.' | sort || true)
  while IFS= read -r l; do
    [[ "$l" > "$cur" ]] || break
    info="$PKG_STORE/$l/BUILDINFO.txt"
    [[ -f "$PKG_STORE/$l/.complete" && -f "$info" ]] || continue
    for k in obs_commit nv_headers_tag profile; do
      [[ "$(sed -n "s/^$k=//p" "$info" | head -n1)" == "$(installed_info "$k")" ]] || continue 2
    done
    [[ "$(sed -n 's/^recipe=//p' "$info" | head -n1 | grep . || echo 1)" == "$RECIPE_VERSION" ]] || continue
    have=$(grep '^plugin\.' -- "$info" | sort || true)
    [[ "$have" == "$want" ]] || continue
    printf '%s\n' "$l"
    return 0
  done < <(store_sets | sort -r)
  return 1
}

# Готовий набір замість збирання: SET_DIR, SET_FILES, SET_CONFLICTS, LABEL і решта,
# що потрібне install_set та історії.
use_stored_set() {   # $1 = набір
  local info="$PKG_STORE/$1/BUILDINFO.txt"
  SET_DIR="$PKG_STORE/$1" LABEL=$1
  mapfile -t SET_FILES < <(find "$SET_DIR" -maxdepth 1 -type f -name "*-$PKG_SUFFIX-*.pkg.tar.*" ! -name '*.sig' | sort)
  ((${#SET_FILES[@]})) || return 1
  set_conflicts_from_files "${SET_FILES[@]}"
  BUILD_NUMBER=$(sed -n 's/^build_number=//p' "$info" | head -n1)
  CEF_BUILD_RESOLVED=$(sed -n 's/^cef_build=//p' "$info" | head -n1)
}

cmd_install() {   # $1 = install | update | rebuild
  local previous old_ver pending='' from_store=0
  INSTALL_MODE=${1:-install}
  preflight_common
  select_priv_tool
  require_tools flock find awk sed tar sha256sum df
  acquire_lock
  detect_foreign_install
  case "$INSTALL_MODE" in
    update)  print_header "$(t "оновлення: спершу перевірка, що нового" "update: first check what is new")" ;;
    rebuild) print_header "$(t "перезбирання начисто й встановлення" "clean rebuild and installation")" ;;
    *)       print_header "$(t "збірка, пакування й встановлення" "build, packaging and installation")" ;;
  esac
  if [[ "$INSTALL_MODE" == rebuild ]]; then clear_stale_git_locks; fi
  old_ver=$(installed_info obs_version)
  build_set
  if (( NOTHING_NEW )) && pending=$(pending_set) && use_stored_set "$pending"; then
    say "$(t "Готовий набір, ще не встановлений" "A ready set that is not installed yet")"
    printf "$(t '  %s зібрано з тих самих вихідних кодів і під той самий профіль, але не встановлено\n' \
                '  %s was built from the same sources and for the same profile, but not installed\n')" "$pending" >&2
    printf "$(t '  (минулого разу встановлення скасували чи воно обірвалося) — ставлю його без перезбирання.\n' \
                '  (last time the installation was cancelled or interrupted) — installing it without rebuilding.\n')" >&2
    NOTHING_NEW=0 from_store=1
  fi
  if (( NOTHING_NEW )); then
    printf "$(t '\n%sОНОВЛЕНЬ НЕМАЄ%s: усе вже найновіше, збирати й ставити нічого.\n' \
                '\n%sNO UPDATES%s: everything is up to date, nothing to build or install.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
    printf "$(t '  Якщо OBS поводиться дивно або минуле збирання обірвалося (вимкнулося світло, закрилося\n' \
                '  If OBS behaves strangely or the last build was interrupted (a power cut, a closed\n')" >&2
    printf "$(t '  вікно) — %s: він збере все начисто й поставить знову.\n' \
                '  window) — %s: it rebuilds everything from scratch and installs it again.\n')" "$(rebuild_action)" >&2
    summary "$(t "оновлень немає — встановлено найновіше: $(installed_summary)" \
                 "no updates — the newest is installed: $(installed_summary)")"
    return 0
  fi
  backup_obs_config
  previous=$(installed_label || true)
  install_set
  if legacy_leftovers_present; then remove_legacy_install; fi
  mkdir -p -- "$STATE_DIR"
  printf '%s\t%s\t%s\t%s\tcef=%s\theaders=%s\tpkgrel=%s\tprofile=%s\n' "$(date -Is)" "$LABEL" "$OBS_DESCRIBE" \
    "$OBS_COMMIT" "$CEF_BUILD_RESOLVED" "$NV_HEADERS_RESOLVED" "$BUILD_NUMBER" "$PROFILE_SIG" >> "$STATE_DIR/history.tsv"
  prune_store
  cleanup_stages
  hash -r

  printf "$(t "\n%sГОТОВО%s: OBS %s зібрано під цей комп'ютер, NVENC перевірено, пакети встановлено.\n" \
              "\n%sDONE%s: OBS %s is built for this computer, NVENC is tested, the packages are installed.\n")" \
    "$C_GRN$C_BLD" "$C_RST" "$OBS_DESCRIBE" >&2
  print_set_summary
  printf "$(t '  Запуск: obs (або OBS Studio в меню програм)\n' \
              '  Launch: obs (or OBS Studio in the applications menu)\n')" >&2
  printf "$(t '  Стан:   %s status   ·   pacman -Qs %s\n' \
              '  Status: %s status   ·   pacman -Qs %s\n')" "$SELF_PATH" "$PKG_SUFFIX" >&2
  if [[ -n "$previous" ]]; then
    printf "$(t '  Відкат на попередній набір (%s): %s rollback\n' \
                '  Roll back to the previous set (%s): %s rollback\n')" "$previous" "$SELF_PATH" >&2
  fi
  printf "$(t '  Сторонні плагіни (AUR) ставляться на цей OBS як завжди: він «надає» obs-studio.\n' \
              '  Third-party plugins (AUR) install on this OBS as usual: it “provides” obs-studio.\n')" >&2
  if have pgrep && pgrep -x obs >/dev/null 2>&1; then
    warn "$(t "OBS зараз запущений — нова версія запрацює після його перезапуску." \
              "OBS is running right now — the new version will work after you restart it.")"
  fi
  if (( from_store )); then
    summary "$(t "поставлено готовий набір $LABEL (без перезбирання): $(installed_summary)" \
                 "installed the ready set $LABEL (without rebuilding): $(installed_summary)")"
  else
    summary "$(install_summary "$old_ver")"
  fi
}

# Оновити: зібрати й поставити лише тоді, коли є що оновлювати.
cmd_update() { cmd_install update; }

# Перезібрати начисто (кеш збірки OBS очищається) і поставити знову.
cmd_rebuild() { cmd_install rebuild; }

# Лише зібрати пакети — нічого не встановлювати (sudo — тільки якщо бракує
# пакетів для збірки).
cmd_package() {
  preflight_common
  select_priv_tool
  require_tools flock find awk sed tar sha256sum df
  acquire_lock
  print_header "$(t "збірка й пакування (без встановлення)" "build and packaging (without installation)")"
  build_set
  prune_store
  cleanup_stages
  printf "$(t "\n%sПАКЕТИ ГОТОВІ%s: OBS %s зібрано під цей комп'ютер і запаковано (нічого не встановлено).\n" \
              "\n%sPACKAGES READY%s: OBS %s is built for this computer and packed (nothing was installed).\n")" \
    "$C_GRN$C_BLD" "$C_RST" "$OBS_DESCRIBE" >&2
  print_set_summary
  printf "$(t '  Встановити: %s (або sudo pacman -U %s/*.pkg.tar.zst)\n' \
              '  Install: %s (or sudo pacman -U %s/*.pkg.tar.zst)\n')" "$(install_action)" "$SET_DIR" >&2
}

cmd_status() {
  local l mark latest built id ver active
  local -a pkgs=()
  preflight_common
  print_header "$(t "стан" "status")"
  resolve_selected_plugins
  say "$(t "Пакети OBS-Legacy" "OBS-Legacy packages")"
  mapfile -t pkgs < <(pacman -Q 2>/dev/null | grep -E -- "-$PKG_SUFFIX " || true)
  active=$(installed_label || true)
  if ((${#pkgs[@]})); then
    printf '  %s\n' "${pkgs[@]}" >&2
  else
    warn "$(t "Пакети OBS-Legacy не встановлено." "The OBS-Legacy packages are not installed.")"
  fi
  if [[ -n "$active" ]]; then
    ok "$(t "Встановлена збірка: $active" "Installed build: $active")"
    sed 's/^/  /' "$INFO_DIR/BUILDINFO.txt" >&2
    if compgen -G "$INFO_DIR/plugins/*" >/dev/null; then cat -- "$INFO_DIR"/plugins/* | sed 's/^/  /' >&2; fi
    if [[ -f "$INFO_DIR/ffmpeg.txt" ]]; then sed 's/^/  ffmpeg./' "$INFO_DIR/ffmpeg.txt" >&2; fi
    if [[ -x "$PKG_LIBCHECK" ]]; then
      l=$("$PKG_LIBCHECK" 2>/dev/null || true)
      if [[ -n "$l" ]]; then printf '%s\n' "$l" >&2; else ok "$(t "Бібліотеки на місці, ffmpeg актуальний." \
                                                                  "Libraries are in place, ffmpeg is up to date.")"; fi
    fi
  fi
  if legacy_install_present; then
    warn "$(t "Лишилася стара збірка в $INSTALL_ROOT (активна: $(current_label || echo '—')) — прибереться під час встановлення пакетів." \
              "An old build is left in $INSTALL_ROOT (active: $(current_label || echo '—')) — it will be removed when the packages are installed.")"
  fi
  if [[ -d "$PKG_STORE" ]]; then
    say "$(t "Збережені набори пакетів ($PKG_STORE)" "Saved package sets ($PKG_STORE)")"
    while IFS= read -r l; do
      [[ -n "$l" ]] || continue
      mark=''
      if [[ "$l" == "$active" ]]; then mark="$(t ' [встановлено]' ' [installed]')"; fi
      if [[ ! -f "$PKG_STORE/$l/.complete" ]]; then mark+="$(t ' [неповний]' ' [incomplete]')"; fi
      printf '  %s%s\n' "$l" "$mark" >&2
    done < <(store_sets)
  fi

  say "$(t "Інтеграція" "Integration")"
  printf "$(t '  Команда obs → %s\n' \
              '  Command obs → %s\n')" "$(command -v obs || echo "$(t 'не знайдена' 'not found')")" >&2
  if grep -qs -- "$MARKER" "$PKG_HOOK"; then ok "$(t "pacman-хук $PKG_HOOK на місці." \
                                                     "The pacman hook $PKG_HOOK is in place.")"
  elif [[ -n "$active" && "$INSTALL_PACMAN_HOOK" == 1 ]]; then warn "$(t "pacman-хука немає (з'явиться після наступного встановлення)." \
                                                                         "No pacman hook (it will appear after the next installation).")"
  fi
  l=$(nvenc_driver_api || true)
  printf "$(t '  NVENC API драйвера: %s\n' '  Driver NVENC API: %s\n')" "${l:-$(t невідомо unknown)}" >&2
  printf "$(t '  Вибрані плагіни: %s\n' '  Selected plugins: %s\n')" "${PLUGINS_SELECTED[*]:-$(t немає none)}" >&2

  if have git && have timeout; then
    latest=$(timeout 20 git ls-remote --tags --refs "$OBS_REPO" 2>/dev/null | awk -F/ '{print $3}' \
             | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1 || true)
    built=$(installed_info obs_version)
    if [[ -n "$latest" ]]; then
      if [[ "$latest" == "$built" ]]; then ok "$(t "Встановлено найновішу стабільну версію OBS ($latest)." \
                                                   "The newest stable OBS version is installed ($latest).")"
      else say "$(t "Найновіша стабільна версія OBS: $latest (встановлено: ${built:-—}). Оновлення: $(install_action)" \
                    "Newest stable OBS version: $latest (installed: ${built:-—}). To update: $(install_action)")"; fi
    fi
    if [[ -n "$active" ]]; then
      say "$(t "Оновлення плагінів" "Plugin updates")"
      for id in "${PLUGINS_SELECTED[@]}"; do
        ver=$(sed -n "s/^plugin\.$id=\([^ ]*\).*/\1/p" "$INFO_DIR/plugins/$id" 2>/dev/null || true)
        latest=$(plugin_latest_remote "$id" || true)
        if [[ -z "$ver" ]]; then
          warn "$(t "$id: не встановлено — з'явиться після $(install_action)." \
                    "$id: not installed — it will appear after $(install_action).")"
        elif [[ -z "$latest" ]]; then
          printf "$(t '  %s: %s (не вдалося перевірити оновлення)\n' \
                      '  %s: %s (could not check for updates)\n')" "$id" "$ver" >&2
        elif [[ "$latest" == "$ver" ]]; then
          ok "$(t "$id: найновіша версія ($ver)." "$id: newest version ($ver).")"
        else
          say "$(t "$id: доступна версія $latest (встановлено: $ver). Оновлення: $(install_action)" \
                   "$id: version $latest is available (installed: $ver). To update: $(install_action)")"
        fi
      done
    fi
  fi
}

# Стара схема /opt: перемикання симлінка current (лише поки пакетів немає).
rollback_legacy() {   # $1 = збірка або порожньо
  local target=$1 cur l
  local -a older=()
  cur=$(current_label || true)
  [[ -n "$cur" ]] || die "$(t "OBS-Legacy не встановлено — немає з чого відкочуватися." \
                              "OBS-Legacy is not installed — there is nothing to roll back from.")"
  if [[ -z "$target" ]]; then
    while IFS= read -r l; do
      [[ -n "$l" && "$l" != "$cur" && -f "$VERSIONS_DIR/$l/.complete" ]] || continue
      if [[ "$l" < "$cur" ]]; then older+=("$l"); fi
    done < <(list_versions)
    if ((${#older[@]} == 0)); then die "$(t "Немає старішої збереженої збірки (див. $SELF_PATH status)." \
                                            "There is no older saved build (see $SELF_PATH status).")"; fi
    target=${older[-1]}
  fi
  [[ "$target" =~ ^[A-Za-z0-9._-]+$ && -f "$VERSIONS_DIR/$target/.complete" ]] \
    || die "$(t "Збірки '$target' немає або вона неповна (див. $SELF_PATH status)." \
                "Build '$target' does not exist or is incomplete (see $SELF_PATH status).")"
  [[ "$target" != "$cur" ]] || die "$(t "Збірка $target уже активна." "Build $target is already active.")"
  quick_check_binary "$VERSIONS_DIR/$target/bin/obs" \
    || die "$(t "Збірці $target бракує бібліотек — відкат на неї не допоможе." \
                "Build $target is missing libraries — rolling back to it will not help.")"
  confirm "$(t "Перемкнути OBS з $cur на $target?" "Switch OBS from $cur to $target?")" || cancelled
  priv_validate || die "$(t "Не вдалося отримати права адміністратора." \
                            "Could not get administrator rights.")" "$PRIV_HINT"
  activate_version "$target"
  ok "$(t "Готово. Повернутися назад: $SELF_PATH rollback $cur" "Done. To go back: $SELF_PATH rollback $cur")"
}

cmd_rollback() {
  local target=${1:-} cur l
  local -a older=() files=() stale=() replaced=()
  preflight_common
  select_priv_tool
  require_tools flock
  acquire_lock
  print_header "$(t "відкат" "rollback")"
  cur=$(installed_label || true)
  # Набір пакетів можна поставити й тоді, коли пакетів ще немає (замість старої
  # збірки в /opt); інакше без пакетів відкат — між старими збірками в /opt.
  if [[ -z "$cur" && ! ( -n "$target" && -f "$PKG_STORE/$target/.complete" ) ]] && legacy_install_present; then
    rollback_legacy "$target"
    return 0
  fi
  [[ -n "$cur" || -n "$target" ]] || die "$(t "OBS-Legacy не встановлено — немає з чого відкочуватися." \
                                              "OBS-Legacy is not installed — there is nothing to roll back from.")"
  if [[ -z "$target" ]]; then
    while IFS= read -r l; do
      [[ -n "$l" && "$l" != "$cur" && -f "$PKG_STORE/$l/.complete" ]] || continue
      if [[ "$l" < "$cur" ]]; then older+=("$l"); fi
    done < <(store_sets)
    if ((${#older[@]} == 0)); then die "$(t "Немає старішого збереженого набору пакетів (див. $SELF_PATH status)." \
                                            "There is no older saved package set (see $SELF_PATH status).")"; fi
    target=${older[-1]}
  fi
  [[ "$target" =~ ^[A-Za-z0-9._-]+$ && -f "$PKG_STORE/$target/.complete" ]] \
    || die "$(t "Набору '$target' немає або він неповний (див. $SELF_PATH status)." \
                "Set '$target' does not exist or is incomplete (see $SELF_PATH status).")"
  [[ "$target" != "$cur" ]] || die "$(t "Набір $target уже встановлено." "Set $target is already installed.")"
  mapfile -t files < <(find "$PKG_STORE/$target" -maxdepth 1 -type f -name "*-$PKG_SUFFIX-*.pkg.tar.*" ! -name '*.sig' | sort)
  ((${#files[@]})) || die "$(t "У наборі $target немає пакетів." "Set $target has no packages.")"
  mapfile -t stale < <(stale_plugin_packages "${files[@]}")
  set_conflicts_from_files "${files[@]}"
  mapfile -t replaced < <(replaced_packages)
  if [[ -n "$cur" && "$target" < "$cur" ]]; then
    printf "$(t '  Буде встановлено (pacman попередить, що версії старіші):\n' \
                '  Will be installed (pacman will warn that the versions are older):\n')" >&2
  else
    printf "$(t '  Буде встановлено:\n' '  Will be installed:\n')" >&2
  fi
  printf '    %s\n' "${files[@]##*/}" >&2
  if ((${#replaced[@]})); then
    printf "$(t '  Буде замінено (pacman спитає — відповідай y): %s\n' \
                '  Will be replaced (pacman will ask — answer y): %s\n')" "${replaced[*]}" >&2
  fi
  if ((${#stale[@]})); then
    printf "$(t '  Потім буде прибрано (плагінів немає в наборі): %s\n' \
                '  Will then be removed (the plugins are not in the set): %s\n')" "${stale[*]}" >&2
  fi
  if legacy_leftovers_present; then
    printf "$(t '  Потім буде прибрано стару схему: %s, обгортку %s, ярлик-перекриття і хук у /etc.\n' \
                '  The old layout will then be removed: %s, the wrapper %s, the overriding shortcut and the hook in /etc.\n')" \
      "$INSTALL_ROOT" "$WRAPPER" >&2
  fi
  check_pacman_lock
  confirm "$(t "Поставити набір $target${cur:+ замість $cur}?" \
               "Install set $target${cur:+ instead of $cur}?")" || cancelled
  priv_validate || die "$(t "Не вдалося отримати права адміністратора." \
                            "Could not get administrator rights.")" "$PRIV_HINT"
  priv pacman -U -- "${files[@]}" \
    || die "$(t "pacman не встановив пакети — нічого не змінено." \
                "pacman did not install the packages — nothing was changed.")" \
      "$(t "Якщо ти відповів «n» на питання pacman — так і має бути. Інакше дивись повідомлення pacman вище." \
           "If you answered “n” to pacman's question, that is expected. Otherwise see the pacman messages above.")"
  remove_stale_plugins "${files[@]}"
  if legacy_leftovers_present; then remove_legacy_install; fi
  if [[ -n "$cur" ]]; then
    ok "$(t "Готово: встановлено набір $target. Повернутися: $SELF_PATH rollback $cur" \
            "Done: set $target is installed. To go back: $SELF_PATH rollback $cur")"
  else
    ok "$(t "Готово: встановлено набір $target." "Done: set $target is installed.")"
  fi
}

cmd_uninstall() {
  local p
  local -a pkgs=() others=()
  preflight_common
  select_priv_tool
  require_tools flock
  acquire_lock
  print_header "$(t "видалення OBS-Legacy" "removing OBS-Legacy")"
  mapfile -t pkgs < <(pacman -Qq 2>/dev/null | grep -E -- "-$PKG_SUFFIX\$" || true)
  if ((${#pkgs[@]} == 0)) && ! legacy_leftovers_present; then
    ok "$(t "OBS-Legacy не встановлено — прибирати нічого." "OBS-Legacy is not installed — nothing to remove.")"
    return 0
  fi
  if have pgrep && pgrep -x obs >/dev/null 2>&1; then
    die "$(t "OBS зараз запущений." "OBS is running right now.")" \
        "$(t "Закрий OBS і повтори." "Close OBS and try again.")"
  fi
  for p in "${pkgs[@]}"; do
    if [[ "$p" != "$FFMPEG_PKG" ]]; then others+=("$p"); fi
  done
  printf "$(t 'Буде виконано:\n' 'Will be done:\n')" >&2
  if [[ " ${pkgs[*]} " == *" $FFMPEG_PKG "* ]]; then
    printf "$(t '  - pacman -S ffmpeg: системний ffmpeg з репозиторію замінить %s\n' \
                '  - pacman -S ffmpeg: the system ffmpeg from the repository will replace %s\n')" "$FFMPEG_PKG" >&2
  fi
  if ((${#others[@]})); then printf '  - pacman -R %s\n' "${others[*]}" >&2; fi
  if legacy_leftovers_present; then
    printf "$(t '  - прибрано стару збірку %s, обгортку %s, ярлик і хук у /etc\n' \
                '  - the old build %s, the wrapper %s, the shortcut and the hook in /etc will be removed\n')" "$INSTALL_ROOT" "$WRAPPER" >&2
  fi
  printf "$(t 'Не буде змінено: налаштування ~/.config/obs-studio, кеш збірки, набори пакетів у %s.\n' \
              'Will not be changed: the ~/.config/obs-studio settings, the build cache, the package sets in %s.\n')" "$PKG_STORE" >&2
  check_pacman_lock
  confirm "$(t "Продовжити?" "Continue?")" || cancelled
  priv_validate || die "$(t "Не вдалося отримати права адміністратора." \
                            "Could not get administrator rights.")" "$PRIV_HINT"
  if [[ " ${pkgs[*]} " == *" $FFMPEG_PKG "* ]]; then
    priv pacman -S ffmpeg || die "$(t "Не вдалося повернути системний ffmpeg." \
                                      "Could not bring back the system ffmpeg.")" \
      "$(t "pacman спитає, чи замінити $FFMPEG_PKG — відповідай y. Якщо бачиш 404 чи «failed retrieving file», спершу онови систему: sudo pacman -Syu." \
           "pacman will ask whether to replace $FFMPEG_PKG — answer y. If you see 404 or “failed retrieving file”, first upgrade the system: sudo pacman -Syu.")"
  fi
  if ((${#others[@]})); then
    priv pacman -R -- "${others[@]}" || die "$(t "pacman не прибрав пакети OBS-Legacy." \
                                                 "pacman did not remove the OBS-Legacy packages.")" \
      "$(t "Якщо інші пакети (плагіни з AUR) потребують obs-studio — постав звичайний OBS замість OBS-Legacy: sudo pacman -S obs-studio (pacman запропонує замінити $OBS_PKG), а потім прибери плагіни -$PKG_SUFFIX: sudo pacman -R <назви>." \
           "If other packages (AUR plugins) need obs-studio, install the regular OBS instead of OBS-Legacy: sudo pacman -S obs-studio (pacman will offer to replace $OBS_PKG), then remove the -$PKG_SUFFIX plugins: sudo pacman -R <names>.")"
  fi
  if legacy_leftovers_present; then remove_legacy_install; fi
  ok "$(t "OBS-Legacy прибрано." "OBS-Legacy removed.")"
  printf "$(t '  Звичайний OBS з репозиторію: sudo pacman -S obs-studio (на драйвері 580xx — без NVENC).\n' \
              '  The regular OBS from the repository: sudo pacman -S obs-studio (no NVENC on the 580xx driver).\n')" >&2
  printf "$(t '  Набори пакетів лишились (%s): rm -rf %q\n' \
              '  The package sets are kept (%s): rm -rf %q\n')" "$(du -sh -- "$PKG_STORE" 2>/dev/null | cut -f1 || echo '?')" "$PKG_STORE" >&2
  printf "$(t '  Кеш збірки: rm -rf %q · копії налаштувань OBS: %s\n' \
              '  Build cache: rm -rf %q · OBS settings backups: %s\n')" "$BUILD_ROOT" "$STATE_DIR/config-backups" >&2
  if [[ -s "$STATE_DIR/packages-installed-by-script.txt" ]]; then
    printf "$(t '  Пакети, які колись поставив скрипт (перевір, чи вони потрібні іншим програмам):\n    %s\n' \
                '  Packages the script once installed (check whether other programs need them):\n    %s\n')" \
      "$(sort -u "$STATE_DIR/packages-installed-by-script.txt" | tr '\n' ' ')" >&2
  fi
}

# Лише пакети для збірки: той самий список і ті самі запобіжники, що й в install
# (confirm, pacman -S --needed, без -Syu). «deps --missing» — для TUI: друкує, чого
# бракує («repo <пакет>» або «norepo <пакет>»), нічого не ставить і не питає.
cmd_deps() {
  local p
  if [[ "${1:-}" == --missing ]]; then
    have pacman || return 0
    resolve_selected_plugins 2>/dev/null
    load_ffmpeg_deps 1
    plan_dependencies
    for p in "${DEP_INSTALL[@]}"; do printf 'repo %s\n' "$p"; done
    for p in "${DEP_NOREPO_REQ[@]}"; do printf 'norepo %s\n' "$p"; done
    return 0
  fi
  preflight_common
  select_priv_tool
  acquire_lock
  print_header "$(t "встановлення пакетів для збірки" "installing the build dependencies")"
  resolve_selected_plugins
  load_ffmpeg_deps 0
  install_dependencies
  plan_dependencies
  if ((${#DEP_INSTALL[@]})); then
    die "$(t "Не всі пакети встановилися: ${DEP_INSTALL[*]}." \
             "Not all packages were installed: ${DEP_INSTALL[*]}.")" "$NOREPO_HINT"
  fi
  printf "$(t '\n%sПАКЕТИ ГОТОВІ%s: усе потрібне для збірки встановлено.\n' \
              '\n%sPACKAGES READY%s: everything needed for the build is installed.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
  hint "$(t "Тепер перевір систему ще раз ($(check_action)) — усе має бути ✓." \
            "Now check the system again ($(check_action)) — everything should be ✓.")"
}

cmd_clean() {
  local before after
  preflight_common
  acquire_lock
  before=$(du -sh -- "$BUILD_ROOT" 2>/dev/null | cut -f1 || true)
  # Включно з каталогами попередньої версії скрипта. Вихідні коди (OBS, плагіни,
  # git-репозиторій ffmpeg), архів CEF і набори пакетів лишаються.
  rm_cache_path "$BUILD_DIR" "$CHECK_DIR" "$STAGE_DIR" "$CEF_DIR" "$TOOLS_DIR" "$MAKEPKG_DIR" \
    "$BUILD_ROOT/obs-build" "$BUILD_ROOT/obs-check-build" "$BUILD_ROOT/cef-extract"
  if [[ -d "$PLUGINS_DIR" ]]; then
    find "$PLUGINS_DIR" -mindepth 2 -maxdepth 2 -type d \( -name build -o -name stage \) -print0 \
      | while IFS= read -r -d '' d; do rm_cache_path "$d"; done
  fi
  after=$(du -sh -- "$BUILD_ROOT" 2>/dev/null | cut -f1 || true)
  ok "$(t "Кеш: було ${before:-?}, стало ${after:-?}. Вихідні коди (OBS, плагіни, ffmpeg) й архів CEF залишено." \
          "Cache: was ${before:-?}, now ${after:-?}. Sources (OBS, plugins, ffmpeg) and the CEF archive are kept.")"
  printf "$(t '  Повністю прибрати кеш: rm -rf %q\n' '  Remove the cache completely: rm -rf %q\n')" "$BUILD_ROOT" >&2
  printf "$(t '  Набори пакетів для відкату (%s): %s\n' \
              '  Package sets for rollback (%s): %s\n')" "$(du -sh -- "$PKG_STORE" 2>/dev/null | cut -f1 || echo '0')" "$PKG_STORE" >&2
}

# Каталог плагінів. --tsv — машинний формат для TUI:
# id, назва, вибрано (1/0), типово (1/0), репозиторій, пакети, опис, назва пакета.
cmd_plugins() {
  local r id name repo pkgs desc sel def pkg
  local -a f=()
  resolve_selected_plugins
  if [[ "${1:-}" != --tsv ]]; then
    printf "$(t 'Плагіни, які збираються разом з OBS (OBS_PLUGINS="%s"):\n\n' \
                'Plugins built together with OBS (OBS_PLUGINS="%s"):\n\n')" "${PLUGINS_SELECTED[*]:-none}"
  fi
  for r in "${PLUGIN_CATALOG[@]}"; do
    IFS='|' read -r -a f <<<"$r"
    id=${f[0]} name=${f[1]-} repo=${f[2]-} pkgs=${f[3]-} desc=${f[5]-}
    pkg=$(plugin_pkgname "$id")
    sel=0 def=0
    if [[ " ${PLUGINS_SELECTED[*]} " == *" $id "* ]]; then sel=1; fi
    if [[ "$id" == obs-multi-rtmp ]]; then def=1; fi
    if [[ "${1:-}" == --tsv ]]; then
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$name" "$sel" "$def" "$repo" "$pkgs" "$desc" "$pkg"
    else
      printf "$(t '  [%s] %-24s %s\n        %s\n        %s · пакет %s\n' \
                  '  [%s] %-24s %s\n        %s\n        %s · package %s\n')" "$( ((sel)) && echo x || echo ' ')" "$id" "$name" \
        "$desc" "$repo" "$pkg"
    fi
  done
  if [[ "${1:-}" != --tsv ]]; then
    printf "$(t '\nВибір: OBS_PLUGINS="obs-multi-rtmp,move-transition" або файл %s\n' \
                '\nTo choose: OBS_PLUGINS="obs-multi-rtmp,move-transition" or the file %s\n')" "$SETTINGS_FILE"
  fi
}

# Запускає команду з дублюванням виводу у звіт; завершує скрипт її кодом.
run_logged() {
  local fn=$1 raw rc
  shift
  mkdir -p -- "$REPORT_DIR"
  raw="$REPORT_DIR/$(date +%Y%m%d-%H%M%S)-${fn#cmd_}.raw"
  trap - ERR
  set +e
  (
    set -Eeuo pipefail
    trap 'on_error "$LINENO" "$BASH_COMMAND"' ERR
    "$fn" "$@"
  ) 2>&1 | tee -- "$raw"
  rc=${PIPESTATUS[0]}
  # Звіт без кольорів і без проміжних кадрів індикаторів прогресу. Останній
  # рядок «Результат: …» читає TUI (головний екран і список звітів).
  if sed -E 's/\x1B\[[0-9;]*[A-Za-z]//g; s/^.*\r//' -- "$raw" > "${raw%.raw}.txt" 2>/dev/null; then
    rm -f -- "$raw"
    case "$rc" in
      0) printf '\n%s\n' "$(t 'Результат: успішно' 'Result: success')" ;;
      "$EXIT_CANCELLED") printf '\n%s\n' "$(t 'Результат: скасовано' 'Result: cancelled')" ;;
      *) printf '\n%s\n' "$(t "Результат: помилка (код $rc)" "Result: error (code $rc)")" ;;
    esac >> "${raw%.raw}.txt"
    printf '\n%s: %s\n' "$(t Звіт Report)" "${raw%.raw}.txt" >&2
  fi
  exit "$rc"
}

choose_from_menu() {
  local choice=''
  if [[ ! -t 0 ]]; then usage; exit 1; fi
  if ((LANG_EN)); then
    cat >&2 <<EOF

OBS Studio with NVENC for NVIDIA 580.xx — $SELF_NAME $SCRIPT_VERSION
  1) Check (installs nothing, no sudo)
  2) Install (or update, if there is something to update)
  3) Status
  4) Roll back to the previous package set
  5) Remove OBS-Legacy
  6) Clean intermediate build files
  7) Plugin catalog
  8) Install the build dependencies
  9) Only build the packages (without installing)
  r) Rebuild from scratch and install again
  0) Quit
EOF
  else
    cat >&2 <<EOF

OBS Studio з NVENC для NVIDIA 580.xx — $SELF_NAME $SCRIPT_VERSION
  1) Перевірка (нічого не встановлює, без sudo)
  2) Встановити (або оновити, якщо є що оновлювати)
  3) Стан
  4) Відкат на попередній набір пакетів
  5) Видалити OBS-Legacy
  6) Очистити проміжні файли збірки
  7) Каталог плагінів
  8) Встановити пакети для збірки
  9) Лише зібрати пакети (без встановлення)
  r) Перезібрати начисто й встановити знову
  0) Вихід
EOF
  fi
  printf "$(t 'Вибір: ' 'Choice: ')" >&2
  read -r choice || exit 1
  case "$choice" in
    1) MENU_CMD=check ;;
    2) if [[ -f "$INFO_DIR/BUILDINFO.txt" ]]; then MENU_CMD=update; else MENU_CMD=install; fi ;;
    3) MENU_CMD=status ;;
    4) MENU_CMD=rollback ;;
    5) MENU_CMD=uninstall ;;
    6) MENU_CMD=clean ;;
    7) MENU_CMD=plugins ;;
    8) MENU_CMD=deps ;;
    9) MENU_CMD=package ;;
    r|R|к|К) MENU_CMD=rebuild ;;
    0|'') exit 0 ;;
    *) die "$(t "Невідомий вибір: $choice" "Unknown choice: $choice")" ;;
  esac
}

main() {
  local cmd=${1:-}
  if (( $# > 0 )); then shift; fi
  cmd=${cmd#--}
  if [[ -z "$cmd" ]]; then
    choose_from_menu
    cmd=$MENU_CMD
  fi
  case "$cmd" in
    help|h|-h)      usage ;;
    check)          run_logged cmd_check ;;
    install)        run_logged cmd_install ;;
    update)         run_logged cmd_update ;;
    rebuild)        run_logged cmd_rebuild ;;
    package)        run_logged cmd_package ;;
    rollback)       run_logged cmd_rollback "$@" ;;
    uninstall)      run_logged cmd_uninstall ;;
    deps)
      if [[ "${1:-}" == --missing ]]; then cmd_deps --missing; else run_logged cmd_deps; fi
      ;;
    status)         cmd_status ;;
    clean)          cmd_clean ;;
    plugins)        cmd_plugins "$@" ;;
    version)        printf '%s %s\n' "$SELF_NAME" "$SCRIPT_VERSION" ;;
    *)              usage >&2; die "$(t "Невідома команда: $cmd" "Unknown command: $cmd")" ;;
  esac
}

main "$@"
