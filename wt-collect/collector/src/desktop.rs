#![cfg_attr(not(windows), allow(dead_code))]
//! Desktop integration for the GUI (0.2.2): open URL in the default browser, autostart at login
//! (HKCU\Software\Microsoft\Windows\CurrentVersion\Run), native window placement (physical pixels,
//! survives minimize / DPI / multi-monitor), show/hide by HWND, system tray icon.
//! No keyboard hooks and no hotkeys: nothing here reads keyboard input.

pub const ICON64: &[u8] = include_bytes!("../assets/icon64.rgba");
pub const ICON32: &[u8] = include_bytes!("../assets/icon32.rgba");
pub const RUN_VALUE: &str = "WT-Collector";

/// Only http(s) URLs are opened (the value comes from wt-collector.toml).
pub fn valid_url(u: &str) -> bool {
    let l = u.trim().to_ascii_lowercase();
    (l.starts_with("https://") || l.starts_with("http://")) && !u.chars().any(|c| c.is_control() || c == '"' || c == ' ')
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !valid_url(url) {
        return Err(format!("地址无效（只支持 http/https）：{url}"));
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let r = ShellExecuteW(std::ptr::null_mut(), wide("open").as_ptr(), wide(url.trim()).as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
        if (r as isize) <= 32 {
            return Err(format!("无法打开浏览器（错误 {}）", r as isize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url.trim()).spawn().map(|_| ()).map_err(|e| format!("无法打开浏览器：{e}"))
    }
}

/// Command line written to the Run key: the exe itself, window mode, started hidden in the tray.
pub fn autostart_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" gui --tray", exe.display()))
}

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Some(true) = our entry exists and points at this exe; Some(false) = no entry / other path; None = unsupported.
pub fn autostart_get() -> Option<bool> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let r = RegGetValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(RUN_VALUE).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut len);
        if r != 0 {
            return Some(false);
        }
        let n = (len as usize / 2).saturating_sub(1);
        let v = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        Some(autostart_command().map(|c| c.eq_ignore_ascii_case(v.trim_end_matches('\0'))).unwrap_or(false))
    }
    #[cfg(not(windows))]
    None
}

pub fn autostart_set(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        if on {
            let cmd = autostart_command().ok_or("找不到程序路径")?;
            let w = wide(&cmd);
            let r = RegSetKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(RUN_VALUE).as_ptr(), REG_SZ, w.as_ptr() as *const _, (w.len() * 2) as u32);
            if r != 0 {
                return Err(format!("写入注册表失败（错误 {r}）"));
            }
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(RUN_VALUE).as_ptr());
            if r != 0 && r != 2 {
                return Err(format!("删除注册表项失败（错误 {r}）"));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err("仅 Windows 支持开机自动启动".into())
    }
}

// ---------------------------------------------------------------- native window

/// HWND as an integer (0 = unknown / not Windows).
#[derive(Clone, Copy, Default, Debug)]
pub struct Hwnd(pub isize);

impl Hwnd {
    pub fn from_handle(h: &dyn raw_window_handle::HasWindowHandle) -> Hwnd {
        match h.window_handle().map(|w| w.as_raw()) {
            #[cfg(windows)]
            Ok(raw_window_handle::RawWindowHandle::Win32(w)) => Hwnd(w.hwnd.get()),
            _ => Hwnd(0),
        }
    }
    pub fn ok(self) -> bool {
        self.0 != 0
    }

    /// Restored ("normal") window rect in workspace pixels, valid even while minimized/maximized/hidden.
    pub fn placement(self) -> Option<[i32; 4]> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
            if !self.ok() {
                return None;
            }
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(self.0 as _, &mut wp) == 0 {
                return None;
            }
            let r = wp.rcNormalPosition;
            if r.right - r.left < 100 || r.bottom - r.top < 100 {
                return None;
            }
            Some([r.left, r.top, r.right, r.bottom])
        }
        #[cfg(not(windows))]
        None
    }

    /// Restore a saved rect if it is (mostly) on a connected monitor. Does not change visibility.
    pub fn restore_placement(self, r: [i32; 4]) -> bool {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::RECT;
            use windows_sys::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, SetWindowPlacement, IsWindowVisible, SW_HIDE, SW_SHOWNOACTIVATE, WINDOWPLACEMENT};
            if !self.ok() || r[2] - r[0] < 200 || r[3] - r[1] < 200 || r[2] - r[0] > 8000 || r[3] - r[1] > 8000 {
                return false;
            }
            // the title bar strip must be on some monitor, otherwise the user could not grab the window
            let title = RECT { left: r[0] + 40, top: r[1], right: r[2] - 40, bottom: r[1] + 30 };
            if MonitorFromRect(&title, MONITOR_DEFAULTTONULL).is_null() {
                return false;
            }
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(self.0 as _, &mut wp) == 0 {
                return false;
            }
            wp.rcNormalPosition = RECT { left: r[0], top: r[1], right: r[2], bottom: r[3] };
            wp.showCmd = if IsWindowVisible(self.0 as _) != 0 { SW_SHOWNOACTIVATE as u32 } else { SW_HIDE as u32 };
            wp.flags = 0;
            SetWindowPlacement(self.0 as _, &wp) != 0
        }
        #[cfg(not(windows))]
        {
            let _ = r;
            false
        }
    }

    pub fn is_minimized(self) -> bool {
        #[cfg(windows)]
        unsafe {
            self.ok() && windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(self.0 as _) != 0
        }
        #[cfg(not(windows))]
        false
    }

    pub fn is_visible(self) -> bool {
        #[cfg(windows)]
        unsafe {
            !self.ok() || windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.0 as _) != 0
        }
        #[cfg(not(windows))]
        true
    }

    /// Show + restore + bring to front (only on an explicit user action: tray click / menu).
    pub fn show(self) {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
            if self.ok() {
                ShowWindow(self.0 as _, SW_SHOW);
                if IsIconic(self.0 as _) != 0 {
                    ShowWindow(self.0 as _, SW_RESTORE);
                }
                SetForegroundWindow(self.0 as _);
            }
        }
    }

    pub fn hide(self) {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
            if self.ok() {
                ShowWindow(self.0 as _, SW_HIDE);
            }
        }
    }
}

