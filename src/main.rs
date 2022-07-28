#![windows_subsystem = "windows"]

use directories::ProjectDirs;
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use qt_core::q_init_resource;
use qt_widgets::QApplication;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc as mpsc_std;
use std::sync::Mutex;
use std::time::Duration;
use tokio::task::JoinHandle;

mod bus;
mod common;
mod forms;
mod output;
mod smart_table;
mod ui;

use common::Nit;

const UI_CMD_INTERVAL: Duration = Duration::from_millis(100);
const UI_CLEANUP_INTERVAL: Duration = Duration::from_millis(100);
const BUS_CLIENT_NAME: &str = "ecmui";
const CONTROLLER_SVC_PFX: &str = "eva.controller.";

lazy_static! {
    static ref CLIENT_CHANNEL: Mutex<Option<bus::CommandTx>> = <_>::default();
    static ref CLIENT_NAME: Mutex<Option<String>> = <_>::default();
    static ref CONNECTION: Mutex<Option<JoinHandle<()>>> = <_>::default();
    static ref NIT_HANDLER: Mutex<Option<JoinHandle<()>>> = <_>::default();
    static ref LAST_NIT: Mutex<Option<Nit>> = <_>::default();
    static ref UI_TX: OnceCell<Mutex<mpsc_std::SyncSender<ui::Command>>> = <_>::default();
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

fn main() {
    std::env::set_var("QT_AUTO_SCREEN_SCALE_FACTOR", "1");
    QApplication::init(|_| {
        q_init_resource!("resources");
        let ui = ui::Ui::new();
        unsafe {
            ui.show();
            let res = QApplication::exec();
            ui.terminate();
            res
        }
    })
}
