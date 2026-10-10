//! Native window (egui/eframe, OpenGL) with data-labeling buttons. 0.2.0; 0.2.2: one 标点 ping button,
//! embedded app icon, system tray (show/hide/exit, close-to-tray), native window placement restore,
//! "打开战术地图" button ([gui] map_url), status summary line, optional autostart (HKCU Run).
//! Keyboard: no hooks / hotkeys; label buttons react to mouse clicks only (a focused button never fires on
//! Space/Enter, so keys meant for the game can't create labels), and the window never grabs focus by itself.
//! Collection runs on the tokio runtime in the background; the window only reads shared state and
//! queues label events (Shared::add_label). All UI text is Simplified Chinese.
use crate::desktop::{self, Hwnd, Tray, TrayCmd};
use crate::labels::{self, LabelDef};
use crate::{gate, Cfg, Shared, UiLink, VERSION};
use eframe::egui::{self, Color32, RichText};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

// ---------------------------------------------------------------- persisted window settings

#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, Debug)]
struct WinState {
    x: Option<f32>,
    y: Option<f32>,
    w: Option<f32>,
    h: Option<f32>,
    always_on_top: Option<bool>,
    /// 0.2.2: restored window rect in physical (workspace) pixels from GetWindowPlacement
    #[serde(default)]
    rect_px: Option<[i32; 4]>,
}

fn state_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("wt-collector").join("gui.json"))
}

fn load_state() -> WinState {
    state_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_state(s: &WinState) {
    if let Some(p) = state_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec_pretty(s).unwrap_or_default());
    }
}

// ---------------------------------------------------------------- fonts

/// Font candidates with Chinese glyphs: "path" or "path#index" (for .ttc collections).
fn font_candidates(cfg_font: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(f) = cfg_font {
        v.push(f.to_string());
    }
    if let Ok(f) = std::env::var("WT_GUI_FONT") {
        v.push(f);
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for f in ["msyh.ttc", "msyh.ttf", "Deng.ttf", "simhei.ttf", "simsun.ttc", "msjh.ttc"] {
        v.push(format!("{windir}\\Fonts\\{f}"));
    }
    for f in [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc#2",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc#2",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ] {
        v.push(f.to_string());
    }
    v
}

fn install_fonts(ctx: &egui::Context, cfg_font: Option<&str>) -> Option<String> {
    for c in font_candidates(cfg_font) {
        let (path, idx) = match c.rsplit_once('#') {
            Some((p, i)) if i.chars().all(|d| d.is_ascii_digit()) && !i.is_empty() => (p.to_string(), i.parse().unwrap_or(0)),
            _ => (c.clone(), 0u32),
        };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let mut fd = egui::FontData::from_owned(bytes);
        fd.index = idx;
        let mut defs = egui::FontDefinitions::default();
        defs.font_data.insert("cjk".into(), Arc::new(fd));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(fam).or_default().push("cjk".into());
        }
        ctx.set_fonts(defs);
        return Some(c);
    }
    None
}

// ---------------------------------------------------------------- Windows console handling

/// Double-clicked exe: the console window was created just for us -> hide it (the window is the UI).
/// Started from cmd/PowerShell (`wt-collector gui`): keep the console.
#[cfg(windows)]
fn detach_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList, SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    unsafe {
        let mut pids = [0u32; 4];
        let n = GetConsoleProcessList(pids.as_mut_ptr(), 4);
        if n == 1 {
            FreeConsole();
            // std treats a null std handle as a sink: println!/eprintln! become no-ops instead of failing
            SetStdHandle(STD_OUTPUT_HANDLE, std::ptr::null_mut());
            SetStdHandle(STD_ERROR_HANDLE, std::ptr::null_mut());
            SetStdHandle(STD_INPUT_HANDLE, std::ptr::null_mut());
        }
    }
}
#[cfg(not(windows))]
fn detach_console() {}

