//! WT 战术指挥 desktop client (1.0.0): native window (egui/eframe, OpenGL) without any data-labeling UI.
//! Collection runs on the tokio runtime in the background; the window only shows state.
//! Embedded app icon, system tray (show/hide/open map/exit, close-to-tray by default), native window placement
//! restore, "打开战术地图" button ([gui] map_url), autostart (HKCU Run, starts hidden in the tray), single instance.
//! Keyboard: no hooks / hotkeys and no text inputs; the window never grabs focus by itself.
//! All UI text is Simplified Chinese.
use crate::desktop::{self, Hwnd, Tray, TrayCmd};
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
    let title = format!("WT 战术指挥 v{VERSION}");
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
        .with_inner_size([st.w.unwrap_or(380.0).clamp(300.0, 1200.0), st.h.unwrap_or(430.0).clamp(300.0, 1400.0)])
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
    flash: Option<Flash>,
    win: WinState,
    win_saved: WinState,
    last_save: Instant,
    on_top: bool,
    ask_close: bool,
    really_close: bool,
    font: Option<String>,
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
            flash: None,
            win_saved: win.clone(),
            win,
            last_save: Instant::now(),
            on_top,
            ask_close: false,
            really_close: false,
            font,
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
                crate::say!("[界面] 已隐藏到系统托盘，采集继续；单击右下角托盘图标可重新显示");
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

    /// Compact product status (5 rows). Raw numbers live in details_ui.
    fn status_ui(&self, ui: &mut egui::Ui, sh: &Shared) {
        let ok = Color32::from_rgb(90, 200, 110);
        let warn = Color32::from_rgb(240, 180, 60);
        let bad = Color32::from_rgb(235, 90, 80);
        let g = sh.game.lock().unwrap().clone();
        let paused = *sh.paused.lock().unwrap();
        let (fail, err, buf_ms, has_ok) = {
            let u = sh.up.lock().unwrap();
            (u.offline || !u.last_err.is_empty(), u.last_err.clone(), u.buf_age_ms, u.last_ok.is_some())
        };
        let ck = sh.clock.lock().unwrap().clone();
        egui::Grid::new("status").num_columns(2).spacing([12.0, 5.0]).show(ui, |ui| {
            ui.label("游戏连接");
            if sh.reachable() {
                ui.colored_label(ok, "已连接");
            } else {
                ui.colored_label(warn, "未连接（请启动 War Thunder）");
            }
            ui.end_row();

            ui.label("对局");
            let in_battle = g.map_valid == Some(true) || g.player.is_some();
            match paused {
                Some((gate::Reason::Hangar, _)) => ui.colored_label(warn, "机库待命"),
                Some((gate::Reason::GameDown, _)) => ui.colored_label(warn, "等待游戏"),
                None if in_battle => {
                    let kind = match g.player.as_deref() {
                        Some("aircraft") => " · 空战",
                        Some(t) if t.contains("ground") => " · 陆战",
                        Some(_) => "",
                        None => " · 加载中",
                    };
                    ui.colored_label(ok, format!("对局中{kind}"))
                }
                None if g.map_valid == Some(false) => ui.colored_label(warn, "机库待命"),
                None => ui.label("—"),
            };
            ui.end_row();

            ui.label("上传");
            if paused.is_some() {
                ui.colored_label(warn, "已暂停（机库中不上传）");
            } else if fail {
                ui.colored_label(bad, if err.is_empty() { "失败：无法连接服务器".to_string() } else { format!("失败：{}", short_err(&err)) });
            } else if has_ok {
                ui.colored_label(ok, format!("正常 · 延迟 {:.1} 秒", buf_ms as f64 / 1000.0));
            } else {
                ui.label("连接中…");
            }
            ui.end_row();

            ui.label("时钟同步");
            if ck.valid() {
                ui.colored_label(ok, "已同步");
            } else if ck.last_err.is_empty() {
                ui.colored_label(warn, "同步中…");
            } else {
                ui.colored_label(bad, "未同步（数据照常采集）");
            }
            ui.end_row();

            ui.label("玩家");
            match self.cfg.gui.nickname.as_deref().filter(|n| !n.trim().is_empty()) {
                Some(n) => ui.label(format!("{n}（ID {}）", self.cfg.player)),
                None => ui.label(format!("ID {}", self.cfg.player)),
            };
            ui.end_row();
        });
    }

    /// Technical details (collapsed by default).
    fn details_ui(&self, ui: &mut egui::Ui, sh: &Shared) {
        let base = sh.base();
        let up = {
            let u = sh.up.lock().unwrap();
            (u.rate, u.pending + u.inflight, u.buf_age_ms, u.drop_too_old + u.drop_overflow + u.drop_no_sync, u.srv_too_old + u.srv_from_future + u.srv_no_sync + u.rejected, u.last_err.clone())
        };
        let ck = sh.clock.lock().unwrap().clone();
        let g = sh.game.lock().unwrap().clone();
        let small = |ui: &mut egui::Ui, t: String| {
            ui.label(RichText::new(t).size(12.0).weak());
        };
        small(ui, format!("游戏接口：{}{}", crate::game::short(&base), if sh.reachable() { "" } else { "（无响应）" }));
        small(ui, format!("服务器：{}", self.cfg.server));
        small(ui, format!("会话：{}", sh.session));
        small(ui, format!("上传：{:.0} 条/秒 · 队列 {} · 缓冲 {:.1} 秒 · 本地丢弃 {} · 服务器拒收 {}", up.0, up.1, up.2 as f64 / 1000.0, up.3, up.4));
        if !up.5.is_empty() {
            small(ui, format!("上传错误：{}", up.5));
        }
        small(ui, format!("机库期间跳过的重复数据：{} 条", sh.suppressed.load(Ordering::Relaxed)));
        small(ui, format!("时钟：偏差 {:+.0} ms · 往返 {:.0} ms{}", ck.offset_ms, ck.rtt_ms, if ck.last_err.is_empty() { String::new() } else { format!(" · {}", ck.last_err) }));
        small(ui, format!("地图对象 {} · 敌方标记 {}", g.objects, g.enemies));
        if let Some(f) = &self.font {
            small(ui, format!("字体：{f}"));
        }
        egui::ScrollArea::vertical().max_height(130.0).stick_to_bottom(true).show(ui, |ui| {
            for l in crate::LOG.lock().unwrap().iter().rev().take(60).collect::<Vec<_>>().into_iter().rev() {
                ui.label(RichText::new(l).monospace().size(11.0));
            }
        });
    }
}

