//! Довідка і «Про програму».

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::intro::{REPO_URL, credit_lines, notice_lines};
use super::{bullet, footer, panel, section, top_bar};
use crate::app::{App, short_path};
use crate::reports;
use crate::script;
use crate::settings;
use crate::theme::Theme;

fn text(t: &Theme, width: u16) -> Vec<Line<'static>> {
    let p = |s: &str| vec![Line::from(s.to_string())];
    let b = |s: &str| bullet(Span::styled("  •  ", Style::new().fg(t.accent2)), s, width);
    let n = |i: u8, s: &str| bullet(Span::styled(format!("  {i}.  "), t.title()), s, width);
    let k = |key: &str, what: &str| {
        vec![Line::from(vec![Span::styled(format!("  {key:<16}"), t.key()), Span::raw(what.to_string())])]
    };
    let code = |s: &str| vec![Line::from(Span::styled(format!("    {s}"), Style::new().fg(t.accent2)))];
    let gap = || vec![Line::from("")];
    let head = |s: &'static str| vec![section(t, s)];
    let script_path = short_path(&script::data_dir().join(script::SCRIPT_NAME));
    let store = short_path(&script::package_store());
    let parts: Vec<Vec<Line<'static>>> = vec![
        head("Швидкий старт"),
        n(1, "«Перевірити систему» — переконатися, що все готово. Нічого не змінює, 5–15 хв."),
        n(2, "«Плагіни» — за бажанням обери, що зібрати разом з OBS (obs-multi-rtmp уже обрано)."),
        n(
            3,
            "«Встановити / оновити OBS» — збирає OBS під твій комп'ютер, перевіряє NVENC і ставить пакети \
               pacman. 30–90 хв (уперше з ffmpeg — довше).",
        ),
        n(4, "Запускай OBS як завжди: з меню програм, командою obs або пунктом «Запустити OBS»."),
        gap(),
        p("Не знаєш, що робити далі? Панель «Що далі» на головному екрані підкаже наступний крок, а курсор \
           у меню вже стоїть на ньому — досить натиснути Enter."),
        gap(),
        head("Що це"),
        p("OBS-Legacy збирає OBS Studio з робочим апаратним кодуванням NVENC для відеокарт NVIDIA на \
           драйверах гілки 580xx (Maxwell / Pascal / Volta: GTX 900, GTX 1000, Titan V) на Artix та Arch \
           Linux. Ці драйвери підтримують NVENC API лише до 13.0, а OBS і ffmpeg з репозиторіїв зібрані з \
           новішим SDK 13.1 — тому NVENC там не вмикається. OBS-Legacy збирає офіційний OBS із заголовками \
           NVENC 13.0, офіційним CEF (Browser Source), вибраними плагінами й ffmpeg і ставить усе пакетами \
           pacman з припискою -legacy-580xx: obs-studio-legacy-580xx, <плагін>-legacy-580xx, \
           ffmpeg-legacy-580xx."),
        gap(),
        head("Оптимізація під залізо"),
        b("Програма сама визначає дистрибутив, процесор, відеокарту й драйвер і підбирає параметри: прапорці \
           компілятора під цей процесор (-march=native: AVX тощо), решту — з makepkg.conf дистрибутива; \
           заголовки NVENC — найновіші, які знає драйвер; для ffmpeg — CUDA-архітектуру відеокарти."),
        b("Усе це видно в звіті на етапі «Оптимізація під залізо» і в «Твоя система» (клавіша d)."),
        b("Такі пакети найшвидші, але працюють лише на цьому процесорі. Після заміни процесора програма \
           попередить і запропонує перезібрати. Переносні пакети: «Налаштування → Під який процесор»."),
        gap(),
        head("Якщо щось пішло не так"),
        b("Після кожного запуску внизу з'являється картка: що сталося і що робити. Поради пише сам скрипт."),
        b("Якщо це можна виправити одразу (наприклад, поставити пакети, яких бракує), головна кнопка картки — \
           «Виправити» (Enter). Після неї програма запропонує перевірити систему ще раз."),
        b("Кожен запуск зберігає повний звіт. Клавіша o відкриває його прямо тут, c копіює шлях, e відкриває \
           в програмі для тексту. Усі звіти — «Додатково → Звіти»."),
        b("У переглядачі звіту n і p стрибають між попередженнями й помилками."),
        b("Пакети ставляться лише після всіх перевірок, тож поточний OBS не зламається."),
        b("Не подобається нова версія — «Відкат на іншу збірку» (попередній набір пакетів). Не потрібен \
           OBS-Legacy — «Видалити OBS-Legacy»."),
        b("Не допомогло — створи issue на GitHub і додай звіт."),
        gap(),
        head("Надовго, замість OBS з репозиторію"),
        b("OBS-Legacy ставиться в ті самі місця, що й OBS з репозиторію (/usr), і «надає» obs-studio: \
           pacman сам пропонує замінити obs-studio чи obs-studio-git, а плагіни з AUR ставляться на нього \
           як завжди. Команда obs і ярлик меню — звичайні, з пакета."),
        b("Пакет OBS містить pacman-хук: після кожного pacman -Syu він перевіряє бібліотеки пакетів \
           OBS-Legacy й підказує, коли перезібрати (зазвичай після оновлення ffmpeg чи Qt), а також — коли \
           в репозиторії вийшов новий ffmpeg."),
        b("Оновлення — той самий пункт «Встановити / оновити OBS»: коли OBS уже стоїть, типова кнопка — \
           «Оновити». Вона спершу перевіряє, що нового (OBS, плагіни, ffmpeg, бібліотеки), і збирає, лише \
           якщо є що оновлювати; картка результату показує, що змінилося («OBS 33.0.0 → 33.0.1»). «Що далі» \
           підкаже, коли вийшла нова версія, а сповіщення (нижче) — навіть коли програма закрита."),
        b("«Перезібрати» (клавіша R у діалозі оновлення або «Додатково → Перезібрати OBS») збирає все з нуля \
           й ставить знову — коли збирання обірвалося (вимкнулося світло, закрилося вікно) чи OBS поводиться \
           дивно. Обірване збирання «Що далі» помітить само."),
        b("Кожне збирання зберігається набором пакетів; попередній лишається — відкат без перезбирання."),
        b("Якщо колись нова версія OBS перестане працювати з драйвером 580xx, у «Налаштуваннях» можна \
           зафіксувати останню робочу (поле «Версія OBS»)."),
        gap(),
        head("Сповіщення про оновлення"),
        b("«Додатково → Сповіщення про оновлення» вмикає фонову перевірку: раз на 12 годин — чи вийшли нові \
           OBS, плагіни чи сам OBS-Legacy; після кожного оновлення системи — чи власному OBS не бракує \
           бібліотек. Про кожну новину — одне сповіщення на робочому столі, а клацання по ньому відкриває \
           OBS-Legacy."),
        b("Перевірку запускає служба користувача твоєї init-системи: systemd, dinit, OpenRC (0.60+), runit чи \
           s6 (тека служб користувача, напр. від turnstile). Якщо init-система служб користувача не запускає — \
           автозапуск графічної сесії. sudo не потрібен."),
        b("Без меню: obs-legacy --notifier status | enable | disable | test, а перевірити просто зараз — \
           obs-legacy --check-updates."),
        gap(),
        head("Browser Source і плагіни"),
        p("Browser Source вбудований: збірка використовує офіційний Linux-архів CEF від OBS, а його SHA-256 \
           звіряється з офіційним хешем із вихідних кодів OBS. Якщо obs-browser не зібрався, встановлення \
           скасовується — поточний OBS лишається як був."),
        gap(),
        p("Кожен плагін збирається з вихідних кодів саме під твою збірку OBS і під твій процесор і стає \
           окремим пакетом <назва в AUR>-legacy-580xx (напр. obs-multi-rtmp-legacy-580xx), тож \
           оновлюється й відкочується разом з OBS, а pacman сам пропонує замінити такий самий плагін з AUR. \
           Якщо плагін не збереться (наприклад, ще не підтримує нову версію OBS), OBS встановиться без \
           нього, з попередженням. Зафіксувати версію плагіна: рядок PLUGIN_REF_<id>=тег у settings.env \
           (дефіси в id замінити на _), напр. PLUGIN_REF_obs_multi_rtmp=0.7.4.5."),
        gap(),
        head("Клавіші"),
        k("↑↓ / j k", "вибір у списках"),
        k("Enter", "відкрити / запустити / змінити"),
        k("Esc / q", "назад (у меню — вихід)"),
        k("o", "переглянути звіт"),
        k("c / e / f", "копіювати шлях звіту / відкрити в програмі / відкрити теку"),
        k("d", "панель «Твоя система»: коротко або докладно"),
        k("?  F1", "ця довідка"),
        k("r  F5", "оновити відомості"),
        k("Пробіл", "поставити / зняти галочку плагіна"),
        k("PgUp / PgDn", "прокрутка виводу і звітів"),
        k("Ctrl+C ×2", "перервати завдання (ще ×2 — зупинити примусово)"),
        k("Миша", "клац — вибрати, ще клац — відкрити; коліщатко — прокрутка"),
        k("Shift+миша", "виділити й скопіювати текст у більшості терміналів"),
        gap(),
        p("Під час роботи літери, Enter і Backspace ідуть просто в скрипт: так вводяться пароль sudo/doas і \
           відповіді на запитання [y/N]. Коли скрипт чекає відповіді, внизу з'являється жовта підказка. \
           Стрілки, Home і End гортають вивід, а в скрипт не йдуть. Відповідай y латиницею: в українській \
           розкладці на клавіші Y — «н», і скрипт перепитає."),
        gap(),
        head("Де що лежить"),
        b("Пакети: pacman -Qs legacy-580xx; відомості про збірку — /usr/share/obs-legacy/"),
        b(&format!("Набори пакетів для відкату: {store}/")),
        b(&format!("Звіти: {}", short_path(&reports::dir()))),
        b("Кеш збірки: ~/.cache/obs-studio-nvenc-artix/  (журнали плагінів — plugins/<id>/build.log)"),
        b("Копії налаштувань OBS перед кожним оновленням: ~/.local/share/obs-studio-nvenc-artix/config-backups/"),
        b(&format!("Налаштування OBS-Legacy: {}", short_path(&settings::settings_path()))),
        b(&format!("Сповіщення про оновлення: що вже показано і журнал — {}", short_path(&script::state_dir()))),
        b(&format!("Скрипт збирання: {script_path} — його можна запускати й без TUI:")),
        code(&format!("{script_path} help")),
        gap(),
        head("Про програму"),
        vec![Line::from(vec![
            Span::styled(format!("OBS-Legacy v{}", env!("CARGO_PKG_VERSION")), t.title()),
            Span::styled(format!(" · скрипт {} · ", script::script_version()), t.muted()),
            Span::styled(REPO_URL, Style::new().fg(t.accent2).add_modifier(Modifier::UNDERLINED)),
        ])],
        gap(),
    ];
    let mut v: Vec<Line<'static>> = parts.into_iter().flatten().collect();
    v.extend(notice_lines(t, width));
    v.push(Line::from(""));
    v.extend(credit_lines(t));
    v.push(Line::from(""));
    v.push(Line::from(Span::styled(
        "Окрема подяка розробникам OBS Studio, FFmpeg/nv-codec-headers, ratatui та авторам плагінів.",
        t.muted(),
    )));
    v
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let [bar, _, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
            .areas(area);
    top_bar(f, app, bar, &["Довідка"], &format!("v{}", env!("CARGO_PKG_VERSION")));
    let width = body.width.saturating_sub(2).min(110);
    let r = body.centered_horizontally(Constraint::Length(width));
    let block = panel(&t, Line::from(Span::styled(" Довідка і про програму ", t.title())), true)
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = block.inner(r);
    let para = Paragraph::new(text(&t, inner.width)).wrap(Wrap { trim: false });
    let total = para.line_count(inner.width) as u16;
    app.help_max_scroll = total.saturating_sub(inner.height);
    app.help_scroll = app.help_scroll.min(app.help_max_scroll);
    f.render_widget(para.block(block).scroll((app.help_scroll, 0)), r);
    let pos = if app.help_max_scroll == 0 {
        String::new()
    } else {
        format!("{}%", u32::from(app.help_scroll) * 100 / u32::from(app.help_max_scroll))
    };
    footer(f, app, foot, &[("Esc", "назад"), ("↑↓ PgUp PgDn", "прокрутка"), ("", pos.as_str())], true);
}