/// Fatal error before the window exists (e.g. bad wt-collector.toml).
pub fn fatal(msg: &str) {
    eprintln!("ERROR: {msg}");
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let text = w(&format!("{msg}\n\n请检查 wt-collector.toml（与 exe 同一文件夹）。"));
        let title = w("wt-collector 错误");
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

// ---------------------------------------------------------------- app

pub fn run_gui(cfg: Cfg, rt: &tokio::runtime::Runtime, start_in_tray: bool) -> i32 {
    crate::QUIET.store(true, Ordering::Relaxed);
    detach_console();
    let title = format!("WT 战术指挥 · 数据采集 v{VERSION}");
    if desktop::activate_existing(&title) {
        // already running (maybe hidden in the tray): its window was brought to the front
        return 0;
    }
    let (stop_tx, stop_rx) = watch::channel(false);
    let (shared_tx, shared_rx) = std::sync::mpsc::channel();
    let link = UiLink { shared_tx, stop: stop_rx };
    let collector: Handle = Arc::new(std::sync::Mutex::new(Some(rt.spawn(crate::run(cfg.clone(), Some(link))))));

    let st = load_state();
    let on_top = st.always_on_top.unwrap_or(cfg.gui.always_on_top);
    let mut vp = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("wt-collector")
        .with_icon(egui::IconData { rgba: desktop::ICON64.to_vec(), width: 64, height: 64 })
        .with_inner_size([st.w.unwrap_or(400.0).clamp(300.0, 1200.0), st.h.unwrap_or(600.0).clamp(300.0, 1400.0)])
        .with_min_inner_size([300.0, 300.0]);
    // logical position only as a fallback for old gui.json files without rect_px (non-Windows / first run)
    if let (Some(x), Some(y), None) = (st.x, st.y, st.rect_px) {
        // ignore positions far off-screen (monitor removed)
        if x > -200.0 && y > -50.0 && x < 6000.0 && y < 4000.0 {
            vp = vp.with_position([x, y]);
        }
    }
    if on_top {
        vp = vp.with_window_level(egui::WindowLevel::AlwaysOnTop);
    }
    if start_in_tray && cfg!(windows) {
        vp = vp.with_visible(false).with_active(false);
    }
    let opts = eframe::NativeOptions { viewport: vp, renderer: eframe::Renderer::Glow, ..Default::default() };
    let app_cfg = cfg.clone();
    let collector2 = collector.clone();
    let res = eframe::run_native(
        "wt-collector",
        opts,
        Box::new(move |cc| {
            let font = install_fonts(&cc.egui_ctx, app_cfg.gui.font.as_deref());
            cc.egui_ctx.set_zoom_factor(app_cfg.gui.scale);
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            if font.is_none() {
                crate::say!("[界面] 未找到中文字体（可在 [gui] font = \"C:\\\\Windows\\\\Fonts\\\\msyh.ttc\" 指定）");
            }
            let hwnd = Hwnd::from_handle(cc);
            if let Some(r) = st.rect_px {
                hwnd.restore_placement(r);
            }
            let ctx = cc.egui_ctx.clone();
            let cmds: Arc<std::sync::Mutex<Vec<TrayCmd>>> = Default::default();
            let cmds2 = cmds.clone();
            let tray = match Tray::new(move |c| {
                // runs inside the window's message loop; the window may be hidden, so show it natively first
                if matches!(c, TrayCmd::Show | TrayCmd::Exit) {
                    hwnd.show();
                } else if c == TrayCmd::Toggle {
                    if hwnd.is_visible() && !hwnd.is_minimized() {
                        hwnd.hide();
                    } else {
                        hwnd.show();
                    }
                }
                cmds2.lock().unwrap().push(c);
                ctx.request_repaint();
            }) {
                Ok(t) => Some(t),
                Err(e) => {
                    crate::say!("[界面] 系统托盘不可用：{e}");
                    if start_in_tray {
                        hwnd.show();
                    }
                    None
                }
            };
            let has_tray = tray.is_some();
            let mut app = App::new(app_cfg, shared_rx, collector2, st, on_top, font, hwnd, tray, cmds);
            app.start_hidden = start_in_tray && has_tray;
            Ok(Box::new(app))
        }),
    );
    let _ = stop_tx.send(true);
    if let Err(e) = &res {
        fatal(&format!("无法创建窗口（OpenGL）：{e}\n可改用命令行模式：wt-collector.exe run"));
    }
    // collector keeps running until here; give it time to flush memory + pending labels
    let h = collector.lock().unwrap().take();
    if let Some(h) = h {
        rt.block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(15), h).await;
        });
    }
    if res.is_err() { 1 } else { 0 }
}

type Handle = Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<anyhow::Result<()>>>>>;

struct Flash {
    text: String,
    at: Instant,
    color: Color32,
}