/// First line / first 40 chars of an error for the compact status.
fn short_err(e: &str) -> String {
    let l = e.lines().next().unwrap_or("");
    let mut t: String = l.chars().take(40).collect();
    if l.chars().count() > 40 {
        t.push('…');
    }
    t
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
            t.set_tooltip(&format!("WT 战术指挥 · {txt}"));
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
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("WT 战术指挥").strong().size(22.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("v{VERSION}"));
                });
            });
            if let Some(e) = &self.collector_err {
                ui.colored_label(Color32::from_rgb(235, 90, 80), e);
            }
            let sh = self.sh.clone();
            match &sh {
                Some(sh) => {
                    let (txt, col) = App::summary_of(sh);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").color(col).size(16.0));
                        ui.add(egui::Label::new(RichText::new(txt).color(col).strong()).wrap());
                    });
                }
                None => {
                    ui.label("启动中：正在查找游戏…");
                }
            }
            ui.add_space(6.0);
            let map_btn = egui::Button::new(RichText::new("打开战术地图").size(20.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(30, 90, 160));
            if ui.add_sized([ui.available_width(), 56.0], map_btn).on_hover_text(format!("在浏览器中打开 {}", self.cfg.gui.map_url)).clicked_by(egui::PointerButton::Primary) {
                self.open_map();
            }
            // one fixed-height feedback row (map / autostart results)
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_height(22.0);
                if let Some(f) = self.flash.as_ref().filter(|f| f.at.elapsed() < Duration::from_millis(3000)) {
                    ui.colored_label(f.color, &f.text);
                }
            });
            if let Some(sh) = &sh {
                self.status_ui(ui, sh);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(mut on) = self.autostart {
                    if ui.checkbox(&mut on, "开机自启").on_hover_text("登录 Windows 后自动在托盘中启动").changed() {
                        match desktop::autostart_set(on) {
                            Ok(()) => {
                                self.autostart = desktop::autostart_get();
                                let t = if on { "已开启开机自启（启动后在托盘中）" } else { "已关闭开机自启" };
                                self.flash = Some(Flash { text: t.into(), at: Instant::now(), color: Color32::from_rgb(120, 170, 230) });
                            }
                            Err(e) => self.flash = Some(Flash { text: e, at: Instant::now(), color: Color32::from_rgb(235, 90, 80) }),
                        }
                    }
                }
                if ui.checkbox(&mut self.on_top, "窗口置顶").changed() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
                }
            });
            let mut hide = false;
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 8.0) / 2.0;
                let hide_txt = if self.tray.is_some() { "隐藏到托盘" } else { "最小化" };
                if ui.add_sized([w, 30.0], egui::Button::new(hide_txt)).on_hover_text("窗口隐藏，后台继续采集").clicked() {
                    hide = true;
                }
                if ui.add_sized([w, 30.0], egui::Button::new("退出")).on_hover_text("停止采集并退出").clicked() {
                    self.ask_close = true;
                }
            });
            if hide {
                if self.tray.is_some() {
                    self.to_tray(&ctx);
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
            ui.add_space(4.0);
            if let Some(sh) = &sh {
                egui::CollapsingHeader::new(RichText::new("详情").size(12.0).weak()).id_salt("details").default_open(false).show(ui, |ui| {
                    self.details_ui(ui, sh);
                });
            }
            if self.font.is_none() {
                ui.colored_label(Color32::YELLOW, "未找到中文字体：在 wt-collector.toml 的 [gui] 里设置 font");
            }
            });
        });

        let mut hide_after = false;
        if self.ask_close {
            let m = egui::Modal::new(egui::Id::new("close")).show(&ctx, |ui| {
                ui.set_width(260.0);
                ui.heading("退出 WT 战术指挥？");
                ui.label(if self.tray.is_some() { "退出会停止数据采集。隐藏到托盘则在后台继续采集。" } else { "退出会停止数据采集。最小化则在后台继续采集。" });
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
