#![windows_subsystem = "windows"]

use clap::Parser;
use cpp_core::CppBox;
use directories::ProjectDirs;
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use qt_core::{q_init_resource, qs};
use qt_gui::{QIcon, QPixmap};
use qt_widgets::QApplication;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tokio::task::JoinHandle;

mod bus;
mod com_channel;
mod common;
mod forms;
mod output;
mod smart_table;
mod ui;

use common::Nit;

const UI_CLEANUP_INTERVAL: Duration = Duration::from_millis(500);
const BUS_CLIENT_NAME: &str = "ecmui";
const CONTROLLER_SVC_PFX: &str = "eva.controller.";

lazy_static! {
    static ref CLIENT_CHANNEL: Mutex<Option<bus::CommandTx>> = <_>::default();
    static ref CLIENT_NAME: Mutex<Option<String>> = <_>::default();
    static ref CONNECTION: Mutex<Option<JoinHandle<()>>> = <_>::default();
    static ref NIT_HANDLER: Mutex<Option<JoinHandle<()>>> = <_>::default();
    static ref LAST_NIT: Mutex<Option<Nit>> = <_>::default();
    static ref UI_TX: OnceCell<Mutex<com_channel::ComChannel<ui::Command>>> = <_>::default();
    static ref CONFIG_FILE: Option<PathBuf> =
        if let Some(dirs) = ProjectDirs::from("com", "bohemia-automation", "ecmui") {
            let mut p = dirs.config_dir().to_owned();
            p.push("config.yml");
            Some(p)
        } else {
            None
        };
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

//extern "C" {
//pub(crate) fn qwebview_load(view: *mut QWidget, url: *const c_char);
//pub(crate) fn qwebview_eval(view: *mut QWidget, code: *const c_char);
//}

unsafe fn main_icon() -> CppBox<QIcon> {
    let pixmap = QPixmap::new();
    pixmap.load_1a(&qs(":/i/logo.png"));
    let icon = QIcon::new();
    icon.add_pixmap_1a(&pixmap);
    icon
}

fn main() {
    eva_common::self_test();
    let args = common::Args::parse();
    std::env::set_var("QT_AUTO_SCREEN_SCALE_FACTOR", "1");
    unsafe {
        qt_core::QCoreApplication::set_attribute_2a(
            qt_core::ApplicationAttribute::AAEnableHighDpiScaling,
            true,
        );
        qt_core::QCoreApplication::set_attribute_2a(
            qt_core::ApplicationAttribute::AAUseHighDpiPixmaps,
            true,
        );
    }
    QApplication::init(|_| {
        q_init_resource!("resources");
        let ui = ui::Ui::new(args);
        unsafe {
            ui.show();
            let icon = main_icon();
            QApplication::set_window_icon(&icon);
            ui.window.widget.set_window_icon(&icon);
            let res = QApplication::exec();
            ui.terminate();
            res
        }
    })
}