struct App {
    cfg: Cfg,
    shared_rx: std::sync::mpsc::Receiver<Arc<Shared>>,
    sh: Option<Arc<Shared>>,
    collector: Handle,
    collector_err: Option<String>,
    note: String,
    note_focus: bool,
    flash: Option<Flash>,
    last_click: Option<(String, Instant)>,
    win: WinState,
    win_saved: WinState,
    last_save: Instant,
    on_top: bool,
    ask_close: bool,
    really_close: bool,
    font: Option<String>,
    show_log: bool,
    exiting: Option<Instant>,
    hwnd: Hwnd,
    tray: Option<Tray>,
    tray_cmds: Arc<std::sync::Mutex<Vec<TrayCmd>>>,
    autostart: Option<bool>,
    hint_tray_once: bool,
    /// started with `gui --tray`: hide again right after eframe's first frame (eframe shows the window then)
    start_hidden: bool,
    frames: u64,
}

impl App {
    fn new(
        cfg: Cfg,
        shared_rx: std::sync::mpsc::Receiver<Arc<Shared>>,
        collector: Handle,
        win: WinState,
        on_top: bool,
        font: Option<String>,
        hwnd: Hwnd,
        tray: Option<Tray>,
        tray_cmds: Arc<std::sync::Mutex<Vec<TrayCmd>>>,
    ) -> Self {
        App {
            cfg,
            shared_rx,
            sh: None,
            collector,
            collector_err: None,
            note: String::new(),
            note_focus: false,
            flash: None,
            last_click: None,
            win_saved: win.clone(),
            win,
            last_save: Instant::now(),
            on_top,
            ask_close: false,
            really_close: false,
            font,
            show_log: false,
            exiting: None,
            hwnd,
            tray,
            tray_cmds,
            autostart: desktop::autostart_get(),
            hint_tray_once: false,
            start_hidden: false,
            frames: 0,
        }
    }

    fn label(&mut self, def: &LabelDef, note: Option<String>) {
        let Some(sh) = &self.sh else { return };
        let ev = sh.add_label(&def.id, &def.text, note, "button");
        let t = chrono::DateTime::from_timestamp_millis(ev.t_local).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S%.3f").to_string()).unwrap_or_default();
        let t = t.get(..t.len().saturating_sub(2)).unwrap_or(&t).to_string(); // 20:15:29.5
        self.flash = Some(Flash {
            text: match ev.note.as_deref() {
                Some(n) => format!("✔ {t} 备注「{n}」{}", if ev.clock_synced { "" } else { "（未对时）" }),
                None => format!("✔ {t} 已记录：{}{}", ev.text, if ev.clock_synced { "" } else { "（未对时）" }),
            },
            at: Instant::now(),
            color: Color32::from_rgb(46, 160, 67),
        });
        self.last_click = Some((def.id.clone(), Instant::now()));
    }

    fn open_map(&mut self) {
        let url = self.cfg.gui.map_url.clone();
        match desktop::open_url(&url) {
            Ok(()) => self.flash = Some(Flash { text: "已在浏览器中打开战术地图".into(), at: Instant::now(), color: Color32::from_rgb(70, 110, 160) }),
            Err(e) => self.flash = Some(Flash { text: e, at: Instant::now(), color: Color32::from_rgb(180, 70, 60) }),
        }
    }

