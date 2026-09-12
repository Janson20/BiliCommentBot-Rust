//! 桌面集成：系统托盘、关闭主窗口行为、退出流程
//!
//! 关闭主窗口时的三种行为（由 `config.toml` 的 `[app] close_action` 决定）：
//!
//! * `ask`  —— 弹出系统对话框，让用户选择「最小化到托盘」还是「退出程序」
//! * `tray` —— 直接隐藏窗口，机器人继续在后台运行
//! * `exit` —— 直接退出程序
//!
//! 无论哪种行为，窗口都不会被真正销毁：隐藏到托盘后进程继续存活，
//! 通过托盘菜单「显示主窗口」即可恢复，只有「退出程序」才会结束进程。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::api::dialog::{MessageDialogBuilder, MessageDialogButtons, MessageDialogKind};
use tauri::{
    AppHandle, CustomMenuItem, GlobalWindowEvent, Manager, Runtime, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem, Window, WindowEvent,
};

use crate::bot::BotState;
use crate::config::{AppConfig, CloseAction};

/// 主窗口 label
pub const MAIN_WINDOW: &str = "main";

/// 托盘菜单项 ID
const TRAY_MENU_SHOW: &str = "tray-show";
const TRAY_MENU_QUIT: &str = "tray-quit";

/// 托盘图标悬停提示
const TRAY_TOOLTIP: &str = "BiliCommentBot-RS — B站评论自动回复机器人";

/// 询问对话框的标题
const DIALOG_TITLE: &str = "BiliCommentBot-RS";

/// 询问对话框正文
const DIALOG_MESSAGE: &str = "要如何处理主窗口？\n\n\
· 最小化到托盘：窗口隐藏，机器人在后台继续运行\n\
· 退出程序：完全结束程序，机器人停止\n\n\
提示：可在「系统设置 → 启动与窗口」中固定选择，不再每次询问。";

// ════════════════════════════════════════════════════════════════
//  运行时状态
// ════════════════════════════════════════════════════════════════

/// 桌面运行时状态（由 `main` 通过 `app.manage` 注册）
#[derive(Default)]
pub struct DesktopState {
    /// 是否已决定退出程序：置位后关闭请求直接放行，不再弹询问框
    quitting: AtomicBool,
    /// 是否正在显示「关闭窗口」询问框，避免连点关闭按钮弹出多个对话框
    asking: AtomicBool,
}

impl DesktopState {
    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    pub fn mark_quitting(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    /// 标记开始询问；返回 `true` 表示此前已在询问中
    pub fn begin_asking(&self) -> bool {
        self.asking.swap(true, Ordering::SeqCst)
    }

    /// 解除询问中标记
    pub fn finish_asking(&self) {
        self.asking.store(false, Ordering::SeqCst);
    }
}

// ════════════════════════════════════════════════════════════════
//  托盘
// ════════════════════════════════════════════════════════════════

/// 构建系统托盘（图标取自 tauri.conf.json 的 `tauri.systemTray.iconPath`）
pub fn tray() -> SystemTray {
    let menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new(TRAY_MENU_SHOW, "显示主窗口"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new(TRAY_MENU_QUIT, "退出程序"));

    SystemTray::new().with_tooltip(TRAY_TOOLTIP).with_menu(menu)
}

/// 托盘事件分发
pub fn on_tray_event<R: Runtime>(app: &AppHandle<R>, event: SystemTrayEvent) {
    match event {
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            TRAY_MENU_SHOW => show_main_window(app),
            TRAY_MENU_QUIT => {
                log::info!("托盘菜单：退出程序");
                quit_app(app);
            }
            other => log::debug!("未处理的托盘菜单项: {}", other),
        },
        // 左键 / 双击托盘图标 = 打开主窗口
        SystemTrayEvent::LeftClick { .. } | SystemTrayEvent::DoubleClick { .. } => {
            show_main_window(app)
        }
        _ => {}
    }
}

// ════════════════════════════════════════════════════════════════
//  窗口显示 / 隐藏 / 退出
// ════════════════════════════════════════════════════════════════

/// 显示并聚焦主窗口
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        log::warn!("未找到主窗口 {}", MAIN_WINDOW);
    }
}

/// 隐藏主窗口（最小化到托盘）
pub fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
}

/// 退出程序：先标记退出（避免再次弹询问框），停止机器人后结束进程
pub fn quit_app<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<DesktopState>() {
        state.mark_quitting();
    }
    if let Some(bot) = app.try_state::<Arc<BotState>>() {
        bot.shutdown.store(true, Ordering::Relaxed);
        bot.running.store(false, Ordering::Relaxed);
        log::info!("已通知机器人停止");
    }
    log::info!("程序退出");
    app.exit(0);
}

// ════════════════════════════════════════════════════════════════
//  关闭窗口事件
// ════════════════════════════════════════════════════════════════

/// 窗口事件处理器（注册到 `Builder::on_window_event`）
pub fn on_window_event<R: Runtime>(event: GlobalWindowEvent<R>) {
    let WindowEvent::CloseRequested { api, .. } = event.event() else {
        return;
    };

    let window = event.window();
    let app = window.app_handle();

    // 已经确定要退出（托盘退出 / 清空数据）：放行关闭
    if app
        .try_state::<DesktopState>()
        .map(|state| state.is_quitting())
        .unwrap_or(false)
    {
        return;
    }

    // 阻止默认的「直接关闭」，改由下面的策略决定窗口去留
    api.prevent_close();

    let action = app
        .try_state::<AppConfig>()
        .map(|cfg| cfg.get().app.close_action_kind())
        .unwrap_or(CloseAction::Ask);

    match action {
        CloseAction::Tray => {
            log::info!("关闭主窗口 → 最小化到系统托盘（配置：tray）");
            hide_main_window(&app);
        }
        CloseAction::Exit => {
            log::info!("关闭主窗口 → 退出程序（配置：exit）");
            quit_app(&app);
        }
        CloseAction::Ask => {
            let already_asking = app
                .try_state::<DesktopState>()
                .map(|state| state.begin_asking())
                .unwrap_or(false);
            if already_asking {
                log::debug!("关闭询问框已在显示中，忽略重复的关闭请求");
            } else {
                ask_close_action(window.clone());
            }
        }
    }
}

/// 弹出原生对话框，询问是「最小化到托盘」还是「退出程序」
///
/// 使用 Tauri v1 的非阻塞对话框 API（内部另起线程弹窗），
/// 不会阻塞事件循环；对话框以主窗口为父窗口，居中显示。
fn ask_close_action<R: Runtime>(window: Window<R>) {
    let app = window.app_handle();
    MessageDialogBuilder::new(DIALOG_TITLE, DIALOG_MESSAGE)
        .buttons(MessageDialogButtons::OkCancelWithLabels(
            "最小化到托盘".into(),
            "退出程序".into(),
        ))
        .kind(MessageDialogKind::Info)
        .parent(&window)
        .show(move |minimize| {
            // 解除「询问中」标记，下次关闭窗口可以再次询问
            if let Some(state) = app.try_state::<DesktopState>() {
                state.finish_asking();
            }
            if minimize {
                log::info!("关闭主窗口 → 用户选择最小化到系统托盘");
                hide_main_window(&app);
            } else {
                log::info!("关闭主窗口 → 用户选择退出程序");
                quit_app(&app);
            }
        });
}