/// Single instance: if another GUI instance of this user is running, show + focus its window and return true
/// (the caller then exits instead of starting a second collector). The mutex handle is kept for the process life.
pub fn activate_existing(title: &str) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
        let m = CreateMutexW(std::ptr::null(), 0, wide("Local\\wt-collector-gui").as_ptr());
        if m.is_null() || GetLastError() != ERROR_ALREADY_EXISTS {
            return false;
        }
        // finds hidden (tray) windows too
        for _ in 0..20 {
            let h = FindWindowW(std::ptr::null(), wide(title).as_ptr());
            if !h.is_null() {
                Hwnd(h as isize).show();
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        false
    }
}

// ---------------------------------------------------------------- tray

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayCmd {
    Show,
    Toggle,
    OpenMap,
    Exit,
}

#[cfg(windows)]
pub struct Tray {
    icon: tray_icon::TrayIcon,
    last_tip: String,
}

#[cfg(windows)]
impl Tray {
    /// Create the tray icon. Commands are delivered through `on_cmd` (called on the UI thread's message loop).
    pub fn new(on_cmd: impl Fn(TrayCmd) + Send + Sync + 'static) -> Result<Tray, String> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
        let menu = Menu::new();
        let items = [
            MenuItem::with_id("show", "显示窗口", true, None),
            MenuItem::with_id("toggle", "隐藏 / 显示", true, None),
            MenuItem::with_id("map", "打开战术地图", true, None),
        ];
        for i in &items {
            menu.append(i).map_err(|e| e.to_string())?;
        }
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&MenuItem::with_id("exit", "退出采集", true, None)).map_err(|e| e.to_string())?;
        let icon = tray_icon::Icon::from_rgba(ICON32.to_vec(), 32, 32).map_err(|e| e.to_string())?;
        let on_cmd = std::sync::Arc::new(on_cmd);
        let f1 = on_cmd.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let c = match e.id.0.as_str() {
                "show" => TrayCmd::Show,
                "toggle" => TrayCmd::Toggle,
                "map" => TrayCmd::OpenMap,
                "exit" => TrayCmd::Exit,
                _ => return,
            };
            f1(c);
        }));
        let f2 = on_cmd.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => f2(TrayCmd::Toggle),
            TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => f2(TrayCmd::Show),
            _ => {}
        }));
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("WT 数据采集")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray { icon, last_tip: String::new() })
    }

    pub fn set_tooltip(&mut self, tip: &str) {
        // Windows limits tray tooltips to 127 UTF-16 units
        let t: String = tip.chars().take(60).collect();
        if t != self.last_tip {
            let _ = self.icon.set_tooltip(Some(&t));
            self.last_tip = t;
        }
    }
}

#[cfg(not(windows))]
pub struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub fn new(_on_cmd: impl Fn(TrayCmd) + Send + Sync + 'static) -> Result<Tray, String> {
        Err("系统托盘仅在 Windows 版提供".into())
    }
    pub fn set_tooltip(&mut self, _tip: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_validation() {
        assert!(valid_url("https://wt.zuens2020.top/proto/"));
        assert!(valid_url("http://127.0.0.1:5173/"));
        assert!(!valid_url("file:///C:/Windows/System32/cmd.exe"));
        assert!(!valid_url("cmd.exe /c calc"));
        assert!(!valid_url("https://x/\" & calc"));
        assert!(!valid_url("javascript:alert(1)"));
    }
    #[test]
    fn icon_assets_sized() {
        assert_eq!(ICON64.len(), 64 * 64 * 4);
        assert_eq!(ICON32.len(), 32 * 32 * 4);
    }
    #[test]
    fn autostart_cmd_is_tray_gui() {
        let c = autostart_command().unwrap();
        assert!(c.starts_with('"') && c.ends_with("\" gui --tray"));
    }
}