    /// Hide to the tray (falls back to minimize when there is no tray).
    fn to_tray(&mut self, ctx: &egui::Context) {
        crate::say!("[界面] 隐藏到托盘（tray={} hwnd={}）", self.tray.is_some(), self.hwnd.ok());
        if self.tray.is_some() && self.hwnd.ok() {
            self.save_now();
            // winit path (keeps winit's visibility state in sync); the tray shows the window natively
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            if !self.hint_tray_once {
                self.hint_tray_once = true;
                crate::say!("[界面] 已隐藏到系统托盘，采集继续；单击托盘图标可重新显示");
            }
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    fn save_now(&mut self) {
        if let Some(r) = self.hwnd.placement() {
            self.win.rect_px = Some(r);
        }
        save_state(&self.win);
        self.win_saved = self.win.clone();
        self.last_save = Instant::now();
    }

    /// One-line overall state in plain Chinese (also used as the tray tooltip).
    fn summary_of(sh: &Shared) -> (String, Color32) {
        let ok = Color32::from_rgb(90, 200, 110);
        let warn = Color32::from_rgb(240, 180, 60);
        let bad = Color32::from_rgb(235, 90, 80);
        let g = sh.game.lock().unwrap().clone();
        let paused = *sh.paused.lock().unwrap();
        let (fail, has_ok) = {
            let u = sh.up.lock().unwrap();
            (u.offline || !u.last_err.is_empty(), u.last_ok.is_some())
        };
        if !sh.reachable() {
            return ("等待游戏：未检测到 War Thunder 本机接口（请启动游戏）".into(), warn);
        }
        if let Some((gate::Reason::GameDown, _)) = paused {
            return ("等待游戏：游戏接口无响应，已暂停上传".into(), warn);
        }
        if let Some((gate::Reason::Hangar, _)) = paused {
            return ("待命：在机库中，进入对局后自动开始上传".into(), warn);
        }
        if fail {
            return ("上传失败：请检查网络（数据只在内存中暂存约 10 秒）".into(), bad);
        }
        let in_battle = g.map_valid == Some(true) || g.player.is_some();
        if in_battle && has_ok {
            return ("正在采集并上传本局数据".into(), ok);
        }
        if in_battle {
            return ("对局中：正在连接服务器…".into(), warn);
        }
        ("已连接游戏，等待进入对局".into(), warn)
    }

    fn status_ui(&self, ui: &mut egui::Ui, sh: &Shared) {
        let ok = Color32::from_rgb(90, 200, 110);
        let warn = Color32::from_rgb(240, 180, 60);
        let bad = Color32::from_rgb(235, 90, 80);
        let g = sh.game.lock().unwrap().clone();
        let reach = sh.reachable();
        let base_s = sh.base();
        let base = crate::game::short(&base_s).to_string();
        let paused = *sh.paused.lock().unwrap();
        let up = {
            let u = sh.up.lock().unwrap();
            (u.offline || !u.last_err.is_empty(), u.last_err.clone(), u.rate, u.buf_age_ms, u.drop_too_old + u.drop_overflow + u.drop_no_sync, u.srv_too_old + u.srv_from_future + u.srv_no_sync + u.rejected, u.last_ok.map(|t| t.elapsed()), u.pending + u.inflight)
        };
        let ck = sh.clock.lock().unwrap().clone();
        let (nlab, npend, lerr) = {
            let b = sh.labels.lock().unwrap();
            (b.count_effective(), b.pending(), b.last_err.clone())
        };
        egui::Grid::new("status").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
            ui.label("游戏接口");
            if reach {
                ui.colored_label(ok, format!("已连接 {base}"));
            } else {
                let k = sh.last_err_kind.lock().unwrap().clone();
                let why = match k.as_str() {
                    "connect_refused" => "游戏未运行/接口未开放",
                    "connect_timeout" => "连接超时",
                    "read_timeout" => "响应超时（加载中？）",
                    "" => "探测中…",
                    _ => "无响应",
                };
                ui.colored_label(bad, format!("未连接 {base} · {why}"));
            }
            ui.end_row();

            ui.label("游戏状态");
            let in_battle = g.map_valid == Some(true) || g.player.is_some();
            match paused {
                Some((gate::Reason::Hangar, _)) => ui.colored_label(warn, "机库 · 已暂停上传"),
                Some((gate::Reason::GameDown, _)) => ui.colored_label(warn, "游戏未连接 · 已暂停上传"),
                None if in_battle => {
                    let kind = match g.player.as_deref() {
                        Some("aircraft") => "空战",
                        Some(t) if t.contains("ground") => "陆战",
                        Some(_) => "对局",
                        None => "加载中",
                    };
                    ui.colored_label(ok, format!("对局中 · {kind} · 敌方标记 {} · 对象 {}", g.enemies, g.objects))
                }
                None if g.map_valid == Some(false) => ui.colored_label(warn, "机库/加载中"),
                None => ui.label("—"),
            };
            ui.end_row();

            ui.label("上传");
            // drop counters inline (a conditional extra row would shift the buttons under the mouse)
            let drops = if up.4 + up.5 > 0 { format!(" · 丢弃 {}", up.4 + up.5) } else { String::new() };
            if paused.is_some() {
                ui.colored_label(warn, format!("暂停中（每 {} 秒心跳） · 已跳过 {} 条重复数据{drops}", gate::HEARTBEAT.as_secs(), sh.suppressed.load(Ordering::Relaxed)));
            } else if up.0 {
                ui.colored_label(bad, format!("失败：{}{drops}", if up.1.is_empty() { "离线".into() } else { up.1.clone() }));
            } else if up.6.is_some() {
                ui.colored_label(ok, format!("正常 · {:.0} 条/秒 · 队列 {} · 延迟 {:.1} 秒{drops}", up.2, up.7, up.3 as f64 / 1000.0));
            } else {
                ui.label("等待中…");
            }
            ui.end_row();

            ui.label("时钟");
            if ck.valid() {
                ui.colored_label(ok, format!("偏差 {:+.0} ms · 往返 {:.0} ms", ck.offset_ms, ck.rtt_ms));
            } else if ck.last_err.is_empty() {
                ui.colored_label(warn, "对时中…（标签照常记录）");
            } else {
                ui.colored_label(bad, format!("未对时（{}）· 标签照常发送并标记", ck.last_err));
            }
            ui.end_row();

            ui.label("标签");
            let txt = format!("本次 {nlab} 个 · 待发送 {npend}");
            if npend > 0 && !lerr.is_empty() {
                ui.colored_label(bad, format!("{txt} · {lerr}"));
            } else {
                ui.label(txt);
            }
            ui.end_row();
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(250));
        if self.sh.is_none() {
            if let Ok(s) = self.shared_rx.try_recv() {
                self.sh = Some(s);
            }
        }
        let done = self.collector.lock().unwrap().as_ref().map(|h| h.is_finished()).unwrap_or(false);
        if done {
            if let Some(h) = self.collector.lock().unwrap().take() {
                let r = futures_poll(h);
                self.collector_err = Some(match r {
                    Some(Ok(Ok(()))) => "采集已停止".into(),
                    Some(Ok(Err(e))) => format!("采集出错：{e:#}"),
                    Some(Err(e)) => format!("采集任务异常：{e}"),
                    None => "采集已停止".into(),
                });
            }
        }

        self.frames += 1;
        if self.start_hidden && self.frames >= 2 {
            self.start_hidden = false;
            self.hint_tray_once = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // --- tray commands (queued by the tray callback; the window was already shown natively)
        let cmds: Vec<TrayCmd> = std::mem::take(&mut *self.tray_cmds.lock().unwrap());
        for c in cmds {
            match c {
                TrayCmd::OpenMap => self.open_map(),
                TrayCmd::Exit => self.really_close = true,
                TrayCmd::Show | TrayCmd::Toggle => {}
            }
        }
        if let (Some(t), Some(sh)) = (self.tray.as_mut(), self.sh.as_ref()) {
            let (txt, _) = App::summary_of(sh);
            t.set_tooltip(&format!("WT 数据采集 · {txt}"));
        }

        // --- window position / size memory (skip while minimized / hidden: Windows reports -32000,-32000 then)
        let (outer, inner, close_req, minimized) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested(), i.viewport().minimized.unwrap_or(false)));
        let normal = !minimized && !self.hwnd.is_minimized() && self.hwnd.is_visible();
        if normal {
            if let Some(r) = outer {
                if r.min.x > -10000.0 && r.min.y > -10000.0 {
                    self.win.x = Some(r.min.x);
                    self.win.y = Some(r.min.y);
                }
            }
            if let Some(r) = inner {
                if r.width() >= 200.0 && r.height() >= 200.0 {
                    self.win.w = Some(r.width());
                    self.win.h = Some(r.height());
                }
            }
            if let Some(r) = self.hwnd.placement() {
                self.win.rect_px = Some(r);
            }
        }
        self.win.always_on_top = Some(self.on_top);
        if self.win != self.win_saved && self.last_save.elapsed() > Duration::from_secs(2) {
            save_state(&self.win);
            self.win_saved = self.win.clone();
            self.last_save = Instant::now();
        }

        // --- close handling: ask (minimize keeps collecting) / minimize / exit
        if close_req && !self.really_close {
            match self.cfg.gui.on_close.as_str() {
                "exit" => self.really_close = true,
                "tray" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.to_tray(&ctx);
                }
                "minimize" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                _ => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.ask_close = true;
                }
            }
        }
        if self.really_close {
            if self.exiting.is_none() {
                self.save_now();
            }
            let pend = self.sh.as_ref().map(|s| s.labels.lock().unwrap().pending()).unwrap_or(0);
            let t = *self.exiting.get_or_insert_with(Instant::now);
            // wait (max 3 s) for pending labels before closing
            if pend == 0 || t.elapsed() > Duration::from_secs(3) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if close_req {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }

        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("WT 战术指挥 · 数据采集").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("v{VERSION}"));
                });
            });
            if let Some(e) = &self.collector_err {
                ui.colored_label(Color32::from_rgb(235, 90, 80), e);
            }
            let sh = self.sh.clone();
            if let Some(sh) = &sh {
                let (txt, col) = App::summary_of(sh);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(col).size(16.0));
                    ui.add(egui::Label::new(RichText::new(txt).color(col).strong()).wrap());
                });
            }
            let map_btn = egui::Button::new(RichText::new("打开战术地图").size(15.0).strong().color(Color32::WHITE))
                .fill(Color32::from_rgb(30, 90, 160))
                .min_size(egui::vec2(ui.available_width(), 32.0));
            let r = ui.add(map_btn).on_hover_text(format!("在默认浏览器中打开 {}", self.cfg.gui.map_url));
            if r.clicked_by(egui::PointerButton::Primary) {
                self.open_map();
            }
            match &sh {
                Some(sh) => self.status_ui(ui, sh),
                None => {
                    ui.label("启动中：正在探测游戏接口（8111 / 9222）…");
                }
            }
            ui.separator();

            // --- flash confirmation + undo
            let flash_on = self.flash.as_ref().map(|f| f.at.elapsed() < Duration::from_millis(2500)).unwrap_or(false);
            let undo = sh.as_ref().and_then(|s| s.labels.lock().unwrap().undoable().map(|(e, left)| (e.text.clone(), left)));
            // one fixed-height row (no layout shift under the mouse): confirmation/hint left, undo right
            let mut do_undo = false;
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.set_min_height(30.0);
                if let Some((_, left)) = &undo {
                    if ui.button(format!("撤销 {}s", left.as_secs() + 1)).on_hover_text("撤销最近一个标签（10 秒内）").clicked() {
                        do_undo = true;
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if flash_on {
                        let f = self.flash.as_ref().unwrap();
                        egui::Frame::new().fill(f.color).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&f.text).color(Color32::WHITE).strong()).truncate());
                        });
                    } else {
                        ui.weak("在事件发生的瞬间点击对应按钮");
                    }
                });
            });
            if do_undo {
                if let Some(sh) = &sh {
                    if let Some(t) = sh.undo_last_label() {
                        self.flash = Some(Flash { text: format!("已撤销：{t}"), at: Instant::now(), color: Color32::from_rgb(110, 110, 110) });
                    }
                }
            }
            ui.add_space(4.0);

            // --- label buttons (data-driven)
            let defs = self.cfg.labels.clone();
            let cols = self.cfg.gui.columns.max(1);
            let enabled = sh.is_some();
            let avail = ui.available_width();
            let bw = ((avail - (cols as f32 - 1.0) * 6.0) / cols as f32).max(60.0);
            let mut clicked: Option<LabelDef> = None;
            egui::Grid::new("buttons").num_columns(cols).spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, d) in defs.iter().enumerate() {
                    let base = d.color.as_deref().and_then(labels::parse_color).map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(Color32::from_rgb(70, 80, 95));
                    let hot = self.last_click.as_ref().map(|(id, t)| id == &d.id && t.elapsed() < Duration::from_millis(600)).unwrap_or(false);
                    let fill = if hot { Color32::from_rgb(250, 250, 250) } else { base };
                    let tc = if hot { Color32::BLACK } else { Color32::WHITE };
                    let b = egui::Button::new(RichText::new(&d.text).size(16.0).color(tc).strong()).fill(fill).min_size(egui::vec2(bw, 44.0));
                    // mouse only: a focused egui button would also fire on Space/Enter (keys meant for the game)
                    if ui.add_enabled(enabled, b).clicked_by(egui::PointerButton::Primary) {
                        clicked = Some(d.clone());
                    }
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
            if let Some(d) = clicked {
                if d.note {
                    if self.note.trim().is_empty() {
                        self.note_focus = true;
                        self.flash = Some(Flash { text: "请输入备注内容，回车发送".into(), at: Instant::now(), color: Color32::from_rgb(70, 110, 160) });
                    } else {
                        let n = std::mem::take(&mut self.note);
                        self.label(&d, Some(n));
                    }
                } else {
                    self.label(&d, None);
                }
            }
            ui.add_space(6.0);

            // --- quick note
            let note_def = defs.iter().find(|d| d.note).cloned().unwrap_or(LabelDef { id: labels::NOTE_ID.into(), text: "自定义备注".into(), color: None, note: true });
            ui.horizontal(|ui| {
                let te = egui::TextEdit::singleline(&mut self.note).hint_text("备注（回车发送）").desired_width(ui.available_width() - 56.0);
                let r = ui.add_enabled(enabled, te);
                if self.note_focus {
                    r.request_focus();
                    self.note_focus = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let send = ui.add_enabled(enabled, egui::Button::new("发送")).clicked();
                if (enter || send) && !self.note.trim().is_empty() {
                    let n = std::mem::take(&mut self.note);
                    self.label(&note_def, Some(n));
                    if enter {
                        self.note_focus = true;
                    }
                }
            });
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.on_top, "窗口置顶").changed() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
                }
                ui.checkbox(&mut self.show_log, "日志");
                if let Some(mut on) = self.autostart {
                    if ui.checkbox(&mut on, "开机自动启动").on_hover_text("登录 Windows 时自动在后台（托盘）启动采集。写入 HKCU\\…\\Run，可随时取消").changed() {
                        match desktop::autostart_set(on) {
                            Ok(()) => {
                                self.autostart = desktop::autostart_get();
                                let t = if on { "已设置开机自动启动（启动后在托盘中）" } else { "已取消开机自动启动" };
                                self.flash = Some(Flash { text: t.into(), at: Instant::now(), color: Color32::from_rgb(70, 110, 160) });
                            }
                            Err(e) => self.flash = Some(Flash { text: e, at: Instant::now(), color: Color32::from_rgb(180, 70, 60) }),
                        }
                    }
                }
                let mut hide = false;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("退出").clicked() {
                        self.ask_close = true;
                    }
                    if self.tray.is_some() {
                        if ui.button("隐藏到托盘").on_hover_text("窗口隐藏，采集继续；单击右下角托盘图标重新显示").clicked() {
                            hide = true;
                        }
                    } else if ui.button("最小化").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
                if hide {
                    self.to_tray(&ctx);
                }
            });
            if let Some(sh) = &sh {
                ui.weak(format!("会话 {}", sh.session));
            }
            if self.font.is_none() {
                ui.colored_label(Color32::YELLOW, "未找到中文字体：在 wt-collector.toml 的 [gui] 里设置 font");
            }
            if self.show_log {
                egui::ScrollArea::vertical().max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
                    for l in crate::LOG.lock().unwrap().iter().rev().take(60).collect::<Vec<_>>().into_iter().rev() {
                        ui.label(RichText::new(l).monospace().size(11.0));
                    }
                });
            }
        });

        let mut hide_after = false;
        if self.ask_close {
            let m = egui::Modal::new(egui::Id::new("close")).show(&ctx, |ui| {
                ui.set_width(260.0);
                ui.heading("关闭窗口？");
                ui.label(if self.tray.is_some() { "退出会停止数据采集。隐藏到托盘则在后台继续采集。" } else { "退出会停止数据采集。最小化则在后台继续采集。" });
                if let Some(sh) = &self.sh {
                    let p = sh.labels.lock().unwrap().pending();
                    if p > 0 {
                        ui.colored_label(Color32::YELLOW, format!("还有 {p} 个标签未发送（退出时最多再等 3 秒）"));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let lbl = if self.tray.is_some() { "隐藏到托盘（继续采集）" } else { "最小化（继续采集）" };
                    if ui.button(lbl).clicked() {
                        self.ask_close = false;
                        hide_after = true;
                    }
                    if ui.button("退出采集").clicked() {
                        self.ask_close = false;
                        self.really_close = true;
                    }
                    if ui.button("取消").clicked() {
                        self.ask_close = false;
                    }
                });
            });
            if m.should_close() {
                self.ask_close = false;
            }
        }
        if hide_after {
            self.to_tray(&ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(r) = self.hwnd.placement() {
            self.win.rect_px = Some(r);
        }
        save_state(&self.win);
    }
}

/// Take the result of a finished JoinHandle without an async context.
fn futures_poll<T>(h: tokio::task::JoinHandle<T>) -> Option<Result<T, tokio::task::JoinError>> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut h = h;
    let mut cx = Context::from_waker(Waker::noop());
    match std::pin::Pin::new(&mut h).poll(&mut cx) {
        Poll::Ready(r) => Some(r),
        Poll::Pending => None,
    }
}
